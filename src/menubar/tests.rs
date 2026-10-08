use super::*;

#[test]
fn playlist_actions_preserve_server_editability() {
    let item = Item {
        kind: crate::model::ItemKind::Playlist,
        title: "Shared".into(),
        subtitle: vec![],
        thumbnail: Some("https://example.invalid/huge.jpg".into()),
        target: Some(Target::browse("VLPLfixture")),
        play: Some(Target::Watch {
            video_id: None,
            playlist_id: Some("PLfixture".into()),
            params: None,
        }),
        track: None,
        index: None,
        stripe: None,
        editable: None,
    };
    let row = MenuRow::from_item(item.clone());
    assert!(row.play.is_some());
    assert!(
        row.editable.is_none(),
        "subscribed playlists must not be writable"
    );
    let text = serde_json::to_string(&row).unwrap();
    assert!(!text.contains("huge.jpg"), "no cover transport or decoding");
    let row = MenuRow::from_item(Item {
        editable: Some("PLmine".into()),
        ..item
    });
    assert_eq!(row.editable.as_deref(), Some("PLmine"));
}

#[test]
fn add_action_uses_captured_song_not_later_playback() {
    let request: Request =
        serde_json::from_str(r#"{"op":"add","playlist":"PLmine","video":"abcdefghijk"}"#).unwrap();
    let Request::Add { playlist, video } = request else {
        panic!("wrong request")
    };
    assert_eq!(playlist, "PLmine");
    assert_eq!(video, "abcdefghijk");
    assert!(valid_video(&video));
    assert!(!valid_video("abc\ninvalid"));
}

#[test]
fn continuation_rows_are_explicit_and_do_not_duplicate() {
    let mut entry = PageEntry::new(Target::browse("VLPLtest"), 7);
    entry.page.rows = vec![MenuRow {
        title: "A".into(),
        ..Default::default()
    }];
    entry.tokens = vec![Continuation {
        token: "page2".into(),
        shelf: Some(0),
    }];
    entry.pending = Some("page2".into());
    assert!(!entry.accept_more(
        "stale",
        More::Items {
            items: vec![],
            next: None
        }
    ));
    assert!(entry.accept_more(
        "page2",
        More::Items {
            items: vec![],
            next: None
        }
    ));
    assert!(!entry.accept_more(
        "page2",
        More::Items {
            items: vec![],
            next: None
        }
    ));
    assert_eq!(entry.page.rows.len(), 1);
    assert!(!entry.page.more);
}

#[test]
fn native_command_input_is_bounded_and_rejects_unknown_actions() {
    assert!(serde_json::from_str::<Request>(r#"{"op":"delete_all"}"#).is_err());
    assert!(serde_json::from_str::<Request>(r#"{"op":"add","playlist":"PL1"}"#).is_err());
}

#[test]
fn refresh_keeps_rows_and_failed_refresh_does_not_replay_old_header_actions() {
    let mut entry = PageEntry::new(Target::browse("VLPLtest"), 7);
    entry.page.rows = vec![MenuRow {
        title: "Saved song".into(),
        ..Default::default()
    }];
    entry.page.play = Some("old playlist".into());
    entry.page.loading = false;
    entry.refresh(8);
    assert_eq!(entry.seq, 8);
    assert_eq!(entry.page.rows[0].title, "Saved song");
    assert!(entry.page.loading);
    entry.page.loading = false;
    entry.page.message = Some("Offline".into());
    assert_eq!(entry.page.rows.len(), 1);
    entry.replace(Page::default(), false);
    assert!(
        entry.page.play.is_none(),
        "a new page without a header must not inherit old Play"
    );
}

#[test]
fn account_snapshot_is_visible_while_network_refresh_is_pending() {
    let mut entry = PageEntry::new(Target::browse("VLPLtest"), 1);
    let page = Page {
        header: Some(crate::model::Header {
            title: "Saved playlist".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    entry.replace(page.clone(), true);
    assert_eq!(entry.page.title, "Saved playlist");
    assert!(entry.page.loading);
    assert!(entry.fetched.is_none());
    entry.replace(page, false);
    assert!(!entry.page.loading);
    assert!(entry.fetched.is_some());
}

fn snapshot_fixture(pages: Option<Vec<&MenuPage>>) -> MenuSnapshot<'_> {
    MenuSnapshot {
        track: Some(MenuTrack {
            id: "abcdefghijk",
            title: "Night \"Live\"\n夜\0",
            artist: "Fixture artist".into(),
        }),
        playing: true,
        loading: false,
        position: 12.5,
        duration: 180.0,
        volume: 70.0,
        shuffle: true,
        format: Some("Opus 256 kbps"),
        source: Some("Fixture playlist"),
        normalize: true,
        signed_in: true,
        account_checking: false,
        account_unverified: false,
        account: "Fixture · Chrome".into(),
        profile: Some("fixture/default"),
        profiles: vec![MenuProfile {
            id: "fixture/default",
            label: "Chrome · Test profile",
        }],
        pages,
        notice: Some("Added to playlist"),
        error: None,
        adding: false,
        show: true,
        quit: false,
    }
}

#[test]
fn borrowed_snapshot_preserves_native_wire_contract() {
    let page = MenuPage {
        key: "browse:fixture:".into(),
        target: "fixture target".into(),
        title: "Fixture playlist".into(),
        rows: vec![MenuRow {
            title: "A song".into(),
            subtitle: "An artist".into(),
            play: Some("fixture play".into()),
            video: Some("abcdefghijk".into()),
            ..Default::default()
        }],
        loading: true,
        more: true,
        ..Default::default()
    };
    let mut snapshot = snapshot_fixture(Some(vec![&page]));
    let pointer = ffi_encoded_result(|| Ok(serde_json::to_string(&snapshot)?));
    // SAFETY: this is the single owner of the string returned above, exactly
    // as in the native caller. Escaped NUL/UTF-8 must survive that boundary.
    let response = unsafe { CString::from_raw(pointer) };
    let actual: Value = serde_json::from_slice(response.as_bytes()).unwrap();
    let expected = json!({
        "track": {"id": "abcdefghijk", "title": "Night \"Live\"\n夜\0", "artist": "Fixture artist"},
        "playing": true, "loading": false, "position": 12.5, "duration": 180.0,
        "volume": 70.0, "shuffle": true, "format": "Opus 256 kbps", "source": "Fixture playlist", "normalize": true,
        "signed_in": true, "account_checking": false, "account_unverified": false,
        "account": "Fixture · Chrome", "profile": "fixture/default",
        "profiles": [{"id": "fixture/default", "label": "Chrome · Test profile"}],
        "pages": [{
            "key": "browse:fixture:", "target": "fixture target", "title": "Fixture playlist",
            "rows": [{"title": "A song", "subtitle": "An artist", "play": "fixture play",
                "browse": null, "video": "abcdefghijk", "editable": null}],
            "play": null, "loading": true, "more": true, "message": null
        }],
        "notice": "Added to playlist", "error": null, "adding": false, "show": true, "quit": false
    });
    assert_eq!(actual, expected);

    // A clean catalogue means null (retain native pages), while an emptied
    // catalogue means [] (clear native pages). Neither key may be omitted.
    snapshot.pages = None;
    snapshot.track = None;
    let clean = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(clean.get("pages"), Some(&Value::Null));
    assert_eq!(clean.get("track"), Some(&Value::Null));
    snapshot.pages = Some(vec![]);
    assert_eq!(serde_json::to_value(&snapshot).unwrap()["pages"], json!([]));
}

// The previous production path, kept only as a synthetic benchmark baseline:
// clone the catalogue, construct its complete owned JSON tree, then encode.
fn owned_snapshot(snapshot: &MenuSnapshot<'_>) -> String {
    let pages = snapshot.pages.as_ref().map(|pages| {
        pages
            .iter()
            .map(|page| (**page).clone())
            .collect::<Vec<_>>()
    });
    json!({
        "track": snapshot.track, "playing": snapshot.playing, "loading": snapshot.loading,
        "position": snapshot.position, "duration": snapshot.duration, "volume": snapshot.volume,
        "shuffle": snapshot.shuffle, "format": snapshot.format, "source": snapshot.source, "normalize": snapshot.normalize,
        "signed_in": snapshot.signed_in, "account_checking": snapshot.account_checking,
        "account_unverified": snapshot.account_unverified, "account": snapshot.account,
        "profile": snapshot.profile, "profiles": snapshot.profiles, "pages": pages,
        "notice": snapshot.notice, "error": snapshot.error, "adding": snapshot.adding,
        "show": snapshot.show, "quit": snapshot.quit
    })
    .to_string()
}

#[test]
#[ignore = "Synthetic serialization benchmark; run explicitly in release mode"]
fn snapshot_serialization_benchmark() {
    let pages: Vec<_> = (0..MAX_PAGES)
        .map(|index| MenuPage {
            key: format!("browse:fixture-{index}:"),
            target: format!("fixture target {index}"),
            title: format!("Fixture playlist {index}"),
            rows: (0..MAX_ROWS)
                .map(|index| {
                    let video = format!("test{index:07}");
                    MenuRow {
                        title: format!("Track {index:04} — a longer synthetic library title"),
                        subtitle: "Fixture artist · Collection".into(),
                        play: serde_json::to_string(&Target::Watch {
                            video_id: Some(video.clone()),
                            playlist_id: Some("PLfixture".into()),
                            params: None,
                        })
                        .ok(),
                        video: Some(video),
                        ..Default::default()
                    }
                })
                .collect(),
            ..Default::default()
        })
        .collect();
    let snapshot = snapshot_fixture(Some(pages.iter().collect()));
    let direct = serde_json::to_string(&snapshot).unwrap();
    let previous = owned_snapshot(&snapshot);
    assert_eq!(
        serde_json::from_str::<Value>(&direct).unwrap(),
        serde_json::from_str::<Value>(&previous).unwrap()
    );
    let bytes = direct.len();
    drop((direct, previous));

    let iterations = 20;
    let mut owned = Duration::ZERO;
    let mut borrowed = Duration::ZERO;
    for _ in 0..iterations {
        let started = Instant::now();
        std::hint::black_box(owned_snapshot(&snapshot));
        owned += started.elapsed();
        let started = Instant::now();
        std::hint::black_box(serde_json::to_string(&snapshot).unwrap());
        borrowed += started.elapsed();
    }
    eprintln!(
        "snapshot_serialization {}",
        json!({
            "pages": MAX_PAGES, "rows": MAX_PAGES * MAX_ROWS, "bytes": bytes,
            "iterations": iterations,
            "previous_ms_per_snapshot": owned.as_secs_f64() * 1000.0 / f64::from(iterations),
            "borrowed_ms_per_snapshot": borrowed.as_secs_f64() * 1000.0 / f64::from(iterations)
        })
    );
}

#[test]
fn collapsed_catalogue_sends_no_rows_and_browse_sends_only_its_page() {
    let pages: HashMap<_, _> = (0..MAX_PAGES)
        .map(|i| {
            let mut entry = PageEntry::new(Target::browse(format!("fixture-{i}")), i as u64);
            entry.page.rows = (0..MAX_ROWS).map(|_| MenuRow::default()).collect();
            (entry.page.key.clone(), entry)
        })
        .collect();
    assert!(visible_pages(&pages, None).is_empty());
    assert!(visible_pages(&pages, Some("unknown")).is_empty());
    let key = Target::browse("fixture-3").key();
    let visible = visible_pages(&pages, Some(&key));
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].key, key);
    assert_eq!(visible[0].rows.len(), MAX_ROWS);
    assert!(matches!(
        serde_json::from_str::<Request>(r#"{"op":"collapse"}"#).unwrap(),
        Request::Collapse
    ));
    let legacy: Request = serde_json::from_str(r#"{"op":"play","target":"{}"}"#).unwrap();
    assert!(matches!(
        legacy,
        Request::Play {
            source: None,
            collection: false,
            ..
        }
    ));
}
