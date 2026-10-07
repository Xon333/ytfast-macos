//! Turns a video id into a playable audio URL through yt-dlp.
//!
//! yt-dlp solves YouTube's JS challenges and, with the session's cookies,
//! reaches Premium's Opus ~256 kbps (itag 774). Cold lookups depend on YouTube
//! and its JS challenges. Resolves run concurrently at two priorities.
//! Playback (the current song, then the next one) has its own slots and
//! never waits behind speculation; speculation (songs on screen, under the
//! pointer, further ahead in the queue) has two more slots, runs niced, and
//! keeps a short most-likely-first backlog. A song asked for twice shares
//! one run, and a playback run nobody waits for any more is stopped.
//! Results are cached within the verified account until ten minutes before
//! the URL expires, in memory and in the private runtime directory (0600).
//! The iOS client's direct URLs were tried and dropped: they stop after the
//! first bytes.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

use crate::innertube::Stream;

/// Runs at once for playback: a click can start while the song before it
/// still resolves.
const PLAYBACK_SLOTS: usize = 2;
const SPECULATIVE_SLOTS: usize = 2;
/// Guesses waiting for a speculative slot; older ones fall off.
const BACKLOG: usize = 8;
/// A cached URL is used until this many seconds before it expires.
const MARGIN: u64 = 600;

#[derive(Clone, Serialize, Deserialize)]
struct Cached {
    itag: u32,
    url: String,
    user_agent: Option<String>,
    expires: u64,
}

impl Cached {
    fn stream(&self) -> Stream {
        Stream {
            itag: self.itag,
            url: self.url.clone(),
            user_agent: self.user_agent.clone(),
            expires: self.expires,
        }
    }
}

/// A saved cache belongs to exactly one browser/account session. The enclosing
/// shape intentionally rejects the old, unscoped map of signed stream URLs.
#[derive(Default, Serialize, Deserialize)]
struct Cache {
    scope: Option<String>,
    streams: HashMap<String, Cached>,
}

#[derive(Clone, Default)]
struct CookieSession {
    path: Option<PathBuf>,
    scope: Option<String>,
    generation: u64,
}

type Outcome = Option<Result<Stream, String>>;

/// One yt-dlp run, shared by everyone who wants its song.
struct Flight {
    result: watch::Sender<Outcome>,
    waiters: AtomicUsize,
    /// Started for playback: stopped when nobody waits for it any more.
    cancellable: AtomicBool,
    abort: Mutex<Option<tokio::task::AbortHandle>>,
}

pub struct Resolver {
    cache: Mutex<Cache>,
    /// The verified session and an opaque, account-specific cache scope.
    cookies: Mutex<CookieSession>,
    /// The runtime directory: cookie copies and the saved cache.
    scratch: PathBuf,
    flights: Mutex<HashMap<String, Arc<Flight>>>,
    playback: Arc<Semaphore>,
    speculative: Arc<Semaphore>,
    backlog: Mutex<VecDeque<String>>,
    /// Serialises writes of the saved cache.
    saving: Mutex<()>,
    runs: AtomicU64,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn describe(itag: u32) -> String {
    match itag {
        774 => "Opus 256 kbps · Premium (itag 774)".into(),
        141 => "AAC 256 kbps · Premium (itag 141)".into(),
        251 => "Opus 160 kbps (itag 251)".into(),
        140 => "AAC 128 kbps (itag 140)".into(),
        250 => "Opus 70 kbps (itag 250)".into(),
        249 => "Opus 50 kbps (itag 249)".into(),
        139 => "AAC 48 kbps (itag 139)".into(),
        other => format!("itag {other}"),
    }
}

/// A resolve asked for: known at once, or a share of a run.
pub enum Request {
    Ready(Result<Stream>),
    Waiting(Waiting),
}

/// A share of a run; dropping the last share of a playback run stops it.
pub struct Waiting {
    resolver: Arc<Resolver>,
    id: String,
    flight: Arc<Flight>,
}

impl Request {
    pub async fn wait(self) -> Result<Stream> {
        let waiting = match self {
            Request::Ready(result) => return result,
            Request::Waiting(waiting) => waiting,
        };
        let mut rx = waiting.flight.result.subscribe();
        let outcome = rx
            .wait_for(Option::is_some)
            .await
            .map(|o| o.clone())
            .map_err(|_| anyhow!("the stream lookup stopped"))?;
        drop(waiting);
        outcome
            .unwrap_or_else(|| Err("the stream lookup stopped".into()))
            .map_err(|e| anyhow!(e))
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        let abort = {
            let mut flights = self.resolver.flights.lock().expect("flights lock");
            let last = self.flight.waiters.fetch_sub(1, Ordering::SeqCst) == 1;
            if !(last
                && self.flight.cancellable.load(Ordering::SeqCst)
                && self.flight.result.borrow().is_none())
            {
                return;
            }
            if flights
                .get(&self.id)
                .is_some_and(|f| Arc::ptr_eq(f, &self.flight))
            {
                flights.remove(&self.id);
            }
            self.flight.abort.lock().expect("abort lock").take()
        };
        if let Some(abort) = abort {
            abort.abort();
            log::info!("stopped resolving {}: no longer wanted", self.id);
        }
    }
}

/// Removes a finished or stopped run from the table.
struct Landing {
    resolver: Arc<Resolver>,
    id: String,
    flight: Arc<Flight>,
}

impl Drop for Landing {
    fn drop(&mut self) {
        // A run that ends without an answer (stopped, or panicked) says so.
        if self.flight.result.borrow().is_none() {
            self.flight
                .result
                .send_replace(Some(Err("the stream lookup stopped".into())));
        }
        let mut flights = self.resolver.flights.lock().expect("flights lock");
        if flights
            .get(&self.id)
            .is_some_and(|f| Arc::ptr_eq(f, &self.flight))
        {
            flights.remove(&self.id);
        }
    }
}

/// yt-dlp's private copy of the cookie file, removed however the run ends.
struct CookieCopy(PathBuf);

impl Drop for CookieCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Resolver {
    /// Starts with the streams saved by an earlier run that are still valid.
    pub fn new(scratch: PathBuf) -> Self {
        let deadline = now() + MARGIN;
        let mut cache: Cache = std::fs::read(scratch.join("streams.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        cache.streams.retain(|_, c| c.expires > deadline);
        Self::trim_cache(&mut cache.streams);
        if !cache.streams.is_empty() {
            log::info!("{} saved streams still valid", cache.streams.len());
        }
        Self {
            cache: Mutex::new(cache),
            cookies: Mutex::default(),
            scratch,
            flights: Mutex::default(),
            playback: Arc::new(Semaphore::new(PLAYBACK_SLOTS)),
            speculative: Arc::new(Semaphore::new(SPECULATIVE_SLOTS)),
            backlog: Mutex::default(),
            saving: Mutex::default(),
            runs: AtomicU64::new(0),
        }
    }

    /// Install a verified account's cookie export. An account change cancels
    /// old work and discards its URLs; relaunching the same session can reuse
    /// the private, expiring cache without resolving every song again.
    pub fn set_cookie_file(&self, path: Option<PathBuf>, scope: Option<String>) {
        let aborts = {
            // Same lock order as request: flights, cookies, cache.
            let mut flights = self.flights.lock().expect("flights lock");
            let mut cookies = self.cookies.lock().expect("cookie lock");
            let changed = cookies.scope != scope || cookies.path.is_some() != path.is_some();
            cookies.path = path;
            cookies.scope = scope.clone();
            let mut cache = self.cache.lock().expect("cache lock");
            if cache.scope != scope {
                cache.scope = scope;
                cache.streams.clear();
            }
            if changed {
                cookies.generation = cookies.generation.wrapping_add(1);
                self.backlog.lock().expect("backlog lock").clear();
                flights
                    .drain()
                    .filter_map(|(_, flight)| {
                        flight.result.send_replace(Some(Err(
                            "The account changed. Press Play again.".into(),
                        )));
                        flight.abort.lock().expect("abort lock").take()
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };
        for abort in aborts {
            abort.abort();
        }
        self.save();
    }

    /// A cached stream for this exact account, still valid for ten minutes.
    pub fn cached(&self, video_id: &str) -> Option<Stream> {
        let cookies = self.cookies.lock().expect("cookie lock");
        let cache = self.cache.lock().expect("cache lock");
        if cache.scope != cookies.scope {
            return None;
        }
        cache
            .streams
            .get(video_id)
            .filter(|c| c.expires > now() + MARGIN)
            .map(Cached::stream)
    }

    pub fn forget(&self, video_id: &str) {
        self.cache
            .lock()
            .expect("cache lock")
            .streams
            .remove(video_id);
        self.save();
    }

    /// E2E: forgets `video_id`'s stream; true if nothing is resolving it or
    /// waiting to, so a click on it is a cold one.
    #[cfg(feature = "e2e")]
    pub fn make_cold(&self, video_id: &str) -> bool {
        self.forget(video_id);
        !self
            .flights
            .lock()
            .expect("flights lock")
            .contains_key(video_id)
            && !self
                .backlog
                .lock()
                .expect("backlog lock")
                .iter()
                .any(|id| id == video_id)
    }

    /// The best stream the account can get, for playback.
    pub async fn resolve(self: &Arc<Self>, video_id: &str) -> Result<Stream> {
        self.request(video_id).wait().await
    }

    /// Asks for a stream for playback. The share is taken at once, so a run
    /// handed from one waiter to the next is never stopped in between.
    pub fn request(self: &Arc<Self>, video_id: &str) -> Request {
        #[cfg(feature = "e2e")]
        if crate::e2e::sabotaged(video_id) {
            return Request::Ready(Ok(Stream {
                itag: 251,
                url: "http://127.0.0.1:9/ytfast-e2e-broken".into(),
                user_agent: None,
                expires: now() + 3600,
            }));
        }
        #[cfg(feature = "e2e")]
        if crate::e2e::offline() {
            return Request::Ready(Err(anyhow!("Unable to reach YouTube (simulated offline)")));
        }
        let mut flights = self.flights.lock().expect("flights lock");
        if let Some(stream) = self.cached(video_id) {
            return Request::Ready(Ok(stream));
        }
        let flight = match flights.get(video_id) {
            Some(flight) => flight.clone(),
            None => self.launch(&mut flights, video_id, None),
        };
        flight.waiters.fetch_add(1, Ordering::SeqCst);
        Request::Waiting(Waiting {
            resolver: self.clone(),
            id: video_id.to_owned(),
            flight,
        })
    }

    /// Resolves likely songs ahead of a click, most likely first, without
    /// taking a playback slot.
    pub fn prepare_many(self: &Arc<Self>, mut video_ids: Vec<String>) {
        if cfg!(feature = "menubar") {
            return;
        }
        #[cfg(feature = "e2e")]
        if crate::e2e::offline() {
            return;
        }
        video_ids.retain(|id| self.cached(id).is_none());
        if video_ids.is_empty() {
            return;
        }
        {
            // A playback run that is also a good guess keeps going if playback moves on.
            let flights = self.flights.lock().expect("flights lock");
            for id in &video_ids {
                if let Some(flight) = flights.get(id) {
                    flight.cancellable.store(false, Ordering::SeqCst);
                }
            }
        }
        {
            let mut backlog = self.backlog.lock().expect("backlog lock");
            backlog.retain(|id| !video_ids.contains(id));
            for id in video_ids.into_iter().rev() {
                backlog.push_front(id);
            }
            backlog.truncate(BACKLOG);
        }
        self.pump();
    }

    pub fn prepare(self: &Arc<Self>, video_id: &str) {
        if cfg!(feature = "menubar") {
            return;
        }
        self.prepare_many(vec![video_id.to_owned()]);
    }

    /// Waits for a song through the speculative path: prepared ahead (most
    /// likely first), sharing a run already under way, never taking a
    /// playback slot. For previews (Audition), which must not delay playback.
    pub async fn prepared(self: &Arc<Self>, video_id: &str) -> Result<Stream> {
        #[cfg(feature = "e2e")]
        if crate::e2e::offline() {
            bail!("Unable to reach YouTube (simulated offline)");
        }
        let deadline = Instant::now() + std::time::Duration::from_secs(90);
        loop {
            if let Some(stream) = self.cached(video_id) {
                return Ok(stream);
            }
            let share = {
                let flights = self.flights.lock().expect("flights lock");
                flights.get(video_id).cloned().map(|flight| {
                    flight.waiters.fetch_add(1, Ordering::SeqCst);
                    Waiting {
                        resolver: self.clone(),
                        id: video_id.to_owned(),
                        flight,
                    }
                })
            };
            if let Some(share) = share {
                return Request::Waiting(share).wait().await;
            }
            if Instant::now() > deadline {
                bail!("the stream lookup took too long");
            }
            // Still waiting for a speculative slot: stay first in line.
            self.prepare(video_id);
            if !self
                .flights
                .lock()
                .expect("flights lock")
                .contains_key(video_id)
            {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }

    /// Starts guesses from the backlog while speculative slots are free.
    fn pump(self: &Arc<Self>) {
        loop {
            let Ok(permit) = self.speculative.clone().try_acquire_owned() else {
                return;
            };
            let mut flights = self.flights.lock().expect("flights lock");
            let next = loop {
                let Some(id) = self.backlog.lock().expect("backlog lock").pop_front() else {
                    break None;
                };
                if !flights.contains_key(&id) && self.cached(&id).is_none() {
                    break Some(id);
                }
            };
            let Some(id) = next else { return };
            self.launch(&mut flights, &id, Some(permit));
        }
    }

    /// Starts a run: speculative with a slot in hand, otherwise for playback
    /// once a playback slot is free.
    fn launch(
        self: &Arc<Self>,
        flights: &mut HashMap<String, Arc<Flight>>,
        video_id: &str,
        permit: Option<OwnedSemaphorePermit>,
    ) -> Arc<Flight> {
        let speculative = permit.is_some();
        let session = self.cookies.lock().expect("cookie lock").clone();
        let flight = Arc::new(Flight {
            result: watch::Sender::new(None),
            waiters: AtomicUsize::new(0),
            cancellable: AtomicBool::new(!speculative),
            abort: Mutex::default(),
        });
        flights.insert(video_id.to_owned(), flight.clone());
        let this = self.clone();
        let id = video_id.to_owned();
        let shared = flight.clone();
        let task = tokio::spawn(async move {
            let _landing = Landing {
                resolver: this.clone(),
                id: id.clone(),
                flight: shared.clone(),
            };
            let queued = Instant::now();
            let permit = match permit {
                Some(permit) => permit,
                None => match this.playback.clone().acquire_owned().await {
                    Ok(permit) => permit,
                    Err(_) => return,
                },
            };
            let waited = queued.elapsed();
            let started = Instant::now();
            let result = this.run_ytdlp(&id, speculative, &session).await;
            let kind = if speculative { "ahead" } else { "for playback" };
            match &result {
                Ok(stream) => log::info!(
                    "resolved {id} {kind}: itag {} in {:.1}s (waited {:.1}s for a slot)",
                    stream.itag,
                    started.elapsed().as_secs_f64(),
                    waited.as_secs_f64()
                ),
                Err(error) => log::warn!(
                    "resolving {id} {kind} failed after {:.1}s: {error:#}",
                    started.elapsed().as_secs_f64()
                ),
            }
            {
                let cookies = this.cookies.lock().expect("cookie lock");
                if cookies.generation != session.generation {
                    return;
                }
                let outcome = match result {
                    Ok(stream) => {
                        this.store(&id, &stream, session.scope);
                        Ok(stream)
                    }
                    Err(error) => Err(format!("{error:#}")),
                };
                shared.result.send_replace(Some(outcome));
            }
            drop(permit);
            // Disk persistence does not sit between a resolved URL and playback.
            let saving = this.clone();
            tokio::task::spawn_blocking(move || saving.save());
            if speculative {
                this.pump();
            }
        });
        *flight.abort.lock().expect("abort lock") = Some(task.abort_handle());
        flight
    }

    fn trim_cache(cache: &mut HashMap<String, Cached>) {
        if cfg!(feature = "menubar") {
            while cache.len() > 32 {
                let oldest = cache
                    .iter()
                    .min_by_key(|(_, item)| item.expires)
                    .map(|(id, _)| id.clone());
                if let Some(id) = oldest {
                    cache.remove(&id);
                } else {
                    break;
                }
            }
        }
    }

    /// Caller holds the session lock, so an old resolve cannot repopulate a
    /// new account's cache between the generation check and insertion.
    fn store(&self, video_id: &str, stream: &Stream, scope: Option<String>) {
        let mut cache = self.cache.lock().expect("cache lock");
        if cache.scope != scope {
            cache.scope = scope;
            cache.streams.clear();
        }
        cache
            .streams
            .retain(|_, item| item.expires > now() + MARGIN);
        cache.streams.insert(
            video_id.to_owned(),
            Cached {
                itag: stream.itag,
                url: stream.url.clone(),
                user_agent: stream.user_agent.clone(),
                expires: stream.expires,
            },
        );
        Self::trim_cache(&mut cache.streams);
    }

    /// Writes the valid streams to the runtime directory, readable only by
    /// the user (the URLs are tied to the account).
    fn save(&self) {
        let _turn = self.saving.lock().expect("saving lock");
        let bytes = {
            let mut cache = self.cache.lock().expect("cache lock");
            let deadline = now() + MARGIN;
            cache.streams.retain(|_, c| c.expires > deadline);
            match serde_json::to_vec(&*cache) {
                Ok(bytes) => bytes,
                Err(_) => return,
            }
        };
        let path = self.scratch.join("streams.json");
        let written = crate::paths::write_atomic(&path, &bytes);
        if let Err(error) = written {
            log::warn!("couldn't save resolved streams: {error}");
        }
    }

    /// One yt-dlp run using the session captured when it was requested.
    async fn run_ytdlp(
        &self,
        video_id: &str,
        speculative: bool,
        session: &CookieSession,
    ) -> Result<Stream> {
        if video_id.is_empty()
            || video_id.len() > 64
            || !video_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            bail!("Invalid YouTube song identifier");
        }
        let copy = {
            let active = self.cookies.lock().expect("cookie lock");
            if active.generation != session.generation {
                bail!("The account changed. Press Play again.");
            }
            let copy = active.path.as_ref().map(|_| {
                let run = self.runs.fetch_add(1, Ordering::Relaxed);
                CookieCopy(self.scratch.join(format!("ytdlp-{video_id}-{run}.txt")))
            });
            if let (Some(from), Some(to)) = (&active.path, &copy) {
                std::fs::copy(from, &to.0).context("copying cookies for yt-dlp")?;
            }
            copy
        };
        // Guesses yield the CPU to playback's runs.
        let mut command = if speculative {
            let mut nice = tokio::process::Command::new("/usr/bin/nice");
            nice.args(["-n", "10"]).arg(crate::platform::tool("yt-dlp"));
            nice
        } else {
            tokio::process::Command::new(crate::platform::tool("yt-dlp"))
        };
        command.env("PATH", crate::platform::tool_path());
        command.args([
            "--ignore-config",
            "--no-warnings",
            "--no-playlist",
            "--socket-timeout=10",
            "--extractor-retries=1",
            "-f",
            "774/141/251/140/250/249/139",
        ]);
        // yt-dlp's own client choice: forcing `web_music` stopped working for
        // this account on 2026-10-01 (it now needs a PO token and yields no audio).
        command.args([
            "--print",
            "%(format_id)s\t%(http_headers.User-Agent)s\t%(url)s",
        ]);
        if let Some(copy) = &copy {
            command.arg("--cookies").arg(&copy.0);
        }
        command.arg(format!("https://music.youtube.com/watch?v={video_id}"));
        command
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null());
        let output =
            tokio::time::timeout(std::time::Duration::from_secs(30), command.output()).await;
        drop(copy);
        let output = output
            .context("yt-dlp timed out")?
            .context("running yt-dlp")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let line = stderr
                .lines()
                .rev()
                .find(|l| l.contains("ERROR"))
                .unwrap_or("yt-dlp failed");
            bail!("{}", line.trim());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut parts = stdout.trim().splitn(3, '\t');
        let (Some(format), Some(agent), Some(url)) = (parts.next(), parts.next(), parts.next())
        else {
            bail!("yt-dlp printed no stream");
        };
        let itag = format
            .split('-')
            .next()
            .and_then(|f| f.parse().ok())
            .unwrap_or(0);
        let stream = Stream {
            itag,
            expires: crate::innertube::expiry(url),
            url: url.to_owned(),
            user_agent: (agent != "NA").then(|| agent.to_owned()),
        };
        Ok(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("ytfast-resolver-{:032x}", fastrand::u128(..)));
            crate::paths::private_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn stream() -> Stream {
        Stream {
            itag: 774,
            url: "https://example.invalid/private-stream".into(),
            user_agent: None,
            expires: now() + 3600,
        }
    }

    #[test]
    fn saved_premium_streams_are_reused_only_by_the_verified_account() {
        let dir = Scratch::new();
        let resolver = Resolver::new(dir.0.clone());
        resolver.set_cookie_file(Some(dir.0.join("cookies-a")), Some("account-a".into()));
        resolver.store("song", &stream(), Some("account-a".into()));
        resolver.save();
        assert_eq!(
            std::fs::metadata(dir.0.join("streams.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let restored = Resolver::new(dir.0.clone());
        assert!(
            restored.cached("song").is_none(),
            "unverified launch must not use an account's URL"
        );
        restored.set_cookie_file(Some(dir.0.join("new-cookies-a")), Some("account-a".into()));
        assert_eq!(restored.cached("song").unwrap().itag, 774);
        restored.set_cookie_file(Some(dir.0.join("cookies-b")), Some("account-b".into()));
        assert!(restored.cached("song").is_none());
        restored.set_cookie_file(None, None);
        assert!(restored.cached("song").is_none());
    }

    #[test]
    fn reconnecting_same_account_preserves_flights_but_switching_invalidates_them() {
        let dir = Scratch::new();
        let resolver = Resolver::new(dir.0.clone());
        resolver.set_cookie_file(Some(dir.0.join("first")), Some("account-a".into()));
        let flight = Arc::new(Flight {
            result: watch::Sender::new(None),
            waiters: AtomicUsize::new(1),
            cancellable: AtomicBool::new(true),
            abort: Mutex::default(),
        });
        resolver
            .flights
            .lock()
            .unwrap()
            .insert("song".into(), flight.clone());
        let generation = resolver.cookies.lock().unwrap().generation;
        resolver.set_cookie_file(Some(dir.0.join("refreshed")), Some("account-a".into()));
        assert_eq!(resolver.cookies.lock().unwrap().generation, generation);
        assert!(flight.result.borrow().is_none());
        resolver.set_cookie_file(Some(dir.0.join("other")), Some("account-b".into()));
        assert!(flight.result.borrow().as_ref().unwrap().is_err());
        assert!(resolver.flights.lock().unwrap().is_empty());
    }

    #[test]
    fn legacy_unscoped_cache_and_nearly_expired_urls_are_rejected() {
        let dir = Scratch::new();
        let legacy = serde_json::json!({ "song": { "itag":774, "url":"https://example.invalid", "user_agent":null, "expires":now()+3600, "signed_in":true } });
        std::fs::write(dir.0.join("streams.json"), legacy.to_string()).unwrap();
        let resolver = Resolver::new(dir.0.clone());
        assert!(resolver.cached("song").is_none());
        let mut expiring = stream();
        expiring.expires = now() + MARGIN;
        resolver.store("song", &expiring, None);
        assert!(resolver.cached("song").is_none());
    }

    #[cfg(feature = "menubar")]
    #[test]
    fn restored_and_live_url_caches_are_bounded() {
        let dir = Scratch::new();
        let resolver = Resolver::new(dir.0.clone());
        for i in 0..40 {
            resolver.store(&format!("song-{i}"), &stream(), None);
        }
        assert_eq!(resolver.cache.lock().unwrap().streams.len(), 32);
        resolver.save();
        assert_eq!(
            Resolver::new(dir.0.clone())
                .cache
                .lock()
                .unwrap()
                .streams
                .len(),
            32
        );
    }
}
