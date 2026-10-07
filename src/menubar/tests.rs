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
