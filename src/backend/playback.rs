use super::*;

pub(super) enum StartError {
    Player(anyhow::Error),
    Stream(anyhow::Error),
}

/// Work a user selection can start before its authoritative queue arrives.
/// The resolver share and player task stay owned across the queue handoff;
/// cancelling or replacing that selection drops both without leaving work
/// running in the background.
pub(super) struct Preparation {
    asked: Instant,
    stream: Option<(String, resolver::Request)>,
    player: PreparingPlayer,
    source: Option<String>,
    collection: bool,
}

enum PreparingPlayer {
    Ready(Arc<Mpv>),
    Starting(tokio::task::JoinHandle<anyhow::Result<Arc<Mpv>>>),
}

impl PreparingPlayer {
    async fn wait(&mut self) -> anyhow::Result<Arc<Mpv>> {
        match self {
            Self::Ready(mpv) => Ok(mpv.clone()),
            Self::Starting(task) => task
                .await
                .map_err(|error| anyhow::anyhow!("audio player preparation stopped: {error}"))?,
        }
    }
}

impl Drop for PreparingPlayer {
    fn drop(&mut self) {
        if let Self::Starting(task) = self {
            task.abort();
        }
    }
}

pub(super) struct NextStream {
    id: u64,
    video_id: String,
    stream: Stream,
}

impl super::Worker {
    pub(super) fn fresh_playback_allowed(&self) -> bool {
        if self.account_checking {
            self.sink.send(Event::Error(
                "Connecting… Try playback when the account is ready.".into(),
            ));
            false
        } else {
            true
        }
    }

    /// Checking suspends fresh playback without exposing the old session as a
    /// guest or destroying its scoped URL cache. The loaded current entry is
    /// the only stream allowed to remain in mpv.
    pub(super) async fn begin_account_check(&mut self) {
        self.account_checking = true;
        let pending_start = self.starting || self.waiting_for_network;
        self.new_epoch();
        self.generation += 1;
        self.cancel_resolution();
        for (_, (_, task)) in self.player_requests.drain() {
            task.abort();
        }
        self.ready_next = None;
        self.appended = None;
        self.finish_blend().await;
        self.drop_cued().await;
        if self.state.audition.is_some() {
            self.end_audition().await;
        }
        let mut keep_current = !pending_start && self.current_entry.is_some() && !self.idle;
        if let Some(mpv) = &self.mpv {
            if keep_current {
                // Atomic playlist clearing cannot advance to a removed next
                // entry, unlike removing index 1 if EOF already selected it.
                let cleared = mpv.command(json!(["playlist-clear"])).await.is_ok();
                let kept = if cleared {
                    mpv.get("playlist/0/id")
                        .await
                        .ok()
                        .and_then(|value| value.as_i64())
                } else {
                    None
                };
                keep_current = kept == self.current_entry;
            }
            if !keep_current {
                let _ = mpv.command(json!(["stop"])).await;
            }
        }
        self.starting = false;
        if !keep_current {
            self.current_entry = None;
            self.buffering = false;
            self.seeking = false;
            self.idle = true;
            self.paused = true;
            self.resume_at = Some(self.state.position);
        }
        self.update_transport();
    }

    /// A play request replaces the queue: results of earlier ones no longer apply.
    pub(super) fn new_epoch(&mut self) -> u64 {
        self.epoch += 1;
        if let Some(task) = self.queue_request.take() {
            task.abort();
        }
        self.pending_target = None;
        self.extending = false;
        self.advance_pending = false;
        self.waiting_for_network = false;
        self.decks.radio = false;
        self.decks.autoplay.clear();
        self.epoch
    }

    /// Fetch a chosen list without letting an earlier pending song win while
    /// the request is in flight. The selected target survives a loading Pause.
    pub(super) async fn play_target(&mut self, target: Target) {
        let collection = matches!(
            &target,
            Target::Watch {
                video_id: None,
                playlist_id: Some(_),
                ..
            }
        );
        self.play_selection(target, None, collection).await;
    }

    pub(super) async fn play_selection(
        &mut self,
        target: Target,
        source: Option<String>,
        collection: bool,
    ) {
        if !self.fresh_playback_allowed() {
            return;
        }
        // A song target already identifies its stream. Start it and the one
        // audio player alongside watch-next; neither depends on the queue's
        // metadata. Take a resolver share before cancelling the prior queue
        // or next lookup, so another click keeps work already in flight.
        let video_id = match &target {
            Target::Watch { video_id, .. } if !(collection && self.state.shuffle) => {
                video_id.as_deref()
            }
            _ => None,
        };
        let mut preparation = self.prepare_start(video_id);
        preparation.source = source;
        preparation.collection = collection;
        let epoch = self.new_epoch();
        self.generation += 1;
        self.cancel_resolution();
        self.current_entry = None;
        self.appended = None;
        self.ready_next = None;
        self.starting = true;
        self.buffering = false;
        self.seeking = false;
        self.paused = false;
        self.idle = true;
        self.decks.radio = deck::is_radio(&target);
        self.pending_target = Some(target.clone());
        self.update_transport();
        let current = self.client.clone();
        let client = Arc::new(current.snapshot());
        let session_epoch = client.session_epoch();
        let tx = self.internal_tx.clone();
        let task = tokio::spawn(async move {
            let result = client.next(&target).await.map(|v| parse::watch_next(&v));
            if current.session_epoch() != session_epoch {
                return;
            }
            if matches!(result, Err(ApiError::Auth)) {
                let _ = tx.send(Internal::AuthFailed(session_epoch));
            }
            let result = result.map_err(|e| e.to_string()).and_then(|info| {
                if info.tracks.is_empty() {
                    Err("Nothing to play here".to_owned())
                } else {
                    Ok(info)
                }
            });
            let mut token = result
                .as_ref()
                .ok()
                .and_then(|info| info.continuation.clone());
            let mut total = result.as_ref().map(|info| info.tracks.len()).unwrap_or(0);
            let _ = tx.send(Internal::Queue {
                epoch,
                result,
                preparation,
            });
            while let Some(t) = token.take().filter(|_| total < 500) {
                let Ok(value) = client.next_continuation(&t).await else {
                    break;
                };
                if current.session_epoch() != session_epoch {
                    break;
                }
                let more = parse::watch_next(&value);
                if more.tracks.is_empty() {
                    break;
                }
                total += more.tracks.len();
                token = more.continuation;
                if tx
                    .send(Internal::Extended {
                        epoch,
                        tracks: more.tracks,
                        then_play: false,
                        autoplay: false,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        self.queue_request = Some(task.abort_handle());
        self.finish_blend().await;
        self.drop_cued().await;
        if let Some(mpv) = &self.mpv {
            let _ = mpv.command(json!(["stop"])).await;
        }
    }

    /// Pause remains useful before audio starts: cancel the pending work and
    /// leave the selection/seek position ready for a later Play.
    pub(super) async fn toggle_pause(&mut self) {
        if self.account_checking && (self.current_entry.is_none() || self.idle) {
            self.fresh_playback_allowed();
            return;
        }
        if self.starting || self.waiting_for_network {
            self.cancel_resolution();
            if let Some(task) = self.queue_request.take() {
                task.abort();
            }
            self.epoch += 1;
            self.generation += 1;
            self.extending = false;
            self.advance_pending = false;
            self.waiting_for_network = false;
            self.starting = false;
            self.buffering = false;
            self.seeking = false;
            self.ready_next = None;
            self.current_entry = None;
            self.appended = None;
            self.idle = true;
            self.paused = true;
            if self.pending_target.is_none() {
                self.resume_at = Some(self.state.position);
            }
            self.update_transport();
            if let Some(mpv) = &self.mpv {
                let _ = mpv.command(json!(["stop"])).await;
            }
            self.save_session(true);
        } else if let Some(target) = self.pending_target.clone() {
            self.play_target(target).await;
        } else if !self.idle
            && let (Some(mpv), Some(_)) = (self.mpv.clone(), self.current_entry)
        {
            let paused = !self.paused;
            if paused {
                self.finish_blend().await;
            }
            // Publish the command intent immediately; mpv confirms or reports
            // an error without an inert transport control.
            self.paused = paused;
            self.update_transport();
            if let Err(error) = mpv.set("pause", json!(paused)).await {
                self.sink.send(Event::Error(format!(
                    "Couldn't {} playback: {error}",
                    if paused { "pause" } else { "resume" }
                )));
            }
            self.save_session(true);
        } else if let Some(pos) = self.pos {
            let at = self.resume_at.take();
            self.start_at(pos, at).await;
        }
    }

    fn cancel_resolution(&mut self) {
        if let Some(task) = self.resolving.take() {
            task.abort();
        }
        if let Some(task) = self.prefetching.take() {
            task.abort();
        }
    }

    /// One composition point for asynchronous preparation and mpv's transport
    /// properties. An old "not buffering" event cannot hide a new lookup.
    pub(super) fn update_transport(&mut self) {
        self.state.loading = self.starting
            || self.waiting_for_network
            || (!self.paused && (self.buffering || self.seeking));
        self.state.playing =
            self.current_entry.is_some() && !self.paused && !self.idle && !self.state.loading;
        self.emit(true);
    }

    /// A verified browser/account changed. Keep already playing audio local,
    /// but cancel pending selections and remove account-derived follow-up work.
    pub(super) async fn account_changed(&mut self) {
        let preparing = self.starting && self.current_entry.is_none();
        self.new_epoch();
        self.generation += 1;
        self.cancel_resolution();
        // Audio already heard under the previous account must not create a
        // listening-history write on the newly selected account.
        if self.current_entry.is_some() {
            self.reported = true;
        }
        self.players.clear();
        for (_, (_, task)) in self.player_requests.drain() {
            task.abort();
        }
        self.drop_appended().await;
        self.waiting_for_network = false;
        if preparing {
            self.starting = false;
            self.paused = true;
            self.update_transport();
            self.sink.send(Event::Error(
                "Account changed. Press Play to use the selected account.".into(),
            ));
        }
    }

    /// A new list as the queue, `start` (a list index) current; shuffled,
    /// it plays first and the rest at random.
    pub(super) fn set_queue(&mut self, tracks: Vec<Track>, start: usize) {
        self.queue.replace(tracks);
        self.pos = None;
        if !self.queue.is_empty() {
            let start = start.min(self.queue.len() - 1);
            self.pos = Some(if self.state.shuffle {
                self.queue.shuffle(start)
            } else {
                start
            });
        }
        self.send_queue();
    }

    pub(super) fn send_queue(&mut self) {
        self.sink.send(Event::Queue(self.queue.tracks()));
        self.save_session(true);
    }

    pub(super) fn track_at(&self, pos: usize) -> Option<&Track> {
        self.queue.track(pos)
    }

    /// The song playing, or ready to play.
    pub(super) fn current(&self) -> Option<&Track> {
        self.pos.and_then(|p| self.queue.track(p))
    }

    /// Applies a queue edit. The current song stays current wherever it
    /// moves; if the song after it changed, the one queued in mpv behind
    /// it is dropped and the new next one is prepared and queued instead.
    /// An edit never fetches autoplay's radio: songs removed or cleared
    /// stay gone, and autoplay continues when the last song ends
    /// ([`Self::next`]), as in YouTube Music.
    pub(super) async fn edit_queue(
        &mut self,
        edit: impl FnOnce(&mut queue::Queue, Option<usize>, bool),
    ) {
        let current = self.pos.and_then(|p| self.queue.id(p));
        let next = self.pos.and_then(|p| self.queue.id(p + 1));
        edit(&mut self.queue, self.pos, self.state.shuffle);
        self.pos = current.and_then(|id| self.queue.position(id));
        self.state.index = self.pos;
        let new_next = self.pos.and_then(|p| self.queue.id(p + 1));
        if new_next != next {
            self.drop_appended().await;
            self.prefetch();
        }
        self.send_queue();
        self.emit(true);
    }

    /// Play next (`next`) or Add to queue. With nothing to play yet, the
    /// songs become the queue and start.
    pub(super) async fn add(&mut self, tracks: Vec<Track>, next: bool) {
        if tracks.is_empty() {
            return;
        }
        if self.pos.is_none() {
            self.new_epoch();
            self.set_queue(tracks, 0);
            if let Some(pos) = self.pos {
                self.start(pos).await;
            }
            return;
        }
        self.edit_queue(|queue, pos, _| {
            if next {
                queue.play_next(pos, tracks);
            } else {
                queue.add_to_queue(pos, tracks);
            }
        })
        .await;
    }

    /// Starts the track at play-order position `pos`.
    pub(super) async fn start(&mut self, pos: usize) {
        self.start_at(pos, None).await;
    }

    /// Starts the track at `pos`, from `at` seconds in.
    pub(super) async fn start_at(&mut self, pos: usize, at: Option<f64>) {
        self.start_prepared(pos, at, None).await;
    }

    async fn start_prepared(
        &mut self,
        pos: usize,
        at: Option<f64>,
        preparation: Option<Preparation>,
    ) {
        if !self.fresh_playback_allowed() {
            return;
        }
        let Some(track) = self.track_at(pos).cloned() else {
            return;
        };
        let needs_stop = self.current_entry.is_some() || !self.idle;
        if self.pending_target.is_some() {
            self.new_epoch();
        }
        self.generation += 1;
        let preparation = preparation.unwrap_or_else(|| self.prepare_start(Some(&track.video_id)));
        self.pos = Some(pos);
        self.queue.reached(pos);
        self.current_entry = None;
        self.appended = None;
        self.ready_next = None;
        self.starting = true;
        self.buffering = false;
        self.seeking = false;
        self.paused = false;
        self.idle = true;
        // A blend in progress ends, and a song cued on the second deck goes.
        self.finish_blend().await;
        self.drop_cued().await;
        self.retried = false;
        self.reported = false;
        self.waiting_for_network = false;
        self.resume_at = at;
        self.asked = preparation.asked;
        self.state.index = Some(pos);
        self.state.loading = true;
        self.state.playing = false;
        self.state.position = at.unwrap_or(0.0);
        self.state.duration = track.duration.map(f64::from).unwrap_or(0.0);
        self.state.format = None;
        self.state.gain = None;
        self.state.lyrics = None;
        self.state.related = None;
        self.emit(true);
        if needs_stop && let Some(mpv) = &self.mpv {
            // A queue handoff has already stopped the old file. Do not pay a
            // second serial IPC acknowledgment before loading its new audio.
            let _ = mpv.command(json!(["stop"])).await;
        }
        if self.sleeping_at_song_end() {
            // The timer now waits for this song's end.
            self.restore_fade().await;
        }
        let next_video = self.track_at(pos + 1).map(|t| t.video_id.clone());
        self.player_requests.retain(|id, (_, task)| {
            let keep = *id == track.video_id || Some(id) == next_video.as_ref();
            if !keep {
                task.abort();
            }
            keep
        });
        self.resolve_prepared(&track.video_id, preparation);
        // Reuse a ready successor; defer a cold next lookup until the current
        // load is accepted so first playback does not compete with another resolver.
        self.prefetch();
        self.fetch_watch_info(&track.video_id);
        self.fetch_player(&track.video_id);
        self.maybe_extend();
        self.save_session(true);
    }

    /// Resolves the current song for playback. Its share of the run is
    /// taken before the previous song's resolve and the old prefetch are
    /// stopped, so a song that was next keeps resolving as it becomes current.
    pub(super) fn resolve_current(&mut self, video_id: &str) {
        if !self.fresh_playback_allowed() {
            self.starting = false;
            self.update_transport();
            return;
        }
        let preparation = self.prepare_start(Some(video_id));
        self.resolve_prepared(video_id, preparation);
    }

    fn prepare_start(&self, video_id: Option<&str>) -> Preparation {
        let asked = Instant::now();
        let stream = video_id
            .filter(|id| !id.is_empty())
            .map(|id| (id.to_owned(), self.resolver.request(id)));
        let player = match &self.mpv {
            Some(mpv) => PreparingPlayer::Ready(mpv.clone()),
            None => {
                // A retried selection keeps its generation. Its cancelled
                // child may still be exiting, so each startup owns a socket.
                let socket = self
                    .paths
                    .runtime
                    .join(format!("mpv-{:016x}.sock", fastrand::u64(..)));
                let volume = self.main_volume();
                let events = self.mpv_tx.clone();
                PreparingPlayer::Starting(tokio::spawn(async move {
                    Mpv::spawn(&socket, volume, events).await
                }))
            }
        };
        Preparation {
            asked,
            stream,
            player,
            source: None,
            collection: false,
        }
    }

    fn resolve_prepared(&mut self, video_id: &str, preparation: Preparation) {
        if !self.fresh_playback_allowed() {
            self.starting = false;
            self.update_transport();
            return;
        }
        let generation = self.generation;
        let Preparation {
            stream, mut player, ..
        } = preparation;
        // watch-next chooses the queue/current item. A target's suggested id
        // may differ (unavailable song, playlist, mix); its URL must never be
        // loaded under that other song's metadata.
        let request = match stream {
            Some((id, request)) if id == video_id => request,
            _ => self.resolver.request(video_id),
        };
        self.starting = true;
        self.state.loading = true;
        let tx = self.internal_tx.clone();
        let task = tokio::spawn(async move {
            // Process startup and network resolution are independent. Neither
            // blocks transport/queue commands on the worker's event loop.
            let result = tokio::try_join!(
                async { request.wait().await.map_err(StartError::Stream) },
                async { player.wait().await.map_err(StartError::Player) }
            );
            let _ = tx.send(Internal::Started { generation, result });
        });
        if let Some(old) = self.resolving.replace(task.abort_handle()) {
            old.abort();
        }
        if let Some(old) = self.prefetching.take() {
            old.abort();
        }
    }

    pub(super) fn fetch_watch_info(&self, video_id: &str) {
        if self.account_checking || cfg!(feature = "menubar") {
            // Lyrics, related music and this response's like status have no
            // native consumer. Do not fetch a full radio solely to discard it.
            return;
        }
        let generation = self.generation;
        let current = self.client.clone();
        let client = Arc::new(current.snapshot());
        let session_epoch = client.session_epoch();
        let tx = self.internal_tx.clone();
        let target = Target::Watch {
            video_id: Some(video_id.to_owned()),
            playlist_id: None,
            params: None,
        };
        tokio::spawn(async move {
            if let Ok(value) = client.next(&target).await {
                if current.session_epoch() != session_epoch {
                    return;
                }
                let _ = tx.send(Internal::Watch {
                    generation,
                    info: parse::watch_next(&value),
                });
            }
        });
    }

    /// Resolve the next song at once. Its optional loudness/history request
    /// runs independently so it cannot delay a ready stream's handoff.
    pub(super) fn prefetch(&mut self) {
        // Current audio gets the CPU/network first. The existing accepted-load
        // callback resumes this same prefetch path; cached successors and quick
        // skips still share their in-flight resolver request.
        if self.account_checking
            || (cfg!(feature = "menubar") && self.starting && self.current_entry.is_none())
        {
            return;
        }
        let Some(pos) = self.pos else { return };
        if let Some(after) = self.track_at(pos + 2) {
            let id = after.video_id.clone();
            self.resolver.prepare(&id);
        }
        // With the sleep timer at the song's end, nothing follows in mpv.
        if self.sleeping_at_song_end() {
            return;
        }
        let Some(entry) = self.queue.get(pos + 1) else {
            return;
        };
        if self.appended.as_ref().is_some_and(|a| a.id == entry.id)
            || self
                .ready_next
                .as_ref()
                .is_some_and(|next| next.id == entry.id)
            || self
                .prefetching
                .as_ref()
                .is_some_and(|task| !task.is_finished())
            || self
                .decks
                .cued
                .as_ref()
                .is_some_and(|c| c.next.id == entry.id)
        {
            return;
        }
        let (id, video_id) = (entry.id, entry.track.video_id.clone());
        let generation = self.generation;
        let request = self.resolver.request(&video_id);
        self.fetch_player(&video_id);
        let tx = self.internal_tx.clone();
        let task = tokio::spawn(async move {
            match request.wait().await {
                Ok(stream) => {
                    let _ = tx.send(Internal::NextReady {
                        generation,
                        id,
                        video_id,
                        stream,
                    });
                }
                Err(error) => log::warn!("resolving the next track failed: {error:#}"),
            }
        });
        if let Some(old) = self.prefetching.replace(task.abort_handle()) {
            old.abort();
        }
    }

    pub(super) async fn next(&mut self, automatic: bool) {
        if self.account_checking {
            if automatic {
                self.current_entry = None;
                self.idle = true;
                self.starting = false;
                self.buffering = false;
                self.seeking = false;
                self.update_transport();
            } else {
                self.fresh_playback_allowed();
            }
            return;
        }
        let Some(pos) = self.pos else { return };
        if self.decks.blending() {
            if !automatic {
                // A manual Next during a blend completes it at once.
                self.finish_blend().await;
                return;
            }
            // The song blended into ended or failed during the blend: the
            // old one stops before the queue moves on.
            self.stop_tail().await;
        }
        if automatic && self.sleeping_at_song_end() {
            self.sleep_after_song().await;
            return;
        }
        if pos + 1 < self.queue.len() {
            if let Some(cued) = &self.decks.cued
                && self.queue.position(cued.next.id) == Some(pos + 1)
            {
                self.swap(0.0).await;
                return;
            }
            if let (Some(appended), Some(mpv), false) =
                (self.appended.clone(), self.mpv.clone(), self.idle)
                && self.queue.position(appended.id) == Some(pos + 1)
            {
                self.starting = true;
                self.state.loading = true;
                self.state.playing = false;
                self.emit(true);
                // Select the known entry absolutely: an EOF already queued
                // on the event channel must not turn one Next into two skips.
                match mpv.set("playlist-pos", json!(1)).await {
                    Ok(_) => {
                        self.appended = None;
                        self.current_entry = Some(appended.entry);
                        let _ = mpv.command(json!(["playlist-remove", 0])).await;
                        self.advanced(appended).await;
                    }
                    Err(error) => {
                        self.sink
                            .send(Event::Error(format!("Couldn't skip the song: {error}")));
                        self.start(pos + 1).await;
                    }
                }
                return;
            }
            self.start(pos + 1).await;
        } else if self.state.repeat == Repeat::All && !self.queue.is_empty() {
            self.start(0).await;
        } else if self.state.autoplay {
            // Continue with radio for the last track; play it as soon as it arrives.
            self.starting = true;
            self.update_transport();
            if self.extending {
                self.advance_pending = true;
            } else {
                self.extend(true);
            }
        } else if automatic {
            self.starting = false;
            self.current_entry = None;
            self.idle = true;
            self.state.playing = false;
            self.state.loading = false;
            self.emit(true);
        }
    }

    pub(super) async fn seek(&mut self, seconds: f64) {
        let seconds = if self.state.duration > 0.0 {
            seconds.clamp(0.0, self.state.duration)
        } else {
            seconds.max(0.0)
        };
        self.finish_blend().await;
        if let (Some(mpv), Some(_)) = (&self.mpv, self.current_entry) {
            let _ = mpv.command(json!(["seek", seconds, "absolute"])).await;
            self.state.position = seconds;
            self.emit(true);
            self.save_session(true);
        } else if self.pos.is_some() {
            // Nothing loaded yet (a restored session): Play starts here.
            self.resume_at = Some(seconds);
            self.state.position = seconds;
            self.emit(true);
            self.save_session(true);
        }
    }

    pub(super) async fn drop_appended(&mut self) {
        self.ready_next = None;
        if let Some(task) = self.prefetching.take() {
            task.abort();
        }
        if self.appended.take().is_some()
            && let Some(mpv) = &self.mpv
        {
            let _ = mpv.command(json!(["playlist-remove", 1])).await;
        }
        self.drop_cued().await;
    }

    /// Autoplay: when the last track in the queue is playing, fetch a radio
    /// to follow it.
    pub(super) fn maybe_extend(&mut self) {
        if self.account_checking {
            return;
        }
        let Some(pos) = self.pos else { return };
        if self.state.autoplay && !self.extending && pos + 1 >= self.queue.len() {
            self.extend(false);
        }
    }

    /// Fetches YouTube Music's radio for the last track of the queue.
    fn extend(&mut self, then_play: bool) {
        let Some(last) = self.queue.last() else {
            return;
        };
        self.extending = true;
        let target = Target::Watch {
            video_id: Some(last.video_id.clone()),
            playlist_id: Some(format!("RDAMVM{}", last.video_id)),
            params: Some("wAEB".into()),
        };
        let known = self.queue.video_ids();
        let current = self.client.clone();
        let client = Arc::new(current.snapshot());
        let session_epoch = client.session_epoch();
        let tx = self.internal_tx.clone();
        let epoch = self.epoch;
        tokio::spawn(async move {
            let tracks = match client.next(&target).await {
                Ok(value) => parse::watch_next(&value)
                    .tracks
                    .into_iter()
                    .filter(|t| !known.contains(&t.video_id))
                    .collect(),
                Err(error) => {
                    log::warn!("autoplay radio failed: {error}");
                    Vec::new()
                }
            };
            if current.session_epoch() != session_epoch {
                return;
            }
            let _ = tx.send(Internal::Extended {
                epoch,
                tracks,
                then_play,
                autoplay: true,
            });
        });
    }

    pub(super) async fn internal(&mut self, message: Internal) {
        match message {
            Internal::Connected(result) => self.connected(result).await,
            Internal::AuthFailed(epoch) => self.auth_failed(epoch).await,
            Internal::Started { generation, result } => {
                if generation != self.generation {
                    return;
                }
                self.resolving = None;
                let Some(track) = self.current().cloned() else {
                    return;
                };
                match result {
                    Ok((stream, mpv)) => {
                        log::debug!(
                            "playback stream/player ready {:.3}s after selection",
                            self.asked.elapsed().as_secs_f64()
                        );
                        if self
                            .mpv
                            .as_ref()
                            .is_none_or(|old| old.serial() != mpv.serial())
                        {
                            self.mpv = Some(mpv.clone());
                            self.apply_loop().await;
                            self.apply_equalizer(&mpv).await;
                            // Volume may have changed while queue/stream work
                            // ran and this player was not yet the active one.
                            self.apply_volumes().await;
                        }
                        self.appended = None;
                        let (options, gain) =
                            self.file_options(&track.video_id, &stream, self.resume_at);
                        match mpv.load(&stream.url, "replace", &options).await {
                            Ok(entry) => {
                                log::debug!(
                                    "playback load accepted {:.3}s after selection",
                                    self.asked.elapsed().as_secs_f64()
                                );
                                self.resume_at = None;
                                self.current_entry = Some(entry);
                                let _ = mpv.set("pause", json!(false)).await;
                                self.state.format = Some(stream.description());
                                self.state.gain = gain;
                                self.emit(true);
                                if let Some(next) = self.ready_next.take() {
                                    self.queue_next(next).await;
                                } else {
                                    self.prefetch();
                                }
                                #[cfg(feature = "e2e")]
                                self.probe_gain();
                            }
                            Err(error) => self.fail(&track, &format!("{error:#}")).await,
                        }
                    }
                    Err(StartError::Player(error)) => {
                        self.starting = false;
                        self.update_transport();
                        self.sink.send(Event::Error(format!(
                            "Couldn't start the audio player: {error:#}"
                        )));
                    }
                    Err(StartError::Stream(error)) => {
                        self.fail(&track, &format!("{error:#}")).await
                    }
                }
            }
            Internal::NextReady {
                generation,
                id,
                video_id,
                stream,
            } => {
                if generation != self.generation {
                    return;
                }
                self.prefetching = None;
                self.queue_next(NextStream {
                    id,
                    video_id,
                    stream,
                })
                .await;
            }
            Internal::Watch { generation, info } => {
                if generation != self.generation {
                    return;
                }
                self.state.lyrics = info.lyrics;
                self.state.related = info.related;
                if let Some(like) = info.like {
                    self.sink.send(Event::Likes(vec![like]));
                }
                self.emit(true);
            }
            Internal::Queue {
                epoch,
                result,
                preparation,
            } => {
                if epoch != self.epoch {
                    return;
                }
                log::debug!(
                    "playback queue ready {:.3}s after selection",
                    preparation.asked.elapsed().as_secs_f64()
                );
                match result {
                    Ok(info) => {
                        // Shuffle of a whole collection should start randomly,
                        // not always at track one. An explicitly chosen song
                        // still starts exactly there; the existing Queue owns
                        // all subsequent order and reversible shuffling.
                        let start = collection_start(
                            self.state.shuffle,
                            preparation.collection,
                            info.current,
                            info.tracks.len(),
                        );
                        self.pending_target = None;
                        self.state.source = preparation.source.clone();
                        self.set_queue(info.tracks, start);
                        if let Some(pos) = self.pos {
                            self.start_prepared(pos, None, Some(preparation)).await;
                        }
                    }
                    Err(error) => {
                        self.starting = false;
                        self.update_transport();
                        self.sink
                            .send(Event::Error(format!("Couldn't start playback: {error}")));
                    }
                }
            }
            Internal::Extended {
                epoch,
                tracks,
                then_play,
                autoplay,
            } => {
                if epoch != self.epoch {
                    return;
                }
                if autoplay {
                    self.extending = false;
                }
                let play_now = then_play || (autoplay && std::mem::take(&mut self.advance_pending));
                let first_new = self.queue.len();
                let known = self.queue.video_ids();
                self.queue.extend(
                    tracks
                        .into_iter()
                        .filter(|t| !known.contains(&t.video_id))
                        .collect(),
                );
                if autoplay {
                    // Autoplay continues as a radio: its songs blend in.
                    let ids: Vec<u64> = (first_new..self.queue.len())
                        .filter_map(|p| self.queue.id(p))
                        .collect();
                    self.decks.autoplay.extend(ids);
                }
                self.send_queue();
                if play_now && first_new < self.queue.len() {
                    self.start(first_new).await;
                } else if play_now {
                    self.starting = false;
                    self.state.loading = false;
                    self.state.playing = false;
                    self.emit(true);
                } else if self.appended.is_none() && self.decks.cued.is_none() {
                    self.prefetch();
                }
            }
            Internal::Failed {
                generation,
                title,
                error,
                online,
            } => {
                if generation != self.generation {
                    return;
                }
                if online {
                    self.sink.send(Event::Error(format!(
                        "Couldn't play “{title}”, skipped it. {error}"
                    )));
                    self.next(true).await;
                } else {
                    // Offline: don't skip through the queue; play this song when the connection returns.
                    self.waiting_for_network = true;
                    self.state.loading = true;
                    self.emit(true);
                    self.sink.send(Event::Error(format!(
                        "No connection. “{title}” will play when it's back."
                    )));
                    let client = self.client.clone();
                    let tx = self.internal_tx.clone();
                    let task = tokio::spawn(async move {
                        loop {
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            if client.reachable().await {
                                let _ = tx.send(Internal::Online { generation });
                                return;
                            }
                            if tx.is_closed() {
                                return;
                            }
                        }
                    });
                    self.resolving = Some(task.abort_handle());
                }
            }
            Internal::Online { generation } => {
                if generation == self.generation
                    && self.waiting_for_network
                    && let Some(pos) = self.pos
                {
                    self.start(pos).await;
                }
            }
            Internal::Player {
                epoch,
                video_id,
                info,
            } => {
                if epoch != self.client.session_epoch() {
                    return;
                }
                self.player_requests.remove(&video_id);
                if let Some(info) = info {
                    self.player_arrived(video_id, info).await;
                }
            }
            Internal::SleepTick { stamp } => self.sleep_tick(stamp).await,
            Internal::EqualizerSettled { stamp } => self.equalizer_settled(stamp).await,
            Internal::Deck(message) => self.deck_message(message).await,
        }
    }

    async fn queue_next(&mut self, next: NextStream) {
        if self.account_checking {
            return;
        }
        let NextStream {
            id,
            video_id,
            stream,
        } = next;
        if self.pos.and_then(|p| self.queue.id(p + 1)) != Some(id)
            || self.appended.is_some()
            || self.decks.cued.is_some()
            || self.sleeping_at_song_end()
        {
            return;
        }
        if self.current_entry.is_none() {
            self.ready_next = Some(NextStream {
                id,
                video_id,
                stream,
            });
            return;
        }
        if self.blends_into(id) {
            if !self.decks.blending() {
                self.cue(id, &video_id, &stream).await;
            }
            return;
        }
        let (options, gain) = self.file_options(&video_id, &stream, None);
        if let Some(mpv) = &self.mpv
            && let Ok(entry) = mpv.load(&stream.url, "append", &options).await
        {
            self.appended = Some(Appended {
                id,
                format: stream.description(),
                entry,
                gain,
            });
            self.emit(true);
        }
    }

    /// A track failed: retry once with a freshly resolved stream; then skip,
    /// unless YouTube is unreachable, in which case wait for the connection.
    async fn fail(&mut self, track: &Track, error: &str) {
        log::warn!("playback failed: {error}");
        self.starting = true;
        self.state.loading = true;
        self.state.playing = false;
        self.emit(true);
        // A song that fails while it blends in takes the old one with it.
        self.stop_tail().await;
        if !self.retried {
            self.retried = true;
            self.resolver.forget(&track.video_id);
            self.resolve_current(&track.video_id);
            return;
        }
        let generation = self.generation;
        let client = self.client.clone();
        let tx = self.internal_tx.clone();
        let (title, error) = (track.title.clone(), error.to_owned());
        let task = tokio::spawn(async move {
            let online = client.reachable().await;
            let _ = tx.send(Internal::Failed {
                generation,
                title,
                error,
                online,
            });
        });
        self.resolving = Some(task.abort_handle());
    }

    /// Adds the current song to the account's history, with the tracking
    /// URL of the player response fetched when it started if there is one.
    fn report_play(&self) {
        let Some(track) = self.current() else { return };
        let client = Arc::new(self.client.snapshot());
        let id = track.video_id.clone();
        let tracking = self.players.get(&id).and_then(|p| p.tracking.clone());
        tokio::spawn(async move {
            let result = match tracking {
                Some(url) => client.ping_playback(&url).await,
                None => client.report_play(&id).await,
            };
            if let Err(error) = result {
                log::warn!("reporting a play failed: {error}");
            }
        });
    }

    /// The song after the current one (`next`, queued gapless in mpv or
    /// cued on the second deck) started and is now the current one.
    pub(super) async fn advanced(&mut self, next: Appended) {
        let Some(pos) = self.queue.position(next.id) else {
            // Removed from the queue as it started: move on (boxed: Next can
            // start a song cued on the second deck, which comes back here).
            Box::pin(self.next(true)).await;
            return;
        };
        self.generation += 1;
        self.starting = true;
        self.buffering = false;
        self.seeking = false;
        self.state.loading = true;
        self.state.playing = false;
        self.asked = Instant::now();
        self.pos = Some(pos);
        self.queue.reached(pos);
        self.retried = false;
        self.reported = false;
        let track = self.track_at(pos).cloned();
        self.state.index = Some(pos);
        self.state.position = 0.0;
        self.state.duration = track
            .as_ref()
            .and_then(|t| t.duration)
            .map(f64::from)
            .unwrap_or(0.0);
        // When the prefetched file opens at once, mpv reports its `duration`
        // in the same batch as, and before, the change of file (observed
        // order), where it was taken as the old song's: ask for it again.
        if let Some(mpv) = &self.mpv
            && let Ok(duration) = mpv.get("duration").await
            && let Some(duration) = duration.as_f64()
        {
            self.state.duration = duration;
        }
        self.state.format = Some(next.format);
        self.state.gain = next.gain;
        if let Some(video_id) = track.as_ref().map(|t| t.video_id.as_str())
            && let Some(gain) = self.gain_for(video_id)
            && self.state.gain != Some(gain)
        {
            self.apply_gain(gain).await;
        }
        self.state.lyrics = None;
        self.state.related = None;
        self.emit(true);
        if let Some(track) = track {
            self.fetch_watch_info(&track.video_id);
            self.fetch_player(&track.video_id);
        }
        self.prefetch();
        self.maybe_extend();
        self.save_session(true);
        #[cfg(feature = "e2e")]
        self.probe_gain();
    }

    pub(super) async fn mpv_event(&mut self, event: MpvEvent) {
        match event {
            MpvEvent::Property { entry, name, data } => {
                // IPC replies and events are asynchronous. Only the selected
                // entry can update position, buffering or transport state.
                if Some(entry) != self.current_entry {
                    return;
                }
                match name.as_str() {
                    "time-pos" => {
                        // Before the current file loads, positions belong to the previous one.
                        let (Some(position), Some(_)) = (data.as_f64(), self.current_entry) else {
                            return;
                        };
                        self.state.position = position;
                        if self.deck_position().await {
                            // The next song took over on the second deck.
                            return;
                        }
                        if !self.reported && position >= 10.0 {
                            self.reported = true;
                            self.report_play();
                        }
                        self.song_end_fade().await;
                        self.emit(false);
                        self.save_session(false);
                    }
                    "duration" => {
                        if let (Some(duration), Some(_)) = (data.as_f64(), self.current_entry) {
                            self.state.duration = duration;
                            self.emit(true);
                        }
                    }
                    "pause" => {
                        self.paused = data.as_bool() == Some(true);
                        self.update_transport();
                        if self.paused {
                            self.save_session(true);
                        }
                    }
                    "paused-for-cache" => {
                        self.buffering = data.as_bool() == Some(true);
                        self.update_transport();
                    }
                    "seeking" => {
                        self.seeking = data.as_bool() == Some(true);
                        self.update_transport();
                    }
                    "idle-active" => {
                        self.idle = data.as_bool() == Some(true);
                        self.update_transport();
                        if self.idle
                            && !self.state.loading
                            && self.appended.is_some()
                            && self.pos.is_some()
                        {
                            log::warn!("mpv went idle with a track thought queued; advancing");
                            self.appended = None;
                            self.next(true).await;
                        }
                    }
                    _ => {}
                }
            }
            MpvEvent::EndFile {
                reason,
                error,
                entry,
            } => {
                log::debug!(
                    "mpv end-file {entry} {reason} {error:?}; current {:?}, queued {:?}",
                    self.current_entry,
                    self.appended.as_ref().map(|a| a.entry)
                );
                // Events of replaced or queued entries are not about the current track.
                if Some(entry) != self.current_entry {
                    return;
                }
                match reason.as_str() {
                    "eof"
                        if self.appended.is_none()
                            && (self.state.repeat != Repeat::One
                                || self.sleeping_at_song_end()) =>
                    {
                        self.next(true).await
                    }
                    "error" => {
                        // Keep mpv from moving on to the queued track: this one is
                        // retried or skipped first.
                        self.drop_appended().await;
                        self.current_entry = None;
                        if let Some(track) = self.current().cloned() {
                            let error = error.unwrap_or_else(|| "the stream failed".into());
                            self.fail(&track, &error).await;
                        }
                    }
                    _ => {}
                }
            }
            MpvEvent::StartFile { entry } => {
                if self
                    .appended
                    .as_ref()
                    .is_some_and(|next| next.entry == entry)
                {
                    let next = self.appended.take().expect("matching appended entry");
                    self.current_entry = Some(entry);
                    if let Some(mpv) = &self.mpv {
                        let _ = mpv.command(json!(["playlist-remove", 0])).await;
                    }
                    self.stop_tail().await;
                    self.advanced(next).await;
                }
            }
            MpvEvent::PlaybackRestart { entry } => {
                if Some(entry) != self.current_entry {
                    return;
                }
                if self.starting {
                    log::info!(
                        "playback ready {:.3}s after selection (mpv restart)",
                        self.asked.elapsed().as_secs_f64()
                    );
                }
                self.starting = false;
                self.buffering = false;
                self.seeking = false;
                self.idle = false;
                self.update_transport();
            }
            MpvEvent::Died => {
                self.mpv = None;
                self.af.clear();
                self.appended = None;
                self.current_entry = None;
                self.idle = true;
                let was_playing = self.state.playing || self.starting;
                self.starting = false;
                self.buffering = false;
                self.seeking = false;
                self.state.playing = false;
                self.state.loading = false;
                self.emit(true);
                let recent = self
                    .last_death
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(30));
                self.last_death = Some(Instant::now());
                match (was_playing, recent, self.pos) {
                    (true, false, Some(pos)) => {
                        self.sink.send(Event::Error(
                            "The audio player stopped unexpectedly; restarting the song.".into(),
                        ));
                        self.start(pos).await;
                    }
                    (true, true, _) => {
                        self.sink.send(Event::Error(
                            "The audio player keeps stopping. Press Play to try again.".into(),
                        ));
                    }
                    _ => {}
                }
            }
        }
    }
}

fn collection_start(shuffle: bool, collection: bool, current: usize, count: usize) -> usize {
    if shuffle && collection && count > 0 {
        fastrand::usize(..count)
    } else {
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn worker() -> (super::super::Worker, Scratch) {
        let root =
            std::env::temp_dir().join(format!("ytfast-playback-{:032x}", fastrand::u128(..)));
        crate::paths::private_dir(&root).unwrap();
        let paths = Paths {
            config: root.clone(),
            cache: root.clone(),
            runtime: root.clone(),
        };
        let (tx, _) = std::sync::mpsc::channel();
        let (now, _) = tokio::sync::watch::channel(crate::desktop::Now::default());
        let sink = Sink {
            tx,
            wake: Arc::new(|| {}),
            now: Arc::new(now),
        };
        let resolver = Arc::new(Resolver::new(root.clone()));
        (
            super::super::Worker::new(Arc::new(Client::new()), resolver, paths, sink),
            Scratch(root),
        )
    }

    fn track(id: &str) -> Track {
        Track {
            video_id: id.into(),
            title: id.into(),
            artists: Vec::new(),
            album: None,
            thumbnail: None,
            duration: Some(180),
            like: None,
            set_video_id: None,
        }
    }

    #[test]
    fn collection_shuffle_keeps_explicit_songs_and_reversible_order() {
        // A page Play/Shuffle action denotes the whole collection even when
        // YouTube's target includes a representative video id. A song never does.
        assert_eq!(collection_start(false, true, 2, 6), 2);
        assert_eq!(collection_start(true, false, 2, 6), 2);
        assert_eq!(collection_start(true, true, 0, 0), 0);
        assert_eq!(collection_start(true, true, 0, 1), 0);
        let start = collection_start(true, true, 0, 6);
        assert!(start < 6);
        let mut queue = queue::Queue::default();
        queue.replace((0..6).map(|i| track(&i.to_string())).collect());
        let pos = queue.shuffle(start);
        let selected = queue.id(pos).unwrap();
        assert_eq!(queue.track(pos).unwrap().video_id, start.to_string());
        let restored = queue.unshuffle(pos);
        assert_eq!(queue.id(restored), Some(selected));
        assert_eq!(
            queue
                .tracks()
                .iter()
                .map(|t| t.video_id.clone())
                .collect::<Vec<_>>(),
            (0..6).map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }

    #[cfg(feature = "menubar")]
    #[tokio::test]
    async fn cold_start_defers_successor_and_session_retains_source() {
        let (mut worker, _dir) = worker();
        worker.state.source = Some("Fixture collection".into());
        worker.set_queue(vec![track("current"), track("successor")], 0);
        worker.starting = true;
        worker.current_entry = None;
        worker.prefetch();
        assert!(
            worker.prefetching.is_none(),
            "no second cold resolver before current load"
        );
        assert!(worker.player_requests.is_empty());
        worker.save_session(true);
        worker.state.source = None;
        worker.restore_session();
        assert_eq!(worker.state.source.as_deref(), Some("Fixture collection"));
        assert!(worker.mpv.is_none());
    }

    #[cfg(feature = "menubar")]
    fn fixture_stream(id: &str) -> Stream {
        Stream {
            itag: 774,
            url: format!("https://example.invalid/{id}"),
            user_agent: None,
            expires: resolver::now() + 3600,
            audio: None,
        }
    }

    #[cfg(feature = "menubar")]
    #[tokio::test]
    async fn queue_handoff_reuses_started_preparation_and_the_original_selection_clock() {
        let (mut worker, _dir) = worker();
        // Optional metadata is already known, so this test never contacts an
        // account or network. The IPC fixture is the only audio process owner.
        worker.state.autoplay = false;
        worker.remember_player("selected".into(), sound::PlayerInfo::default());
        let (player, mut commands) = Mpv::test_ipc(71);
        let prepared_player = player.clone();
        let (started, starting) = tokio::sync::oneshot::channel();
        let (ready, waiting) = tokio::sync::oneshot::channel();
        let asked = Instant::now();
        let preparation = Preparation {
            asked,
            source: Some("Fixture context".into()),
            collection: false,
            stream: Some((
                "selected".into(),
                resolver::Request::Ready(Ok(fixture_stream("selected"))),
            )),
            player: PreparingPlayer::Starting(tokio::spawn(async move {
                let _ = started.send(());
                waiting.await.unwrap();
                Ok(prepared_player)
            })),
        };
        starting.await.unwrap();
        worker
            .internal(Internal::Queue {
                epoch: worker.epoch,
                result: Ok(WatchNext {
                    tracks: vec![track("selected")],
                    ..Default::default()
                }),
                preparation,
            })
            .await;
        assert_eq!(
            worker.asked, asked,
            "queue time belongs to click-to-play time"
        );
        assert_eq!(worker.current().unwrap().video_id, "selected");
        assert_eq!(worker.state.source.as_deref(), Some("Fixture context"));
        assert!(worker.starting);
        assert!(worker.mpv.is_none(), "a second player must not be started");
        worker.state.volume = 26.0;
        ready.send(()).unwrap();
        let message = tokio::time::timeout(
            Duration::from_secs(1),
            worker.internal_rx.as_mut().unwrap().recv(),
        )
        .await
        .unwrap()
        .unwrap();
        let Internal::Started { generation, result } = message else {
            panic!("expected the prepared stream and player");
        };
        let Ok((stream, reused)) = result else {
            panic!("preparation must succeed without a resolver process");
        };
        assert!(Arc::ptr_eq(&player, &reused));
        assert_eq!(stream.url, fixture_stream("selected").url);
        worker
            .internal(Internal::Started {
                generation,
                result: Ok((stream, reused)),
            })
            .await;
        let mut loads = Vec::new();
        let mut volumes = Vec::new();
        while let Ok(command) = commands.try_recv() {
            if command[0] == "loadfile" {
                loads.push(command.clone());
            }
            if command[0] == "set_property" && command[1] == "volume" {
                volumes.push(command[2].clone());
            }
        }
        assert_eq!(loads.len(), 1);
        assert_eq!(loads[0][1], fixture_stream("selected").url);
        assert_eq!(volumes, vec![json!(26.0)]);
    }

    #[cfg(feature = "menubar")]
    #[tokio::test]
    async fn authoritative_queue_selection_cannot_adopt_a_different_prepared_stream() {
        let (mut worker, _dir) = worker();
        worker.state.autoplay = false;
        worker.remember_player("actual".into(), sound::PlayerInfo::default());
        let cached = json!({
            "scope": null,
            "streams": { "actual": {
                "itag": 774,
                "url": "https://example.invalid/actual",
                "user_agent": null,
                "expires": resolver::now() + 3600
            }}
        });
        std::fs::write(
            worker.paths.runtime.join("streams.json"),
            cached.to_string(),
        )
        .unwrap();
        worker.resolver = Arc::new(Resolver::new(worker.paths.runtime.clone()));
        let (player, _commands) = Mpv::test_ipc(71);
        worker
            .internal(Internal::Queue {
                epoch: worker.epoch,
                result: Ok(WatchNext {
                    tracks: vec![track("actual")],
                    ..Default::default()
                }),
                preparation: Preparation {
                    asked: Instant::now(),
                    source: None,
                    collection: false,
                    stream: Some((
                        "suggested".into(),
                        resolver::Request::Ready(Ok(fixture_stream("suggested"))),
                    )),
                    player: PreparingPlayer::Ready(player.clone()),
                },
            })
            .await;
        let message = tokio::time::timeout(
            Duration::from_secs(1),
            worker.internal_rx.as_mut().unwrap().recv(),
        )
        .await
        .unwrap()
        .unwrap();
        let Internal::Started {
            result: Ok((stream, reused)),
            ..
        } = message
        else {
            panic!("expected a cache-backed current stream");
        };
        assert_eq!(stream.url, "https://example.invalid/actual");
        assert!(Arc::ptr_eq(&player, &reused));
    }

    #[tokio::test]
    async fn abandoned_queues_drop_their_player_preparation() {
        for outcome in ["stale", "failed", "paused", "account-check"] {
            let (mut worker, _dir) = worker();
            let (started, starting) = tokio::sync::oneshot::channel();
            let (released, dropped) = tokio::sync::oneshot::channel();
            struct Release(Option<tokio::sync::oneshot::Sender<()>>);
            impl Drop for Release {
                fn drop(&mut self) {
                    let _ = self.0.take().unwrap().send(());
                }
            }
            let task = tokio::spawn(async move {
                let _released = Release(Some(released));
                let _ = started.send(());
                std::future::pending::<anyhow::Result<Arc<Mpv>>>().await
            });
            starting.await.unwrap();
            let preparation = Preparation {
                asked: Instant::now(),
                source: None,
                collection: false,
                stream: None,
                player: PreparingPlayer::Starting(task),
            };
            if matches!(outcome, "stale" | "failed") {
                worker
                    .internal(Internal::Queue {
                        epoch: worker.epoch + u64::from(outcome == "stale"),
                        result: Err("fixture queue failure".into()),
                        preparation,
                    })
                    .await;
            } else {
                // Matches the queue task's ownership when Pause/account check
                // aborts it before any queue response has been delivered.
                let request = tokio::spawn(async move {
                    let _preparation = preparation;
                    std::future::pending::<()>().await
                });
                worker.queue_request = Some(request.abort_handle());
                worker.starting = true;
                if outcome == "paused" {
                    worker.toggle_pause().await;
                } else {
                    worker.begin_account_check().await;
                }
                assert!(request.await.unwrap_err().is_cancelled());
            }
            tokio::time::timeout(Duration::from_secs(1), dropped)
                .await
                .unwrap()
                .unwrap();
            assert!(worker.mpv.is_none());
        }
    }

    #[tokio::test]
    async fn checking_blocks_new_playback_and_cancels_preparation_without_losing_cached_scope() {
        let (mut worker, _dir) = worker();
        let (sent, events) = std::sync::mpsc::channel();
        worker.sink.tx = sent;
        let cached = json!({
            "scope": "previous-account",
            "streams": { "selected": {
                "itag": 774, "url": "https://example.invalid/private-stream", "user_agent": null,
                "expires": resolver::now() + 3600
            }}
        });
        crate::paths::write_atomic(
            &worker.paths.runtime.join("streams.json"),
            cached.to_string().as_bytes(),
        )
        .unwrap();
        worker.resolver = Arc::new(Resolver::new(worker.paths.runtime.clone()));
        worker.resolver.set_cookie_file(
            Some(worker.paths.runtime.join("cookies-previous")),
            Some("previous-account".into()),
        );
        worker.set_queue(vec![track("selected"), track("next")], 0);
        worker.starting = true;
        let resolving = tokio::spawn(std::future::pending::<()>());
        let prefetching = tokio::spawn(std::future::pending::<()>());
        let queue = tokio::spawn(std::future::pending::<()>());
        worker.resolving = Some(resolving.abort_handle());
        worker.prefetching = Some(prefetching.abort_handle());
        worker.queue_request = Some(queue.abort_handle());
        worker.begin_account_check().await;
        assert!(resolving.await.unwrap_err().is_cancelled());
        assert!(prefetching.await.unwrap_err().is_cancelled());
        assert!(queue.await.unwrap_err().is_cancelled());
        assert!(worker.account_checking);
        assert!(!worker.state.loading);
        assert_eq!(worker.resolver.cached("selected").unwrap().itag, 774);
        let generation = worker.generation;
        worker
            .command(Command::PlayTracks {
                tracks: vec![track("other")],
                start: 0,
            })
            .await;
        worker.play_target(Target::browse("other-playlist")).await;
        worker.next(false).await;
        worker.toggle_pause().await;
        worker.prefetch();
        worker.maybe_extend();
        assert_eq!(worker.generation, generation);
        assert_eq!(worker.current().unwrap().video_id, "selected");
        assert_eq!(worker.state.source.as_deref(), Some("Fixture context"));
        assert!(worker.resolving.is_none());
        assert!(worker.prefetching.is_none());
        assert!(worker.queue_request.is_none());
        assert_eq!(events.try_iter().filter(|event| matches!(event, Event::Error(message) if message.starts_with("Connecting"))).count(), 4);
    }

    #[tokio::test]
    async fn checking_retires_successor_and_keeps_loaded_current_pause_resume() {
        let (mut worker, _dir) = worker();
        let (mpv, mut commands) = Mpv::test_ipc(1);
        worker.mpv = Some(mpv);
        worker.set_queue(vec![track("current"), track("next")], 0);
        worker.current_entry = Some(1);
        worker.appended = Some(Appended {
            id: worker.queue.id(1).unwrap(),
            format: resolver::describe(774),
            entry: 2,
            gain: None,
        });
        worker.idle = false;
        worker.update_transport();
        worker.begin_account_check().await;
        assert_eq!(commands.recv().await.unwrap(), json!(["playlist-clear"]));
        assert_eq!(
            commands.recv().await.unwrap(),
            json!(["get_property", "playlist/0/id"])
        );
        assert_eq!(worker.current_entry, Some(1));
        assert!(worker.state.playing);
        assert!(!worker.state.next_ready);
        assert!(worker.appended.is_none());
        worker.toggle_pause().await;
        assert_eq!(
            commands.recv().await.unwrap(),
            json!(["set_property", "pause", true])
        );
        assert!(!worker.state.playing);
        worker.toggle_pause().await;
        assert_eq!(
            commands.recv().await.unwrap(),
            json!(["set_property", "pause", false])
        );
        assert!(worker.state.playing);
        worker.next(false).await;
        assert!(
            commands.try_recv().is_err(),
            "Next must not reach mpv while Checking"
        );
    }

    #[tokio::test]
    async fn checking_stops_a_successor_that_crossed_eof_before_it_was_cleared() {
        let (mut worker, _dir) = worker();
        let (mpv, mut commands) = Mpv::test_ipc(2);
        worker.mpv = Some(mpv);
        worker.set_queue(vec![track("current"), track("next")], 0);
        worker.current_entry = Some(1);
        worker.appended = Some(Appended {
            id: worker.queue.id(1).unwrap(),
            format: resolver::describe(774),
            entry: 2,
            gain: None,
        });
        worker.idle = false;
        worker.begin_account_check().await;
        assert_eq!(commands.recv().await.unwrap(), json!(["playlist-clear"]));
        assert_eq!(
            commands.recv().await.unwrap(),
            json!(["get_property", "playlist/0/id"])
        );
        assert_eq!(commands.recv().await.unwrap(), json!(["stop"]));
        assert!(worker.current_entry.is_none());
        assert!(!worker.state.playing);
        assert!(!worker.state.loading);
    }

    #[tokio::test]
    async fn missing_audio_player_stops_loading_without_retrying_youtube() {
        let (mut worker, _dir) = worker();
        worker.set_queue(vec![track("selected")], 0);
        worker.starting = true;
        worker.update_transport();
        worker
            .internal(Internal::Started {
                generation: worker.generation,
                result: Err(StartError::Player(anyhow::anyhow!(
                    "audio player unavailable"
                ))),
            })
            .await;
        assert!(!worker.state.loading);
        assert!(!worker.state.playing);
        assert!(worker.resolving.is_none());
        assert!(
            !worker.retried,
            "a local player failure must not trigger another network lookup"
        );
    }

    #[tokio::test]
    async fn carried_audio_does_not_write_history_to_a_new_account() {
        let (mut worker, _dir) = worker();
        worker.set_queue(vec![track("old-account-song")], 0);
        worker.current_entry = Some(1);
        worker.idle = false;
        worker.update_transport();
        worker.account_changed().await;
        assert!(worker.reported);
        assert_eq!(worker.current_entry, Some(1));
        assert_eq!(worker.current().unwrap().video_id, "old-account-song");
    }

    #[tokio::test]
    async fn stale_mpv_properties_cannot_finish_a_new_selection() {
        let (mut worker, _dir) = worker();
        worker.set_queue(vec![track("new")], 0);
        worker.starting = true;
        worker.current_entry = Some(2);
        worker.update_transport();
        for name in ["paused-for-cache", "seeking", "idle-active"] {
            worker
                .mpv_event(MpvEvent::Property {
                    entry: 1,
                    name: name.into(),
                    data: json!(false),
                })
                .await;
        }
        worker
            .mpv_event(MpvEvent::PlaybackRestart { entry: 1 })
            .await;
        assert!(worker.state.loading);
        assert!(!worker.state.playing);
        worker
            .mpv_event(MpvEvent::Property {
                entry: 2,
                name: "idle-active".into(),
                data: json!(false),
            })
            .await;
        assert!(worker.state.loading, "idle=false is not output readiness");
        worker
            .mpv_event(MpvEvent::PlaybackRestart { entry: 2 })
            .await;
        assert!(!worker.state.loading);
        assert!(worker.state.playing);
    }

    #[tokio::test]
    async fn pause_cancels_loading_and_preserves_the_target_and_seek() {
        let (mut worker, _dir) = worker();
        worker.set_queue(vec![track("selected")], 0);
        worker.state.position = 42.0;
        worker.starting = true;
        worker.update_transport();
        let resolving = tokio::spawn(std::future::pending::<()>());
        worker.resolving = Some(resolving.abort_handle());
        worker.toggle_pause().await;
        assert!(resolving.await.unwrap_err().is_cancelled());
        assert!(!worker.state.loading);
        assert!(!worker.state.playing);
        assert_eq!(worker.resume_at, Some(42.0));
        assert_eq!(worker.current().unwrap().video_id, "selected");
        assert_eq!(worker.state.source.as_deref(), Some("Fixture context"));
        worker.pending_target = Some(Target::browse("chosen-playlist"));
        worker.starting = true;
        worker.update_transport();
        let queue = tokio::spawn(std::future::pending::<()>());
        worker.queue_request = Some(queue.abort_handle());
        worker.toggle_pause().await;
        assert!(queue.await.unwrap_err().is_cancelled());
        assert_eq!(
            worker.pending_target.as_ref().unwrap().key(),
            Target::browse("chosen-playlist").key()
        );
        assert!(!worker.state.loading);
    }

    #[tokio::test]
    async fn next_url_survives_arriving_before_current_player() {
        let (mut worker, _dir) = worker();
        worker.set_queue(vec![track("current"), track("next")], 0);
        worker.starting = true;
        let next = NextStream {
            id: worker.queue.id(1).unwrap(),
            video_id: "next".into(),
            stream: Stream {
                itag: 774,
                url: "https://example.invalid/next".into(),
                user_agent: None,
                expires: resolver::now() + 3600,
                audio: None,
            },
        };
        worker.queue_next(next).await;
        assert_eq!(worker.ready_next.as_ref().unwrap().video_id, "next");
        worker.drop_appended().await;
        assert!(
            worker.ready_next.is_none(),
            "a queue edit must discard the old upcoming URL"
        );
    }

    #[tokio::test]
    async fn buffering_and_seeking_do_not_erase_each_other() {
        let (mut worker, _dir) = worker();
        worker.set_queue(vec![track("song")], 0);
        worker.current_entry = Some(1);
        worker.idle = false;
        for (name, value) in [
            ("seeking", true),
            ("paused-for-cache", true),
            ("seeking", false),
        ] {
            worker
                .mpv_event(MpvEvent::Property {
                    entry: 1,
                    name: name.into(),
                    data: json!(value),
                })
                .await;
        }
        assert!(worker.state.loading);
        worker
            .mpv_event(MpvEvent::Property {
                entry: 1,
                name: "paused-for-cache".into(),
                data: json!(false),
            })
            .await;
        assert!(!worker.state.loading);
        assert!(worker.state.playing);
    }
}
