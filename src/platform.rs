//! Small process-boundary helpers; no changes to the audio engine or API.

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Hold this file for the entire process. The OS releases the lock after a
/// crash; another launch must not unlink the live instance's socket.
pub fn instance_lock(runtime: &Path) -> std::io::Result<Option<File>> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(runtime.join("instance.lock"))?;
    // SAFETY: file owns a live descriptor, and flock retains no Rust pointers.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(Some(file));
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::WouldBlock {
        Ok(None)
    } else {
        Err(error)
    }
}

/// Finder does not load shell profiles. Call exactly once, at the start of
/// main, before logging, a runtime, or any other threads have been started.
#[cfg(target_os = "macos")]
pub fn finder_path() -> anyhow::Result<()> {
    let mut paths: Vec<_> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .filter(|path| path.is_absolute())
        .collect();
    for directory in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ] {
        let directory = std::path::PathBuf::from(directory);
        if !paths.contains(&directory) {
            paths.push(directory);
        }
    }
    let path = std::env::join_paths(paths)?;
    // SAFETY: main calls this before creating any threads or AppKit objects.
    unsafe {
        std::env::set_var("PATH", path);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn check_dependencies() -> anyhow::Result<()> {
    use std::process::{Command, Stdio};
    let missing: Vec<_> = ["mpv", "yt-dlp", "deno"]
        .into_iter()
        .filter(|name| {
            !std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).any(|directory| {
                use std::os::unix::fs::PermissionsExt;
                std::fs::metadata(directory.join(name))
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
        })
        .collect();
    if !missing.is_empty() {
        let message = format!(
            "Missing: {}. Install with: brew install mpv yt-dlp deno",
            missing.join(", ")
        );
        let _ = Command::new("/usr/bin/osascript")
            .args([
                "-e",
                "on run argv\ndisplay alert \"YTfast\" message (item 1 of argv)\nend run",
                &message,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        anyhow::bail!("{message}");
    }
    Ok(())
}

/// Remove leftover cookie exports on startup and normal exit, while holding
/// the instance lock. The resolver owns its bounded, expiring, account-scoped
/// URL cache; deleting it here would force every relaunch to resolve again.
#[cfg(any(target_os = "macos", test))]
pub struct SessionFiles(pub std::path::PathBuf);

#[cfg(any(target_os = "macos", test))]
impl SessionFiles {
    pub fn clear(&self) {
        if let Ok(entries) = std::fs::read_dir(&self.0) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                let session_export = name
                    .strip_prefix("cookies-")
                    .and_then(|name| name.strip_suffix(".txt"))
                    .is_some_and(|id| id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit()));
                if name == "cookies.txt"
                    || session_export
                    || (name.starts_with("ytdlp-") && name.ends_with(".txt"))
                {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

#[cfg(any(target_os = "macos", test))]
impl Drop for SessionFiles {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_cleanup_keeps_reusable_urls_but_removes_cookie_exports() {
        let directory = std::env::temp_dir().join(format!(
            "ytfast-session-cleanup-{:032x}",
            fastrand::u128(..)
        ));
        crate::paths::private_dir(&directory).unwrap();
        for name in [
            "cookies.txt",
            "cookies-00000000000000000000000000000001.txt",
            "cookies-notes.txt",
            "ytdlp-fixture-1.txt",
            "streams.json",
            "unrelated.txt",
        ] {
            std::fs::write(directory.join(name), b"fixture").unwrap();
        }
        let exports = SessionFiles(directory.clone());
        exports.clear();
        assert!(!directory.join("cookies.txt").exists());
        assert!(
            !directory
                .join("cookies-00000000000000000000000000000001.txt")
                .exists()
        );
        assert!(directory.join("cookies-notes.txt").exists());
        assert!(!directory.join("ytdlp-fixture-1.txt").exists());
        assert_eq!(
            std::fs::read(directory.join("streams.json")).unwrap(),
            b"fixture"
        );
        assert!(directory.join("unrelated.txt").exists());

        std::fs::write(directory.join("ytdlp-fixture-2.txt"), b"fixture").unwrap();
        drop(exports);
        assert!(!directory.join("ytdlp-fixture-2.txt").exists());
        assert!(directory.join("streams.json").exists());
        assert!(directory.join("unrelated.txt").exists());
        std::fs::remove_dir_all(directory).unwrap();
    }
}

/// Find external tools for Finder launches without loading shell profiles.
pub fn tool(name: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(&tool_path())
        .map(|p| p.join(name))
        .find(|p| {
            std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
        .unwrap_or_else(|| name.into())
}

/// Child-only PATH. AppKit can start threads before Rust is entered.
pub fn tool_path() -> std::ffi::OsString {
    let mut paths: Vec<_> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .filter(|p| p.is_absolute())
        .collect();
    for name in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ] {
        let path = std::path::PathBuf::from(name);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    std::env::join_paths(paths).unwrap_or_else(|_| "/usr/bin:/bin".into())
}
