//! [`ProbeSources`]: where a state probe reads a model this crate cannot name.
//!
//! Most of what a probe reports sits in crates this one already depends on —
//! the identity, the region, the parcel, the camera mode, the selection, the
//! inventory, the notification history. A few models live in the viewer's
//! heavy crates instead (the conversations with the rendering stack, the
//! toasts with the audio, the scene's asset stores with the whole world), and
//! depending on those would pull CEF, GStreamer and the renderer into every
//! automation build. So the crate that owns each model exports a reader, and
//! the viewer's assembly — which already depends on all of them — registers it
//! here.
//!
//! A reader is a plain function over the world, called only when a probe is
//! asked for, so a registered source costs nothing while idle.

use bevy::prelude::*;
use sl_automation_proto::{ConversationReadout, TeleportReadout};
use sl_viewer_notifications::{NotificationId, ToastButton};

/// The notifications on screen, each with the buttons its card shows.
pub type LiveNotifications = Vec<(NotificationId, Vec<ToastButton>)>;

/// The readers for the models a state probe cannot reach itself, registered by
/// the app's assembly.
///
/// An absent reader means the app has no such model: a partial app (a fixture
/// world, a UI harness) reads what it has and says what it lacks.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct ProbeSources {
    /// Every open conversation and its transcript.
    pub conversations: Option<fn(&mut World) -> Vec<ConversationReadout>>,
    /// The notifications on screen, or queued to be, each with the buttons
    /// its card shows.
    pub live_notifications: Option<fn(&mut World) -> LiveNotifications>,
    /// The teleport the viewer's progress display tracks.
    pub teleport: Option<fn(&mut World) -> Option<TeleportReadout>>,
    /// The own L$ balance, once the grid said it.
    pub balance: Option<fn(&mut World) -> Option<i64>>,
    /// The scene's outstanding work: assets in flight or queued, and decoded
    /// work not yet built, across every store.
    pub scene_work: Option<fn(&mut World) -> u64>,
}

impl ProbeSources {
    /// The registered sources of `world`, or none when nothing registered any.
    #[must_use]
    pub fn of(world: &World) -> Self {
        world.get_resource::<Self>().copied().unwrap_or_default()
    }
}
