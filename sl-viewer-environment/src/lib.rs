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
//! - [`settings_editor`] — the fixed sky and water editors, over a settings
//!   **asset** in inventory: the same knobs on tabs, plus a name and
//!   Save / Save As / Revert.
//! - [`day_cycle_editor`] — the day-cycle editor, over a day-cycle asset: the
//!   keyframes of one track on a timeline, a scrubber that previews any time of
//!   day, and the same knob tabs over whichever keyframe is selected.
//! - [`my_environments`] — the My Environments library: every settings asset in
//!   inventory, filterable by kind and name, with apply-to-self, edit, rename
//!   and delete, and the three creators that mint a fresh sky, water or day.
//! - [`settings_picker`] — the chooser another panel summons for one settings
//!   field, over the same list narrowed to one kind.
//! - [`land_environment`] — the panel the Region / Estate and About Land
//!   floaters host: not an editor at all but a **publisher**, writing the
//!   `ExtEnvironment` capability so the land itself carries the environment.
//! - [`bulk_import`] — World ▸ Environment ▸ Bulk Import: a whole folder of
//!   pre-EEP WindLight presets converted and filed as settings assets in one
//!   go. No window of its own — a folder chooser, a progress-free run, and a
//!   summary.
//!
//! The rows those last two draw are one projection ([`settings_list`]): the
//! library and the picker differ in their chrome and in what a pick does, and
//! not at all in what a row is.
//!
//! The knobs themselves are one table ([`knobs`]) and the controls that draw
//! them one set of spawners ([`rows`]), so a value cannot be labelled or scaled
//! one way in one window and another way in the next. Which knob sits on which
//! tab is a table too ([`tabs`]), shared by the two windows that show a frame's
//! pages — as the reference shares the panels themselves.
//!
//! # Which layer an editor writes
//!
//! The reference viewer's Personal Lighting floater edits its `ENV_LOCAL` layer
//! and only that, which is what makes it safe to leave open while walking
//! around: a region change replaces the *shared* environment underneath, and
//! whatever the user is holding on top of it survives. The settings editors
//! write the layer **above** it, `ENV_EDIT`, which is what makes *their*
//! preview non-destructive: it is taken away again when the window closes, and
//! the personal environment underneath is still whatever it was.
//!
//! For the local layer — every control writes
//! [`EnvironmentState::set_local_instant`](sl_viewer_world_scene::environment::EnvironmentState::set_local_instant),
//! and the sky, water, terrain and fog drivers pick it up on the next frame with
//! no editor-specific path through the renderer at all.

#![expect(
    clippy::module_name_repetitions,
    reason = "each window's module is named for the window it draws, and its floater-spec \
              constructor must be `<module>_floater_spec` for the viewer's registry guard to \
              find it — so the repetition is the protocol, not an accident of naming"
)]

pub mod bulk_import;
pub mod day_cycle_editor;
pub mod knobs;
pub mod land_environment;
pub mod my_environments;
pub mod personal_lighting;
pub mod rows;
pub mod settings_editor;
pub mod settings_list;
pub mod settings_picker;
pub mod tabs;

use bevy::prelude::*;
use sl_client_bevy::{FolderType, InventoryFolderKey};
use sl_viewer_inventory::inventory::InventoryModel;

/// The folder a fresh settings item goes in: the Settings system folder, or
/// the agent's root when the skeleton has no such folder.
///
/// Crate-level because two unrelated surfaces mint settings items into it —
/// the WindLight bulk importer ([`bulk_import`]) and the day-cycle editor's
/// Save As when the cycle it is holding belongs to *land* rather than to an
/// item ([`day_cycle_editor`]) — and a second copy is a place for the two to
/// disagree about where a new environment lands.
pub(crate) fn settings_destination(inventory: &InventoryModel) -> Option<InventoryFolderKey> {
    inventory
        .folder_by_type(FolderType::Settings)
        .or_else(|| inventory.agent_root())
}

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

    /// A list row's height, logical px.
    pub(crate) const ROW_HEIGHT: f32 = 20.0;

    /// A scrolling list's backdrop.
    pub(crate) const LIST_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);

    /// A selected row's background.
    pub(crate) const SELECTED_BACKGROUND: Color = Color::srgba(0.24, 0.34, 0.52, 0.55);
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
        if !app.is_plugin_added::<land_environment::LandEnvironmentPlugin>() {
            app.add_plugins(land_environment::LandEnvironmentPlugin);
        }
        app.add_plugins(personal_lighting::PersonalLightingPlugin)
            .add_plugins(settings_editor::SettingsEditorPlugin)
            .add_plugins(day_cycle_editor::DayCycleEditorPlugin)
            .add_plugins(my_environments::MyEnvironmentsPlugin)
            .add_plugins(settings_picker::SettingsPickerPlugin)
            .add_plugins(bulk_import::WindlightBulkImportPlugin);
    }
}

#[cfg(test)]
mod tests {
    use super::EnvironmentUiPlugins;
    use bevy::prelude::*;
    use sl_viewer_ui_core::i18n::install_untranslated;
    use sl_viewer_ui_core::ui::UiRoot;
    use sl_viewer_ui_widgets::floater::FloaterPlugin;
    use sl_viewer_ui_widgets::ui_color_picker::ColorPicked;
    use sl_viewer_world_api::TexturePicked;

    /// **Every window in this crate can actually be scheduled.**
    ///
    /// This is not a layout check — the floater sweep already measures the
    /// chrome. It is the check that the *systems* run at all, and it exists
    /// because a system whose two queries overlap
    /// (`Query<&mut Text>` beside `Query<(&mut Text, &mut TextColor)>`, which is
    /// easy to reach for once a window writes both cells and a label) panics
    /// with Bevy's `B0001` **the first time it runs** — and takes the whole
    /// viewer down with it, on the first frame, before a person can see
    /// anything. Every other test in this crate is over pure functions, and the
    /// viewer's own floater sweep builds chrome from the specs without adding
    /// these plugins, so nothing here had ever scheduled them.
    ///
    /// It catches a second failure of the same shape, and that one is why the
    /// world below is stood up rather than left bare: a `MessageReader` or
    /// `MessageWriter` for a message **nothing registered** fails validation the
    /// same way, and Bevy's default handler turns that into a panic too. So a
    /// window that writes a message its plugin forgot to `add_message` is not a
    /// button that quietly does nothing — it is the viewer falling over. The
    /// seams here are the ones the *host* owns (the two picker replies, the
    /// session channels, the locale); everything a window in this crate speaks
    /// over itself must be registered by its own plugin, and is.
    #[test]
    fn every_environment_window_schedules_without_conflicting() {
        let mut app = App::new();
        app.init_resource::<UiScale>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<bevy::input_focus::InputFocus>()
            // The clock the day-cycle editor's playback reads. A `Res<T>` for a
            // resource nothing inserted fails param validation exactly as an
            // unregistered message does, so the host's clock is one of the seams
            // this test has to bring.
            .init_resource::<Time>()
            // The seams the host brings: the two picker replies the swatches are
            // answered on, and the session's command / event channels.
            .add_message::<ColorPicked>()
            .add_message::<TexturePicked>()
            .add_message::<sl_client_bevy::SlCommand>()
            .add_message::<sl_client_bevy::SlEvent>()
            .add_plugins((FloaterPlugin, EnvironmentUiPlugins));
        // Every key resolves to itself, which is all a scheduling check needs —
        // and without it `Translator` has no `Localization` to read.
        install_untranslated(&mut app);
        let root = app.world_mut().spawn(Node::default()).id();
        app.insert_resource(UiRoot(root));
        // Two frames rather than one: the windows defer their content to the
        // first open, and the systems that bind it only exist to run on a later
        // frame than the one that spawned the chrome.
        app.update();
        app.update();
    }
}
