//! An audio-only mpv process controlled over its JSON IPC socket.
//!
//! mpv plays the resolved stream URLs; ytfast keeps at most the current and
//! the next track in an mpv's playlist, so track changes are gapless. Smooth
//! mixes and Audition run more than one process at once ("decks"); every
//! process has a serial that tags its events, so the worker can tell whose
//! they are as the decks swap roles.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::OwnedWriteHalf;
use tokio::sync::{Mutex, mpsc, oneshot};

/// Serials of mpv processes, unique for the run.
static SERIAL: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub enum MpvEvent {
    Property {
        /// Entry active when mpv emitted this property, to reject stale events.
        entry: i64,
        name: String,
        data: Value,
    },
    /// A playlist entry finished: `eof`, `error`, `stop`, `quit` or `redirect`.
    EndFile {
        reason: String,
        entry: i64,
        error: Option<String>,
    },
    StartFile {
        entry: i64,
    },
    /// Decoding/output has restarted after loading or seeking this entry.
    PlaybackRestart {
        entry: i64,
    },
    /// The process exited.
    Died,
}

/// A command must leave the pending table even if its caller is cancelled,
/// writing fails, or mpv never sends a reply.
struct PendingCommand {
    id: u64,
    pending: Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
}

impl Drop for PendingCommand {
    fn drop(&mut self) {
        self.pending.lock().expect("pending lock").remove(&self.id);
    }
}

pub struct Mpv {
    serial: u64,
    writer: Mutex<OwnedWriteHalf>,
    next_id: AtomicU64,
    pending: Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    _child: tokio::process::Child,
}

/// Properties whose changes are reported as [`MpvEvent::Property`].
const OBSERVED: &[&str] = &[
    "time-pos",
    "duration",
    "pause",
    "playlist-pos",
    "idle-active",
    "paused-for-cache",
    "volume",
    "seeking",
];

// Reuse mpv's actual built-in embedding profile (etc/builtin.conf [libmpv]),
// rather than cloning its media-control/input setup. Disable only its unused
// console/overlay scripts. The reliable audio buffers and decode path stay put.
// These public options are supported by mpv 0.41+; no libmpv linkage is added.
const NATIVE_OPTIONS: &[&str] = &[
    "--profile=libmpv",
    "--load-scripts=no",
    "--load-stats-overlay=no",
    "--load-console=no",
    "--load-commands=no",
    "--load-auto-profiles=no",
    "--load-select=no",
    "--osd-level=0",
    "--autoload-files=no",
    "--demuxer-max-back-bytes=1MiB",
    "--cache-secs=60",
    "--demuxer-readahead-secs=60",
];

impl Mpv {
    /// Starts a process; its events arrive on `events` tagged with its
    /// [`serial`](Self::serial).
    pub async fn spawn(
        socket: &Path,
        volume: f64,
        events: mpsc::UnboundedSender<(u64, MpvEvent)>,
    ) -> Result<Arc<Self>> {
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let _ = std::fs::remove_file(socket);
        let mut child = tokio::process::Command::new(crate::platform::tool("mpv"))
            .args([
                "--idle=yes",
                "--no-video",
                "--no-terminal",
                "--no-config",
                "--ytdl=no",
                "--gapless-audio=yes",
                "--prefetch-playlist=yes",
                "--cache=yes",
                if cfg!(feature = "menubar") {
                    "--demuxer-max-bytes=4MiB"
                } else {
                    "--demuxer-max-bytes=64MiB"
                },
                "--audio-display=no",
                "--audio-client-name=ytfast",
                "--replaygain=no",
            ])
            .args(if cfg!(feature = "menubar") {
                NATIVE_OPTIONS
            } else {
                &[]
            })
            .args(if cfg!(test) { &["--ao=null"][..] } else { &[] })
            .env("PATH", crate::platform::tool_path())
            .arg(format!("--volume={volume}"))
            .arg(format!("--input-ipc-server={}", socket.display()))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("starting mpv")?;
        let stream = connect(socket, &mut child).await?;
        let (reader, writer) = stream.into_split();
        let pending: Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<Value>>>> = Arc::default();
        let pending_reader = pending.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            let mut entry = -1;
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if let Some(id) = message.get("request_id").and_then(Value::as_u64) {
                    if let Some(sender) = pending_reader.lock().expect("pending lock").remove(&id) {
                        let _ = sender.send(message);
                    }
                    continue;
                }
                let event = match message.get("event").and_then(Value::as_str) {
                    Some("property-change") => MpvEvent::Property {
                        entry,
                        name: message
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        data: message.get("data").cloned().unwrap_or(Value::Null),
                    },
                    Some("end-file") => MpvEvent::EndFile {
                        reason: message
                            .get("reason")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        entry: message
                            .get("playlist_entry_id")
                            .and_then(Value::as_i64)
                            .unwrap_or(-1),
                        error: message
                            .get("file_error")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    },
                    Some("start-file") => {
                        entry = message
                            .get("playlist_entry_id")
                            .and_then(Value::as_i64)
                            .unwrap_or(-1);
                        MpvEvent::StartFile { entry }
                    }
                    Some("playback-restart") => MpvEvent::PlaybackRestart { entry },
                    _ => continue,
                };
                if events.send((serial, event)).is_err() {
                    break;
                }
            }
            // Dropping the senders wakes every command immediately on EOF.
            pending_reader.lock().expect("pending lock").clear();
            let _ = events.send((serial, MpvEvent::Died));
        });
        let mpv = Arc::new(Self {
            serial,
            writer: Mutex::new(writer),
            next_id: AtomicU64::new(1),
            pending,
            _child: child,
        });
        // Observation registration has no reply dependencies. Submit it in
        // one ordered write instead of paying eight serial IPC round trips
        // before the first load can begin. Every reply is still checked.
        let observations: Vec<_> = OBSERVED
            .iter()
            .enumerate()
            .map(|(i, name)| json!(["observe_property", i + 1, name]))
            .collect();
        mpv.commands(&observations).await?;
        Ok(mpv)
    }

    /// A local IPC responder for account-barrier state tests. Native CI tests
    /// the playlist-clear command and actual transport against real mpv.
    #[cfg(test)]
    pub(crate) fn test_ipc(kept_entry: i64) -> (Arc<Self>, mpsc::UnboundedReceiver<Value>) {
        let (socket, peer) = UnixStream::pair().expect("local IPC pair");
        let (_reader, writer) = socket.into_split();
        let pending: Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<Value>>>> = Arc::default();
        let replies = pending.clone();
        let (sent, commands) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut lines = BufReader::new(peer).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let request: Value = serde_json::from_str(&line).expect("IPC request");
                let command = request["command"].clone();
                let data = if command == json!(["get_property", "playlist/0/id"]) {
                    json!(kept_entry)
                } else {
                    Value::Null
                };
                let _ = sent.send(command);
                if let Some(reply) = replies
                    .lock()
                    .expect("pending lock")
                    .remove(&request["request_id"].as_u64().expect("request id"))
                {
                    let _ = reply.send(json!({ "error": "success", "data": data }));
                }
            }
        });
        let child = tokio::process::Command::new("/usr/bin/true")
            .spawn()
            .expect("fixture child");
        (
            Arc::new(Self {
                serial: 0,
                writer: Mutex::new(writer),
                next_id: AtomicU64::new(1),
                pending,
                _child: child,
            }),
            commands,
        )
    }

    /// This process's serial: its events carry it.
    pub fn serial(&self) -> u64 {
        self.serial
    }

    /// Runs a command and returns its `data`.
    pub async fn command(&self, args: Value) -> Result<Value> {
        Ok(self.commands(&[args]).await?.remove(0))
    }

    /// Submit independent commands in order without waiting between writes.
    /// Request ids preserve reply identity even if mpv answers out of order;
    /// one timeout and the guards cover the entire batch, including cancel.
    async fn commands(&self, args: &[Value]) -> Result<Vec<Value>> {
        let mut waiting = Vec::with_capacity(args.len());
        let mut guards = Vec::with_capacity(args.len());
        let mut lines = Vec::new();
        for args in args {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let (sender, receiver) = oneshot::channel();
            self.pending
                .lock()
                .expect("pending lock")
                .insert(id, sender);
            guards.push(PendingCommand {
                id,
                pending: self.pending.clone(),
            });
            waiting.push(receiver);
            serde_json::to_writer(&mut lines, &json!({ "command": args, "request_id": id }))?;
            lines.push(b'\n');
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            self.writer
                .lock()
                .await
                .write_all(&lines)
                .await
                .context("writing to mpv")?;
            let mut results = Vec::with_capacity(waiting.len());
            for receiver in waiting {
                let reply = receiver.await.map_err(|_| anyhow!("mpv closed"))?;
                match reply.get("error").and_then(Value::as_str) {
                    Some("success") => {
                        results.push(reply.get("data").cloned().unwrap_or(Value::Null));
                    }
                    Some(error) => bail!("mpv: {error}"),
                    None => bail!("mpv: malformed reply"),
                }
            }
            Ok(results)
        })
        .await
        .map_err(|_| anyhow!("mpv did not answer"))?
    }

    pub async fn set(&self, property: &str, value: Value) -> Result<()> {
        self.command(json!(["set_property", property, value]))
            .await
            .map(|_| ())
    }

    pub async fn get(&self, property: &str) -> Result<Value> {
        self.command(json!(["get_property", property])).await
    }

    /// Loads `url` (`replace` or `append`) with per-file options: the
    /// request headers the stream needs, its loudness gain, a start time.
    /// mpv applies them when the file starts and restores them when it ends,
    /// so each holds for its own song, gapless handoff included.
    pub async fn load(&self, url: &str, mode: &str, options: &[(&str, String)]) -> Result<i64> {
        // mpv's option lists split on commas; `%N%value` quotes a value of N bytes.
        let options = options
            .iter()
            .map(|(name, value)| format!("{name}=%{}%{value}", value.len()))
            .collect::<Vec<_>>()
            .join(",");
        let data = self
            .command(json!(["loadfile", url, mode, -1, options]))
            .await?;
        Ok(data
            .get("playlist_entry_id")
            .and_then(Value::as_i64)
            .unwrap_or(-1))
    }
}

async fn connect(socket: &Path, child: &mut tokio::process::Child) -> Result<UnixStream> {
    let path = PathBuf::from(socket);
    for _ in 0..250 {
        if let Ok(stream) = UnixStream::connect(&path).await {
            return Ok(stream);
        }
        if let Ok(Some(status)) = child.try_wait() {
            bail!("mpv exited ({status})");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    bail!("mpv's control socket did not appear")
}

#[cfg(all(test, feature = "menubar"))]
#[path = "mpv_tests.rs"]
mod tests;
