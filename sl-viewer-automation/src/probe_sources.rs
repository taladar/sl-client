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
//!
//! The file dialog is reached the same way, for the same reason: the dialog
//! service lives beside the audio device in the platform crate, and an
//! `AnswerFileDialog` request answers it through [`FileDialogAnswerer`].

use std::collections::BTreeMap;
use std::path::PathBuf;

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
    /// The scene's outstanding work by bucket: assets in flight or queued,
    /// and decoded work not yet built, across every store — the empty
    /// buckets left out.
    pub scene_work: Option<SceneWorkReader>,
    /// Answers the file dialog the viewer waits on.
    pub file_dialog: Option<FileDialogAnswerer>,
    /// What the scene's environment holds beyond the sky RLV reads: the
    /// water drawn, a manual cross-fade under way, the windows previewing.
    pub environment_scene: Option<fn(&mut World) -> SceneEnvironment>,
}

/// What the scene's environment holds beyond the sky the RLV slot publishes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SceneEnvironment {
    /// How far a manual change's cross-fade has got, while one runs.
    pub transition: Option<f32>,
    /// The water drawn.
    pub water: Option<SceneWater>,
    /// The windows previewing through the edit layer, the one on top last.
    pub previewing: Vec<String>,
}

/// The water the scene draws.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneWater {
    /// The water frame's name.
    pub name: String,
    /// The underwater fog density.
    pub fog_density: f32,
}

/// Answers the file dialog the viewer waits on with the path picked, or
/// `None` for Cancel.
pub type FileDialogAnswerer = fn(&mut World, Option<PathBuf>) -> FileDialogAnswer;

/// What came of answering a file dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileDialogAnswer {
    /// The waiting dialog was answered.
    Answered {
        /// The purpose its reply is tagged with.
        purpose: String,
        /// Its title.
        title: String,
        /// Whether it asked for a folder rather than a file.
        folder: bool,
    },
    /// No dialog is waiting yet.
    NothingWaiting,
    /// The viewer shows the desktop's own chooser, which only a person can
    /// answer.
    ShownOnDesktop,
}

/// Reads the scene's outstanding work by bucket.
pub type SceneWorkReader = fn(&mut World) -> BTreeMap<String, u64>;

impl ProbeSources {
    /// The registered sources of `world`, or none when nothing registered any.
    #[must_use]
    pub fn of(world: &World) -> Self {
        world.get_resource::<Self>().copied().unwrap_or_default()
    }
}
