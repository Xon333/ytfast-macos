//! The interface's handle to a tokio runtime that does all I/O: InnerTube,
//! the browser session, yt-dlp and mpv. The two sides talk only through
//! [`Command`] (interface → backend) and [`Event`] (backend → interface);
//! every event wakes the window.
//!
//! Playback state belongs to the worker. Results of asynchronous work carry
//! the stamp they were started under, so a late answer never acts on newer
//! state: `generation` changes with the current track, `epoch` with the
//! queue (each play request), and mpv's playlist entry ids tell the current
//! file's events from those of replaced or queued ones. Several mpv
//! processes can run at once (Smooth mixes, Audition: see [`deck`]); their
//! events carry the process's serial and reach the deck's current role.

mod account;
mod audition;
mod deck;
mod pages;
mod playback;
mod queue;
mod resume;
mod session;
mod sound;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::json;
use tokio::sync::mpsc;

use crate::equalizer::Equalizer;
use crate::innertube::{ApiError, Client, Stream};
use crate::model::{Account, Lyrics, Page, Playback, Repeat, Sleep, Target, Track, WatchNext};
use crate::mpv::{Mpv, MpvEvent};
use crate::parse::{self, More};
use crate::paths::Paths;
use crate::resolver::{self, Resolver};

pub enum Command {
    /// Open a page: cached copy first, then fresh. `seq` identifies the
    /// request; only the newest one's answer is used.
    Page {
        target: Target,
        seq: u64,
    },
    /// Load the next part of a page (`shelf: None`) or of one of its shelves.
    More {
        key: String,
        token: String,
        search: bool,
        shelf: Option<usize>,
    },
    Suggest(String),
    /// Lyrics for a song (timed when anyone has them). `browse_id` is its
    /// YouTube Music lyrics page if known; `duration` in seconds, 0 if unknown.
    Lyrics {
        track: Track,
        browse_id: Option<String>,
        duration: f64,
    },
    /// Read the recent searches; answered with [`Event::Searches`].
    LoadSearches,
    /// Save the recent searches, newest first.
    SaveSearches(Vec<String>),
    /// Play `tracks`, starting at `start`, as the queue.
    PlayTracks {
        tracks: Vec<Track>,
        start: usize,
    },
    /// Play a song radio, playlist, album or mix through watch-next.
    PlayTarget(Target),
    TogglePause,
    Next,
    Previous,
    Seek(f64),
    Volume(f64),
    ToggleShuffle,
    CycleRepeat,
    Autoplay(bool),
    /// Jump to a queue position (play order).
    JumpTo(usize),
    Reconnect,
    /// Resolve a song the pointer rests on, ahead of a likely click.
    Prepare(String),
    /// Use this browser profile's YouTube session from now on, and reconnect.
    UseProfile(String),
    /// Settings: song-change notifications on or off (saved for next time).
    Notifications(bool),
    /// A change to the signed-in account; `op` stamps the answer. On success
    /// the `refresh` pages are asked for again once YouTube Music shows it.
    AccountEdit {
        op: u64,
        edit: crate::account::Edit,
        refresh: Vec<Target>,
    },
    /// Fetch a song's rating on the account (answered with `Event::Likes`).
    LikeStatus(String),
    /// Insert songs right after the current one (before earlier additions).
    PlayNext(Vec<Track>),
    /// Queue songs after earlier additions, before the rest of the list.
    AddToQueue(Vec<Track>),
    /// Remove the song at a queue position (play order); not the current one.
    RemoveFromQueue(usize),
    /// Move the song at queue position `from` to `to` (play order; `to`
    /// counts after it is taken out).
    MoveInQueue {
        from: usize,
        to: usize,
    },
    /// Remove every song after the current one.
    ClearUpcoming,
    /// Set (`Some`) or cancel the sleep timer.
    SleepTimer(Option<Sleep>),
    Equalizer(Equalizer),
    /// Turn loudness levelling between songs on or off.
    Normalize(bool),
    /// Resolve songs likely to be played next (on screen when a page
    /// loads), most likely first, without delaying playback.
    PrepareMany(Vec<String>),
    /// Most-replayed heat for a song (by video id), asked once per song;
    /// answered with [`Event::Heat`].
    Heat(String),
    /// Settings: theme-painted covers on or off (saved for next time).
    PaintCovers(bool),
    /// Audition: preview `track` over the ducked current song, from `start`
    /// seconds in (its best part; `None` plays from a third of the way in),
    /// until [`Command::EndAudition`]. Holding another song switches to it.
    /// Never touches the queue, the session or history.
    Audition {
        track: Track,
        start: Option<f64>,
    },
    /// The held song was let go: it fades out and the current song comes back.
    EndAudition,
    /// Settings: Smooth mixes on radios and mixes, and their length.
    Mixes(crate::model::Mixes),
    /// E2E: read every deck's volume and position back five times a second.
    #[cfg(feature = "e2e")]
    SampleDecks(bool),
    /// Search YouTube Music for Play anything (Ctrl+K): answered with
    /// [`Event::QuickResults`], never saved to disk.
    QuickSearch(String),
}

pub enum Event {
    Account(Account),
    Page {
        key: String,
        seq: u64,
        result: Result<Box<Page>, String>,
        cached: bool,
    },
    /// The answer to a continuation `token`.
    More {
        key: String,
        shelf: Option<usize>,
        token: String,
        result: Result<More, String>,
    },
    Suggestions {
        input: String,
        items: Vec<String>,
    },
    /// Lyrics for the song with video id `id`.
    Lyrics {
        id: String,
        result: Result<Option<Lyrics>, String>,
    },
    /// The saved recent searches, newest first.
    Searches(Vec<String>),
    /// The queue in play order.
    Queue(Vec<Track>),
    Playback(Playback),
    /// A readable error for the error strip.
    Error(String),
    /// The browser profiles signed in to YouTube, and the one in use.
    Profiles {
        list: Vec<crate::auth::Profile>,
        current: Option<String>,
    },
    /// The answer to `Command::AccountEdit` number `op`.
    AccountEdited {
        op: u64,
        result: Result<crate::account::Done, crate::account::Failure>,
    },
    /// Ratings as YouTube Music returned them: (video id, rating).
    Likes(Vec<(String, crate::model::LikeStatus)>),
    /// Pages to fetch again after an account change.
    AccountRefresh(Vec<Target>),
    /// A song's most-replayed heat; `None` when it has none or the request failed.
    Heat {
        id: String,
        heat: Option<crate::heat::Heat>,
    },
    /// The answer to `Command::QuickSearch` for `query`.
    QuickResults {
        query: String,
        result: Result<Box<Page>, String>,
    },
}

#[derive(Clone)]
struct Sink {
    tx: std::sync::mpsc::Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The desktop integration's copy of the queue and playback state.
    now: Arc<tokio::sync::watch::Sender<crate::desktop::Now>>,
}

impl Sink {
    fn send(&self, event: Event) {
        crate::desktop::observe(&self.now, &event);
        let _ = self.tx.send(event);
        (self.wake)();
    }
}

pub struct Backend {
    commands: mpsc::UnboundedSender<Command>,
    pub events: std::sync::mpsc::Receiver<Event>,
    pub http: reqwest::Client,
    pub runtime: tokio::runtime::Handle,
    /// The queue and playback state as last sent, for MPRIS, notifications
    /// and the command line (which work with no window open).
    pub now: tokio::sync::watch::Receiver<crate::desktop::Now>,
    resolver: Arc<Resolver>,
    shutdown: mpsc::UnboundedSender<std::sync::mpsc::Sender<()>>,
    _runtime: tokio::runtime::Runtime,
}

impl Backend {
    pub fn start(paths: Paths, wake: impl Fn() + Send + Sync + 'static) -> anyhow::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("ytfast-io")
            .build()?;
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (shutdown, shutdown_rx) = mpsc::unbounded_channel();
        let (tx, events) = std::sync::mpsc::channel();
        let (now_tx, now) = tokio::sync::watch::channel(crate::desktop::Now::default());
        let sink = Sink {
            tx,
            wake: Arc::new(wake),
            now: Arc::new(now_tx),
        };
        let client = Arc::new(Client::new());
        let http = client.http().clone();
        let resolver = Arc::new(Resolver::new(paths.runtime.clone()));
        let worker = Worker::new(client, resolver.clone(), paths, sink);
        runtime.spawn(worker.run(command_rx, shutdown_rx));
        Ok(Self {
            commands,
            events,
            http,
            runtime: runtime.handle().clone(),
            now,
            resolver,
            shutdown,
            _runtime: runtime,
        })
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// A sender for backend commands from other threads (MPRIS, the command line).
    pub fn commands(&self) -> mpsc::UnboundedSender<Command> {
        self.commands.clone()
    }

    /// Whether a song's stream is resolved, so a click on it starts at once.
    pub fn prepared(&self, video_id: &str) -> bool {
        self.resolver.cached(video_id).is_some()
    }

    /// E2E: drops a song's resolved stream; true if a click on it is now cold.
    #[cfg(feature = "e2e")]
    pub fn make_cold(&self, video_id: &str) -> bool {
        self.resolver.make_cold(video_id)
    }

    /// Saves the session and stops playback. Runs when the backend is
    /// dropped; call it first if the process ends any other way.
    pub fn shutdown(&self) {
        let (done, wait) = std::sync::mpsc::channel();
        if self.shutdown.send(done).is_ok() {
            let _ = wait.recv_timeout(Duration::from_secs(2));
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.shutdown();
    }
}

enum Internal {
    Connected(session::Connection),
    AuthFailed(u64),
    Started {
        generation: u64,
        result: Result<(Stream, Arc<Mpv>), playback::StartError>,
    },
    /// The next song resolved, independently of optional player metadata.
    NextReady {
        generation: u64,
        /// Its queue entry.
        id: u64,
        video_id: String,
        stream: Stream,
    },
    Watch {
        generation: u64,
        info: WatchNext,
    },
    Queue {
        epoch: u64,
        result: Result<WatchNext, String>,
        preparation: playback::Preparation,
    },
    /// More queue: a long playlist's next page, or the autoplay radio.
    Extended {
        epoch: u64,
        tracks: Vec<Track>,
        then_play: bool,
        autoplay: bool,
    },
    /// A track failed twice; `online` says whether YouTube is reachable.
    Failed {
        generation: u64,
        title: String,
        error: String,
        online: bool,
    },
    /// The connection is back after a failure while offline.
    Online {
        generation: u64,
    },
    /// A song's player response (loudness, play tracking).
    Player {
        epoch: u64,
        video_id: String,
        info: Option<sound::PlayerInfo>,
    },
    /// The sleep timer's clock, for the timer `stamp`.
    SleepTick {
        stamp: u64,
    },
    /// Equalizer edits stopped (no newer one than `stamp`).
    EqualizerSettled {
        stamp: u64,
    },
    /// Audition and Smooth mixes: the volume clock, previews' streams.
    Deck(deck::Message),
}

/// The track queued in mpv behind the current one.
#[derive(Clone)]
struct Appended {
    /// Its queue entry.
    id: u64,
    format: String,
    /// mpv's playlist entry id.
    entry: i64,
    /// The loudness gain it was queued with.
    gain: Option<f64>,
}

struct Worker {
    client: Arc<Client>,
    resolver: Arc<Resolver>,
    paths: Paths,
    sink: Sink,
    internal_tx: mpsc::UnboundedSender<Internal>,
    internal_rx: Option<mpsc::UnboundedReceiver<Internal>>,
    /// Events of every mpv process, tagged with its serial.
    mpv_tx: mpsc::UnboundedSender<(u64, MpvEvent)>,
    mpv_rx: Option<mpsc::UnboundedReceiver<(u64, MpvEvent)>>,
    /// The main deck: the current song plays on it.
    mpv: Option<Arc<Mpv>>,
    last_connect: Option<Instant>,
    connect_epoch: u64,
    connecting: Option<tokio::task::AbortHandle>,
    /// No fresh playback can use the previous resolver session while checking.
    account_checking: bool,
    session_cookie: Option<session::CookieFile>,
    last_death: Option<Instant>,

    queue: queue::Queue,
    /// Position in the play order of the current track.
    pos: Option<usize>,
    appended: Option<Appended>,
    /// mpv's playlist entry id of the current track.
    current_entry: Option<i64>,
    /// Bumped whenever the current track changes.
    generation: u64,
    /// Bumped whenever a play request replaces the queue.
    epoch: u64,
    retried: bool,
    reported: bool,
    /// An autoplay radio fetch for the current epoch is in flight.
    extending: bool,
    /// Next was asked for while the radio was still loading.
    advance_pending: bool,
    /// A track failed while offline; it plays when the connection returns.
    waiting_for_network: bool,
    /// Account writes, made one at a time in the order asked (`account.rs`).
    account_writes: Option<mpsc::UnboundedSender<account::Write>>,
    state: Playback,
    last_emit: Instant,
    /// mpv's `pause` and `idle-active`: playing means neither.
    paused: bool,
    idle: bool,
    /// Where the current song starts when it next loads: a restored
    /// session's position, or a seek made before Play.
    resume_at: Option<f64>,
    /// The current song's resolve and the next song's prefetch, stopped
    /// when they no longer apply.
    resolving: Option<tokio::task::AbortHandle>,
    prefetching: Option<tokio::task::AbortHandle>,
    /// The next URL can arrive while the current URL/player is still starting.
    ready_next: Option<playback::NextStream>,
    /// Fetching a chosen playlist/radio and its continuations.
    queue_request: Option<tokio::task::AbortHandle>,
    /// Retained when loading is cancelled so Play retries the selected list.
    pending_target: Option<Target>,
    /// Until mpv reports playback-restart for the selected entry.
    starting: bool,
    buffering: bool,
    seeking: bool,
    /// When the current song was asked for, to log how long it took to start.
    asked: Instant,
    /// Loudness and play tracking from player responses, by video id.
    players: HashMap<String, sound::PlayerInfo>,
    player_requests: HashMap<String, (u64, tokio::task::AbortHandle)>,
    /// The `af` value every deck has.
    af: String,
    /// Bumped by every equalizer change.
    eq_stamp: u64,
    /// Bumped by every sleep timer change; its clock stops on a stale one.
    sleep_stamp: Arc<AtomicU64>,
    /// The sleep timer's fade: the share of the volume playing (1 = none).
    fade: f64,
    last_save: Instant,
    /// The other decks: Smooth mixes and Audition.
    decks: deck::Decks,
}

impl Worker {
    fn new(client: Arc<Client>, resolver: Arc<Resolver>, paths: Paths, sink: Sink) -> Self {
        let (internal_tx, internal_rx) = mpsc::unbounded_channel();
        let (mpv_tx, mpv_rx) = mpsc::unbounded_channel();
        let mut settings = crate::settings::Settings::load(&paths);
        if cfg!(feature = "menubar") {
            settings.mixes.on = false;
        }
        Self {
            client,
            resolver,
            paths,
            sink,
            internal_tx,
            internal_rx: Some(internal_rx),
            mpv_tx,
            mpv_rx: Some(mpv_rx),
            mpv: None,
            last_connect: None,
            connect_epoch: 0,
            connecting: None,
            account_checking: false,
            session_cookie: None,
            last_death: None,
            queue: queue::Queue::default(),
            pos: None,
            appended: None,
            current_entry: None,
            generation: 0,
            epoch: 0,
            retried: false,
            reported: false,
            extending: false,
            advance_pending: false,
            waiting_for_network: false,
            account_writes: None,
            state: Playback {
                volume: 100.0,
                autoplay: true,
                normalize: settings.normalizes(),
                equalizer: settings.equalizer,
                mixes: settings.mixes,
                ..Playback::default()
            },
            last_emit: Instant::now(),
            paused: false,
            idle: true,
            resume_at: None,
            resolving: None,
            prefetching: None,
            ready_next: None,
            queue_request: None,
            pending_target: None,
            starting: false,
            buffering: false,
            seeking: false,
            asked: Instant::now(),
            players: HashMap::new(),
            player_requests: HashMap::new(),
            af: String::new(),
            eq_stamp: 0,
            sleep_stamp: Arc::default(),
            fade: 1.0,
            last_save: Instant::now(),
            decks: deck::Decks::new(settings.mixes),
        }
    }

    async fn run(
        mut self,
        mut commands: mpsc::UnboundedReceiver<Command>,
        mut shutdown: mpsc::UnboundedReceiver<std::sync::mpsc::Sender<()>>,
    ) {
        let mut internal = self.internal_rx.take().expect("internal receiver");
        let mut mpv_events = self.mpv_rx.take().expect("mpv receiver");
        self.restore_session();
        self.connect().await;
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => self.command(command).await,
                    None => break,
                },
                Some(message) = internal.recv() => self.internal(message).await,
                Some((serial, event)) = mpv_events.recv() => self.deck_event(serial, event).await,
                Some(done) = shutdown.recv() => {
                    self.save_session(true);
                    let _ = done.send(());
                    break;
                }
            }
        }
    }

    // ---- commands ----

    async fn command(&mut self, command: Command) {
        match command {
            Command::Page { target, seq } => self.load_page(target, seq),
            Command::More {
                key,
                token,
                search,
                shelf,
            } => {
                let current = self.client.clone();
                let client = current.snapshot();
                let epoch = client.session_epoch();
                let sink = self.sink.clone();
                let internal = self.internal_tx.clone();
                tokio::spawn(async move {
                    let result = if search {
                        client.search_continuation(&token).await
                    } else {
                        client.continuation(&token).await
                    };
                    if matches!(result, Err(ApiError::Auth)) {
                        let _ = internal.send(Internal::AuthFailed(epoch));
                    }
                    let result = result.map(|v| parse::more(&v)).map_err(|e| e.to_string());
                    current.if_current(epoch, || {
                        sink.send(Event::More {
                            key,
                            shelf,
                            token,
                            result,
                        })
                    });
                });
            }
            Command::Suggest(input) => {
                let current = self.client.clone();
                let client = current.snapshot();
                let epoch = client.session_epoch();
                let sink = self.sink.clone();
                tokio::spawn(async move {
                    if let Ok(value) = client.suggestions(&input).await {
                        current.if_current(epoch, || {
                            sink.send(Event::Suggestions {
                                items: parse::suggestions(&value),
                                input,
                            })
                        });
                    }
                });
            }
            Command::Lyrics {
                track,
                browse_id,
                duration,
            } => {
                let client = self.client.clone();
                let sink = self.sink.clone();
                tokio::spawn(async move {
                    let result = crate::lyrics::fetch(&client, &track, browse_id, duration).await;
                    sink.send(Event::Lyrics {
                        id: track.video_id,
                        result,
                    });
                });
            }
            Command::LoadSearches => {
                let path = self.paths.searches_file();
                let sink = self.sink.clone();
                tokio::spawn(async move {
                    sink.send(Event::Searches(crate::searches::load(&path).await));
                });
            }
            // Saved in order, here: a later list never lands before an earlier one.
            Command::SaveSearches(list) => {
                if let Err(error) = crate::searches::save(&self.paths.searches_file(), &list).await
                {
                    log::warn!("saving recent searches: {error}");
                }
            }
            Command::PlayTracks { tracks, start } => {
                if !self.fresh_playback_allowed() {
                    return;
                }
                self.new_epoch();
                self.set_queue(tracks, start);
                if let Some(pos) = self.pos {
                    self.start(pos).await;
                }
            }
            Command::PlayTarget(target) => self.play_target(target).await,
            Command::TogglePause => self.toggle_pause().await,
            Command::Next => self.next(false).await,
            Command::Previous => {
                if self.state.position > 3.0 || self.pos == Some(0) {
                    self.seek(0.0).await;
                } else if let Some(pos) = self.pos {
                    self.start(pos - 1).await;
                }
            }
            Command::Seek(seconds) => self.seek(seconds).await,
            Command::Volume(volume) => {
                self.state.volume = volume.clamp(0.0, 100.0);
                self.apply_volumes().await;
                self.emit(true);
                self.save_session(false);
            }
            Command::ToggleShuffle => {
                self.state.shuffle = !self.state.shuffle;
                let shuffle = self.state.shuffle;
                self.edit_queue(|queue, pos, _| {
                    if let Some(pos) = pos {
                        if shuffle {
                            queue.shuffle(pos);
                        } else {
                            queue.unshuffle(pos);
                        }
                    }
                })
                .await;
            }
            Command::CycleRepeat => {
                self.state.repeat = match self.state.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
                self.apply_loop().await;
                self.requeue_next().await;
                self.emit(true);
                self.save_session(true);
            }
            Command::Autoplay(on) => {
                self.state.autoplay = on;
                self.emit(true);
                self.save_session(true);
                self.maybe_extend();
            }
            Command::JumpTo(pos) => {
                if pos < self.queue.len() {
                    self.start(pos).await;
                }
            }
            Command::Reconnect => self.connect().await,
            Command::UseProfile(profile) => {
                let mut settings = crate::settings::Settings::load(&self.paths);
                settings.browser_profile = Some(profile);
                if let Err(error) = settings.save(&self.paths) {
                    self.sink.send(Event::Account(Account::Unverified {
                        reason: format!("Couldn't save the account choice: {error}"),
                    }));
                    return;
                }
                self.connect().await;
            }
            Command::Notifications(on) => {
                let mut settings = crate::settings::Settings::load(&self.paths);
                settings.notifications = on;
                if let Err(error) = settings.save(&self.paths) {
                    self.sink.send(Event::Error(format!(
                        "Couldn't save the notification setting: {error}"
                    )));
                }
            }
            Command::Heat(video_id) => {
                let http = self.client.http().clone();
                let sink = self.sink.clone();
                tokio::spawn(async move {
                    let heat = match crate::heat::fetch(&http, &video_id).await {
                        Ok(heat) => heat,
                        Err(error) => {
                            log::warn!("most replayed for {video_id}: {error}");
                            None
                        }
                    };
                    sink.send(Event::Heat { id: video_id, heat });
                });
            }
            Command::PaintCovers(on) => {
                let mut settings = crate::settings::Settings::load(&self.paths);
                settings.paint_covers = on;
                if let Err(error) = settings.save(&self.paths) {
                    self.sink.send(Event::Error(format!(
                        "Couldn't save the cover painting setting: {error}"
                    )));
                }
            }
            Command::AccountEdit { op, edit, refresh } => self.account_edit(op, edit, refresh),
            Command::LikeStatus(video_id) => self.like_status(video_id),
            Command::Prepare(video_id) => {
                if !self.account_checking {
                    self.resolver.prepare(&video_id);
                }
            }
            Command::PrepareMany(video_ids) => {
                if !self.account_checking {
                    self.resolver.prepare_many(video_ids);
                }
            }
            Command::PlayNext(tracks) => self.add(tracks, true).await,
            Command::AddToQueue(tracks) => self.add(tracks, false).await,
            Command::RemoveFromQueue(at) => {
                if self.pos != Some(at) {
                    self.edit_queue(|queue, _, _| {
                        queue.remove(at);
                    })
                    .await;
                }
            }
            Command::MoveInQueue { from, to } => {
                self.edit_queue(|queue, pos, shuffled| queue.move_entry(from, to, pos, shuffled))
                    .await;
            }
            Command::ClearUpcoming => {
                if self.pos.is_some() {
                    // Pages of the list and radio still on their way don't refill it.
                    self.epoch += 1;
                    self.extending = false;
                    self.advance_pending = false;
                    self.edit_queue(|queue, pos, _| {
                        if let Some(pos) = pos {
                            queue.clear_after(pos);
                        }
                    })
                    .await;
                }
            }
            Command::SleepTimer(choice) => self.set_sleep(choice).await,
            Command::Equalizer(equalizer) => self.set_equalizer(equalizer).await,
            Command::Normalize(on) => self.set_normalize(on).await,
            Command::Audition { track, start } => self.audition(track, start).await,
            Command::EndAudition => self.end_audition().await,
            Command::Mixes(mixes) => self.set_mixes(mixes).await,
            #[cfg(feature = "e2e")]
            Command::SampleDecks(on) => self.sample_decks(on),
            Command::QuickSearch(query) => {
                let current = self.client.clone();
                let client = current.snapshot();
                let epoch = client.session_epoch();
                let sink = self.sink.clone();
                tokio::spawn(async move {
                    let result = client
                        .search(&query, None)
                        .await
                        .map(|v| Box::new(parse::page(&v)))
                        .map_err(|e| e.to_string());
                    current.if_current(epoch, || sink.send(Event::QuickResults { query, result }));
                });
            }
        }
    }

    /// Sends the playback state; position-only updates at most four times a second.
    fn emit(&mut self, always: bool) {
        if !always && self.last_emit.elapsed() < Duration::from_millis(250) {
            return;
        }
        self.last_emit = Instant::now();
        self.state.next_ready = self.appended.is_some() || self.decks.cued.is_some();
        self.sink.send(Event::Playback(self.state.clone()));
    }
}
