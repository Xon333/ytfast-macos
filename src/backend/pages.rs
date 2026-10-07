use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::*;

const MAX_SNAPSHOTS: usize = 32;
const MAX_SNAPSHOT_BYTES: u64 = 1024 * 1024;
const MAX_CACHE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Serialize, Deserialize)]
struct Snapshot {
    scope: String,
    key: String,
    saved: u64,
    page: Page,
}

impl super::Worker {
    pub(super) fn load_page(&self, target: Target, seq: u64) {
        let key = target.key();
        let current = self.client.clone();
        let client = current.snapshot();
        let epoch = client.session_epoch();
        let scope = client.cache_scope().unwrap_or_else(|| "public".to_owned());
        let sink = self.sink.clone();
        let directory = self.paths.cache.join("library-pages-v1");
        let internal = self.internal_tx.clone();
        tokio::spawn(async move {
            let cached = {
                let directory = directory.clone();
                let scope = scope.clone();
                let key = key.clone();
                tokio::task::spawn_blocking(move || read_snapshot(&directory, &scope, &key))
            };
            let refresh = async {
                match &target {
                    Target::Browse { id, params } => client.browse(id, params.as_deref()).await,
                    Target::Search { query, params } => {
                        client.search(query, params.as_deref()).await
                    }
                    Target::Watch { .. } => Err(ApiError::Invalid("not a page".into())),
                }
            };
            tokio::pin!(refresh);
            // Start HTTPS and the disk read together. A late disk read must
            // never replace a faster fresh reply.
            let result = tokio::select! {
                result = &mut refresh => result,
                cached = cached => {
                    if let Ok(Some(page)) = cached {
                        current.if_current(epoch, || sink.send(Event::Page {
                            key: key.clone(), seq, result: Ok(Box::new(page)), cached: true,
                        }));
                    }
                    refresh.await
                }
            };
            match result {
                Ok(value) => {
                    let page = parse::page(&value);
                    current.if_current(epoch, || {
                        sink.send(Event::Page {
                            key: key.clone(),
                            seq,
                            result: Ok(Box::new(page.clone())),
                            cached: false,
                        })
                    });
                    if current.session_epoch() == epoch {
                        tokio::task::spawn_blocking(move || {
                            write_snapshot(&directory, scope, key, page);
                        });
                    }
                }
                Err(error) => {
                    current.if_current(epoch, || {
                        if matches!(error, ApiError::Auth) {
                            let _ = internal.send(Internal::AuthFailed(epoch));
                        }
                        sink.send(Event::Page {
                            key,
                            seq,
                            result: Err(error.to_string()),
                            cached: false,
                        });
                    });
                }
            }
        });
    }
}

fn snapshot_path(directory: &Path, scope: &str, key: &str) -> PathBuf {
    directory.join(format!(
        "{}.json",
        crate::paths::hash(&format!("{scope}\0{key}"))
    ))
}

fn read_snapshot(directory: &Path, scope: &str, key: &str) -> Option<Page> {
    let path = snapshot_path(directory, scope, key);
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_SNAPSHOT_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let snapshot: Snapshot = serde_json::from_slice(&bytes).ok()?;
    if snapshot.scope != scope
        || snapshot.key != key
        || now().saturating_sub(snapshot.saved) > MAX_AGE.as_secs()
    {
        return None;
    }
    Some(snapshot.page)
}

fn write_snapshot(directory: &Path, scope: String, key: String, page: Page) {
    let path = snapshot_path(directory, &scope, &key);
    let snapshot = Snapshot {
        scope,
        key,
        saved: now(),
        page,
    };
    let Ok(bytes) = serde_json::to_vec(&snapshot) else {
        return;
    };
    if bytes.len() as u64 > MAX_SNAPSHOT_BYTES || crate::paths::private_dir(directory).is_err() {
        return;
    }
    if crate::paths::write_atomic(&path, &bytes).is_ok() {
        prune(directory);
    }
}

fn prune(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut kept = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_file() || path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        if metadata.len() > MAX_SNAPSHOT_BYTES || modified.elapsed().is_ok_and(|age| age > MAX_AGE)
        {
            let _ = std::fs::remove_file(path);
        } else {
            kept.push((modified, path, metadata.len()));
        }
    }
    kept.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    let mut bytes = 0;
    for (index, (_, path, size)) in kept.into_iter().enumerate() {
        bytes += size;
        if index >= MAX_SNAPSHOTS || bytes > MAX_CACHE_BYTES {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_are_scoped_expiring_and_private() {
        use std::os::unix::fs::PermissionsExt;
        let directory =
            std::env::temp_dir().join(format!("ytfast-pages-{:032x}", fastrand::u128(..)));
        write_snapshot(
            &directory,
            "account-a".into(),
            "library".into(),
            Page::default(),
        );
        assert!(read_snapshot(&directory, "account-a", "library").is_some());
        assert!(read_snapshot(&directory, "account-b", "library").is_none());
        assert!(read_snapshot(&directory, "account-a", "different-page").is_none());
        let path = snapshot_path(&directory, "account-a", "library");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let mut snapshot: Snapshot =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        snapshot.saved = now() - MAX_AGE.as_secs() - 1;
        std::fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        assert!(read_snapshot(&directory, "account-a", "library").is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn snapshot_count_and_bytes_stay_bounded() {
        let directory =
            std::env::temp_dir().join(format!("ytfast-pages-{:032x}", fastrand::u128(..)));
        for index in 0..MAX_SNAPSHOTS + 3 {
            write_snapshot(
                &directory,
                "account".into(),
                index.to_string(),
                Page::default(),
            );
        }
        assert!(std::fs::read_dir(&directory).unwrap().count() <= MAX_SNAPSHOTS);
        let big = Page {
            message: Some("x".repeat(MAX_SNAPSHOT_BYTES as usize)),
            ..Page::default()
        };
        write_snapshot(&directory, "account".into(), "oversized".into(), big);
        assert!(read_snapshot(&directory, "account", "oversized").is_none());
        for index in 0..13 {
            let page = Page {
                message: Some("x".repeat(750_000)),
                ..Page::default()
            };
            write_snapshot(&directory, "account".into(), format!("large-{index}"), page);
        }
        let bytes: u64 = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().metadata().unwrap().len())
            .sum();
        assert!(bytes <= MAX_CACHE_BYTES);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
