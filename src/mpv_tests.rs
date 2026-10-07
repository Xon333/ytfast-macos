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
    let player = Mpv::spawn(&dir.join("mpv.sock"), 70.0, tx).await.unwrap();
    assert_eq!(player.get("options/input-media-keys").await.unwrap(), false);
    assert_eq!(
        player.get("options/demuxer-max-bytes").await.unwrap(),
        4 * 1024 * 1024
    );
    assert_eq!(
        player.get("options/demuxer-max-back-bytes").await.unwrap(),
        1024 * 1024
    );
    let file = path.to_str().unwrap();
    player.load(file, "replace", &[]).await.unwrap();
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
    player.load(file, "append", &[]).await.unwrap();
    player.load(file, "append", &[]).await.unwrap();
    player.set("pause", json!(false)).await.unwrap();
    let mut starts = 0;
    let deadline = Instant::now() + Duration::from_secs(12);
    while starts < 3 && Instant::now() < deadline {
        if let Ok(Some((_, MpvEvent::StartFile { .. }))) =
            tokio::time::timeout(Duration::from_secs(2), events.recv()).await
        {
            starts += 1;
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
