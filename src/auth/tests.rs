use super::*;

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

// Independently generated with Python hashlib + cryptography, not the code under test.
#[test]
fn chromium_kdf_and_cookie_fixtures() {
    let fixtures = [
        (
            1,
            "1b39dd1f7809dcaf9533f9b31e7ff43d",
            23,
            "7631302ed76a8c846aacc98b1dcab206375bb707b81a00597a242737b298a463fd7ac2",
        ),
        (
            1003,
            "28688bc482696f3be9c98b1321b859f8",
            23,
            "763130af06e778564415af3e1d418ac512ef88a5dca194ba1d4833d1a3188eb16f4dc0",
        ),
        (
            1003,
            "28688bc482696f3be9c98b1321b859f8",
            24,
            "76313076d4752f8d601b932a4cd713407e845b13a39f7f6db136e52a878fd716d608ef5276de28ec14a21f6fcdb2bfe61bb72047be8b6f60c7dd7325ca0527ec50a80d",
        ),
    ];
    for (rounds, expected, schema, encrypted) in fixtures {
        let key = derive_key(b"test-only-key", rounds);
        assert_eq!(key.as_slice(), bytes(expected));
        assert_eq!(
            decrypt(&bytes(encrypted), &key, ".youtube.com", schema).as_deref(),
            Some("test-only-not-a-session")
        );
    }
}

#[test]
fn corrupted_or_wrong_host_cookies_fail_closed() {
    let key = derive_key(b"test-only-key", 1003);
    let encrypted = bytes(
        "76313076d4752f8d601b932a4cd713407e845b13a39f7f6db136e52a878fd716d608ef5276de28ec14a21f6fcdb2bfe61bb72047be8b6f60c7dd7325ca0527ec50a80d",
    );
    assert!(decrypt(&encrypted, &key, ".google.com", 24).is_none());
    assert!(decrypt(&encrypted[..encrypted.len() - 1], &key, ".youtube.com", 24).is_none());
    assert!(decrypt(b"v20unsupported", &key, ".youtube.com", 24).is_none());
    let legacy = bytes("763130af06e778564415af3e1d418ac512ef88a5dca194ba1d4833d1a3188eb16f4dc0");
    assert!(decrypt(&legacy, &key, ".youtube.com", 24).is_none());
}

#[test]
fn database_filters_cookie_domains_on_label_boundaries() {
    let db = rusqlite::Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE TABLE meta (key TEXT, value TEXT);
        INSERT INTO meta VALUES ('version', '24');
        CREATE TABLE cookies (host_key TEXT, name TEXT, value TEXT, encrypted_value BLOB,
                              path TEXT, expires_utc INTEGER, is_secure INTEGER);",
    )
    .unwrap();
    for host in [
        ".youtube.com",
        "music.youtube.com",
        ".google.com",
        "notyoutube.com",
        ".evilgoogle.com",
    ] {
        db.execute(
            "INSERT INTO cookies VALUES (?1, 'SAPISID', 'synthetic', x'', '/', 0, 1)",
            [host],
        )
        .unwrap();
    }
    let (version, rows) = read_rows(&db).unwrap();
    assert_eq!(version, 24);
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .all(|row| !row.0.contains("evil") && row.0 != "notyoutube.com")
    );
}

#[test]
fn selected_profile_never_falls_back_to_a_different_account() {
    let candidate = || Candidate {
        browser: &BROWSERS[0],
        profile: "Default".into(),
        cookies: PathBuf::from("not-opened"),
        modified: SystemTime::UNIX_EPOCH,
    };
    let id = candidate().id();
    let mut list = vec![candidate()];
    select_preferred(&mut list, Some("removed/profile")).unwrap_err();
    let mut list = vec![
        candidate(),
        Candidate {
            profile: "Profile 1".into(),
            ..candidate()
        },
    ];
    select_preferred(&mut list, Some(&id)).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id(), id);
}

#[test]
fn netscape_export_is_private_and_rejects_record_injection() {
    use std::os::unix::fs::PermissionsExt;
    let directory = std::env::temp_dir().join(format!("ytfast-test-{:032x}", fastrand::u128(..)));
    crate::paths::private_dir(&directory).unwrap();
    let path = directory.join("cookies.txt");
    let mut session = Session {
        source: "synthetic".into(),
        profile: "synthetic".into(),
        cookies: vec![Cookie {
            host: ".youtube.com".into(),
            name: "SAPISID".into(),
            value: "synthetic".into(),
            path: "/".into(),
            secure: true,
            expires: 0,
        }],
    };
    session.write_netscape(&path).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    session.cookies[0].value = "bad\nrecord".into();
    assert!(session.write_netscape(&path).is_err());
    assert!(!std::fs::read_to_string(&path).unwrap().contains("bad"));
    std::fs::remove_dir_all(directory).unwrap();
}
