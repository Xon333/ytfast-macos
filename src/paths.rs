//! Platform-native persistent paths and short, private IPC paths.

use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::Digest;

#[derive(Clone, Debug)]
pub struct Paths {
    /// Settings and themes; Application Support on macOS, XDG config on Linux.
    pub config: PathBuf,
    /// Pages and covers (personal data, 0700).
    pub cache: PathBuf,
    /// Private ephemeral files and sockets. macOS socket paths must stay short.
    pub runtime: PathBuf,
}

impl Paths {
    pub fn new() -> Result<Self> {
        let dirs = directories::ProjectDirs::from("", "", "ytfast").context("no home directory")?;
        #[cfg(target_os = "macos")]
        let runtime = PathBuf::from(format!("/tmp/ytfast-{}", effective_uid()));
        #[cfg(target_os = "linux")]
        let runtime = match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(root) => PathBuf::from(root).join("ytfast"),
            None => std::env::temp_dir().join(format!("ytfast-{}", effective_uid())),
        };
        let paths = Self {
            config: dirs.config_dir().to_path_buf(),
            cache: dirs.cache_dir().to_path_buf(),
            runtime,
        };
        for dir in [
            &paths.config,
            &paths.cache,
            &paths.cache.join("pages"),
            &paths.cache.join("covers"),
            &paths.runtime,
        ] {
            private_dir(dir)?;
        }
        Ok(paths)
    }

    pub fn cookie_file(&self) -> PathBuf {
        self.runtime.join("cookies.txt")
    }
    pub fn page_file(&self, key: &str) -> PathBuf {
        self.cache.join("pages").join(format!("{}.json", hash(key)))
    }
    pub fn cover_file(&self, uri: &str) -> PathBuf {
        self.cache.join("covers").join(hash(uri))
    }
    pub fn searches_file(&self) -> PathBuf {
        self.cache.join("searches.json")
    }
}

pub(crate) fn effective_uid() -> u32 {
    // SAFETY: geteuid has no arguments, memory access or preconditions.
    unsafe { libc::geteuid() }
}

pub(crate) fn private_dir(dir: &Path) -> Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("creating {}", dir.display()))?;
    let metadata = std::fs::symlink_metadata(dir)?;
    if !metadata.is_dir() || metadata.uid() != effective_uid() {
        bail!(
            "{} must be a real directory owned by this user",
            dir.display()
        );
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub fn hash(text: &str) -> String {
    sha2::Sha256::digest(text.as_bytes())[..12]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Writes an exclusive 0600 temporary file, then atomically replaces the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("tmp-{:032x}", fastrand::u128(..)));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
