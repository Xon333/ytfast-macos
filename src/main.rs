use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use ytfast::app::{App, Window, WindowKind};
use ytfast::desktop::{Flags, Remote, Request};
use ytfast::single_instance::{self, Message};

const USAGE: &str = "usage: ytfast [command]

Without a command, opens Music (or brings back the running one).

  show               bring back the window
  toggle             play or pause
  play | pause
  next | previous
  like               like or unlike the playing song
  open <link>        open a YouTube Music or YouTube link (starts Music if needed)
  quit               quit Music, stopping playback
  reload-themes      reload the theme";

fn main() -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    ytfast::platform::finder_path()?;
    let started = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.first().map(String::as_str), Some("-h" | "--help" | "help")) {
        println!("{USAGE}");
        return Ok(());
    }
    let message = match args.first() {
        None => Message::Show,
        Some(word) => single_instance::Message::parse(word, args.get(1).map(String::as_str))
            .ok_or_else(|| anyhow!("Unknown command. {USAGE}"))?,
    };
    if let Message::Open(link) = &message
        && ytfast::links::target_from_link(link).is_none() {
        return Err(anyhow!("Not a YouTube Music or YouTube link"));
    }
    let paths = ytfast::paths::Paths::new()?;
    if single_instance::notify(&paths.runtime, &message) { return Ok(()) }
    let Some(_instance) = ytfast::platform::instance_lock(&paths.runtime)? else {
        // A concurrent first launch can own the lock before its socket exists.
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(100));
            if single_instance::notify(&paths.runtime, &message) { return Ok(()) }
        }
        return Err(anyhow!("Music is starting but has not answered. Try again"));
    };
    let link = match message {
        Message::Show => None,
        Message::Open(link) => Some(link),
        Message::ReloadThemes => return Ok(()),
        _ => return Err(anyhow!("Music isn't running")),
    };
    #[cfg(target_os = "macos")]
    ytfast::platform::check_dependencies()?;
    #[cfg(target_os = "macos")]
    let _session_files = {
        let files = ytfast::platform::SessionFiles(paths.runtime.clone());
        files.clear();
        files
    };
    fastframe_log::Logging::new("ytfast", env!("CARGO_PKG_VERSION"))
        .filter("ytfast=info,warn")
        .file(paths.cache.join("ytfast.log"))
        .panic_log(paths.cache.join("panics.log"))
        .init().map_err(|e| anyhow!("logging: {e}"))?;

    let waker = fastframe_shell::Waker::default();
    let backend = {
        let waker = waker.clone();
        ytfast::backend::Backend::start(paths.clone(), move || waker.wake())?
    };
    let flags = Arc::new(Flags::default());
    flags.notifications.store(
        cfg!(target_os = "linux") && ytfast::settings::Settings::load(&paths).notifications,
        std::sync::atomic::Ordering::Relaxed,
    );
    let (request_tx, requests) = std::sync::mpsc::channel();
    if let Some(link) = link { let _ = request_tx.send(Request::Open(link)); }
    let remote = Remote::new(backend.commands(), backend.now.clone(), request_tx.clone(), waker.clone());
    {
        let remote = remote.clone();
        single_instance::listen(&paths.runtime, move |message| remote.deliver(message))?;
    }
    #[cfg(target_os = "linux")]
    {
        ytfast::tray::start(&backend.runtime, remote.clone(), backend.now.clone(), flags.window_open.subscribe());
        ytfast::mpris::start(&backend.runtime, remote, backend.now.clone(), flags.clone(), paths.clone(), backend.http.clone());
    }
    #[cfg(target_os = "macos")]
    ytfast::macos::start(remote, backend.now.clone(), waker.clone())?;
    let app = App::new(backend, paths, flags, (request_tx, requests), waker.clone(), started);
    let shell = fastframe_shell::Shell::new(app, &waker);
    #[cfg(target_os = "macos")]
    let shell = shell.idle(ytfast::macos::idle);
    shell.run(|lease| {
        let kind = lease.peek(|app| app.window);
        let (name, options) = native_options(kind);
        eframe::run_native(name, options, Box::new(move |cc| {
            let mut app = lease.take(&cc.egui_ctx);
            app.attach(&cc.egui_ctx);
            #[cfg(target_os = "linux")]
            let window = Window(app);
            #[cfg(target_os = "macos")]
            let window = ytfast::macos::MacWindow::new(Window(app));
            Ok(Box::new(window))
        }))
    }).map_err(|e| anyhow!("{e}"))
}

fn native_options(kind: WindowKind) -> (&'static str, eframe::NativeOptions) {
    let viewport = match kind {
        WindowKind::Main => egui::ViewportBuilder::default()
            .with_app_id("ytfast").with_title("Music")
            .with_inner_size([1280.0, 820.0]).with_min_inner_size([900.0, 600.0]),
        WindowKind::Mini => egui::ViewportBuilder::default()
            .with_app_id("ytfast-mini").with_title("Music")
            .with_inner_size(ytfast::ui::mini::SIZE).with_min_inner_size([320.0, 120.0]),
    };
    #[cfg(target_os = "macos")]
    let viewport = if kind == WindowKind::Mini {
        viewport.with_window_level(egui::WindowLevel::AlwaysOnTop)
    } else { viewport };
    let name = match kind { WindowKind::Main => "ytfast", WindowKind::Mini => "ytfast-mini" };
    (name, eframe::NativeOptions { viewport, ..Default::default() })
}
