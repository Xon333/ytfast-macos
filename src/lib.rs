//! ytfast: a native YouTube Music client. See docs/SPEC.md and docs/MACOS.md.

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("ytfast currently supports Linux and macOS");

#[cfg(feature = "desktop-ui")]
pub mod account;
#[cfg(feature = "desktop-ui")]
pub mod app;
pub mod auth;
pub mod backend;
#[cfg(feature = "desktop-ui")]
pub mod control;
#[cfg(feature = "desktop-ui")]
pub mod covers;
#[cfg(feature = "desktop-ui")]
pub mod derived;
#[cfg(feature = "desktop-ui")]
pub mod desktop;
#[cfg(feature = "e2e")]
pub mod e2e;
pub mod equalizer;
pub mod heat;
#[cfg(feature = "desktop-ui")]
pub mod icons;
pub mod innertube;
pub mod links;
pub mod lyrics;
#[cfg(target_os = "macos")]
#[cfg(feature = "desktop-ui")]
pub mod macos;
pub mod model;
#[cfg(target_os = "linux")]
#[cfg(feature = "desktop-ui")]
pub mod mpris;
pub mod mpv;
#[cfg_attr(target_os = "macos", path = "notify_unavailable.rs")]
#[cfg(feature = "desktop-ui")]
pub mod notify;
#[cfg(feature = "desktop-ui")]
pub mod palette;
pub mod parse;
pub mod paths;
pub mod platform;
pub mod resolver;
pub mod searches;
pub mod settings;
pub mod single_instance;
#[cfg(feature = "desktop-ui")]
pub mod theme;
#[cfg(target_os = "linux")]
#[cfg(feature = "desktop-ui")]
pub mod tray;
#[cfg(feature = "desktop-ui")]
pub mod ui;

pub mod account_types;
pub mod desktop_state;
#[cfg(not(feature = "desktop-ui"))]
pub use account_types as account;
#[cfg(not(feature = "desktop-ui"))]
pub use desktop_state as desktop;
#[cfg(feature = "menubar")]
pub mod menubar;
