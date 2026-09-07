//! The viewer's **RLVa control surface**: the windows a person opens to see
//! what an object has done to their viewer, and to do it to themselves on
//! purpose.
//!
//! The `sl-rlv` crate is the engine — it decodes the `@`-command language,
//! holds what the held commands mean, and answers what a restriction allows.
//! [`sl_viewer_world_api::rlv`] is where the viewer keeps one of those state
//! machines and the settings that steer it. This crate is the four windows the
//! reference's RLVa menu opens onto that state:
//!
//! - [`rlv_console`] — type RLV commands at your own viewer and watch what
//!   happens to each one. The debugging and authoring tool, and the only place
//!   in the viewer where the agent talks `@` to itself.
//! - [`rlv_behaviours`] — the live list of what is in force: the restrictions
//!   grouped by the object holding each, the exceptions poked in them, and the
//!   modifier slots with the values objects have written.
//! - [`rlv_locks`] — the lock model's four registries, which is the question a
//!   yes/no restriction cannot answer: not "is detaching blocked" but "may
//!   *this* come off".
//! - [`rlv_strings`] — the canned texts RLVa emits, and the user's rewrites of
//!   them.
//!
//! # Why the windows are here and the state is not
//!
//! The floaters *draw* the RLV state, but the chat bar, the session command
//! path and every wear path have to **ask** it — and none of those may depend
//! on the crate that draws windows. So the state lives one tier down, in
//! `sl-viewer-world-api`, and this crate only reads it. That is the same split
//! the selection, the mute list and the presence modes already follow.
//!
//! # Every window is gated on the master switch
//!
//! `RestrainedLove` is off until the user turns it on, and while it is off the
//! four windows are unreachable: the RLVa menu itself is the only entry point,
//! and it hides. That is the reference's arrangement and it is the right one —
//! a viewer that can be restrained by a worn object should never become one by
//! accident.

#![expect(
    clippy::module_name_repetitions,
    reason = "each window's module is named for the RLVa window it draws, and its \
              floater-spec constructor must be `<module>_floater_spec` for the \
              viewer's registry guard to find it — so the repetition is the \
              protocol, not an accident of naming"
)]

pub mod rlv_behaviours;
pub mod rlv_console;
pub mod rlv_locks;
pub mod rlv_strings;

use bevy::prelude::*;

/// The shared palette and geometry the four RLVa windows are drawn with — the
/// values the sibling list floaters (the block list, the radar, the asset
/// blacklist) already use, kept in one place so the family reads as one.
pub(crate) mod style {
    use bevy::prelude::Color;

    /// Header / cell font size, logical px.
    pub(crate) const FONT_SIZE: f32 = 13.0;

    /// Table row height, logical px.
    pub(crate) const ROW_HEIGHT: f32 = 20.0;

    /// The default cell / label colour.
    pub(crate) const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

    /// The dimmed header / secondary colour.
    pub(crate) const DIM_LABEL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

    /// A list viewport's backdrop.
    pub(crate) const LIST_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);

    /// An action button's background.
    pub(crate) const ACTION_BACKGROUND: Color = Color::srgb(0.24, 0.29, 0.38);

    /// The colour a refused command is written in.
    pub(crate) const ERROR_COLOR: Color = Color::srgb(0.92, 0.56, 0.52);

    /// The colour an accepted command is written in.
    pub(crate) const INFO_COLOR: Color = Color::srgb(0.60, 0.82, 0.66);
}

/// Every RLVa window at once, for a host that wants the whole family.
///
/// The viewer adds the four plugins individually alongside its other floaters;
/// this exists so a smaller host (the gallery, a test app) can take the set in
/// one line without having to know how many there are.
#[derive(Debug, Clone, Copy, Default)]
pub struct RlvUiPlugins;

impl Plugin for RlvUiPlugins {
    fn build(&self, app: &mut App) {
        // The one state machine every RLV surface consults. Initialised here
        // rather than in `sl-viewer-world-api` because that crate holds types,
        // not schedules — the same reason the selection and the mute list are
        // initialised by the features that fill them.
        app.init_resource::<sl_viewer_world_api::rlv::RlvSession>();
        app.add_plugins((
            rlv_console::RlvConsolePlugin,
            rlv_behaviours::RlvBehavioursPlugin,
            rlv_locks::RlvLocksPlugin,
            rlv_strings::RlvStringsPlugin,
        ));
    }
}
