//! Renderer-free front end. Swift owns one NSStatusItem; this bridge owns the
//! existing Rust backend. JSON crosses an in-process C boundary, never a server.
//! All exported functions are called on the main thread. Network/audio work
//! stays on the backend runtime; its wake callback only schedules a main-queue tick.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char};
use std::fs::File;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::backend::{Backend, Command, Event};
use crate::model::{Account, Item, LikeStatus, Page, Target};
use crate::parse::More;
use crate::paths::Paths;
use crate::single_instance::{self, Message};

const LIBRARY: &str = "FEmusic_liked_playlists";
const MAX_PAGES: usize = 8;
const MAX_ROWS: usize = 1000;

thread_local! {
    static CORE: RefCell<Option<Core>> = const { RefCell::new(None) };
}

/// No artwork, rich text, or renderer resources enter a native menu.
#[derive(Clone, Debug, Default, Serialize)]
struct MenuRow {
    title: String,
    subtitle: String,
    play: Option<String>,
    browse: Option<String>,
    video: Option<String>,
    editable: Option<String>,
}

impl MenuRow {
    fn from_item(item: Item) -> Self {
        let encode = |t: &Target| serde_json::to_string(t).ok();
        let play = item
            .play
            .as_ref()
            .or_else(|| {
                item.target
                    .as_ref()
                    .filter(|t| matches!(t, Target::Watch { .. }))
            })
            .and_then(encode)
            .or_else(|| {
                item.track.as_ref().and_then(|t| {
                    encode(&Target::Watch {
                        video_id: Some(t.video_id.clone()),
                        playlist_id: None,
                        params: None,
                    })
                })
            });
        Self {
            title: item.title,
            subtitle: item.subtitle.iter().map(|r| r.text.as_str()).collect(),
            browse: item
                .target
                .as_ref()
                .filter(|t| !matches!(t, Target::Watch { .. }))
                .and_then(encode),
            play,
            video: item.track.map(|t| t.video_id),
            editable: item.editable,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
struct MenuPage {
    key: String,
    target: String,
    title: String,
    rows: Vec<MenuRow>,
    play: Option<String>,
    loading: bool,
    more: bool,
    message: Option<String>,
}

struct Continuation {
    token: String,
    shelf: Option<usize>,
}

struct PageEntry {
    target: Target,
    seq: u64,
    touched: Instant,
    fetched: Option<Instant>,
    page: MenuPage,
    tokens: Vec<Continuation>,
    pending: Option<String>,
}

impl PageEntry {
    fn new(target: Target, seq: u64) -> Self {
        Self {
            page: MenuPage {
                key: target.key(),
                target: serde_json::to_string(&target).unwrap_or_default(),
                loading: true,
                ..Default::default()
            },
            target,
            seq,
            touched: Instant::now(),
            fetched: None,
            tokens: vec![],
            pending: None,
        }
    }

    fn refresh(&mut self, seq: u64) {
        self.seq = seq;
        self.touched = Instant::now();
        self.page.loading = true;
        self.page.message = None;
        self.pending = None;
    }

    fn replace(&mut self, page: Page, cached: bool) {
        self.page.rows.clear();
        self.tokens.clear();
        self.page.title.clear();
        self.page.play = None;
        if let Some(h) = page.header {
            self.page.title = h.title;
            self.page.play = h.play.and_then(|t| serde_json::to_string(&t).ok());
        }
        self.page.message = page.message;
        for (index, shelf) in page.shelves.into_iter().enumerate() {
            self.page
                .rows
                .extend(shelf.items.into_iter().map(MenuRow::from_item));
            if let Some(token) = shelf.continuation {
                self.tokens.push(Continuation {
                    token,
                    shelf: Some(index),
                });
            }
        }
        if let Some(token) = page.continuation {
            self.tokens.push(Continuation { token, shelf: None });
        }
        if !cached {
            self.fetched = Some(Instant::now());
        }
        self.page.loading = cached;
        self.pending = None;
        self.finish();
    }

    fn finish(&mut self) {
        if self.page.rows.len() >= MAX_ROWS {
            self.page.rows.truncate(MAX_ROWS);
            self.tokens.clear();
            self.page.message =
                Some("Showing the first 1,000 entries. Open YouTube Music for the rest.".into());
        }
        self.page.more = !self.tokens.is_empty();
    }

    fn accept_more(&mut self, token: &str, more: More) -> bool {
        if self.pending.as_deref() != Some(token) {
            return false;
        }
        let Some(index) = self.tokens.iter().position(|c| c.token == token) else {
            return false;
        };
        let old = self.tokens.remove(index);
        match more {
            More::Items { items, next } => {
                self.page
                    .rows
                    .extend(items.into_iter().map(MenuRow::from_item));
                if let Some(token) = next {
                    self.tokens.push(Continuation {
                        token,
                        shelf: old.shelf,
                    });
                }
            }
            More::Shelves { shelves, next } => {
                for shelf in shelves {
                    self.page
                        .rows
                        .extend(shelf.items.into_iter().map(MenuRow::from_item));
                    if let Some(token) = shelf.continuation {
                        self.tokens.push(Continuation {
                            token,
                            shelf: old.shelf,
                        });
                    }
                }
                if let Some(token) = next {
                    self.tokens.push(Continuation { token, shelf: None });
                }
            }
        }
        self.pending = None;
        self.page.loading = false;
        self.finish();
        true
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Poll,
    Browse {
        target: String,
        #[serde(default)]
        force: bool,
    },
    More {
        key: String,
    },
    Play {
        target: String,
    },
    Transport {
        action: String,
    },
    Shuffle,
    Volume {
        value: f64,
    },
    Seek {
        value: f64,
    },
    Add {
        playlist: String,
        video: String,
    },
    Profile {
        id: String,
    },
    Reconnect,
    Dismiss,
    Quit,
}

struct Core {
    backend: Backend,
    messages: std::sync::mpsc::Receiver<Message>,
    _lock: File,
    paths: Paths,
    account: Account,
    profiles: Vec<crate::auth::Profile>,
    profile: Option<String>,
    pages: HashMap<String, PageEntry>,
    seq: u64,
    catalog_dirty: bool,
    notice: Option<String>,
    error: Option<String>,
    pending_add: Option<u64>,
    quit: bool,
    show: bool,
}

impl Core {
    fn browse(&mut self, target: Target, force: bool) {
        let key = target.key();
        if let Some(entry) = self.pages.get_mut(&key) {
            entry.touched = Instant::now();
            if entry.page.loading
                || (!force
                    && entry
                        .fetched
                        .is_some_and(|at| at.elapsed() < Duration::from_secs(300)))
            {
                return;
            }
        }
        if !self.pages.contains_key(&key) && self.pages.len() >= MAX_PAGES {
            // Keep the playlist index: add-to-playlist validates against this page.
            let library_key = Target::browse(LIBRARY).key();
            let oldest = self
                .pages
                .iter()
                .filter(|(k, _)| **k != library_key)
                .min_by_key(|(_, v)| v.touched)
                .map(|(k, _)| k.clone());
            if let Some(key) = oldest {
                self.pages.remove(&key);
            }
        }
        self.seq += 1;
        if let Some(entry) = self.pages.get_mut(&key) {
            entry.refresh(self.seq);
        } else {
            self.pages
                .insert(key, PageEntry::new(target.clone(), self.seq));
        }
        self.catalog_dirty = true;
        self.backend.send(Command::Page {
            target,
            seq: self.seq,
        });
    }

    fn transport(&self, action: &str) -> Result<()> {
        let now = self.backend.now.borrow();
        let command = match action {
            "play" if now.playback.playing || now.playback.loading => return Ok(()),
            "pause" if !now.playback.playing && !now.playback.loading => return Ok(()),
            "play" | "pause" | "toggle" => Command::TogglePause,
            "next" => Command::Next,
            "previous" => Command::Previous,
            _ => bail!("Unknown playback command"),
        };
        if now.track().is_some() || now.playback.loading {
            self.backend.send(command);
        }
        Ok(())
    }

    fn request(&mut self, request: Request) -> Result<()> {
        match request {
            Request::Poll => {}
            Request::Browse { target, force } => {
                if !matches!(self.account, Account::SignedIn { .. }) {
                    bail!("Reconnect to load your library");
                }
                let target: Target =
                    serde_json::from_str(&target).context("Invalid library target")?;
                if matches!(target, Target::Watch { .. }) {
                    bail!("Expected a library page");
                }
                self.browse(target, force);
            }
            Request::More { key } => {
                let entry = self.pages.get_mut(&key).context("Open the library again")?;
                if entry.page.loading {
                    return Ok(());
                }
                if let Some(next) = entry.tokens.first() {
                    entry.pending = Some(next.token.clone());
                    entry.page.loading = true;
                    self.backend.send(Command::More {
                        key,
                        token: next.token.clone(),
                        search: matches!(&entry.target, Target::Search { .. }),
                        shelf: next.shelf,
                    });
                    self.catalog_dirty = true;
                }
            }
            Request::Play { target } => {
                let target: Target =
                    serde_json::from_str(&target).context("Invalid playback target")?;
                if !matches!(target, Target::Watch { .. }) {
                    bail!("Open this playlist and choose Play");
                }
                self.error = None;
                self.backend.send(Command::PlayTarget(target));
            }
            Request::Transport { action } => self.transport(&action)?,
            Request::Shuffle => self.backend.send(Command::ToggleShuffle),
            Request::Volume { value } => {
                if !value.is_finite() {
                    bail!("Invalid volume");
                }
                self.backend.send(Command::Volume(value.clamp(0.0, 100.0)));
            }
            Request::Seek { value } => {
                if !value.is_finite() {
                    bail!("Invalid position");
                }
                let now = self.backend.now.borrow();
                if now.track().is_some() && now.playback.duration > 0.0 {
                    self.backend
                        .send(Command::Seek(value.clamp(0.0, now.playback.duration)));
                }
            }
            Request::Add { playlist, video } => {
                if !matches!(self.account, Account::SignedIn { .. }) {
                    bail!("Reconnect before adding a song");
                }
                if self.pending_add.is_some() {
                    bail!("A song is already being added");
                }
                if !valid_video(&video) {
                    bail!("Invalid song");
                }
                let owned = self
                    .pages
                    .get(&Target::browse(LIBRARY).key())
                    .is_some_and(|entry| {
                        entry
                            .page
                            .rows
                            .iter()
                            .any(|row| row.editable.as_deref() == Some(playlist.as_str()))
                    });
                if !owned {
                    bail!("Refresh Playlists to choose one you can edit");
                }
                self.seq += 1;
                self.pending_add = Some(self.seq);
                self.error = None;
                self.notice = Some("Adding song…".into());
                self.backend.send(Command::AccountEdit {
                    op: self.seq,
                    edit: crate::account::Edit::Add {
                        playlist_id: playlist.clone(),
                        video_ids: vec![video],
                    },
                    refresh: vec![Target::browse(format!("VL{playlist}"))],
                });
            }
            Request::Profile { id } => {
                if !self.profiles.iter().any(|p| p.id == id) {
                    bail!("That browser profile is unavailable");
                }
                self.clear_account();
                self.backend.send(Command::UseProfile(id));
            }
            Request::Reconnect => {
                self.clear_account();
                self.backend.send(Command::Reconnect);
            }
            Request::Dismiss => {
                self.error = None;
                self.notice = None;
            }
            Request::Quit => self.quit = true,
        }
        Ok(())
    }

    fn clear_account(&mut self) {
        self.account = Account::Checking;
        self.pages.clear();
        self.catalog_dirty = true;
        self.pending_add = None;
        self.notice = None;
        self.error = None;
    }

    fn poll(&mut self) {
        // Drain messages already delivered; never wait on network or audio I/O here.
        while let Ok(message) = self.messages.try_recv() {
            match message {
                Message::Show => self.show = true,
                Message::Quit => self.quit = true,
                Message::Toggle => {
                    let _ = self.transport("toggle");
                }
                Message::Play => {
                    let _ = self.transport("play");
                }
                Message::Pause => {
                    let _ = self.transport("pause");
                }
                Message::Next => {
                    let _ = self.transport("next");
                }
                Message::Previous => {
                    let _ = self.transport("previous");
                }
                Message::Open(link) => {
                    if let Some(target) = crate::links::target_from_link(&link) {
                        if matches!(target, Target::Watch { .. }) {
                            self.backend.send(Command::PlayTarget(target));
                        } else {
                            self.browse(target, false);
                            self.show = true;
                        }
                    }
                }
                Message::Like => {
                    let now = self.backend.now.borrow();
                    if let Some(track) = now
                        .track()
                        .filter(|_| matches!(self.account, Account::SignedIn { .. }))
                    {
                        self.backend.send(Command::AccountEdit {
                            op: 0,
                            edit: crate::account::Edit::Rate {
                                video_id: track.video_id.clone(),
                                status: if track.like == Some(LikeStatus::Like) {
                                    LikeStatus::Indifferent
                                } else {
                                    LikeStatus::Like
                                },
                            },
                            refresh: vec![],
                        });
                    }
                }
                Message::ReloadThemes => {}
            }
        }
        while let Ok(event) = self.backend.events.try_recv() {
            match event {
                Event::Account(account) => {
                    if !matches!(account, Account::SignedIn { .. })
                        || matches!(self.account, Account::Checking)
                    {
                        // A settled attempt also clears temporary playback
                        // feedback raised while the connection was pending.
                        self.clear_account();
                    }
                    self.account = account;
                }
                Event::Profiles { list, current } => {
                    self.profiles = list;
                    self.profile = current;
                }
                // The backend validates the active account scope before publishing
                // snapshots. Keep them usable while the fresh request is in flight.
                Event::Page {
                    key,
                    seq,
                    result,
                    cached,
                } => {
                    if let Some(entry) = self.pages.get_mut(&key).filter(|e| e.seq == seq) {
                        match result {
                            Ok(page) => {
                                // A late disk read must never overwrite a fresh reply.
                                if !cached || entry.fetched.is_none() {
                                    entry.replace(*page, cached);
                                }
                            }
                            Err(error) => {
                                entry.page.loading = false;
                                entry.page.message = Some(error);
                            }
                        }
                        self.catalog_dirty = true;
                    }
                }
                Event::More {
                    key, token, result, ..
                } => {
                    if let Some(entry) = self
                        .pages
                        .get_mut(&key)
                        .filter(|e| e.pending.as_deref() == Some(token.as_str()))
                    {
                        match result {
                            Ok(more) => {
                                entry.accept_more(&token, more);
                            }
                            Err(error) => {
                                entry.pending = None;
                                entry.page.loading = false;
                                entry.page.message = Some(error);
                            }
                        }
                        self.catalog_dirty = true;
                    }
                }
                Event::AccountEdited { op, result } if self.pending_add == Some(op) => {
                    self.pending_add = None;
                    self.notice = None;
                    match result {
                        Ok(_) => {
                            self.error = None;
                            self.notice = Some("Added to playlist".into());
                        }
                        Err(crate::account::Failure::AlreadyInPlaylist) => {
                            self.notice = Some("Already in this playlist".into())
                        }
                        Err(crate::account::Failure::SignedOut) => {
                            self.error = Some("Reconnect, then add the song again".into())
                        }
                        Err(crate::account::Failure::Offline) => {
                            self.error = Some("Offline. The song was not added".into())
                        }
                        Err(crate::account::Failure::Refused(message)) => {
                            self.error = Some(message)
                        }
                    }
                }
                Event::AccountRefresh(targets) => {
                    for target in targets {
                        if self.pages.contains_key(&target.key()) {
                            self.browse(target, true);
                        }
                    }
                }
                Event::Error(error) => self.error = Some(error),
                _ => {}
            }
        }
    }

    fn snapshot(&mut self) -> Value {
        self.poll();
        let now = self.backend.now.borrow();
        let pb = &now.playback;
        let track = now
            .track()
            .map(|t| json!({"id":t.video_id,"title":t.title,"artist":t.artist_line()}));
        let (signed_in, status) = match &self.account {
            Account::Checking => (false, "Connecting…".to_owned()),
            Account::SignedIn { name, source, .. } => (true, format!("{name} · {source}")),
            Account::SignedOut { reason } | Account::Unverified { reason } => {
                (false, reason.clone())
            }
        };
        let pages = std::mem::take(&mut self.catalog_dirty).then(|| {
            self.pages
                .values()
                .map(|e| e.page.clone())
                .collect::<Vec<_>>()
        });
        json!({"track":track,"playing":pb.playing,"loading":pb.loading,"position":pb.position,
            "duration":pb.duration,"volume":pb.volume,"shuffle":pb.shuffle,"format":pb.format,
            "signed_in":signed_in,"account_checking":matches!(self.account, Account::Checking),
            "account":status,"profile":self.profile,
            "profiles":self.profiles.iter().map(|p|json!({"id":p.id,"label":p.label})).collect::<Vec<_>>(),
            "pages":pages,"notice":self.notice,"error":self.error,"adding":self.pending_add.is_some(),
            "show":std::mem::take(&mut self.show),"quit":self.quit})
    }
}

fn valid_video(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

fn ffi_result(f: impl FnOnce() -> Result<Value>) -> *mut c_char {
    let value = match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => json!({"fatal":format!("{error:#}")}),
        Err(_) => json!({"fatal":"YTfast encountered an internal error. Quit and reopen it."}),
    };
    CString::new(value.to_string())
        .expect("JSON has no literal NUL")
        .into_raw()
}

/// Start once on the main thread. `wake` must remain callable until shutdown;
/// it may run on any worker thread and must only enqueue nonblocking UI work.
#[unsafe(no_mangle)]
pub extern "C" fn ytfast_start(wake: extern "C" fn()) -> *mut c_char {
    ffi_result(|| {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if matches!(
            args.first().map(String::as_str),
            Some("--help" | "-h" | "help")
        ) {
            return Ok(
                json!({"exit":0,"message":"YTfast menu bar: show | play | pause | toggle | next | previous | open <YouTube link> | quit"}),
            );
        }
        let paths = Paths::new()?;
        let message = match args.first() {
            None => Message::Show,
            Some(word) => Message::parse(word, args.get(1).map(String::as_str))
                .context("Unknown command. Use --help")?,
        };
        if single_instance::notify(&paths.runtime, &message) {
            return Ok(json!({"exit":0}));
        }
        let Some(lock) = crate::platform::instance_lock(&paths.runtime)? else {
            // A first launch may be between acquiring the lock and binding its socket.
            for _ in 0..20 {
                std::thread::sleep(Duration::from_millis(50));
                if single_instance::notify(&paths.runtime, &message) {
                    return Ok(json!({"exit":0}));
                }
            }
            bail!("YTfast is already starting. Try again shortly");
        };
        if !matches!(message, Message::Show | Message::Open(_)) {
            return Ok(json!({"exit":1,"message":"YTfast isn't running"}));
        }
        #[cfg(target_os = "macos")]
        crate::platform::SessionFiles(paths.runtime.clone()).clear();
        fastframe_log::Logging::new("ytfast", env!("CARGO_PKG_VERSION"))
            .filter("ytfast=info,warn")
            .file(paths.cache.join("ytfast.log"))
            .init()
            .map_err(|e| anyhow::anyhow!("Logging: {e}"))?;
        let backend = Backend::start(paths.clone(), move || wake())?;
        let (sender, messages) = std::sync::mpsc::channel();
        let tx = sender.clone();
        single_instance::listen(&paths.runtime, move |message| {
            let _ = tx.send(message);
            wake();
        })?;
        if matches!(message, Message::Open(_)) {
            let _ = sender.send(message);
        }
        CORE.with(|slot| {
            *slot.borrow_mut() = Some(Core {
                backend,
                messages,
                _lock: lock,
                paths,
                account: Account::Checking,
                profiles: vec![],
                profile: None,
                pages: HashMap::new(),
                seq: 0,
                catalog_dirty: true,
                notice: None,
                error: None,
                pending_add: None,
                show: false,
                quit: false,
            })
        });
        Ok(json!({"ready":true}))
    })
}

/// Run a menu command on the main thread, then return the current snapshot.
/// # Safety
/// `text` must point to a valid NUL-terminated UTF-8 string, at most 64 KiB.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ytfast_call(text: *const c_char) -> *mut c_char {
    ffi_result(|| {
        if text.is_null() {
            bail!("Missing command");
        }
        // SAFETY: the Swift caller passes a withCString buffer for this call.
        let text = unsafe { CStr::from_ptr(text) }.to_str()?;
        if text.len() > 65536 {
            bail!("Command is too long");
        }
        let request: Request = serde_json::from_str(text)?;
        CORE.with(|slot| {
            let mut slot = slot.borrow_mut();
            let core = slot.as_mut().context("YTfast is not running")?;
            core.poll();
            if let Err(error) = core.request(request) {
                core.error = Some(format!("{error:#}"));
            }
            Ok(core.snapshot())
        })
    })
}

/// Release only a string returned by this bridge, exactly once.
/// # Safety
/// `text` must be null or an unfreed pointer returned by ytfast_start/ytfast_call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ytfast_free(text: *mut c_char) {
    if !text.is_null() {
        // SAFETY: allocation ownership is handed back by the Swift caller.
        drop(unsafe { CString::from_raw(text) });
    }
}

/// Stop workers before clearing session exports. No callbacks run after return.
#[unsafe(no_mangle)]
pub extern "C" fn ytfast_stop() {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        CORE.with(|slot| {
            if let Some(core) = slot.borrow_mut().take() {
                drop(core.backend);
                #[cfg(target_os = "macos")]
                crate::platform::SessionFiles(core.paths.runtime.clone()).clear();
                let _ = std::fs::remove_file(core.paths.runtime.join("ytfast.sock"));
                drop(core._lock);
            }
        })
    }));
}

#[cfg(test)]
mod tests;
