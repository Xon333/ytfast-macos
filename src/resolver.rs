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

use crate::innertube::{AudioFormat, Stream};

/// Runs at once for playback: a click can start while the song before it
/// still resolves.
const PLAYBACK_SLOTS: usize = 2;
const SPECULATIVE_SLOTS: usize = 2;
/// Guesses waiting for a speculative slot; older ones fall off.
const BACKLOG: usize = 8;
/// A cached URL is used until this many seconds before it expires.
const MARGIN: u64 = 600;
/// Use yt-dlp's audio ranking, including Premium, language/client suffixes and
/// future format IDs. Preserve the original/default language before quality.
/// A DASH fragment list is not a playable URL; mpv can take HTTPS or HLS here.
const AUDIO_FORMAT: &str = "bestaudio[protocol~='^(https?|m3u8(_native)?)$']";
/// YouTube marks Premium as higher quality and reduces the quality of DRC
/// variants. Keep its source preference (damaged/missing-token protection),
/// then prefer Opus and the better bitrate/sample rate within the same tier.
const AUDIO_SORT: &str = "lang,quality,source,acodec,abr,asr";
/// Print only the chosen audio's metadata and User-Agent, never cookie headers.
const AUDIO_OUTPUT: &str = "%(.{format_id,url,acodec,abr,asr,audio_channels,vcodec,protocol})j\t%(ytfast_user_agent_json|NA)s";
/// A hyphen in an output-template field is arithmetic, so the old
/// `http_headers.User-Agent` silently produced NA. Select just this header via
/// yt-dlp's metadata parser without writing any other header to the pipe.
const AUDIO_AGENT: &str =
    r#"video:%(http_headers)j:"User-Agent"\s*:\s*(?P<ytfast_user_agent_json>"(?:\\.|[^"\\])*")"#;

#[derive(Clone, Serialize, Deserialize)]
struct Cached {
    itag: u32,
    url: String,
    user_agent: Option<String>,
    expires: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    audio: Option<AudioFormat>,
}

impl Cached {
    fn stream(&self) -> Stream {
        Stream {
            itag: self.itag,
            url: self.url.clone(),
            user_agent: self.user_agent.clone(),
            expires: self.expires,
            audio: self.audio.clone(),
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
        774 => "Opus · Premium (itag 774)".into(),
        141 => "AAC · Premium (itag 141)".into(),
        249..=251 => format!("Opus (itag {itag})"),
        140 | 139 => format!("AAC (itag {itag})"),
        other => format!("itag {other}"),
    }
}

impl Stream {
    /// Accurate source metadata where known, without inventing a fixed VBR rate.
    pub fn description(&self) -> String {
        let Some(audio) = &self.audio else {
            return describe(self.itag);
        };
        let codec = match audio.codec.as_deref() {
            Some("opus") => "Opus",
            Some(c) if c.starts_with("mp4a") || c == "aac" => "AAC",
            Some("vorbis") => "Vorbis",
            Some("flac") => "FLAC",
            Some(c) => c,
            None => "Audio",
        };
        let mut label = codec.to_owned();
        if let Some(bitrate) = audio.bitrate_kbps.filter(|b| b.is_finite() && *b > 0.0) {
            label.push_str(&format!(" · {bitrate:.0} kbps"));
        }
        if matches!(self.itag, 774 | 141) {
            label.push_str(" · Premium");
        }
        if audio.format_id.split('-').any(|part| part == "drc") {
            label.push_str(" · DRC");
        }
        let mut details = Vec::new();
        if let Some(rate) = audio.sample_rate_hz.filter(|r| *r > 0) {
            let rate = rate as f64 / 1000.0;
            let precision = usize::from(rate.fract() != 0.0);
            details.push(format!("{rate:.precision$} kHz"));
        }
        if let Some(channels) = audio.channels.filter(|c| *c > 0 && *c != 2) {
            details.push(format!("{channels} ch"));
        }
        details.push(format!("format {}", audio.format_id));
        label.push_str(&format!(" ({})", details.join("; ")));
        label
    }
}

#[derive(Deserialize)]
struct ResolvedAudio {
    format_id: String,
    url: String,
    acodec: Option<String>,
    abr: Option<f64>,
    asr: Option<f64>,
    audio_channels: Option<f64>,
    vcodec: Option<String>,
    protocol: Option<String>,
}

/// Validate the subprocess contract before handing a URL to the audio engine.
/// Keep signed URLs and other subprocess output out of parse-error messages.
fn parse_audio(stdout: &[u8]) -> Result<Stream> {
    let stdout = std::str::from_utf8(stdout)
        .map_err(|_| anyhow!("yt-dlp printed invalid audio metadata"))?;
    let Some((audio, agent)) = stdout.trim().split_once('\t') else {
        bail!("yt-dlp printed no stream");
    };
    let audio: ResolvedAudio = serde_json::from_str(audio)
        .map_err(|_| anyhow!("yt-dlp printed invalid audio metadata"))?;
    let agent: Option<String> = if agent == "NA" {
        None
    } else {
        serde_json::from_str(agent)
            .map_err(|_| anyhow!("yt-dlp printed invalid request metadata"))?
    };
    let url = reqwest::Url::parse(&audio.url)
        .map_err(|_| anyhow!("yt-dlp returned an invalid audio URL"))?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || audio.vcodec.as_deref() != Some("none")
        || audio.acodec.as_deref().is_none_or(|codec| codec == "none")
        || !matches!(
            audio.protocol.as_deref(),
            Some("https" | "http" | "m3u8" | "m3u8_native")
        )
    {
        bail!("yt-dlp did not return a playable audio-only stream");
    }
    if audio.format_id.is_empty()
        || audio.format_id.len() > 128
        || audio.format_id.chars().any(char::is_control)
        || agent
            .as_deref()
            .is_some_and(|a| a.chars().any(char::is_control))
    {
        bail!("yt-dlp printed invalid audio metadata");
    }
    let itag = audio
        .format_id
        .split('-')
        .next()
        .and_then(|id| id.parse().ok())
        .unwrap_or(0);
    Ok(Stream {
        itag,
        expires: crate::innertube::expiry(&audio.url),
        url: audio.url,
        user_agent: agent.filter(|a| !a.is_empty() && a != "NA"),
        audio: Some(AudioFormat {
            format_id: audio.format_id,
            codec: audio.acodec,
            bitrate_kbps: audio.abr.filter(|b| b.is_finite() && *b > 0.0),
            sample_rate_hz: audio.asr.and_then(positive_integer),
            channels: audio.audio_channels.and_then(positive_integer),
        }),
    })
}

fn positive_integer(value: f64) -> Option<u32> {
    (value > 0.0 && value <= u32::MAX as f64 && value.fract() == 0.0).then_some(value as u32)
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
                audio: None,
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
                audio: stream.audio.clone(),
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
            // The player accepts a complete URL, not DASH fragment recipes.
            // Keep HLS fallback, webpage, configs and Premium discovery intact.
            "--extractor-args=youtube:skip=dash",
            "-f",
            AUDIO_FORMAT,
            "-S",
            AUDIO_SORT,
        ]);
        // yt-dlp's own client choice: forcing `web_music` stopped working for
        // this account on 2026-10-01 (it now needs a PO token and yields no audio).
        command.args(["--parse-metadata", AUDIO_AGENT, "--print", AUDIO_OUTPUT]);
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
        parse_audio(&output.stdout)
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
            audio: None,
        }
    }

    fn audio_metadata() -> serde_json::Value {
        serde_json::json!({
            "format_id": "774-1", "url": format!("https://example.invalid/audio?expire={}", now() + 3600),
            "acodec": "opus", "abr": 249.4, "asr": 48000.0, "audio_channels": 2.0,
            "vcodec": "none", "protocol": "https",
        })
    }

    #[test]
    fn source_quality_metadata_survives_account_scoped_cache_reuse() {
        let selected =
            parse_audio(format!("{}\t\"fixture-agent\"\n", audio_metadata()).as_bytes()).unwrap();
        assert_eq!(selected.itag, 774);
        assert_eq!(selected.user_agent.as_deref(), Some("fixture-agent"));
        assert_eq!(
            selected.description(),
            "Opus · 249 kbps · Premium (48 kHz; format 774-1)"
        );
        let dir = Scratch::new();
        let resolver = Resolver::new(dir.0.clone());
        resolver.set_cookie_file(Some(dir.0.join("cookies-a")), Some("account-a".into()));
        resolver.store("song", &selected, Some("account-a".into()));
        resolver.save();
        let restored = Resolver::new(dir.0.clone());
        assert!(restored.cached("song").is_none());
        restored.set_cookie_file(Some(dir.0.join("cookies-a-new")), Some("account-a".into()));
        assert_eq!(
            restored.cached("song").unwrap().description(),
            selected.description()
        );
        restored.set_cookie_file(Some(dir.0.join("cookies-b")), Some("account-b".into()));
        assert!(restored.cached("song").is_none());
    }

    #[test]
    fn optional_metadata_never_invents_a_fixed_bitrate() {
        let mut audio = audio_metadata();
        audio["format_id"] = "251-drc-0".into();
        audio["abr"] = serde_json::Value::Null;
        audio["asr"] = 44100.0.into();
        audio["audio_channels"] = serde_json::Value::Null;
        let selected = parse_audio(format!("{audio}\tNA").as_bytes()).unwrap();
        assert_eq!(selected.user_agent, None);
        assert_eq!(
            selected.description(),
            "Opus · DRC (44.1 kHz; format 251-drc-0)"
        );
        audio["asr"] = 48000.5.into();
        assert!(
            parse_audio(format!("{audio}\tnull").as_bytes())
                .unwrap()
                .audio
                .unwrap()
                .sample_rate_hz
                .is_none()
        );
        assert_eq!(describe(251), "Opus (itag 251)");
    }

    #[test]
    fn subprocess_contract_rejects_video_fragment_recipes_and_private_output() {
        for (key, value) in [
            ("vcodec", "h264"),
            ("acodec", "none"),
            ("protocol", "http_dash_segments"),
            ("url", "file:///private/secret"),
        ] {
            let mut audio = audio_metadata();
            audio[key] = value.into();
            let error = parse_audio(format!("{audio}\tNA").as_bytes())
                .unwrap_err()
                .to_string();
            assert!(!error.contains("secret"));
        }
        let error = parse_audio(b"invalid secret payload\tNA")
            .unwrap_err()
            .to_string();
        assert!(!error.contains("secret"));
        assert!(
            parse_audio(format!("{}\t\"agent\\nInjected: secret\"", audio_metadata()).as_bytes())
                .is_err()
        );
    }

    #[tokio::test]
    async fn handoff_shares_a_flight_and_only_the_last_waiter_cancels_it() {
        let dir = Scratch::new();
        let resolver = Arc::new(Resolver::new(dir.0.clone()));
        // Keep the process boundary out of this test: exercise real Request and
        // Waiting ownership around one deliberately pending lookup task.
        let task = tokio::spawn(std::future::pending::<()>());
        let flight = Arc::new(Flight {
            result: watch::Sender::new(None),
            waiters: AtomicUsize::new(0),
            cancellable: AtomicBool::new(true),
            abort: Mutex::new(Some(task.abort_handle())),
        });
        resolver
            .flights
            .lock()
            .unwrap()
            .insert("song".into(), flight.clone());
        let first = resolver.request("song");
        let successor = resolver.request("song");
        assert_eq!(flight.waiters.load(Ordering::SeqCst), 2);
        drop(first);
        assert_eq!(flight.waiters.load(Ordering::SeqCst), 1);
        assert!(!task.is_finished());
        assert!(resolver.flights.lock().unwrap().contains_key("song"));
        drop(successor);
        assert!(!resolver.flights.lock().unwrap().contains_key("song"));
        assert!(task.await.unwrap_err().is_cancelled());
    }

    /// Exercises the installed upstream selector and output template, not a
    /// reimplementation. --simulate + --no-check-formats avoid all networking.
    #[test]
    #[ignore = "requires yt-dlp; synthetic format selection only, no account or network"]
    fn ytdlp_audio_selection_matches_source_quality_and_language() {
        use serde_json::{Value, json};
        fn format(id: &str, codec: &str, quality: f64, language: i32, bitrate: f64) -> Value {
            json!({
                "format_id": id, "url": format!("https://example.invalid/{id}?expire=9999999999"),
                "acodec": codec, "vcodec": "none", "quality": quality, "abr": bitrate,
                "asr": 48000, "audio_channels": 2, "protocol": "https",
                "source_preference": -1, "language_preference": language,
                "http_headers": {"User-Agent": "ytfast-fixture", "Cookie": "synthetic-cookie-not-for-output"},
            })
        }
        let standard = || format("251", "opus", 3.0, 5, 150.0);
        let premium = || format("774-1", "opus", 4.0, 5, 249.4);
        let aac = || format("141-1", "mp4a.40.2", 4.0, 5, 256.0);
        let mut dash = premium();
        dash["protocol"] = "http_dash_segments".into();
        let mut hls = premium();
        hls["protocol"] = "m3u8_native".into();
        let mut video = premium();
        video["vcodec"] = "h264".into();
        let cases = [
            (
                "Premium Opus suffix",
                vec![standard(), aac(), premium()],
                "774-1",
            ),
            ("Premium AAC fallback", vec![standard(), aac()], "141-1"),
            (
                "original before dubbed Premium",
                vec![
                    format("251-0", "opus", 3.0, 10, 150.0),
                    format("774-2", "opus", 4.0, -1, 270.0),
                ],
                "251-0",
            ),
            (
                "default language",
                vec![premium(), format("774-2", "opus", 4.0, -1, 270.0)],
                "774-1",
            ),
            (
                "non-DRC",
                vec![premium(), format("774-drc", "opus", 3.5, 5, 270.0)],
                "774-1",
            ),
            (
                "new format ID",
                vec![standard(), format("999-1", "opus", 4.0, 5, 280.0)],
                "999-1",
            ),
            ("complete URL only", vec![standard(), dash, video], "251"),
            ("HLS audio fallback", vec![hls], "774-1"),
        ];
        let dir = Scratch::new();
        let path = dir.0.join("audio-fixture.info.json");
        for (case, formats, expected) in cases {
            let fixture = json!({
                "id": "fixture", "title": "Synthetic audio", "formats": formats,
                "http_headers": {"User-Agent": "ytfast-fixture"},
                "_format_sort_fields": ["quality", "res", "fps", "hdr:12", "source", "vcodec", "channels", "acodec", "lang", "proto"],
            });
            crate::paths::write_atomic(&path, &serde_json::to_vec(&fixture).unwrap()).unwrap();
            let output = std::process::Command::new(crate::platform::tool("yt-dlp"))
                .args([
                    "--ignore-config",
                    "--no-warnings",
                    "--simulate",
                    "--no-check-formats",
                    "-f",
                    AUDIO_FORMAT,
                    "-S",
                    AUDIO_SORT,
                    "--parse-metadata",
                    AUDIO_AGENT,
                    "--print",
                    AUDIO_OUTPUT,
                    "--load-info-json",
                ])
                .arg(&path)
                .stdin(std::process::Stdio::null())
                .output()
                .expect("yt-dlp is installed for this explicit fixture check");
            assert!(
                output.status.success(),
                "{case}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let selected = parse_audio(&output.stdout).unwrap();
            assert_eq!(selected.user_agent.as_deref(), Some("ytfast-fixture"));
            assert!(
                !String::from_utf8_lossy(&output.stdout)
                    .contains("synthetic-cookie-not-for-output")
            );
            assert!(
                !String::from_utf8_lossy(&output.stderr)
                    .contains("synthetic-cookie-not-for-output")
            );
            assert_eq!(
                selected.audio.as_ref().unwrap().format_id,
                expected,
                "{case}"
            );
            assert!(selected.description().contains("kbps"));
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
