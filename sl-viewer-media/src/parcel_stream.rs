//! The parcel music stream, as the surfaces that do not own it see it.
//!
//! The stream player lives in `sl-viewer-audio` (`parcel_audio`), beside the
//! mixer it plays into; the **Nearby Media** window that lists it beside the
//! media-on-a-prim faces lives in `sl-viewer-world-view`, beside the faces.
//! Neither crate depends on the other, so what one says to the other is here,
//! in the crate both already stand on: a status the player publishes
//! ([`ParcelStreamStatus`]) and the requests it answers
//! ([`ParcelStreamRequest`]).
//!
//! The same split holds the window's id ([`NEARBY_MEDIA_FLOATER_ID`]): the
//! parcel-audio bar's button opens a window its crate cannot name any other
//! way.

use bevy::prelude::*;

/// The stable id of the Nearby Media floater — the reference's
/// `LLPanelNearByMedia`, opened from the parcel-audio bar.
pub const NEARBY_MEDIA_FLOATER_ID: &str = "nearby-media";

/// What the parcel music stream is doing, published every frame the player
/// changes it (change-guarded, so a reader's `is_changed` means something).
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct ParcelStreamStatus {
    /// The agent's parcel's music URL once it passed the media scheme
    /// allowlist; `None` while the parcel has none (or has not resolved).
    pub url: Option<url::Url>,
    /// The stream's "now playing" title (ICY metadata), when it sends one.
    pub title: Option<String>,
    /// Whether the player is running (connecting, buffering or playing).
    pub running: bool,
    /// Whether the music bus — the stream's one volume path — is muted.
    pub muted: bool,
    /// The music bus level, linear `[0, 1]`.
    pub volume: f32,
}

/// Something asked of the parcel music stream from outside its bar.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
#[expect(
    variant_size_differences,
    reason = "a four-byte level beside a one-byte flag; the message is Copy and \
              rare, so boxing or padding it would buy nothing"
)]
pub enum ParcelStreamRequest {
    /// Start the parcel's stream, as the bar's play button does — an explicit
    /// start, which also lifts the user's earlier stop of this URL.
    Play,
    /// Stop it, as the bar's stop button does — remembered for this URL, so
    /// autoplay does not bring it straight back.
    Stop,
    /// Mute or unmute the music bus.
    SetMuted(bool),
    /// Set the music bus level, linear `[0, 1]`.
    SetVolume(f32),
}

/// Register the vocabulary on `app` — idempotent, called by both sides so
/// neither has to be added first.
pub fn register_parcel_stream_vocabulary(app: &mut App) {
    app.init_resource::<ParcelStreamStatus>()
        .add_message::<ParcelStreamRequest>();
}
