use super::*;

pub(super) enum StartError {
    Player(anyhow::Error),
    Stream(anyhow::Error),
}

pub(super) struct NextStream {
    id: u64,
    video_id: String,
    stream: Stream,
}

impl super::Worker {
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
        let epoch = self.new_epoch();
        self.cancel_resolution();
        self.generation += 1;
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
        self.finish_blend().await;
        self.drop_cued().await;
        if let Some(mpv) = &self.mpv {
            let _ = mpv.command(json!(["stop"])).await;
        }
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
            let _ = tx.send(Internal::Queue { epoch, result });
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
    }

    /// Pause remains useful before audio starts: cancel the pending work and
    /// leave the selection/seek position ready for a later Play.
    pub(super) async fn toggle_pause(&mut self) {
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
        let Some(track) = self.track_at(pos).cloned() else {
            return;
        };
        if self.pending_target.is_some() {
            self.new_epoch();
        }
        self.generation += 1;
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
        self.asked = Instant::now();
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
        if let Some(mpv) = &self.mpv {
            // Stop the previous song at once; the new one follows when resolved.
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
        self.resolve_current(&track.video_id);
        // Start the only next-song lookup alongside the current one. A quick
        // skip shares that same run instead of starting another cold lookup.
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
        let generation = self.generation;
        let request = self.resolver.request(video_id);
        self.starting = true;
        self.state.loading = true;
        let existing = self.mpv.clone();
        let socket = self.paths.runtime.join(format!("mpv-{generation}.sock"));
        let volume = self.main_volume();
        let events = self.mpv_tx.clone();
        let tx = self.internal_tx.clone();
        let task = tokio::spawn(async move {
            // Process startup and network resolution are independent. Neither
            // blocks transport/queue commands on the worker's event loop.
            let player = async move {
                match existing {
                    Some(mpv) => Ok(mpv),
                    None => Mpv::spawn(&socket, volume, events).await,
                }
            };
            let result = tokio::try_join!(
                async { request.wait().await.map_err(StartError::Stream) },
                async { player.await.map_err(StartError::Player) }
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
        if cfg!(feature = "menubar") {
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
                match mpv.command(json!(["playlist-next", "force"])).await {
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
            Internal::AuthFailed(epoch) => self.auth_failed(epoch),
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
                        if self
                            .mpv
                            .as_ref()
                            .is_none_or(|old| old.serial() != mpv.serial())
                        {
                            self.mpv = Some(mpv.clone());
                            self.apply_loop().await;
                            self.apply_equalizer(&mpv).await;
                        }
                        self.appended = None;
                        let (options, gain) =
                            self.file_options(&track.video_id, &stream, self.resume_at);
                        match mpv.load(&stream.url, "replace", &options).await {
                            Ok(entry) => {
                                self.resume_at = None;
                                self.current_entry = Some(entry);
                                let _ = mpv.set("pause", json!(false)).await;
                                self.state.format = Some(resolver::describe(stream.itag));
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
            Internal::Queue { epoch, result } => {
                if epoch != self.epoch {
                    return;
                }
                match result {
                    Ok(info) => {
                        self.pending_target = None;
                        let start = info.current;
                        self.set_queue(info.tracks, start);
                        if let Some(pos) = self.pos {
                            self.start(pos).await;
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
                itag: stream.itag,
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
        self.state.format = Some(resolver::describe(next.itag));
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
