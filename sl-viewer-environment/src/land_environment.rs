//! The **land environment panel** (`viewer-region-environment-panel`):
//! publishing an environment to a region or to a parcel.
//!
//! Every other window in this crate edits something the user is holding — the
//! local override layer, or a settings asset in their inventory. This one
//! writes to the **land**: the `ExtEnvironment` capability's PUT and DELETE,
//! scoped by `?parcelid=` and `?trackno=`, which is what makes an environment
//! everybody standing there sees. It is the reference's
//! `LLPanelEnvironmentInfo`, and like the reference it is *one* panel used
//! twice — the Region / Estate floater's **Environment** tab and About Land's,
//! differing in their scope and in four controls.
//!
//! # A panel, not a window
//!
//! The panel is spawned into somebody else's tab ([`spawn_land_environment_panel`]),
//! and the floater that hosts it keeps it pointed at its subject by writing
//! [`LandEnvironmentSubject`] whenever what it knows changes. Everything below
//! that — requesting the settings, drafting an edit, publishing it — is this
//! module's, driven by systems that iterate *every* panel entity. So two About
//! Land windows on two parcels each edit their own parcel, and neither can be
//! confused by the other's reply.
//!
//! # What a publish is
//!
//! The reference publishes on every change: a slider's mouse-up is a PUT, a
//! pick from the settings picker is a PUT. Here the controls fill a **draft**
//! and **Apply** publishes it, with **Revert** putting the grid's answer back.
//! Two reasons: this toolkit's slider has no end-of-drag signal, so
//! "publish per change" would mean a PUT per pixel of a drag; and a draft is
//! the only thing a Revert can revert to. What reaches the wire is the same
//! set of requests the reference sends, batched —
//!
//! - one PUT with no `trackno` carrying the day length, the day offset, the
//!   track altitudes (region scope only, as in the reference) and the
//!   whole-cycle `day_asset` when one was picked, and
//! - one PUT per **track** whose asset was picked, each scoped with that
//!   track's `trackno`.
//!
//! **Use Default Settings** / **Use Region Settings** is the DELETE
//! ([`Command::ResetEnvironment`]), behind the reference's own confirmation:
//! a region falls back to the grid default, a parcel to its region.
//!
//! # The estate's half of the parcel-override checkbox
//!
//! "Parcel Owners May Override" is not an environment setting at all — it is
//! the estate's `ALLOW_ENVIRONMENT_OVERRIDE` flag, sent with the rest of the
//! estate info. The panel therefore *asks* (the reference's
//! `EstateParcelEnvironmentOverride` confirmation) and then writes
//! [`AllowEnvironmentOverrideRequested`]; the Region / Estate floater, which is
//! the thing that holds the estate name and the rest of the flags, is what
//! turns that into a wire message.
//!
//! # Deliberate divergences
//!
//! - **The altitudes are three number fields, not a vertical multi-slider.**
//!   The reference drags three thumbs up a 100–4000 m track and shuffles the
//!   labels out of each other's way; the toolkit has no multi-slider, and the
//!   numbers are what the wire carries. They are sorted on Apply, as the
//!   simulator sorts them anyway.
//! - **A track's picker is filtered to that track's kind.** The reference's
//!   drop targets take anything and let the simulator sort it out; a water
//!   track being handed a sky is not a choice worth offering.
//! - **No in-place day-cycle editor.** The reference's "Customize Day Cycle"
//!   opens the day-cycle editor on the land's own cycle and takes its commit
//!   back. That needs the editor to hold a cycle that is not an inventory
//!   item, which is [`crate::day_cycle_editor`]'s to grow; until it does, a
//!   cycle is authored in inventory and published here.
//!
//! Reference (Firestorm, read-only): `llpanelenvironment.cpp`,
//! `panel_region_environment.xml`, `llfloaterregioninfo.cpp`
//! (`LLPanelRegionEnvironment`), `llfloaterland.cpp`
//! (`LLPanelLandEnvironment`), `llenvironment.cpp` (`coroUpdateEnvironment` /
//! `coroResetEnvironment`).

use std::time::{SystemTime, UNIX_EPOCH};

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{SliderRange, SliderValue, ValueChange};
use sl_client_bevy::{
    Command, EnvironmentSettings, EnvironmentUpdate, LandArea, Permissions, SettingsKind,
    SlCommand, SlEvent, SlSessionEvent, TRACK_MAX, Uuid,
};
use sl_viewer_inventory::inventory::InventoryModel;
use sl_viewer_notifications::{NotificationResponse, ShowNotification};
use sl_viewer_ui_core::i18n::{Translated, Translator};
use sl_viewer_ui_core::ui::{column, row};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_widgets::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use sl_viewer_world_api::{OpenSettingsPicker, SettingsPicked};

use crate::rows::{ButtonPaint, SliderRow, paint_action_button, spawn_action_button};
use crate::style::{
    ACTION_BACKGROUND, CONTROL_BORDER, DIM_LABEL_COLOR, FONT_SIZE, HEADING_SIZE, LABEL_COLOR,
    THUMB_FILL, TRACK_FILL,
};

/// The element-id prefix every control in a land-environment panel is named by.
const ELEMENT: &str = "land-environment";

/// A day's length, in hours, at each end of the reference's slider
/// (`sld_day_length`).
const DAY_LENGTH_HOURS: (f32, f32) = (4.0, 168.0);

/// A day's offset, in hours, at each end of the reference's slider
/// (`sld_day_offset`). Negative offsets are the reference's own display
/// convention: an offset over twelve hours is shown as the negative below it.
const DAY_OFFSET_HOURS: (f32, f32) = (-11.5, 12.0);

/// Seconds in an hour, as the sliders' unit conversion.
const SECONDS_PER_HOUR: f32 = 3600.0;

/// Seconds in a day, for the apparent time of day.
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

/// Seconds in an hour, as the apparent-time readout's divisor.
const SECONDS_PER_HOUR_I64: i64 = 3600;

/// The lowest and highest a sky-track breakpoint may sit, in metres — the range
/// of the reference's altitude multi-slider. Whole metres, because that is
/// what the panel's number fields edit.
const ALTITUDE_RANGE: (i32, i32) = (100, 4000);

/// The spacing Reset Altitudes puts the three breakpoints back to
/// (`ALTITUDE_DEFAULT_HEIGHT_STEP`).
const ALTITUDE_DEFAULT_STEP: i32 = 1000;

/// The smallest parcel that may carry an environment of its own
/// (`LLPanelEnvironmentInfo::MINIMUM_PARCEL_SIZE`).
const MINIMUM_PARCEL_AREA: LandArea = LandArea(128);

/// The confirmation in front of a reset (the reference's `SettingsConfirmReset`).
const RESET_CONFIRM: &str = "SettingsConfirmReset";

/// The confirmation in front of an estate parcel-override change.
const OVERRIDE_CONFIRM: &str = "EstateParcelEnvironmentOverride";

/// The glyph for a checked toggle.
const CHECKED_GLYPH: &str = "\u{2611}";

/// The glyph for an unchecked toggle.
const UNCHECKED_GLYPH: &str = "\u{2610}";

/// A checked toggle's tick colour.
const CHECK_COLOR: Color = Color::srgb(0.55, 0.85, 0.60);

/// A disabled control's text colour.
const DISABLED_COLOR: Color = Color::srgb(0.45, 0.47, 0.52);

// ---------------------------------------------------------------------------
// The panel's subject.
// ---------------------------------------------------------------------------

/// Which of the two panels this is — fixed when it is spawned, because the two
/// do not draw the same controls.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandPanelKind {
    /// The Region / Estate floater's Environment tab: the region's own
    /// environment, its sky-track altitudes, and the estate's
    /// parcel-override flag.
    Region,
    /// About Land's Environment tab: one parcel's environment. The altitudes
    /// are the region's and are shown read-only, as the reference shows them.
    Parcel,
}

impl LandPanelKind {
    /// Whether this panel may move the sky-track breakpoints. The reference
    /// enables the altitude slider on `isRegion()` alone, because the wire
    /// only carries `track_altitudes` on a region-scoped update.
    const fn owns_altitudes(self) -> bool {
        matches!(self, Self::Region)
    }
}

/// What the hosting floater knows and the panel cannot work out for itself,
/// written onto the panel entity whenever any of it changes.
///
/// Deliberately `Copy` and comparable: the host writes it unconditionally on
/// every refresh and the panel re-requests only when it actually moved, so a
/// floater that repaints every frame does not re-ask the grid every frame.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LandEnvironmentSubject {
    /// The parcel this panel publishes to, region-local. Always `None` for a
    /// [`LandPanelKind::Region`] panel; `None` for a parcel panel whose
    /// subject has not resolved yet.
    pub parcel_id: Option<i32>,
    /// Whether the window's region is the one the agent is standing in. A
    /// window on a region the agent has left takes no replies and publishes
    /// nothing: every `ExtEnvironment` request goes out on the current
    /// circuit, so it would land on the wrong region.
    pub live: bool,
    /// Whether the agent may publish here — estate rights for a region, the
    /// parcel's environment right for a parcel.
    pub editable: bool,
    /// Whether the estate lets parcels publish their own environment. The
    /// region panel shows it as a checkbox; the parcel panel is disabled
    /// without it.
    pub allow_override: bool,
    /// The parcel's area (parcel panels only).
    pub area: LandArea,
}

impl Default for LandEnvironmentSubject {
    /// A panel that has been told nothing yet: frozen, unbound, read-only.
    ///
    /// Spelled out rather than derived because [`LandArea`] has no `Default` —
    /// deliberately, since "no area" and "zero square metres" are the same
    /// number and only one of them is a parcel.
    fn default() -> Self {
        Self {
            parcel_id: None,
            live: false,
            editable: false,
            allow_override: false,
            area: LandArea::ZERO,
        }
    }
}

/// The Region / Estate floater is asked to set the estate's
/// `ALLOW_ENVIRONMENT_OVERRIDE` flag.
///
/// The panel owns the checkbox and the confirmation; the floater owns the
/// estate info the flag has to be sent with, so the write is its.
#[derive(Message, Debug, Clone, Copy)]
pub struct AllowEnvironmentOverrideRequested {
    /// The panel that asked, so a host with more than one can tell which
    /// window the user answered in.
    pub panel: Entity,
    /// What the flag should become.
    pub allow: bool,
}

// ---------------------------------------------------------------------------
// Tracks.
// ---------------------------------------------------------------------------

/// One publishable track of a land environment, in the order the panel draws
/// them — highest sky first, then the ground sky, then the water, as the
/// reference stacks them beside its altitude slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrackRow {
    /// The wire `trackno`: 0 is the water track and 1..=4 the sky tracks,
    /// ground up.
    track: i32,
    /// The row's translated label.
    label_key: &'static str,
    /// Which kind of settings asset may be picked for it.
    kind: SettingsKind,
    /// The index into the three altitude breakpoints this track *starts* at,
    /// or `None` for the two that have no breakpoint (ground and water).
    altitude: Option<usize>,
}

/// The five rows, top to bottom.
const TRACK_ROWS: [TrackRow; TRACK_MAX] = [
    TrackRow {
        track: 4,
        label_key: "land-env-track-sky-4",
        kind: SettingsKind::Sky,
        altitude: Some(2),
    },
    TrackRow {
        track: 3,
        label_key: "land-env-track-sky-3",
        kind: SettingsKind::Sky,
        altitude: Some(1),
    },
    TrackRow {
        track: 2,
        label_key: "land-env-track-sky-2",
        kind: SettingsKind::Sky,
        altitude: Some(0),
    },
    TrackRow {
        track: 1,
        label_key: "land-env-track-ground",
        kind: SettingsKind::Sky,
        altitude: None,
    },
    TrackRow {
        track: 0,
        label_key: "land-env-track-water",
        kind: SettingsKind::Water,
        altitude: None,
    },
];

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// A settings asset chosen for a field but not published yet.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ChosenSettings {
    /// The asset itself, which is what `day_asset` carries.
    asset_id: Uuid,
    /// Its name, which rides along as `day_name`.
    name: String,
    /// The `LLSettingsBase` restriction flags the item's own permissions imply.
    flags: u32,
}

/// The edit in progress: what Apply publishes and Revert throws away.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LandDraft {
    /// The day length in seconds.
    day_length: i32,
    /// The day offset in seconds.
    day_offset: i32,
    /// The three sky-track breakpoints, metres.
    altitudes: [i32; 3],
    /// A whole day cycle picked for the land, if any.
    day: Option<ChosenSettings>,
    /// Per-track assets picked, indexed by wire `trackno`.
    tracks: [Option<ChosenSettings>; TRACK_MAX],
}

/// One panel's whole state.
#[derive(Component, Debug, Default)]
struct LandEnvironmentState {
    /// The settings the grid last answered with for this land.
    current: Option<Box<EnvironmentSettings>>,
    /// The draft Apply publishes.
    draft: LandDraft,
    /// What the draft was seeded from, so "has the user edited this?" is a
    /// comparison rather than a flag per control.
    seeded: LandDraft,
    /// The subject as last acted on, so a request only goes out when it moves.
    seen: Option<LandEnvironmentSubject>,
    /// A request is outstanding for this scope (`-1` = the region).
    awaiting: Option<i32>,
    /// Re-seed every widget from the draft.
    reseed: bool,
}

impl LandEnvironmentState {
    /// Seed the draft (and the comparison base) from `settings`.
    fn seed_from(&mut self, settings: &EnvironmentSettings) {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            reason = "a sky-track breakpoint is a metre height in 100..=4000; the wire carries it \
                      as a real and the panel edits whole metres"
        )]
        let altitudes = [
            settings.track_altitudes[0].round() as i32,
            settings.track_altitudes[1].round() as i32,
            settings.track_altitudes[2].round() as i32,
        ];
        self.draft = LandDraft {
            day_length: settings.day_length,
            day_offset: settings.day_offset,
            altitudes,
            day: None,
            tracks: Default::default(),
        };
        self.seeded = self.draft.clone();
        self.reseed = true;
    }
}

// ---------------------------------------------------------------------------
// Widget tags.
// ---------------------------------------------------------------------------

/// Which of the two day sliders a track is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum DaySlider {
    /// The day length, in hours.
    Length,
    /// The day offset, in hours.
    Offset,
}

/// A control belonging to one panel: every tag carries its panel, because the
/// systems below are over all of them at once.
#[derive(Component, Debug, Clone, Copy)]
struct PanelOf(Entity);

/// An altitude field: which of the three breakpoints it edits.
#[derive(Component, Debug, Clone, Copy)]
struct AltitudeField(usize);

/// A "Use Inventory…" button: which field it picks for.
#[derive(Component, Debug, Clone, Copy)]
struct PickerButton {
    /// The wire `trackno` this button picks a track for, or `None` for the
    /// whole day cycle.
    track: Option<i32>,
    /// The kind the picker is filtered to.
    kind: SettingsKind,
}

/// One of the panel's plain action buttons.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum LandAction {
    /// Drop the stored settings (the DELETE), behind a confirmation.
    UseDefault,
    /// Put the three breakpoints back to 1000 / 2000 / 3000.
    ResetAltitudes,
    /// Publish the draft.
    Apply,
    /// Throw the draft away and seed from the grid's answer again.
    Revert,
}

/// The parcel-override checkbox.
#[derive(Component, Debug, Clone, Copy)]
struct OverrideCheck {
    /// The glyph node.
    glyph: Entity,
    /// The label node.
    label: Entity,
}

/// The retained nodes one panel writes through.
#[derive(Component, Debug)]
struct LandEnvironmentUi {
    /// The "why is this disabled" note, shown alone when it is.
    unavailable: Entity,
    /// Everything the note replaces.
    controls: Entity,
    /// The apparent-time readout.
    apparent_time: Entity,
    /// The per-track name readouts, indexed by wire `trackno`.
    track_names: [Entity; TRACK_MAX],
}

/// The confirmation the user is being asked, if any.
///
/// One slot for the whole viewer because these are modal alerts: a second
/// panel cannot raise one while the first is up, and
/// [`NotificationResponse`] names the template rather than the asker, so a
/// second pending confirmation of the same template would have nothing to
/// distinguish it by.
#[derive(Resource, Debug, Default)]
struct LandEnvironmentConfirm(Option<PendingConfirm>);

/// A confirmation in flight.
#[derive(Debug, Clone, Copy)]
struct PendingConfirm {
    /// The panel that asked.
    panel: Entity,
    /// What it asked about.
    action: ConfirmAction,
}

/// What a pending confirmation is guarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmAction {
    /// Drop this land's stored environment.
    Reset,
    /// Change the estate's parcel-override flag to this.
    Override(bool),
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin driving every land-environment panel in the app.
///
/// Added by each hosting floater's plugin behind an `is_plugin_added` guard,
/// the way the shared row widgets are: the viewer adds the two floaters
/// independently and whichever builds first wins.
#[derive(Debug, Clone, Copy, Default)]
pub struct LandEnvironmentPlugin;

impl Plugin for LandEnvironmentPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<crate::rows::RowsPlugin>() {
            app.add_plugins(crate::rows::RowsPlugin);
        }
        app.init_resource::<LandEnvironmentConfirm>()
            .add_message::<AllowEnvironmentOverrideRequested>()
            .add_message::<OpenSettingsPicker>()
            .add_message::<SettingsPicked>()
            .add_message::<ShowNotification>()
            .add_message::<NotificationResponse>()
            .add_systems(
                Update,
                // Ordered: a subject change has to reach the request before the
                // reply is looked for, and a reply has to reach the draft
                // before the widgets are re-seeded from it — otherwise the
                // panel's first frame after a bind shows the last land's
                // numbers.
                (
                    request_land_environment,
                    ingest_land_environment,
                    take_land_settings_pick,
                    read_altitude_fields,
                    reseed_land_widgets,
                    paint_land_controls,
                    resolve_land_confirmation,
                )
                    .chain(),
            );
    }
}

// ---------------------------------------------------------------------------
// Build.
// ---------------------------------------------------------------------------

/// Spawn a land-environment panel under `parent` and return its entity — what
/// the host writes [`LandEnvironmentSubject`] onto.
///
/// `tab_index` is the first focus stop the panel takes; it uses a run of them
/// from there.
pub fn spawn_land_environment_panel(
    commands: &mut Commands,
    parent: Entity,
    kind: LandPanelKind,
    tab_index: i32,
) -> Entity {
    let panel = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                ..column(Val::Px(6.0))
            },
            kind,
            LandEnvironmentSubject::default(),
            LandEnvironmentState::default(),
            ChildOf(parent),
        ))
        .id();
    let unavailable = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                display: Display::None,
                ..column(Val::Px(0.0))
            },
            ChildOf(panel),
        ))
        .with_child((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Pickable::IGNORE,
        ))
        .id();
    let controls = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                ..column(Val::Px(6.0))
            },
            ChildOf(panel),
        ))
        .id();

    let mut tab = tab_index;
    spawn_heading(commands, controls, "land-env-select-heading");
    let select_row = spawn_row(commands, controls);
    let default_key = match kind {
        LandPanelKind::Region => "land-env-use-default",
        LandPanelKind::Parcel => "land-env-use-region",
    };
    spawn_land_action(
        commands,
        select_row,
        panel,
        "use-default",
        default_key,
        LandAction::UseDefault,
        &mut tab,
    );
    spawn_picker_button(
        commands,
        select_row,
        panel,
        None,
        SettingsKind::DayCycle,
        &mut tab,
    );
    if kind.owns_altitudes() {
        spawn_override_check(commands, controls, panel);
    }

    spawn_heading(commands, controls, "land-env-day-heading");
    spawn_day_slider(
        commands,
        controls,
        panel,
        DaySlider::Length,
        "land-env-day-length",
        DAY_LENGTH_HOURS,
        &mut tab,
    );
    spawn_day_slider(
        commands,
        controls,
        panel,
        DaySlider::Offset,
        "land-env-day-offset",
        DAY_OFFSET_HOURS,
        &mut tab,
    );
    let time_row = spawn_labelled_row(commands, controls, "land-env-apparent-time");
    let apparent_time = spawn_value_node(commands, time_row);

    spawn_heading(commands, controls, "land-env-tracks-heading");
    let mut track_names = [Entity::PLACEHOLDER; TRACK_MAX];
    for row_spec in TRACK_ROWS {
        let track_row = spawn_labelled_row(commands, controls, row_spec.label_key);
        let name = spawn_value_node(commands, track_row);
        #[expect(
            clippy::indexing_slicing,
            reason = "every TRACK_ROWS entry's `track` is a wire track number in 0..TRACK_MAX, \
                      which is exactly this array's length"
        )]
        {
            track_names[usize::try_from(row_spec.track).unwrap_or(0)] = name;
        }
        spawn_picker_button(
            commands,
            track_row,
            panel,
            Some(row_spec.track),
            row_spec.kind,
            &mut tab,
        );
        if let Some(index) = row_spec.altitude {
            spawn_altitude_field(commands, track_row, panel, index, &mut tab);
        }
    }
    if kind.owns_altitudes() {
        let altitude_row = spawn_row(commands, controls);
        spawn_land_action(
            commands,
            altitude_row,
            panel,
            "reset-altitudes",
            "land-env-reset-altitudes",
            LandAction::ResetAltitudes,
            &mut tab,
        );
    }

    let commit_row = spawn_row(commands, controls);
    spawn_land_action(
        commands,
        commit_row,
        panel,
        "apply",
        "land-env-apply",
        LandAction::Apply,
        &mut tab,
    );
    spawn_land_action(
        commands,
        commit_row,
        panel,
        "revert",
        "land-env-revert",
        LandAction::Revert,
        &mut tab,
    );

    commands.entity(panel).insert(LandEnvironmentUi {
        unavailable,
        controls,
        apparent_time,
        track_names,
    });
    panel
}

/// A plain wrapping row.
fn spawn_row(commands: &mut Commands, parent: Entity) -> Entity {
    commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                ..row(Val::Px(8.0))
            },
            ChildOf(parent),
        ))
        .id()
}

/// A wrapping row leading with a translated dim label.
fn spawn_labelled_row(commands: &mut Commands, parent: Entity, label_key: &'static str) -> Entity {
    let row_entity = spawn_row(commands, parent);
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(row_entity),
    ));
    row_entity
}

/// A section heading on its own line.
fn spawn_heading(commands: &mut Commands, parent: Entity, key: &'static str) {
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(HEADING_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(parent),
    ));
}

/// An empty value node the panel writes in place.
fn spawn_value_node(commands: &mut Commands, parent: Entity) -> Entity {
    commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id()
}

/// One of the panel's action buttons, tagged with its panel and its action.
fn spawn_land_action(
    commands: &mut Commands,
    parent: Entity,
    panel: Entity,
    slug: &str,
    label_key: &str,
    action: LandAction,
    tab: &mut i32,
) -> Entity {
    let button = spawn_action_button(commands, parent, ELEMENT, slug, label_key.to_owned(), tab);
    commands
        .entity(button)
        .insert((PanelOf(panel), action))
        .observe(on_land_action);
    button
}

/// A "Use Inventory…" button for one field.
fn spawn_picker_button(
    commands: &mut Commands,
    parent: Entity,
    panel: Entity,
    track: Option<i32>,
    kind: SettingsKind,
    tab: &mut i32,
) -> Entity {
    let slug = match track {
        Some(track) => format!("pick-track-{track}"),
        None => "pick-day".to_owned(),
    };
    let button = spawn_action_button(
        commands,
        parent,
        ELEMENT,
        &slug,
        "land-env-use-inventory".to_owned(),
        tab,
    );
    commands
        .entity(button)
        .insert((PanelOf(panel), PickerButton { track, kind }))
        .observe(on_land_pick_pressed);
    button
}

/// One altitude breakpoint's number field.
fn spawn_altitude_field(
    commands: &mut Commands,
    parent: Entity,
    panel: Entity,
    index: usize,
    tab: &mut i32,
) -> Entity {
    let field = spawn_text_input(
        commands,
        parent,
        &TextInputSpec {
            font_size: FONT_SIZE,
            width_glyphs: 6.0,
            tab_index: *tab,
            max_characters: Some(5),
            ..TextInputSpec::new(ELEMENT, TextInputKind::Integer)
        },
    );
    *tab = tab.saturating_add(1);
    commands
        .entity(field)
        .insert((PanelOf(panel), AltitudeField(index)));
    field
}

/// The parcel-override checkbox: a clickable glyph leading a translated label.
fn spawn_override_check(commands: &mut Commands, parent: Entity, panel: Entity) {
    let row_entity = spawn_row(commands, parent);
    let glyph = commands
        .spawn((
            Text::new(UNCHECKED_GLYPH),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Pickable::IGNORE,
        ))
        .id();
    let label = commands
        .spawn((
            Text::default(),
            Translated::new("land-env-allow-override"),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ))
        .id();
    commands
        .entity(row_entity)
        .insert((
            Button,
            PanelOf(panel),
            OverrideCheck { glyph, label },
            Pickable::default(),
        ))
        .add_child(glyph)
        .add_child(label)
        .observe(on_override_pressed);
}

/// One of the two day sliders, labelled, with the shared readout.
fn spawn_day_slider(
    commands: &mut Commands,
    parent: Entity,
    panel: Entity,
    which: DaySlider,
    label_key: &'static str,
    range: (f32, f32),
    tab: &mut i32,
) -> Entity {
    let (min, max) = range;
    let row_entity = spawn_labelled_row(commands, parent, label_key);
    let readout = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Pickable::IGNORE,
            ChildOf(row_entity),
        ))
        .id();
    let slug = match which {
        DaySlider::Length => "day-length",
        DaySlider::Offset => "day-offset",
    };
    let track = commands
        .spawn((
            bevy::ui_widgets::Slider::default(),
            SliderValue(min),
            SliderRange::new(min, max),
            bevy::ui_widgets::SliderStep(0.5),
            SliderRow {
                readout,
                decimals: 1,
            },
            PanelOf(panel),
            which,
            Node {
                width: Val::Px(160.0),
                height: Val::Px(12.0),
                border: UiRect::all(Val::Px(1.0)),
                ..Node::default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(TRACK_FILL),
            TabIndex(*tab),
            Name::new(format!("{ELEMENT}-{slug}:slider")),
            ChildOf(row_entity),
        ))
        .with_child((
            bevy::ui_widgets::SliderThumb,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(9.0),
                height: Val::Px(12.0),
                ..Node::default()
            },
            sl_viewer_ui_core::ui::LogicalInset(sl_viewer_ui_core::ui::LogicalRect {
                inline_start: Val::Px(0.0),
                ..sl_viewer_ui_core::ui::LogicalRect::ZERO
            }),
            BackgroundColor(THUMB_FILL),
        ))
        .observe(on_day_slider_changed)
        .id();
    *tab = tab.saturating_add(1);
    track
}

// ---------------------------------------------------------------------------
// Pure helpers.
// ---------------------------------------------------------------------------

/// Why the panel cannot be used, in the reference's own order of precedence, or
/// `None` when it can.
///
/// Split out so the ordering is testable: which reason wins matters, because
/// "the parcel is too small" over the top of "you are looking at another
/// region" would send someone off to enlarge a parcel that is not the problem.
fn unavailable_reason(
    kind: LandPanelKind,
    subject: &LandEnvironmentSubject,
) -> Option<&'static str> {
    if !subject.live {
        return Some("land-env-unavailable-cross-region");
    }
    if matches!(kind, LandPanelKind::Parcel) {
        if subject.parcel_id.is_none() {
            return Some("land-env-unavailable-no-parcel");
        }
        if !subject.allow_override {
            return Some("land-env-unavailable-disallowed");
        }
        if subject.area < MINIMUM_PARCEL_AREA {
            return Some("land-env-unavailable-too-small");
        }
    }
    None
}

/// The apparent time of day a `day_length` / `day_offset` pair puts `now`
/// (seconds since the Unix epoch) at: the hour, the minute, and how far
/// through the cycle it is as a percentage.
///
/// The reference's `udpateApparentTimeOfDay`, which is the one place a resident
/// can see what a day offset actually *did* — the sky itself takes a whole
/// cycle to say.
fn apparent_time_of_day(now: i64, day_length: i32, day_offset: i32) -> Option<(i64, i64, i64)> {
    let day_length = i64::from(day_length);
    let stamp = now.saturating_add(i64::from(day_offset));
    // Every division below is guarded by the one that answers `None`: a day
    // length the grid has not set (or set to zero) has no time of day, which is
    // exactly the case the reference hides the readout for.
    let through = stamp.checked_rem_euclid(day_length)?;
    let second_of_day = through
        .saturating_mul(SECONDS_PER_DAY)
        .checked_div(day_length)?;
    let percent = through.saturating_mul(100).checked_div(day_length)?;
    let hour = second_of_day.checked_div(SECONDS_PER_HOUR_I64)?;
    let minute = second_of_day
        .checked_rem(SECONDS_PER_HOUR_I64)?
        .checked_div(60)?;
    Some((hour, minute, percent))
}

/// The reference's display convention for a day offset: an offset past twelve
/// hours is shown as the negative below it, so the slider's range is a
/// half-day either side of noon rather than a whole day forward.
fn offset_seconds_to_hours(day_offset: i32) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "a day offset is a second count well inside f32's exact-integer range"
    )]
    let hours = day_offset as f32 / SECONDS_PER_HOUR;
    if hours > 12.0 { hours - 24.0 } else { hours }
}

/// The inverse of [`offset_seconds_to_hours`]: the reference wraps a negative
/// offset back into the day before sending it.
fn offset_hours_to_seconds(hours: f32) -> i32 {
    let hours = if hours <= 0.0 { hours + 24.0 } else { hours };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "hours is a slider value in -11.5..=12 shifted into 0.5..=24, so the second \
                  count is far inside i32"
    )]
    let seconds = (hours * SECONDS_PER_HOUR).round() as i32;
    seconds
}

/// What to call the asset on `track`: what the grid said, else the day cycle's
/// own name, else a placeholder saying the track holds nothing of its own.
///
/// The reference's `getNameForTrackIndex`, whose placeholder differs by scope:
/// a parcel track with no name of its own is showing the *region's*
/// environment, and saying so is the difference between "nothing is set here"
/// and "this parcel has not overridden the region".
fn track_name(
    settings: &EnvironmentSettings,
    kind: LandPanelKind,
    track: i32,
    empty: &str,
    region: &str,
) -> String {
    let index = usize::try_from(track).unwrap_or(0);
    if let Some(name) = settings.day_names.track(index) {
        return name.to_owned();
    }
    if !settings.day_cycle.name.is_empty() && !track_is_empty(settings, track) {
        return settings.day_cycle.name.clone();
    }
    match kind {
        LandPanelKind::Region => empty.to_owned(),
        LandPanelKind::Parcel => region.to_owned(),
    }
}

/// Whether `track` schedules no keyframes at all — the reference's
/// `isTrackEmpty`, which is how a day cycle says a track is not its business.
fn track_is_empty(settings: &EnvironmentSettings, track: i32) -> bool {
    match track {
        0 => settings.day_cycle.water_track.is_empty(),
        sky => usize::try_from(sky.saturating_sub(1))
            .ok()
            .and_then(|index| settings.day_cycle.sky_tracks.get(index))
            .is_none_or(Vec::is_empty),
    }
}

/// The `LLSettingsBase` restriction flags an item's own owner permissions
/// imply, as the reference computes them at the moment of publishing
/// (`onPickerCommitted`).
const fn restriction_flags(owner: Permissions) -> u32 {
    let mut flags = 0;
    if !owner.contains(Permissions::MODIFY) {
        flags |= EnvironmentUpdate::FLAG_NOMOD;
    }
    if !owner.contains(Permissions::TRANSFER) {
        flags |= EnvironmentUpdate::FLAG_NOTRANS;
    }
    flags
}

/// The requests one Apply sends: the whole-environment PUT first, then one per
/// track whose asset was picked.
///
/// A pure function over the two drafts so the batching is testable without a
/// grid: which fields ride along, which are region-only, and the fact that a
/// track update carries the asset and nothing else are all decisions the wire
/// cares about.
fn publish_requests(
    kind: LandPanelKind,
    parcel_id: Option<i32>,
    seeded: &LandDraft,
    draft: &LandDraft,
) -> Vec<Command> {
    let mut requests = Vec::new();
    let mut environment = EnvironmentUpdate::default();
    let mut whole = false;
    if draft.day_length != seeded.day_length {
        environment.day_length = Some(draft.day_length);
        whole = true;
    }
    if draft.day_offset != seeded.day_offset {
        environment.day_offset = Some(draft.day_offset);
        whole = true;
    }
    if kind.owns_altitudes() && draft.altitudes != seeded.altitudes {
        let mut sorted = draft.altitudes;
        sorted.sort_unstable();
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "a breakpoint is a metre height in 100..=4000, exact in f32"
        )]
        let altitudes = [sorted[0] as f32, sorted[1] as f32, sorted[2] as f32];
        environment.track_altitudes = Some(altitudes);
        whole = true;
    }
    if let Some(day) = &draft.day {
        environment.day_asset = Some(day.asset_id);
        environment.day_name = Some(day.name.clone());
        environment.flags = day.flags;
        whole = true;
    }
    if whole {
        requests.push(Command::SetEnvironment {
            parcel_id,
            track_no: None,
            update: Box::new(environment),
        });
    }
    for (track, chosen) in draft.tracks.iter().enumerate() {
        let Some(chosen) = chosen else {
            continue;
        };
        requests.push(Command::SetEnvironment {
            parcel_id,
            track_no: i32::try_from(track).ok(),
            update: Box::new(EnvironmentUpdate {
                day_asset: Some(chosen.asset_id),
                day_name: Some(chosen.name.clone()),
                flags: chosen.flags,
                ..EnvironmentUpdate::default()
            }),
        });
    }
    requests
}

// ---------------------------------------------------------------------------
// Systems.
// ---------------------------------------------------------------------------

/// Ask the grid for this land's environment when the subject moves.
fn request_land_environment(
    mut panels: Query<(
        &LandPanelKind,
        &LandEnvironmentSubject,
        &mut LandEnvironmentState,
    )>,
    mut commands: MessageWriter<SlCommand>,
) {
    for (kind, subject, mut state) in &mut panels {
        if state.seen == Some(*subject) {
            continue;
        }
        state.seen = Some(*subject);
        state.reseed = true;
        let Some(target) = wire_scope(*kind, subject) else {
            // Nothing to ask about: the window is frozen, or About Land has not
            // been told which parcel it is on yet.
            state.current = None;
            state.awaiting = None;
            continue;
        };
        state.current = None;
        state.awaiting = Some(target.reply_id());
        commands.write(SlCommand(Command::RequestEnvironment {
            parcel_id: target.parcel_id(),
        }));
    }
}

/// The land a panel addresses, once it is addressing one at all.
///
/// Its own type rather than an `Option<Option<i32>>`, because the three
/// answers — nothing, the region, a parcel — are three different things and
/// only two of them are "a scope".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LandTarget {
    /// The whole region (no `?parcelid=` on the wire).
    Region,
    /// One parcel, by its region-local id.
    Parcel(i32),
}

impl LandTarget {
    /// The `?parcelid=` a request to this land carries, or `None` for the
    /// region, which the reference leaves off entirely.
    const fn parcel_id(self) -> Option<i32> {
        match self {
            Self::Region => None,
            Self::Parcel(parcel_id) => Some(parcel_id),
        }
    }

    /// The `parcel_id` an `ExtEnvironment` reply for this land reports
    /// (`-1` is the region's).
    const fn reply_id(self) -> i32 {
        match self {
            Self::Region => -1,
            Self::Parcel(parcel_id) => parcel_id,
        }
    }
}

/// The land this panel addresses, or `None` when it addresses nothing — the
/// window is frozen on a region the agent has left, or About Land has not
/// resolved its parcel yet.
const fn wire_scope(kind: LandPanelKind, subject: &LandEnvironmentSubject) -> Option<LandTarget> {
    if !subject.live {
        return None;
    }
    match kind {
        LandPanelKind::Region => Some(LandTarget::Region),
        LandPanelKind::Parcel => match subject.parcel_id {
            Some(parcel_id) => Some(LandTarget::Parcel(parcel_id)),
            None => None,
        },
    }
}

/// Fold an arriving environment reply into whichever panels asked for it.
///
/// Matched on the reply's own `parcel_id` rather than on request order: the
/// region's environment and a parcel's can be in flight at once, and every
/// other consumer of this event (the renderer's own layers) is reading the
/// same stream.
fn ingest_land_environment(
    mut events: MessageReader<SlEvent>,
    mut panels: Query<&mut LandEnvironmentState>,
) {
    let replies: Vec<&EnvironmentSettings> = events
        .read()
        .filter_map(|event| match &event.0 {
            SlSessionEvent::Environment(settings) => Some(&**settings),
            _ => None,
        })
        .collect();
    if replies.is_empty() {
        return;
    }
    for mut state in &mut panels {
        let Some(awaiting) = state.awaiting else {
            continue;
        };
        let Some(settings) = replies
            .iter()
            .rev()
            .find(|settings| settings.parcel_id == awaiting)
        else {
            continue;
        };
        state.current = Some(Box::new((*settings).clone()));
        state.seed_from(settings);
    }
}

/// A settings asset was picked for one of the panel's fields.
fn take_land_settings_pick(
    mut picks: MessageReader<SettingsPicked>,
    buttons: Query<(&PanelOf, &PickerButton)>,
    inventory: Option<Res<InventoryModel>>,
    mut panels: Query<&mut LandEnvironmentState>,
) {
    for pick in picks.read() {
        if !pick.final_pick {
            // Every selection is published so a consumer can preview it;
            // a land publish is not a preview, so only the committed one lands.
            continue;
        }
        let Ok((PanelOf(panel), button)) = buttons.get(pick.requester) else {
            continue;
        };
        let Ok(mut state) = panels.get_mut(*panel) else {
            continue;
        };
        let Some(chosen) = pick.chosen.as_ref() else {
            continue;
        };
        let flags = inventory
            .as_ref()
            .and_then(|model| model.find_item(chosen.item))
            .map_or(0, |item| restriction_flags(item.permissions.owner));
        let chosen = ChosenSettings {
            asset_id: chosen.asset_id,
            name: chosen.name.clone(),
            flags,
        };
        match button.track {
            // A whole day cycle replaces every track, so the per-track picks
            // made before it are no longer anybody's choice.
            None => {
                state.draft.day = Some(chosen);
                state.draft.tracks = Default::default();
            }
            Some(track) => {
                if let Ok(index) = usize::try_from(track)
                    && let Some(slot) = state.draft.tracks.get_mut(index)
                {
                    *slot = Some(chosen);
                }
            }
        }
        state.reseed = true;
    }
}

/// Mirror the three altitude fields into the draft.
fn read_altitude_fields(
    fields: Query<(&PanelOf, &AltitudeField, &EditableText)>,
    mut panels: Query<(&LandPanelKind, &mut LandEnvironmentState)>,
) {
    for (PanelOf(panel), slot, editable) in &fields {
        let Ok((kind, mut state)) = panels.get_mut(*panel) else {
            continue;
        };
        if !kind.owns_altitudes() || state.reseed {
            continue;
        }
        let Ok(metres) = editable.value().to_string().trim().parse::<i32>() else {
            continue;
        };
        let clamped = metres.clamp(ALTITUDE_RANGE.0, ALTITUDE_RANGE.1);
        if let Some(current) = state.draft.altitudes.get_mut(slot.0)
            && *current != clamped
        {
            *current = clamped;
        }
    }
}

/// A day slider moved: into the draft, in the units the wire carries.
fn on_day_slider_changed(
    change: On<ValueChange<f32>>,
    sliders: Query<(&PanelOf, &DaySlider)>,
    mut panels: Query<(&LandEnvironmentSubject, &mut LandEnvironmentState)>,
    mut commands: Commands,
) {
    let Ok((PanelOf(panel), which)) = sliders.get(change.source) else {
        return;
    };
    let Ok((subject, mut state)) = panels.get_mut(*panel) else {
        return;
    };
    if !subject.editable || state.current.is_none() {
        // A read-only panel's thumb is put back where the grid had it on the
        // next re-seed rather than left where the drag dropped it.
        state.reseed = true;
        return;
    }
    // `SliderValue` is immutable, so the drag's new position is *inserted*
    // rather than assigned — the same write every other slider in this crate
    // makes.
    commands
        .entity(change.source)
        .insert(SliderValue(change.value));
    match which {
        DaySlider::Length => {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::as_conversions,
                reason = "the slider's range is 4..=168 hours, so the second count is far inside \
                          i32"
            )]
            let seconds = (change.value * SECONDS_PER_HOUR).round() as i32;
            state.draft.day_length = seconds;
        }
        DaySlider::Offset => state.draft.day_offset = offset_hours_to_seconds(change.value),
    }
}

/// Write the draft back into every widget that shows it.
fn reseed_land_widgets(
    mut panels: Query<(
        Entity,
        &LandPanelKind,
        &mut LandEnvironmentState,
        &LandEnvironmentUi,
    )>,
    sliders: Query<(Entity, &PanelOf, &DaySlider, &SliderValue)>,
    mut fields: Query<(&PanelOf, &AltitudeField, &mut EditableText)>,
    translator: Translator,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0));
    for (entity, kind, mut state, ui) in &mut panels {
        // The apparent time is a clock readout, so it is written every frame
        // whether or not anything else changed — the reference runs it off an
        // idle callback for the same reason.
        let apparent = state
            .current
            .as_ref()
            .and_then(|_| apparent_time_of_day(now, state.draft.day_length, state.draft.day_offset))
            .map_or_else(
                || translator.get("land-env-loading"),
                |(hour, minute, percent)| format!("{hour:02}:{minute:02} ({percent}%)"),
            );
        set_text(&mut texts, ui.apparent_time, &apparent);
        if !state.reseed {
            continue;
        }
        state.reseed = false;
        for (slider, PanelOf(panel), which, value) in &sliders {
            if *panel != entity {
                continue;
            }
            #[expect(
                clippy::cast_precision_loss,
                clippy::as_conversions,
                reason = "a day length is a second count well inside f32's exact-integer range"
            )]
            let wanted = match which {
                DaySlider::Length => state.draft.day_length as f32 / SECONDS_PER_HOUR,
                DaySlider::Offset => offset_seconds_to_hours(state.draft.day_offset),
            };
            let clamped = match which {
                DaySlider::Length => wanted.clamp(DAY_LENGTH_HOURS.0, DAY_LENGTH_HOURS.1),
                DaySlider::Offset => wanted.clamp(DAY_OFFSET_HOURS.0, DAY_OFFSET_HOURS.1),
            };
            if value.0.to_bits() != clamped.to_bits() {
                commands.entity(slider).insert(SliderValue(clamped));
            }
        }
        for (PanelOf(panel), slot, mut editable) in &mut fields {
            if *panel != entity {
                continue;
            }
            let Some(metres) = state.draft.altitudes.get(slot.0) else {
                continue;
            };
            let wanted = metres.to_string();
            if !editable.is_composing() && editable.value().to_string() != wanted {
                editable.editor_mut().set_text(&wanted);
            }
        }
        let empty = translator.get("land-env-empty");
        let region = translator.get("land-env-region-env");
        let loading = translator.get("land-env-loading");
        for row_spec in TRACK_ROWS {
            let index = usize::try_from(row_spec.track).unwrap_or(0);
            let Some(node) = ui.track_names.get(index).copied() else {
                continue;
            };
            let picked = state
                .draft
                .tracks
                .get(index)
                .and_then(Option::as_ref)
                .or(state.draft.day.as_ref());
            let name = match (picked, state.current.as_deref()) {
                (Some(chosen), _) => chosen.name.clone(),
                (None, Some(settings)) => {
                    track_name(settings, *kind, row_spec.track, &empty, &region)
                }
                (None, None) => loading.clone(),
            };
            set_text(&mut texts, node, &name);
        }
    }
}

/// Show the panel's controls or the note saying why it cannot be used, and
/// grey every control the agent may not touch.
#[expect(
    clippy::too_many_arguments,
    reason = "one system painting every kind of control the panel has — the action buttons, the \
              picker buttons and the override checkbox — each of which is its own query, over \
              the shared button paint and the unavailable note"
)]
fn paint_land_controls(
    mut commands: Commands,
    panels: Query<(
        Entity,
        &LandPanelKind,
        &LandEnvironmentSubject,
        &LandEnvironmentState,
        &LandEnvironmentUi,
    )>,
    actions: Query<(Entity, &PanelOf), With<LandAction>>,
    pickers: Query<(Entity, &PanelOf), With<PickerButton>>,
    checks: Query<(&PanelOf, &OverrideCheck)>,
    translator: Translator,
    mut nodes: Query<&mut Node>,
    mut paint: ButtonPaint,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
) {
    for (entity, kind, subject, state, ui) in &panels {
        let reason = unavailable_reason(*kind, subject);
        show_node(&mut nodes, ui.unavailable, reason.is_some());
        show_node(&mut nodes, ui.controls, reason.is_none());
        if let Some(reason) = reason
            && let Ok(kids) = children.get(ui.unavailable)
            && let Some(node) = kids.iter().next()
        {
            let text = translator.get(reason);
            set_text(&mut texts, node, &text);
        }
        let enabled = reason.is_none() && subject.editable && state.current.is_some();
        let colour_pair = |on: bool| {
            if on {
                (ACTION_BACKGROUND, LABEL_COLOR)
            } else {
                (TRACK_FILL, DISABLED_COLOR)
            }
        };
        for (button, PanelOf(panel)) in &actions {
            if *panel != entity {
                continue;
            }
            paint_action_button(
                &mut commands,
                &children,
                &mut paint,
                button,
                enabled,
                colour_pair(enabled),
            );
        }
        for (button, PanelOf(panel)) in &pickers {
            if *panel != entity {
                continue;
            }
            paint_action_button(
                &mut commands,
                &children,
                &mut paint,
                button,
                enabled,
                colour_pair(enabled),
            );
        }
        for (PanelOf(panel), check) in &checks {
            if *panel != entity {
                continue;
            }
            // The colours come out of the shared button paint rather than a
            // query of this system's own: two `Query<&mut TextColor>` in one
            // system is Bevy's B0001, which panics on the first frame.
            let (_backgrounds, colours, _disabled) = &mut paint;
            set_check_visual(&mut texts, colours, check, subject.allow_override, enabled);
        }
    }
}

/// An action button was pressed.
fn on_land_action(
    mut press: On<Pointer<Press>>,
    buttons: Query<(&PanelOf, &LandAction)>,
    disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    mut panels: Query<(
        &LandPanelKind,
        &LandEnvironmentSubject,
        &mut LandEnvironmentState,
    )>,
    mut confirm: ResMut<LandEnvironmentConfirm>,
    mut notify: MessageWriter<ShowNotification>,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary || disabled.contains(press.entity) {
        return;
    }
    let Ok((PanelOf(panel), action)) = buttons.get(press.entity) else {
        return;
    };
    let Ok((kind, subject, mut state)) = panels.get_mut(*panel) else {
        return;
    };
    press.propagate(false);
    let Some(target) = wire_scope(*kind, subject) else {
        return;
    };
    match action {
        LandAction::UseDefault => {
            if confirm.0.is_none() {
                confirm.0 = Some(PendingConfirm {
                    panel: *panel,
                    action: ConfirmAction::Reset,
                });
                notify.write(ShowNotification::new(RESET_CONFIRM));
            }
        }
        LandAction::ResetAltitudes => {
            let step = ALTITUDE_DEFAULT_STEP;
            state.draft.altitudes = [step, step.saturating_mul(2), step.saturating_mul(3)];
            state.reseed = true;
        }
        LandAction::Apply => {
            let requests = publish_requests(*kind, target.parcel_id(), &state.seeded, &state.draft);
            if requests.is_empty() {
                return;
            }
            for request in requests {
                commands.write(SlCommand(request));
            }
            // The grid answers each PUT with the stored settings, which the
            // ingest folds back in and re-seeds from — so the draft is left
            // alone rather than optimistically promoted.
            state.awaiting = Some(target.reply_id());
        }
        LandAction::Revert => {
            if let Some(settings) = state.current.clone() {
                state.seed_from(&settings);
            }
        }
    }
}

/// A "Use Inventory…" button was pressed: open the picker on that field.
fn on_land_pick_pressed(
    mut press: On<Pointer<Press>>,
    buttons: Query<(&PanelOf, &PickerButton)>,
    disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    panels: Query<&LandEnvironmentState>,
    mut pickers: MessageWriter<OpenSettingsPicker>,
) {
    if press.button != PointerButton::Primary || disabled.contains(press.entity) {
        return;
    }
    let Ok((PanelOf(panel), button)) = buttons.get(press.entity) else {
        return;
    };
    let Ok(state) = panels.get(*panel) else {
        return;
    };
    press.propagate(false);
    let field = match button.track {
        Some(track) => format!("track-{track}"),
        None => "day-cycle".to_owned(),
    };
    // The reference opens the picker on the asset the field is already
    // holding, so Cancel puts it back rather than clearing it.
    let current = match button.track {
        None => state
            .draft
            .day
            .as_ref()
            .map(|chosen| chosen.asset_id)
            .or_else(|| state.current.as_ref().and_then(|env| env.day_asset)),
        Some(track) => usize::try_from(track)
            .ok()
            .and_then(|index| state.draft.tracks.get(index))
            .and_then(Option::as_ref)
            .map(|chosen| chosen.asset_id),
    };
    pickers.write(OpenSettingsPicker {
        requester: press.entity,
        field: field.into(),
        kind: button.kind,
        current,
    });
}

/// The parcel-override checkbox was clicked: ask, as the reference asks.
fn on_override_pressed(
    mut press: On<Pointer<Press>>,
    checks: Query<(&PanelOf, &OverrideCheck)>,
    disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    panels: Query<&LandEnvironmentSubject>,
    mut confirm: ResMut<LandEnvironmentConfirm>,
    mut notify: MessageWriter<ShowNotification>,
) {
    if press.button != PointerButton::Primary || disabled.contains(press.entity) {
        return;
    }
    let Ok((PanelOf(panel), _check)) = checks.get(press.entity) else {
        return;
    };
    let Ok(subject) = panels.get(*panel) else {
        return;
    };
    press.propagate(false);
    if !subject.live || !subject.editable || confirm.0.is_some() {
        return;
    }
    confirm.0 = Some(PendingConfirm {
        panel: *panel,
        action: ConfirmAction::Override(!subject.allow_override),
    });
    notify.write(ShowNotification::new(OVERRIDE_CONFIRM));
}

/// A confirmation came back.
fn resolve_land_confirmation(
    mut responses: MessageReader<NotificationResponse>,
    mut confirm: ResMut<LandEnvironmentConfirm>,
    panels: Query<(&LandPanelKind, &LandEnvironmentSubject)>,
    mut commands: MessageWriter<SlCommand>,
    mut overrides: MessageWriter<AllowEnvironmentOverrideRequested>,
) {
    for response in responses.read() {
        if !matches!(response.template, RESET_CONFIRM | OVERRIDE_CONFIRM) {
            continue;
        }
        let Some(pending) = confirm.0.take() else {
            continue;
        };
        if response.button != Some("OK") {
            continue;
        }
        match pending.action {
            ConfirmAction::Reset => {
                let Ok((kind, subject)) = panels.get(pending.panel) else {
                    continue;
                };
                let Some(target) = wire_scope(*kind, subject) else {
                    continue;
                };
                commands.write(SlCommand(Command::ResetEnvironment {
                    parcel_id: target.parcel_id(),
                    track_no: None,
                }));
            }
            ConfirmAction::Override(allow) => {
                overrides.write(AllowEnvironmentOverrideRequested {
                    panel: pending.panel,
                    allow,
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Small writers.
// ---------------------------------------------------------------------------

/// Set a text node's content, only on change.
fn set_text(texts: &mut Query<&mut Text>, node: Entity, value: &str) {
    if let Ok(mut text) = texts.get_mut(node)
        && text.0 != value
    {
        value.clone_into(&mut text.0);
    }
}

/// Show or hide a node, only on change.
fn show_node(nodes: &mut Query<&mut Node>, node: Entity, shown: bool) {
    let wanted = if shown { Display::Flex } else { Display::None };
    if let Ok(mut node) = nodes.get_mut(node)
        && node.display != wanted
    {
        node.display = wanted;
    }
}

/// Set a checkbox's glyph and label colours, only on change.
fn set_check_visual(
    texts: &mut Query<&mut Text>,
    colours: &mut Query<&mut TextColor>,
    check: &OverrideCheck,
    on: bool,
    enabled: bool,
) {
    let glyph = if on { CHECKED_GLYPH } else { UNCHECKED_GLYPH };
    set_text(texts, check.glyph, glyph);
    let glyph_colour = if !enabled {
        DISABLED_COLOR
    } else if on {
        CHECK_COLOR
    } else {
        DIM_LABEL_COLOR
    };
    for (node, wanted) in [
        (check.glyph, glyph_colour),
        (
            check.label,
            if enabled { LABEL_COLOR } else { DISABLED_COLOR },
        ),
    ] {
        if let Ok(mut colour) = colours.get_mut(node)
            && colour.0 != wanted
        {
            colour.0 = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{Command, DayCycle, DayCycleFrame, DayNames, EnvironmentSettings, Uuid};

    use sl_client_bevy::LandArea;

    use super::{
        LandDraft, LandEnvironmentSubject, LandPanelKind, apparent_time_of_day,
        offset_hours_to_seconds, offset_seconds_to_hours, publish_requests, restriction_flags,
        track_name, unavailable_reason,
    };

    /// A live, editable subject with a big-enough parcel — the case every other
    /// test moves one field of.
    fn workable() -> LandEnvironmentSubject {
        LandEnvironmentSubject {
            parcel_id: Some(7),
            live: true,
            editable: true,
            allow_override: true,
            area: LandArea(1024),
        }
    }

    /// **A window on a region the agent has left says so first.**
    ///
    /// The order matters: every other reason is about the parcel in front of
    /// you, and reporting one of those for a parcel in another region sends
    /// someone off to fix the wrong thing.
    #[test]
    fn cross_region_outranks_every_other_reason() {
        let subject = LandEnvironmentSubject {
            live: false,
            parcel_id: None,
            allow_override: false,
            area: LandArea(16),
            ..workable()
        };
        assert_eq!(
            unavailable_reason(LandPanelKind::Parcel, &subject),
            Some("land-env-unavailable-cross-region")
        );
        assert_eq!(
            unavailable_reason(LandPanelKind::Region, &subject),
            Some("land-env-unavailable-cross-region")
        );
    }

    /// **The three parcel-only reasons, in the reference's order.**
    #[test]
    fn a_parcel_reports_its_own_reasons_in_order() {
        let unbound = LandEnvironmentSubject {
            parcel_id: None,
            allow_override: false,
            area: LandArea(16),
            ..workable()
        };
        assert_eq!(
            unavailable_reason(LandPanelKind::Parcel, &unbound),
            Some("land-env-unavailable-no-parcel")
        );
        let disallowed = LandEnvironmentSubject {
            allow_override: false,
            area: LandArea(16),
            ..workable()
        };
        assert_eq!(
            unavailable_reason(LandPanelKind::Parcel, &disallowed),
            Some("land-env-unavailable-disallowed")
        );
        let small = LandEnvironmentSubject {
            area: LandArea(127),
            ..workable()
        };
        assert_eq!(
            unavailable_reason(LandPanelKind::Parcel, &small),
            Some("land-env-unavailable-too-small")
        );
        assert_eq!(unavailable_reason(LandPanelKind::Parcel, &workable()), None);
    }

    /// **A region panel does not care about the parcel-only reasons.**
    ///
    /// The estate's own override flag is a *checkbox* on the region panel, not
    /// a gate on it: an estate that has switched parcel overrides off can
    /// still change its own environment, and the reference only tests
    /// `mAllowOverride` in the parcel subclass's `canEdit`.
    #[test]
    fn a_region_panel_ignores_the_parcel_gates() {
        let subject = LandEnvironmentSubject {
            parcel_id: None,
            allow_override: false,
            area: LandArea::ZERO,
            ..workable()
        };
        assert_eq!(unavailable_reason(LandPanelKind::Region, &subject), None);
    }

    /// **The day offset round-trips through the reference's display
    /// convention.**
    ///
    /// An offset past twelve hours is shown as the negative below it, and a
    /// negative one is wrapped back into the day before it is sent. Getting
    /// this backwards would put every region's sun half a day out the first
    /// time somebody touched the slider.
    #[test]
    fn the_day_offset_round_trips_through_its_display_form() {
        for seconds in [0, 3600, 43_200, 43_201, 57_600, 86_399] {
            let hours = offset_seconds_to_hours(seconds);
            assert!(
                (-12.0..=12.0).contains(&hours),
                "{seconds}s displayed as {hours}h, outside the slider"
            );
            let back = offset_hours_to_seconds(hours);
            let wrapped = if seconds == 0 { 86_400 } else { seconds };
            assert_eq!(
                back, wrapped,
                "{seconds}s -> {hours}h -> {back}s is not the same offset"
            );
        }
    }

    /// **The apparent time of day is the reference's arithmetic.**
    ///
    /// A four-hour day offset by nothing puts midnight at the top of every
    /// four-hour block, and half way through one is noon.
    #[test]
    fn the_apparent_time_follows_the_cycle() {
        let day: i32 = 4 * 60 * 60;
        let at = |now: i32| apparent_time_of_day(i64::from(now), day, 0);
        assert_eq!(at(0), Some((0, 0, 0)));
        assert_eq!(at(day / 2), Some((12, 0, 50)));
        assert_eq!(at(day), Some((0, 0, 0)));
        // The offset moves the clock, not the day length.
        assert_eq!(
            apparent_time_of_day(0, day, day / 4),
            Some((6, 0, 25)),
            "a quarter-day offset should start the day at six"
        );
        // A day nobody has set has no apparent time at all.
        assert_eq!(apparent_time_of_day(0, 0, 0), None);
    }

    /// **An unedited draft publishes nothing.**
    ///
    /// Apply on a panel nobody has touched must not send a PUT: on a region
    /// that would bump the environment version, wake every viewer standing
    /// there, and change nothing.
    #[test]
    fn an_untouched_draft_sends_no_requests() {
        let draft = LandDraft {
            day_length: 14400,
            day_offset: 0,
            altitudes: [1000, 2000, 3000],
            ..LandDraft::default()
        };
        assert!(
            publish_requests(LandPanelKind::Region, None, &draft, &draft).is_empty(),
            "an unedited draft should publish nothing"
        );
    }

    /// **A parcel never publishes track altitudes.**
    ///
    /// The reference only fills `track_altitudes` for a region-scoped update,
    /// because the bands belong to the region: a parcel moving them under an
    /// agent climbing through them is not a thing the wire can express.
    #[test]
    fn a_parcel_update_omits_the_track_altitudes() {
        let seeded = LandDraft {
            day_length: 14400,
            day_offset: 0,
            altitudes: [1000, 2000, 3000],
            ..LandDraft::default()
        };
        let draft = LandDraft {
            day_length: 7200,
            altitudes: [500, 1500, 2500],
            ..seeded.clone()
        };
        let parcel: Vec<EnvironmentShape> =
            publish_requests(LandPanelKind::Parcel, Some(7), &seeded, &draft)
                .iter()
                .filter_map(environment_shape)
                .collect();
        assert_eq!(
            parcel,
            vec![(Some(7), None, Some(7200), None)],
            "a parcel-scoped update must not carry the region's sky-track bands"
        );
        // A region-scoped one does, sorted.
        let region: Vec<EnvironmentShape> =
            publish_requests(LandPanelKind::Region, None, &seeded, &draft)
                .iter()
                .filter_map(environment_shape)
                .collect();
        assert_eq!(
            region,
            vec![(None, None, Some(7200), Some([500.0, 1500.0, 2500.0]))]
        );
    }

    /// The parts of a `SetEnvironment` a scoping test cares about: where it is
    /// aimed (the parcel and the track), and the two fields that ride only a
    /// whole-environment update.
    type EnvironmentShape = (Option<i32>, Option<i32>, Option<i32>, Option<[f32; 3]>);

    /// Project one request onto [`EnvironmentShape`], dropping anything that
    /// is not a `SetEnvironment`.
    fn environment_shape(request: &Command) -> Option<EnvironmentShape> {
        match request {
            Command::SetEnvironment {
                parcel_id,
                track_no,
                update,
            } => Some((
                *parcel_id,
                *track_no,
                update.day_length,
                update.track_altitudes,
            )),
            _ => None,
        }
    }

    /// **A per-track pick is its own request, scoped with its `trackno`.**
    ///
    /// This is the only way the wire can say "this track and not the others",
    /// and the update carries the asset alone: day length and offset are
    /// whole-environment fields the reference refuses to send with a track.
    #[test]
    fn a_track_pick_rides_its_own_scoped_request() {
        let seeded = LandDraft {
            day_length: 14400,
            ..LandDraft::default()
        };
        let mut draft = seeded.clone();
        draft.day_offset = 3600;
        draft.tracks[0] = Some(super::ChosenSettings {
            asset_id: Uuid::from_u128(0xa1),
            name: "Calm Water".to_owned(),
            flags: 0,
        });
        draft.tracks[2] = Some(super::ChosenSettings {
            asset_id: Uuid::from_u128(0xa2),
            name: "High Sky".to_owned(),
            flags: sl_client_bevy::EnvironmentUpdate::FLAG_NOTRANS,
        });
        let requests = publish_requests(LandPanelKind::Region, None, &seeded, &draft);
        assert_eq!(
            requests.len(),
            3,
            "the whole-environment PUT plus two tracks"
        );
        let scoped: Vec<(Option<i32>, Option<Uuid>, u32)> = requests
            .iter()
            .filter_map(|request| match request {
                Command::SetEnvironment {
                    track_no, update, ..
                } => Some((*track_no, update.day_asset, update.flags)),
                _ => None,
            })
            .collect();
        assert_eq!(
            scoped,
            vec![
                (None, None, 0),
                (Some(0), Some(Uuid::from_u128(0xa1)), 0),
                (
                    Some(2),
                    Some(Uuid::from_u128(0xa2)),
                    sl_client_bevy::EnvironmentUpdate::FLAG_NOTRANS
                ),
            ]
        );
    }

    /// **Publishing a no-transfer item marks the environment no-transfer.**
    #[test]
    fn an_items_permissions_become_the_environments_flags() {
        use sl_client_bevy::{EnvironmentUpdate, Permissions};
        assert_eq!(
            restriction_flags(Permissions::MODIFY | Permissions::TRANSFER),
            0
        );
        assert_eq!(
            restriction_flags(Permissions::TRANSFER),
            EnvironmentUpdate::FLAG_NOMOD
        );
        assert_eq!(
            restriction_flags(Permissions::MODIFY),
            EnvironmentUpdate::FLAG_NOTRANS
        );
        assert_eq!(
            restriction_flags(Permissions::empty()),
            EnvironmentUpdate::FLAG_NOMOD | EnvironmentUpdate::FLAG_NOTRANS
        );
    }

    /// An environment whose ground sky track holds one frame and whose other
    /// tracks hold nothing.
    fn ground_only(names: DayNames) -> EnvironmentSettings {
        let mut settings = EnvironmentSettings::legacy_windlight_default();
        settings.day_cycle = DayCycle {
            name: "Whole Day".to_owned(),
            water_track: Vec::new(),
            sky_tracks: vec![vec![DayCycleFrame {
                keyframe: 0.0,
                name: "Noon".to_owned(),
            }]],
            sky_frames: settings.day_cycle.sky_frames.clone(),
            water_frames: settings.day_cycle.water_frames.clone(),
        };
        settings.day_names = names;
        settings
    }

    /// **What a track is called comes from the grid first, the cycle second,
    /// and the scope's placeholder last.**
    ///
    /// The placeholder is the interesting half: an unnamed *parcel* track is
    /// showing the region's environment, and saying "(empty)" there would
    /// claim the parcel had cleared a sky it has simply never set.
    #[test]
    fn a_tracks_name_falls_back_by_scope() {
        let named = ground_only(DayNames::Tracks([
            "Ocean".to_owned(),
            "Ground".to_owned(),
            String::new(),
            String::new(),
            String::new(),
        ]));
        assert_eq!(
            track_name(&named, LandPanelKind::Region, 0, "(empty)", "(region)"),
            "Ocean"
        );
        assert_eq!(
            track_name(&named, LandPanelKind::Region, 1, "(empty)", "(region)"),
            "Ground"
        );
        // Unnamed track 2 on a region: the cycle does not fill it, so nothing
        // is there.
        assert_eq!(
            track_name(&named, LandPanelKind::Region, 2, "(empty)", "(region)"),
            "(empty)"
        );
        assert_eq!(
            track_name(&named, LandPanelKind::Parcel, 2, "(empty)", "(region)"),
            "(region)"
        );
        // With no `day_names` at all, a filled track takes the cycle's name and
        // an empty one still falls through.
        let unnamed = ground_only(DayNames::Unnamed);
        assert_eq!(
            track_name(&unnamed, LandPanelKind::Region, 1, "(empty)", "(region)"),
            "Whole Day"
        );
        assert_eq!(
            track_name(&unnamed, LandPanelKind::Region, 0, "(empty)", "(region)"),
            "(empty)"
        );
    }
}
