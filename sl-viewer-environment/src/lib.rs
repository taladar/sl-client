//! The viewer's **environment editors**: the windows a person opens to change
//! the sky and the water they are standing under.
//!
//! The environment itself — the grid's EEP settings, the day cycle, the local
//! override layer they stack on — lives in `sl-viewer-world-scene`, and the
//! renderers pull from it every frame. This crate is the other half: the
//! surfaces that *write* that override layer.
//!
//! - [`personal_lighting`] — the Personal Lighting floater. Sliders and swatches
//!   over the sky and water in force, applied **locally**: the region's own
//!   settings are untouched, nothing is published, and Reset gives them back.
//!
//! # Why an editor writes the local layer and nothing else
//!
//! The reference viewer's Personal Lighting floater edits its `ENV_LOCAL` layer
//! and only that, which is what makes it safe to leave open while walking
//! around: a region change replaces the *shared* environment underneath, and
//! whatever the user is holding on top of it survives. The same arrangement
//! here — every control writes
//! [`EnvironmentState::set_local_instant`](sl_viewer_world_scene::environment::EnvironmentState::set_local_instant),
//! and the sky, water, terrain and fog drivers pick it up on the next frame with
//! no editor-specific path through the renderer at all.

#![expect(
    clippy::module_name_repetitions,
    reason = "each window's module is named for the window it draws, and its floater-spec \
              constructor must be `<module>_floater_spec` for the viewer's registry guard to \
              find it — so the repetition is the protocol, not an accident of naming"
)]

pub mod personal_lighting;

use bevy::prelude::*;

/// The shared palette and geometry the environment editors are drawn with — the
/// values the sibling floaters already use, kept in one place so the family
/// reads as one.
pub(crate) mod style {
    use bevy::prelude::Color;

    /// Label / readout font size, logical px.
    pub(crate) const FONT_SIZE: f32 = 13.0;

    /// A section heading's font size, logical px.
    pub(crate) const HEADING_SIZE: f32 = 14.0;

    /// The default label colour.
    pub(crate) const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

    /// The dimmed heading / secondary colour.
    pub(crate) const DIM_LABEL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

    /// A control's border colour.
    pub(crate) const CONTROL_BORDER: Color = Color::srgba(0.34, 0.40, 0.52, 1.0);

    /// A slider track's fill.
    pub(crate) const TRACK_FILL: Color = Color::srgba(0.12, 0.13, 0.16, 1.0);

    /// A slider thumb's fill.
    pub(crate) const THUMB_FILL: Color = Color::srgb(0.72, 0.76, 0.84);

    /// An action button's background.
    pub(crate) const ACTION_BACKGROUND: Color = Color::srgb(0.24, 0.29, 0.38);
}

/// Every environment editor at once, for a host that wants the whole family.
///
/// The viewer adds the plugins individually alongside its other floaters; this
/// exists so a smaller host (the gallery, a test app) can take the set in one
/// line without having to know how many there are.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnvironmentUiPlugins;

impl Plugin for EnvironmentUiPlugins {
    fn build(&self, app: &mut App) {
        app.add_plugins(personal_lighting::PersonalLightingPlugin);
    }
}
