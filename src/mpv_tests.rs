use super::*;
use std::time::Instant;

/// Local PCM fixtures exercise the same mpv subprocess/IPC code as production.
/// No browser, network, account write, or paid service participates.
#[tokio::test]
#[ignore = "requires installed mpv; run in native CI"]
async fn native_audio_transport() {
    let dir = std::env::temp_dir().join(format!("ytm-{:016x}", fastrand::u64(..)));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("fixture.wav");
    let rate = 8000u32;
    let samples = rate * 3;
    let size = samples * 2;
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend((size + 36).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(rate.to_le_bytes());
    wav.extend((rate * 2).to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(size.to_le_bytes());
    wav.resize(wav.len() + size as usize, 0);
    std::fs::write(&path, wav).unwrap();
    let (tx, mut events) = mpsc::unbounded_channel();
    let spawned = Instant::now();
    let player = Mpv::spawn(&dir.join("mpv.sock"), 70.0, tx).await.unwrap();
    let spawn_ms = spawned.elapsed().as_secs_f64() * 1_000.0;
    assert_eq!(player.get("options/input-media-keys").await.unwrap(), false);
    for name in [
        "media-controls",
        "osc",
        "load-scripts",
        "load-stats-overlay",
        "load-console",
        "load-commands",
        "load-select",
        "autoload-files",
    ] {
        assert_eq!(player.get(&format!("options/{name}")).await.unwrap(), false);
    }
    assert_eq!(player.get("options/gapless-audio").await.unwrap(), true);
    assert_eq!(
        player.get("options/demuxer-max-bytes").await.unwrap(),
        4 * 1024 * 1024
    );
    assert_eq!(
        player.get("options/demuxer-max-back-bytes").await.unwrap(),
        1024 * 1024
    );
    let file = path.to_str().unwrap();
    let loaded = Instant::now();
    let current_entry = player.load(file, "replace", &[]).await.unwrap();
    let mut entries = std::collections::HashSet::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match events.recv().await.unwrap().1 {
                MpvEvent::StartFile { entry } => {
                    assert_eq!(entry, current_entry);
                    assert!(entries.insert(entry));
                }
                MpvEvent::PlaybackRestart { entry } if entry == current_entry => break,
                _ => {}
            }
        }
    })
    .await
    .expect("local audio must reach the actual mpv playback-restart event");
    println!(
        "native PCM/null-output fixture: mpv startup {spawn_ms:.3} ms; load-to-restart {:.3} ms",
        loaded.elapsed().as_secs_f64() * 1_000.0
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(player.get("time-pos").await.unwrap().as_f64().unwrap() > 0.0);
    player.set("pause", json!(true)).await.unwrap();
    assert_eq!(player.get("pause").await.unwrap(), true);
    player
        .command(json!(["seek", 1.5, "absolute+exact"]))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!((player.get("time-pos").await.unwrap().as_f64().unwrap() - 1.5).abs() < 0.3);
    // Account checking must retire upcoming audio while preserving the exact
    // loaded current file and its pause state.
    player.load(file, "append", &[]).await.unwrap();
    player.command(json!(["playlist-clear"])).await.unwrap();
    assert_eq!(player.get("playlist-count").await.unwrap(), 1);
    assert_eq!(player.get("playlist/0/id").await.unwrap(), current_entry);
    assert_eq!(player.get("pause").await.unwrap(), true);
    player.load(file, "append", &[]).await.unwrap();
    player.load(file, "append", &[]).await.unwrap();
    // Absolute Next is idempotent if natural advance already selected the
    // same item. A relative playlist-next could skip it in that race.
    player.set("playlist-pos", json!(1)).await.unwrap();
    player.set("playlist-pos", json!(1)).await.unwrap();
    player.set("pause", json!(false)).await.unwrap();
    let mut starts = entries.len();
    let deadline = Instant::now() + Duration::from_secs(12);
    while starts < 3 && Instant::now() < deadline {
        if let Ok(Some((_, MpvEvent::StartFile { entry }))) =
            tokio::time::timeout(Duration::from_secs(2), events.recv()).await
        {
            starts += 1;
            assert!(
                entries.insert(entry),
                "selecting the same entry must not restart it"
            );
        }
    }
    assert_eq!(
        starts, 3,
        "the appended queue must advance across real mpv entries"
    );
    drop(player);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        tokio::net::UnixStream::connect(dir.join("mpv.sock"))
            .await
            .is_err()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn cancelled_ipc_command_releases_its_pending_reply() {
    let (socket, peer) = UnixStream::pair().unwrap();
    let (_reader, writer) = socket.into_split();
    let child = tokio::process::Command::new("/usr/bin/true")
        .spawn()
        .unwrap();
    let player = Arc::new(Mpv {
        serial: 0,
        writer: Mutex::new(writer),
        next_id: AtomicU64::new(1),
        pending: Arc::default(),
        _child: child,
    });
    let copy = player.clone();
    let command = tokio::spawn(async move { copy.get("pause").await });
    let mut peer = BufReader::new(peer).lines();
    assert!(peer.next_line().await.unwrap().is_some());
    assert_eq!(player.pending.lock().unwrap().len(), 1);
    command.abort();
    assert!(command.await.unwrap_err().is_cancelled());
    assert!(player.pending.lock().unwrap().is_empty());
}

#[tokio::test]
async fn lost_ipc_reply_fails_immediately_and_releases_state() {
    let (socket, peer) = UnixStream::pair().unwrap();
    let (_reader, writer) = socket.into_split();
    let child = tokio::process::Command::new("/usr/bin/true")
        .spawn()
        .unwrap();
    let player = Arc::new(Mpv {
        serial: 0,
        writer: Mutex::new(writer),
        next_id: AtomicU64::new(1),
        pending: Arc::default(),
        _child: child,
    });
    let copy = player.clone();
    let command = tokio::spawn(async move { copy.get("pause").await });
    let mut peer = BufReader::new(peer).lines();
    assert!(peer.next_line().await.unwrap().is_some());
    // This is the reader's EOF path: every outstanding sender is dropped.
    player.pending.lock().unwrap().clear();
    let result = tokio::time::timeout(Duration::from_millis(250), command)
        .await
        .unwrap()
        .unwrap();
    assert!(result.unwrap_err().to_string().contains("mpv closed"));
    assert!(player.pending.lock().unwrap().is_empty());
}

#[tokio::test]
async fn observation_batch_has_no_reply_dependencies_and_matches_out_of_order_replies() {
    let (socket, peer) = UnixStream::pair().unwrap();
    let (_reader, writer) = socket.into_split();
    let child = tokio::process::Command::new("/usr/bin/true")
        .spawn()
        .unwrap();
    let player = Arc::new(Mpv {
        serial: 0,
        writer: Mutex::new(writer),
        next_id: AtomicU64::new(1),
        pending: Arc::default(),
        _child: child,
    });
    let observations: Vec<_> = OBSERVED
        .iter()
        .enumerate()
        .map(|(i, name)| json!(["observe_property", i + 1, name]))
        .collect();
    let copy = player.clone();
    let commands = observations.clone();
    let task = tokio::spawn(async move { copy.commands(&commands).await });
    let mut peer = BufReader::new(peer).lines();
    let mut received = Vec::new();
    for _ in 0..observations.len() {
        let line = tokio::time::timeout(Duration::from_secs(1), peer.next_line())
            .await
            .expect("all observations must be written before any reply is needed")
            .unwrap()
            .unwrap();
        received.push(serde_json::from_str::<Value>(&line).unwrap());
    }
    // The peer deliberately withholds replies until the entire batch arrives,
    // then answers in reverse order. Neither can stall or misroute a command.
    for request in received.into_iter().rev() {
        let sender = player
            .pending
            .lock()
            .unwrap()
            .remove(&request["request_id"].as_u64().unwrap())
            .unwrap();
        sender
            .send(json!({"error": "success", "data": request["command"]}))
            .unwrap();
    }
    assert_eq!(task.await.unwrap().unwrap(), observations);
    assert!(player.pending.lock().unwrap().is_empty());

    // Cancellation of a batch must release every waiter, including replies
    // after the first one, using the same guards as an individual command.
    let copy = player.clone();
    let task = tokio::spawn(async move { copy.commands(&observations).await });
    for _ in 0..OBSERVED.len() {
        assert!(peer.next_line().await.unwrap().is_some());
    }
    assert_eq!(player.pending.lock().unwrap().len(), OBSERVED.len());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(player.pending.lock().unwrap().is_empty());
}
