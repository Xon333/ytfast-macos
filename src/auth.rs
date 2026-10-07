//! Reads YouTube's session through a read-only SQLite transaction, including
//! the browser's WAL. No database copies or Keychain values are written to disk.
//! See docs/MACOS.md for platform differences and source references.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use anyhow::{Context, Result, anyhow, bail};
use sha2::Digest;

struct Browser {
    name: &'static str,
    dir: &'static str,
    keyring: &'static str,
}

#[cfg(target_os = "linux")]
const BROWSERS: &[Browser] = &[
    Browser { name: "Brave Origin", dir: "BraveSoftware/Brave-Origin", keyring: "brave" },
    Browser { name: "Brave", dir: "BraveSoftware/Brave-Browser", keyring: "brave" },
    Browser { name: "Google Chrome", dir: "google-chrome", keyring: "chrome" },
    Browser { name: "Chromium", dir: "chromium", keyring: "chromium" },
];

#[cfg(target_os = "macos")]
const BROWSERS: &[Browser] = &[
    Browser { name: "Google Chrome", dir: "Google/Chrome", keyring: "Chrome Safe Storage" },
    Browser { name: "Brave", dir: "BraveSoftware/Brave-Browser", keyring: "Brave Safe Storage" },
    Browser { name: "Chromium", dir: "Chromium", keyring: "Chromium Safe Storage" },
];

#[derive(Clone)]
pub struct Cookie {
    pub host: String,
    pub name: String,
    pub value: String,
    pub path: String,
    pub secure: bool,
    /// Unix seconds; 0 for a session cookie.
    pub expires: i64,
}

#[derive(Clone)]
pub struct Session {
    pub source: String,
    pub profile: String,
    cookies: Vec<Cookie>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("source", &self.source)
            .field("cookies", &self.cookies.len())
            .finish()
    }
}

impl Session {
    /// Every applicable cookie, with the most specific host winning a repeated name.
    pub fn header(&self) -> String {
        let mut chosen: Vec<&Cookie> = Vec::new();
        for cookie in self.cookies.iter().filter(|c| applies_to_music(&c.host)) {
            match chosen.iter_mut().find(|c| c.name == cookie.name) {
                Some(existing) if specificity(&cookie.host) > specificity(&existing.host) => *existing = cookie,
                Some(_) => {},
                None => chosen.push(cookie),
            }
        }
        chosen.iter().map(|c| format!("{}={}", c.name, c.value)).collect::<Vec<_>>().join("; ")
    }

    pub fn sapisid(&self) -> Option<&str> {
        ["SAPISID", "__Secure-3PAPISID"].iter().find_map(|name| {
            self.cookies.iter().find(|c| c.name == *name && applies_to_music(&c.host))
                .map(|c| c.value.as_str())
        })
    }

    /// Atomic 0600 export containing only YouTube/Google cookies.
    pub fn write_netscape(&self, path: &Path) -> Result<()> {
        let mut text = String::from("# Netscape HTTP Cookie File\n");
        for c in &self.cookies {
            if [&c.host, &c.path, &c.name, &c.value].iter().any(|v| v.contains(['\r', '\n', '\t', '\0'])) {
                bail!("The browser returned an invalid cookie record");
            }
            let domain = if c.host.starts_with('.') { "TRUE" } else { "FALSE" };
            let secure = if c.secure { "TRUE" } else { "FALSE" };
            text.push_str(&format!("{}\t{domain}\t{}\t{secure}\t{}\t{}\t{}\n", c.host, c.path, c.expires, c.name, c.value));
        }
        crate::paths::write_atomic(path, text.as_bytes())?;
        Ok(())
    }
}

fn applies_to_music(host: &str) -> bool {
    matches!(host, "music.youtube.com" | ".music.youtube.com" | ".youtube.com" | "youtube.com")
}

fn specificity(host: &str) -> usize {
    host.trim_start_matches('.').len()
}

struct Candidate {
    browser: &'static Browser,
    profile: String,
    cookies: PathBuf,
    modified: SystemTime,
}

impl Candidate {
    fn id(&self) -> String { format!("{}/{}", self.browser.dir, self.profile) }
    fn label(&self) -> String { format!("{} ({})", self.browser.name, self.profile) }
}

/// A profile containing YouTube sign-in cookies. Only the account API can
/// confirm whether it is signed in; enumeration never accesses Keychain.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub id: String,
    pub label: String,
}

fn cookie_path(profile: &Path) -> Option<PathBuf> {
    ["Network/Cookies", "Cookies"].iter().map(|p| profile.join(p)).find(|p| p.is_file())
}

fn candidates() -> Result<Vec<Candidate>> {
    let base = directories::BaseDirs::new().context("no home directory")?;
    #[cfg(target_os = "macos")]
    let config = base.home_dir().join("Library/Application Support");
    #[cfg(target_os = "linux")]
    let config = base.config_dir().to_path_buf();
    let mut candidates = Vec::new();
    for browser in BROWSERS {
        let Ok(entries) = std::fs::read_dir(config.join(browser.dir)) else { continue };
        for entry in entries.flatten() {
            let Some(cookies) = cookie_path(&entry.path()) else { continue };
            let modified = [&cookies, &cookies.with_file_name("Cookies-wal")].iter()
                .filter_map(|p| std::fs::metadata(p).ok()?.modified().ok())
                .max().unwrap_or(SystemTime::UNIX_EPOCH);
            candidates.push(Candidate {
                browser, profile: entry.file_name().to_string_lossy().into_owned(), cookies, modified,
            });
        }
    }
    candidates.sort_by_key(|c| std::cmp::Reverse(c.modified));
    Ok(candidates)
}

pub fn profiles(_scratch: &Path) -> Vec<Profile> {
    candidates().unwrap_or_default().iter()
        .filter(|c| snapshot(c).is_ok_and(|(_, rows)| has_signin(&rows)))
        .map(|c| Profile { id: c.id(), label: c.label() }).collect()
}

fn select_preferred(candidates: &mut Vec<Candidate>, preferred: Option<&str>) -> Result<()> {
    if let Some(id) = preferred {
        if !candidates.iter().any(|c| c.id() == id) {
            bail!("The selected browser profile was removed. Choose another in Settings");
        }
        candidates.retain(|c| c.id() == id);
    }
    Ok(())
}

pub fn load(_scratch: &Path, preferred: Option<&str>) -> Result<Session> {
    let mut candidates = candidates()?;
    if candidates.is_empty() {
        bail!("Sign in to YouTube Music in Chrome, Brave or Chromium, then Reconnect");
    }
    select_preferred(&mut candidates, preferred)?;
    for candidate in &candidates {
        // A denied key or failed selected profile must not select a different account.
        if let Some(session) = read_profile(candidate)? { return Ok(session) }
    }
    Err(anyhow!("No available browser profile has a current YouTube sign-in. Sign in, then Reconnect"))
}

type Row = (String, String, String, Vec<u8>, String, i64, bool);

fn snapshot(candidate: &Candidate) -> Result<(i64, Vec<Row>)> {
    let mut db = rusqlite::Connection::open_with_flags(&candidate.cookies,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("The browser cookie store could not be opened read-only")?;
    db.busy_timeout(Duration::from_secs(2))?;
    // SQLite reads the live database and WAL as one snapshot; copying them
    // separately can combine different browser transactions.
    let transaction = db.transaction()?;
    read_rows(&transaction)
}

fn read_rows(db: &rusqlite::Connection) -> Result<(i64, Vec<Row>)> {
    let version: String = db.query_row("SELECT value FROM meta WHERE key = 'version'", [], |r| r.get(0))?;
    let version = version.parse::<i64>().context("Invalid browser cookie schema version")?;
    let mut statement = db.prepare(
        "SELECT host_key, name, value, encrypted_value, path, expires_utc, is_secure FROM cookies
         WHERE host_key = 'youtube.com' OR host_key LIKE '%.youtube.com'
            OR host_key = 'google.com' OR host_key LIKE '%.google.com'")?;
    let rows = statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))?
        .collect::<rusqlite::Result<Vec<Row>>>()?;
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    let rows = rows.into_iter().filter(|r| r.5 == 0 || r.5 / 1_000_000 - 11_644_473_600 > now).collect();
    Ok((version, rows))
}

fn has_signin(rows: &[Row]) -> bool {
    rows.iter().any(|r| applies_to_music(&r.0) && matches!(r.1.as_str(), "SAPISID" | "__Secure-3PAPISID"))
}

fn read_profile(candidate: &Candidate) -> Result<Option<Session>> {
    let (version, rows) = snapshot(candidate)?;
    if !has_signin(&rows) { return Ok(None) }
    #[cfg(target_os = "macos")]
    let (prefix, iterations) = (b"v10", 1003);
    #[cfg(target_os = "linux")]
    let (prefix, iterations) = (b"v11", 1);
    let key = if rows.iter().any(|r| r.3.starts_with(prefix)) {
        Some(derive_key(&keyring_password(candidate.browser.keyring)?, iterations))
    } else { None };
    let mut cookies = Vec::new();
    for (host, name, value, encrypted, path, expires_utc, secure) in rows {
        let value = if encrypted.is_empty() { Some(value) } else {
            #[cfg(target_os = "linux")]
            let cookie_key = if encrypted.starts_with(b"v10") { Some(derive_key(b"peanuts", 1)) } else { key };
            #[cfg(target_os = "macos")]
            let cookie_key = encrypted.starts_with(b"v10").then_some(key).flatten();
            cookie_key.and_then(|key| decrypt(&encrypted, &key, &host, version))
        };
        let Some(value) = value else { continue };
        if value.contains(['\r', '\n', '\0']) { continue }
        cookies.push(Cookie { host, name, value, path, secure,
            expires: if expires_utc == 0 { 0 } else { expires_utc / 1_000_000 - 11_644_473_600 },
        });
    }
    let session = Session { source: candidate.label(), profile: candidate.id(), cookies };
    if session.sapisid().is_none_or(str::is_empty) {
        bail!("The browser's YouTube cookies could not be decrypted. Sign in again, then Reconnect");
    }
    Ok(Some(session))
}

#[cfg(target_os = "macos")]
fn keyring_password(service: &str) -> Result<Vec<u8>> {
    let output = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-w", "-s", service])
        .stdin(Stdio::null()).stderr(Stdio::null()).output()
        .context("Could not open macOS Keychain")?;
    if !output.status.success() || output.stdout.is_empty() {
        bail!("Allow access to {service} in Keychain, then Reconnect. No other account was selected");
    }
    Ok(trim_command_newline(output.stdout))
}

#[cfg(target_os = "linux")]
fn keyring_password(application: &str) -> Result<Vec<u8>> {
    let output = Command::new("secret-tool").args(["lookup", "application", application])
        .stdin(Stdio::null()).stderr(Stdio::null()).output()
        .context("running secret-tool (libsecret)")?;
    let password = trim_command_newline(output.stdout);
    // Retain Chromium's Linux fallback. It is NEVER used on macOS.
    Ok(if password.is_empty() { b"peanuts".to_vec() } else { password })
}

fn trim_command_newline(mut password: Vec<u8>) -> Vec<u8> {
    if password.last() == Some(&b'\n') { password.pop(); }
    password
}

fn derive_key(password: &[u8], iterations: u32) -> [u8; 16] {
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, b"saltysalt", iterations, &mut key);
    key
}

fn decrypt(encrypted: &[u8], key: &[u8; 16], host: &str, version: i64) -> Option<String> {
    let body = match encrypted.get(..3) {
        Some(b"v10") | Some(b"v11") => &encrypted[3..],
        _ => return None,
    };
    let decryptor = cbc::Decryptor::<aes::Aes128>::new(key.into(), &[b' '; 16].into());
    let plain = decryptor.decrypt_padded_vec_mut::<Pkcs7>(body).ok()?;
    let value = if version >= 24 {
        if plain.get(..32)? != &sha2::Sha256::digest(host.as_bytes())[..] { return None }
        &plain[32..]
    } else { &plain };
    String::from_utf8(value.to_vec()).ok()
}

#[cfg(test)]
mod tests;
