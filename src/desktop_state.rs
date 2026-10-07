//! Playback state shared by native menus and the desktop UI.

use crate::backend::Event;
use crate::model::{Playback, Track};
use tokio::sync::watch;

/// The queue (in play order) and playback state, as the interface has them.
#[derive(Clone, Debug, Default)]
pub struct Now {
    pub queue: Vec<Track>,
    pub playback: Playback,
}

impl Now {
    pub fn track(&self) -> Option<&Track> {
        self.playback.index.and_then(|i| self.queue.get(i))
    }
}

/// Mirrors an event bound for the interface into `now`.
pub(crate) fn observe(now: &watch::Sender<Now>, event: &Event) {
    match event {
        Event::Queue(queue) => now.send_modify(|n| n.queue.clone_from(queue)),
        Event::Playback(playback) => now.send_modify(|n| n.playback.clone_from(playback)),
        _ => {}
    }
}
