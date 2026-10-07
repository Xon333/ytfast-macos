//! ytfast: a native YouTube Music client. See docs/SPEC.md and docs/MACOS.md.

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("ytfast currently supports Linux and macOS");

pub mod account;
pub mod app;
pub mod auth;
pub mod backend;
pub mod control;
pub mod covers;
pub mod derived;
pub mod desktop;
#[cfg(feature = "e2e")]
pub mod e2e;
pub mod equalizer;
pub mod heat;
pub mod icons;
pub mod innertube;
pub mod links;
pub mod lyrics;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod model;
#[cfg(target_os = "linux")]
pub mod mpris;
pub mod mpv;
#[cfg_attr(target_os = "macos", path = "notify_unavailable.rs")]
pub mod notify;
pub mod palette;
pub mod parse;
pub mod paths;
pub mod platform;
pub mod resolver;
pub mod searches;
pub mod settings;
pub mod single_instance;
pub mod theme;
#[cfg(target_os = "linux")]
pub mod tray;
pub mod ui;
