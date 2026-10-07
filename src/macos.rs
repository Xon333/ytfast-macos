//! Main-thread menu-bar integration using the same pinned fastframe release
//! as the UI. The player state and transport remain in the existing backend.

use std::cell::RefCell;
use std::time::Duration;

use anyhow::Context;
use fastframe_tray::{Config, Event, MenuItem, Tray};
use tokio::sync::watch;

use crate::app::Window;
use crate::backend::Command;
use crate::desktop::{Now, Remote, Request};

thread_local! {
    static DESKTOP: RefCell<Option<Desktop>> = const { RefCell::new(None) };
}

struct Desktop {
    tray: Tray,
    remote: Remote,
    now: watch::Receiver<Now>,
    shown: Option<(Option<String>, bool)>,
}

pub fn start(remote: Remote, now: watch::Receiver<Now>, waker: fastframe_shell::Waker) -> anyhow::Result<()> {
    let tray = Tray::spawn(Config {
        id: "ytfast", title: "Music".into(), icon: icon_rgba, template_icon: Some(icon_rgba),
        menu: vec![
            MenuItem::action("show", "Show Music"),
            MenuItem::action("song", "Nothing playing"),
            MenuItem::Separator,
            MenuItem::action("toggle", "Play"),
            MenuItem::action("next", "Next"),
            MenuItem::action("previous", "Previous"),
            MenuItem::Separator,
            MenuItem::action("quit", "Quit Music"),
        ],
    }, move || waker.wake()).context("Could not initialize the macOS menu bar")?;
    DESKTOP.with(|slot| *slot.borrow_mut() = Some(Desktop { tray, remote, now, shown: None }));
    Ok(())
}

fn attach() {
    DESKTOP.with(|slot| {
        if let Some(desktop) = slot.borrow_mut().as_mut() { desktop.tray.attach(); }
    });
}

fn poll() {
    DESKTOP.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(desktop) = slot.as_mut() else { return };
        for event in desktop.tray.events() {
            match event {
                Event::Toggle | Event::Show | Event::Menu("show" | "song") => desktop.remote.request(Request::Show),
                Event::Menu("toggle") => desktop.remote.toggle(),
                Event::Menu("next") => desktop.remote.command(Command::Next),
                Event::Menu("previous") => desktop.remote.command(Command::Previous),
                Event::Menu("quit") => desktop.remote.request(Request::Quit),
                Event::Menu(_) => {},
            }
        }
        let shown = {
            let now = desktop.now.borrow_and_update();
            (now.track().map(|t| format!("{} · {}", t.title, t.artist_line())), now.playback.playing)
        };
        if desktop.shown.as_ref() != Some(&shown) {
            desktop.tray.set_label("song", shown.0.as_deref().unwrap_or("Nothing playing"));
            desktop.tray.set_label("toggle", if shown.1 { "Pause" } else { "Play" });
            for id in ["toggle", "next", "previous"] { desktop.tray.set_visible(id, shown.0.is_some()); }
            desktop.shown = Some(shown);
        }
    });
}

pub fn idle(duration: Duration) {
    // No RefCell borrow is held while AppKit handles callbacks.
    fastframe_tray::idle(duration);
    poll();
}

/// Native template mask; macOS supplies its menu-bar colour in either appearance.
fn icon_rgba(size: usize) -> Vec<u8> {
    let mut rgba = vec![0; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let (x0, y0) = ((x as f32 + 0.5) / size as f32, (y as f32 + 0.5) / size as f32);
            let stem = (0.58..0.69).contains(&x0) && (0.18..0.72).contains(&y0);
            let flag = (0.58..0.86).contains(&x0) && (0.18..0.30).contains(&y0);
            let head = ((x0 - 0.46) / 0.22).powi(2) + ((y0 - 0.73) / 0.16).powi(2) <= 1.0;
            if stem || flag || head { rgba[(y * size + x) * 4 + 3] = 255; }
        }
    }
    rgba
}

/// Delegates to the upstream window; only native events are added. This keeps
/// app.rs, including its headless state machine and playback timers, unchanged.
pub struct MacWindow {
    window: Window,
    attached: bool,
}

impl MacWindow {
    pub fn new(window: Window) -> Self { Self { window, attached: false } }
}

impl eframe::App for MacWindow {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if !self.attached { attach(); self.attached = true; }
        poll();
        eframe::App::logic(&mut self.window, ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.window, ui, frame);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        eframe::App::raw_input_hook(&mut self.window, ctx, raw);
        raw.events.retain(|event| {
            let egui::Event::Key { key, pressed: true, modifiers, .. } = event else { return true };
            if !modifiers.mac_cmd || modifiers.alt || modifiers.ctrl || modifiers.shift { return true }
            match key {
                egui::Key::Q => DESKTOP.with(|slot| {
                    if let Some(desktop) = slot.borrow().as_ref() { desktop.remote.request(Request::Quit); }
                }),
                egui::Key::W => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                // Control+M remains upstream's mini-player shortcut; Command+M minimizes.
                egui::Key::M => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                _ => return true,
            }
            false
        });
    }
}
