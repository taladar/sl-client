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

    use sl_viewer_ui_core::skin_palette::SkinPalette;

    /// Label / readout font size, logical px.
    pub(crate) const FONT_SIZE: f32 = 13.0;

    /// A section heading's font size, logical px.
    pub(crate) const HEADING_SIZE: f32 = 14.0;

    /// The heading size that goes with body text at `font_size`: the same step
    /// above it that [`HEADING_SIZE`] is above [`FONT_SIZE`], so a specimen swept
    /// at a larger UI font keeps its headings a step over the rows under them
    /// (and the live window, at [`FONT_SIZE`], gets [`HEADING_SIZE`] exactly).
    pub(crate) fn heading_size(font_size: f32) -> f32 {
        font_size + (HEADING_SIZE - FONT_SIZE)
    }

    /// The default label colour.
    pub(crate) const LABEL_COLOR: Color = SkinPalette::FALLBACK.text_primary;

    /// The dimmed heading / secondary colour.
    pub(crate) const DIM_LABEL_COLOR: Color = SkinPalette::FALLBACK.text_muted;

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
}

/// Shared plumbing for this crate's gallery / `ui_test` floater specimens.
///
/// A specimen is built by its window's own content builder, and then shown some
/// sample data **by the window's own systems**: the specimen installs the state
/// those systems read (a sample session, a sample list) and runs them once over
/// the freshly built widgets. Neither the gallery nor the layout sweep adds this
/// crate's plugins, so nothing else would ever draw a value into them — and a
/// hand-written copy of "what the reseed would have done" is exactly the
/// look-alike a specimen must not be.
pub(crate) mod specimen {
    use bevy::ecs::system::{RunSystemError, RunSystemOnce as _};
    use bevy::prelude::*;

    /// Run `system` once over `world`, reporting — not swallowing — a failure.
    ///
    /// A failure here means a host stood the specimen up without a seam the
    /// system reads (a locale, a message channel), so its sample data is not
    /// drawn; the window's layout is still the real one, which is why it is an
    /// error to report rather than a reason to panic the gallery.
    pub(crate) fn run_once<Marker>(
        world: &mut World,
        window: &str,
        system: impl IntoSystem<(), (), Marker>,
    ) {
        report(window, world.run_system_once(system));
    }

    /// Register message channel `M` if the host has not — the same idempotent
    /// registration this crate's plugins make, for a specimen whose plugin was
    /// never added. Without it a live system that writes `M` fails validation
    /// and draws nothing.
    pub(crate) fn ensure_message<M: Message>(world: &mut World) {
        world.init_resource::<Messages<M>>();
    }

    /// Write every slider's value readout, as the shared rows plugin's
    /// [`sync_slider_rows`](crate::rows::sync_slider_rows) does each frame in
    /// the viewer — it composes every environment window's sliders, and no
    /// specimen host adds it. Run after the window's reseed has put its
    /// sliders on the sample values, or the readouts beside them stay blank.
    pub(crate) fn draw_slider_readouts(world: &mut World, window: &str) {
        run_once(world, window, crate::rows::sync_slider_rows);
    }

    /// Log a specimen system's failure, naming the window it was drawing.
    pub(crate) fn report(window: &str, result: Result<(), RunSystemError>) {
        if let Err(error) = result {
            error!("{window} specimen: its sample data could not be drawn: {error}");
        }
    }
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
    use super::{
        EnvironmentUiPlugins, day_cycle_editor, my_environments, personal_lighting,
        settings_editor, settings_picker,
    };
    use crate::knobs::SkyKnob;
    use bevy::prelude::*;
    use bevy::text::EditableText;
    use bevy::ui_widgets::SliderValue;
    use bevy_flair::style::components::ClassList;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::SkySettings;
    use sl_viewer_intents::TexturePicked;
    use sl_viewer_ui_core::i18n::install_untranslated;
    use sl_viewer_ui_core::ui::UiRoot;
    use sl_viewer_ui_core::ui_element::ElementCx;
    use sl_viewer_ui_core::virtual_list::VirtualRow;
    use sl_viewer_ui_widgets::floater::FloaterPlugin;
    use sl_viewer_ui_widgets::ui_color_picker::ColorPicked;

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

    /// A specimen spawner's signature, as the viewer's floater registry holds it.
    type SpecimenFn = fn(&mut Commands, Entity, ElementCx) -> Entity;

    /// An app standing one specimen up the way the gallery and the layout sweep
    /// do: **none** of this crate's plugins, only a locale — so whatever sample
    /// data shows up was drawn by the specimen itself, through the window's own
    /// systems.
    fn specimen_app(spawn: SpecimenFn) -> App {
        let mut app = App::new();
        app.init_resource::<UiScale>()
            .init_resource::<bevy::input_focus::InputFocus>();
        install_untranslated(&mut app);
        let slot = app.world_mut().spawn(Node::default()).id();
        {
            let world = app.world_mut();
            let mut commands = world.commands();
            spawn(&mut commands, slot, ElementCx::new());
            world.flush();
        }
        app.update();
        app
    }

    /// The value of every `EditableText` in the app.
    fn field_values(app: &mut App) -> Vec<String> {
        app.world_mut()
            .query::<&EditableText>()
            .iter(app.world())
            .map(|field| field.value().to_string())
            .collect()
    }

    /// The text of the node named `name`, if there is one.
    fn text_named(app: &mut App, name: &str) -> Option<String> {
        app.world_mut()
            .query::<(&Name, &Text)>()
            .iter(app.world())
            .find(|(node, _text)| node.as_str() == name)
            .map(|(_node, text)| text.0.clone())
    }

    /// The value of the slider named `name`, if there is one.
    fn slider_named(app: &mut App, name: &str) -> Option<f32> {
        app.world_mut()
            .query::<(&Name, &SliderValue)>()
            .iter(app.world())
            .find(|(node, _value)| node.as_str() == name)
            .map(|(_node, value)| value.0)
    }

    /// The text of the value readout beside the slider named `name`, if there
    /// is one.
    fn readout_named(app: &mut App, name: &str) -> Option<String> {
        let readout = app
            .world_mut()
            .query::<(&Name, &crate::rows::SliderRow)>()
            .iter(app.world())
            .find(|(node, _row)| node.as_str() == name)
            .map(|(_node, row)| row.readout)?;
        app.world().get::<Text>(readout).map(|text| text.0.clone())
    }

    /// How many pooled list rows the app holds.
    fn row_count(app: &mut App) -> usize {
        app.world_mut()
            .query::<&VirtualRow>()
            .iter(app.world())
            .count()
    }

    /// **The knob windows' specimens are seeded by the live reseed.** A sun
    /// elevation slider still at its range's bottom would mean the sample
    /// session never reached the widgets — the layout would be real and every
    /// number in it a lie.
    #[test]
    fn the_knob_specimens_show_their_sample_frame() {
        let sky = SkySettings::legacy_windlight_default("sample");
        let wanted = SkyKnob::SunElevation.read(&sky);
        let knob_windows: [(SpecimenFn, &str); 2] = [
            (
                personal_lighting::spawn_personal_lighting_specimen,
                "personal-lighting-sun-elevation:slider",
            ),
            (
                settings_editor::spawn_sky_settings_editor_specimen,
                "settings-editor-sky-sun-elevation:slider",
            ),
        ];
        for (spawn, slider) in knob_windows {
            let mut app = specimen_app(spawn);
            assert_eq!(slider_named(&mut app, slider), Some(wanted), "{slider}");
            // The readout is the shared rows plugin's to write, not the
            // window's: a blank one means the specimen skipped that half.
            let shown = readout_named(&mut app, slider).unwrap_or_default();
            assert!(!shown.is_empty(), "{slider}: its readout was never written");
        }
        let mut sky_editor = specimen_app(settings_editor::spawn_sky_settings_editor_specimen);
        assert!(field_values(&mut sky_editor).contains(&"Sample Sky".to_owned()));
        let mut water_editor = specimen_app(settings_editor::spawn_water_settings_editor_specimen);
        assert!(field_values(&mut water_editor).contains(&"Sample Water".to_owned()));
    }

    /// **The day-cycle specimen is drawn by all four of the live systems it
    /// runs**: the name field (the reseed), the readout (the chrome sync), the
    /// keyframe markers (the marker rebuild — the default keyframe, the three
    /// sample ones and the scrubber) and every slider's value (the shared rows
    /// sync).
    #[test]
    fn the_day_cycle_specimen_shows_its_sample_cycle() {
        let mut app = specimen_app(day_cycle_editor::spawn_day_cycle_editor_specimen);
        assert!(field_values(&mut app).contains(&"Sample Day".to_owned()));
        let readout = text_named(&mut app, "day-cycle-editor-time").unwrap_or_default();
        assert!(!readout.is_empty(), "the readout was never written");
        let blank_sliders = app
            .world_mut()
            .query::<(&Name, &crate::rows::SliderRow)>()
            .iter(app.world())
            .filter(|(_name, row)| {
                app.world()
                    .get::<Text>(row.readout)
                    .is_none_or(|text| text.0.is_empty())
            })
            .map(|(name, _row)| name.to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            blank_sliders,
            Vec::<String>::new(),
            "sliders with no readout"
        );
        let shown = app
            .world_mut()
            .query::<(&ClassList, &Node)>()
            .iter(app.world())
            .filter(|(classes, node)| {
                classes.contains("sk-day-marker") && node.display != Display::None
            })
            .count();
        assert_eq!(shown, 5);
    }

    /// **The list specimens pool their sample rows and write their lines**: six
    /// rows in the library, the two skies in a picker aimed at a sky field.
    #[test]
    fn the_list_specimens_show_their_sample_rows() {
        let mut library = specimen_app(my_environments::spawn_my_environments_specimen);
        assert_eq!(row_count(&mut library), 6);
        assert!(
            !text_named(&mut library, "my-environments-status")
                .unwrap_or_default()
                .is_empty(),
            "the count line was never written"
        );
        // Name order, and the selection's name in the rename field.
        assert!(field_values(&mut library).contains(&"Example Deep Water".to_owned()));

        let mut picker = specimen_app(settings_picker::spawn_settings_picker_specimen);
        assert_eq!(row_count(&mut picker), 2);
        assert!(
            !text_named(&mut picker, "settings-picker-count")
                .unwrap_or_default()
                .is_empty(),
            "the count line was never written"
        );
    }
}
