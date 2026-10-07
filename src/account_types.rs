//! Account operation types shared by both front ends.

use crate::model::LikeStatus;

/// A write the backend makes to the account.
#[derive(Clone, Debug)]
pub enum Edit {
    Rate {
        video_id: String,
        status: LikeStatus,
    },
    /// Save an album (its audio playlist) or a playlist to the library, or remove it.
    Save {
        playlist_id: String,
        save: bool,
    },
    Subscribe {
        channel_id: String,
        subscribe: bool,
    },
    Create {
        title: String,
        description: String,
        video_ids: Vec<String>,
    },
    Add {
        playlist_id: String,
        video_ids: Vec<String>,
    },
    Remove {
        playlist_id: String,
        video_id: String,
        set_video_id: String,
    },
    /// Move an entry before `before` (the end when `None`).
    Move {
        playlist_id: String,
        set_video_id: String,
        before: Option<String>,
    },
    Details {
        playlist_id: String,
        title: Option<String>,
        description: Option<String>,
    },
    Delete {
        playlist_id: String,
    },
}

/// What a successful edit returned.
#[derive(Clone, Debug)]
pub enum Done {
    Ok,
    /// A new playlist's id.
    Created(String),
    /// Songs added to a playlist: (video id, entry id).
    Added(Vec<(String, String)>),
}

/// Why an edit didn't happen.
#[derive(Clone, Debug)]
pub enum Failure {
    /// YouTube Music answered and said no (detail for Copy).
    Refused(String),
    AlreadyInPlaylist,
    Offline,
    SignedOut,
}
