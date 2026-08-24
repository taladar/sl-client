//! The **Quick Preferences** panel (`viewer-quick-preferences`): the small,
//! always-reachable floater of the settings you reach for several times an hour —
//! draw distance, the particle cap, and the environment (windlight) preset and
//! time of day — so the full preferences floater never has to be opened for them.
//!
//! It is opened from a gear button in the bottom toolbar's **trailing** area
//! ([`BottomArea::upper_trailing`], beside the parcel-audio cluster), and appears
//! as a draggable floater anchored to the bottom-right corner. Firestorm's
//! `fsfloaterquickprefs` is the model.
//!
//! # A view over the settings store, not a fixed list
//!
//! The reference viewer's defining trait here is that the panel's contents are a
//! *curated view over the settings store*, user-configurable rather than
//! hard-coded. We keep that architecture: the setting rows are built from a
//! data-driven [`QuickPrefEntry`] list, and that list is **persisted per-avatar**
//! (a `quick_preferences.json` in the account directory). A power user can edit
//! that file to add, remove or retype entries — any registered setting can be
//! surfaced without being reimplemented, because each row binds through the shared
//! [`crate::settings_binding`] layer. The in-viewer editor (add / remove / reorder
//! from a picker) that Firestorm's wrench toggle opens is a follow-up
//! ([[viewer-quick-preferences-editor]]); this version ships the curated default
//! set plus the plumbing.
//!
//! # The environment section
//!
//! The environment / time-of-day controls are *not* settings — they drive the
//! live [`EnvironmentState`] the World ▸ Environment menu already uses
//! ([`EnvironmentState::set_fixed`]), mapping Firestorm's sky / water / day-cycle
//! preset combos onto our fixed-environment model: a **preset group** (shared,
//! the region's own day cycle, the ported Legacy WindLight presets, or the modern
//! EEP library skies) crossed with a **time of day** (sunrise / midday / sunset /
//! midnight).
//!
//! Reference (Firestorm, read-only): `quickprefs` (`FloaterQuickPrefs`),
//! `quick_preferences.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui::Checked;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{
    Activate, Button, Slider, SliderRange, SliderStep, SliderThumb, SliderValue,
};
use bevy::window::PrimaryWindow;
use serde::{Deserialize, Serialize};
use sl_settings::{Scope, SettingKind};

use crate::environment::{EnvironmentState, FixedEnvironment};
use crate::floater::{
    DeferredFloaterContent, Floater, FloaterCaps, FloaterCommand, FloaterHandle, FloaterOp,
    FloaterSpec, floater_panel, spawn_floater,
};
use crate::i18n::Translated;
use crate::settings::ViewerSettings;
use crate::settings_binding::{SettingBinding, bound_checkbox, bound_slider};
use crate::sky_presets::FixedSky;
use crate::ui::BottomArea;
use crate::ui::{LogicalInset, LogicalRect, UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_combo::{ComboChanged, ComboSelection, ComboSpec, spawn_combo};
use crate::ui_element::ElementCx;
use crate::ui_font::UiFont;

/// The stable floater id (its geometry-persistence key and lookup handle).
pub(crate) const QUICK_PREFS_FLOATER_ID: &str = "quick-preferences";

/// The per-avatar file the user-configurable entry list is read from / written
/// to, in the account directory.
const ENTRIES_FILE: &str = "quick_preferences.json";

/// The panel's body font size, in logical pixels.
const FONT: f32 = 13.0;

/// A section-heading font size.
const SECTION_FONT: f32 = 14.0;

/// The gap between rows in the content column.
const ROW_GAP: f32 = 8.0;

/// A setting slider's track width, in logical pixels.
const TRACK_WIDTH: f32 = 130.0;
/// A setting slider's thumb width, in logical pixels.
const THUMB_WIDTH: f32 = 12.0;
/// A setting slider's track / thumb height, in logical pixels.
const TRACK_HEIGHT: f32 = 14.0;
/// A checkbox box's side length, in logical pixels.
const CHECK_SIZE: f32 = 16.0;
/// The width of a setting row's trailing value readout, in logical pixels.
const VALUE_WIDTH: f32 = 44.0;

/// The floater's default content size, in logical pixels.
const DEFAULT_SIZE: Vec2 = Vec2::new(300.0, 232.0);
/// The floater's minimum content size, in logical pixels.
const MIN_SIZE: Vec2 = Vec2::new(240.0, 160.0);
/// The gap from the screen's bottom-right corner when the floater first anchors
/// itself, in logical pixels.
const ANCHOR_MARGIN: f32 = 12.0;
/// The spawn-time position sentinel: [`anchor_quick_prefs`] anchors the floater
/// to the bottom-right corner only while its position still equals this (i.e. no
/// saved geometry moved it), so a persisted position is respected.
const SPAWN_POSITION: Vec2 = Vec2::new(-4096.0, -4096.0);

/// Section-heading colour.
const SECTION_COLOR: Color = Color::srgb(0.78, 0.83, 0.9);
/// Row-label colour.
const LABEL_COLOR: Color = Color::srgb(0.86, 0.88, 0.92);
/// The value-readout colour.
const VALUE_COLOR: Color = Color::srgb(0.7, 0.74, 0.82);
/// A control's border colour.
const CONTROL_BORDER: Color = Color::srgb(0.4, 0.5, 0.62);
/// A slider track's fill.
const TRACK_FILL: Color = Color::srgb(0.16, 0.19, 0.25);
/// A slider thumb's fill.
const THUMB_FILL: Color = Color::srgb(0.62, 0.72, 0.86);
/// A checkbox box's fill when unchecked.
const CHECK_OFF: Color = Color::srgb(0.12, 0.14, 0.18);
/// A checkbox box's fill when checked.
const CHECK_ON: Color = Color::srgb(0.3, 0.7, 0.45);
/// A thin divider between the environment section and the settings rows.
const DIVIDER_COLOR: Color = Color::srgb(0.28, 0.31, 0.38);

/// The toolbar gear button's font size, in logical pixels.
const BUTTON_FONT: f32 = 15.0;
/// The toolbar gear button's border.
const BUTTON_BORDER: Color = Color::srgb(0.3, 0.34, 0.42);
/// The toolbar gear button's fill.
const BUTTON_FILL: Color = Color::srgb(0.16, 0.17, 0.2);

// ---------------------------------------------------------------------------
// The entry model — a view over the settings store.
// ---------------------------------------------------------------------------

/// The kind of control a [`QuickPrefEntry`] surfaces its setting through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuickPrefKind {
    /// A boolean setting, shown as a checkbox.
    Checkbox,
    /// A numeric setting, shown as a slider with a value readout.
    Slider,
}

/// Where a [`QuickPrefEntry`]'s label text comes from.
#[derive(Debug, Clone)]
enum QuickPrefLabel {
    /// A Fluent key, resolved (and re-resolved on locale change) via
    /// [`Translated`] — the built-in default entries.
    Key(String),
    /// A literal display string — a hand-added entry from the JSON file.
    Text(String),
}

/// One curated entry: a named setting surfaced with a control. Built from the
/// code defaults ([`default_entries`]) or the per-avatar JSON file.
#[derive(Debug, Clone)]
struct QuickPrefEntry {
    /// The setting key this row edits (via a [`SettingBinding`]).
    control_name: String,
    /// The row's label.
    label: QuickPrefLabel,
    /// The override scope a user edit writes to.
    scope: Scope,
    /// The control kind.
    kind: QuickPrefKind,
    /// Whether a slider's value is an integer (rounds, shows no decimals).
    integer: bool,
    /// A slider's inclusive minimum.
    min: f32,
    /// A slider's inclusive maximum.
    max: f32,
    /// A slider's step / increment.
    increment: f32,
}

/// The built-in default entry set: the settings reached-for most, all of which
/// are load-bearing today. More entries are added by editing the per-avatar JSON
/// file; each binds by name, so no reimplementation is needed.
fn default_entries() -> Vec<QuickPrefEntry> {
    vec![
        QuickPrefEntry {
            control_name: crate::session::SETTING_DRAW_DISTANCE.to_owned(),
            label: QuickPrefLabel::Key("quick-prefs-draw-distance".to_owned()),
            scope: Scope::Global,
            kind: QuickPrefKind::Slider,
            integer: true,
            min: 32.0,
            max: 1024.0,
            increment: 8.0,
        },
        QuickPrefEntry {
            control_name: crate::particles::SETTING_MAX_PARTICLES.to_owned(),
            label: QuickPrefLabel::Key("quick-prefs-max-particles".to_owned()),
            scope: Scope::Global,
            kind: QuickPrefKind::Slider,
            integer: true,
            min: 0.0,
            max: 8192.0,
            increment: 256.0,
        },
        // The object LOD factor (RenderVolumeLODFactor) — the other
        // reached-for-hourly render knob (viewer-preferences-graphics-tab).
        QuickPrefEntry {
            control_name: crate::render_priority::SETTING_LOD_FACTOR.to_owned(),
            label: QuickPrefLabel::Key("quick-prefs-lod-factor".to_owned()),
            scope: Scope::Global,
            kind: QuickPrefKind::Slider,
            integer: false,
            min: crate::render_priority::LOD_FACTOR_MIN,
            max: crate::render_priority::LOD_FACTOR_MAX,
            increment: 0.125,
        },
        // Master audio volume — the volume panel's master bus, surfaced here too
        // (viewer-volume-panel); same setting key, so all three views agree.
        QuickPrefEntry {
            control_name: crate::volume_panel::master_volume_setting(),
            label: QuickPrefLabel::Key("quick-prefs-master-volume".to_owned()),
            scope: Scope::Global,
            kind: QuickPrefKind::Slider,
            integer: false,
            min: 0.0,
            max: 1.0,
            increment: 0.05,
        },
        // The avatar complexity budget (viewer-avatar-complexity-limit) — the
        // reached-for-hourly knob at a crowded event: slide it down until the
        // frame rate comes back and the heaviest avatars turn into silhouettes.
        QuickPrefEntry {
            control_name: crate::avatar_complexity::SETTING_MAX_COMPLEXITY.to_owned(),
            label: QuickPrefLabel::Key("quick-prefs-avatar-complexity".to_owned()),
            scope: Scope::Global,
            kind: QuickPrefKind::Slider,
            integer: true,
            min: 0.0,
            max: crate::avatar_complexity::MAX_COMPLEXITY_SLIDER_MAX,
            increment: crate::avatar_complexity::MAX_COMPLEXITY_SLIDER_STEP,
        },
        // Draw only friends' avatars (viewer-render-friends-only) — the
        // crowded-event escape hatch, reached for exactly when the machine is
        // already struggling, so it belongs on the panel you can open mid-lag.
        // Per avatar, like the setting itself.
        QuickPrefEntry {
            control_name: crate::derender::SETTING_FRIENDS_ONLY.to_owned(),
            label: QuickPrefLabel::Key("quick-prefs-friends-only".to_owned()),
            scope: Scope::Account,
            kind: QuickPrefKind::Checkbox,
            integer: false,
            min: 0.0,
            max: 1.0,
            increment: 1.0,
        },
        // Dynamic content (avatars) in local reflection probes: costlier and it
        // defeats probe change-detection, so it earns a one-click toggle here.
        QuickPrefEntry {
            control_name: crate::probes::PROBE_DYNAMIC_SETTING.to_owned(),
            label: QuickPrefLabel::Key("quick-prefs-probe-dynamic".to_owned()),
            scope: Scope::Global,
            kind: QuickPrefKind::Checkbox,
            integer: false,
            min: 0.0,
            max: 1.0,
            increment: 1.0,
        },
    ]
}

/// The JSON shape of one entry in the per-avatar `quick_preferences.json`. A
/// power user hand-edits this file; the fields mirror the reference viewer's
/// `quick_preferences.xml` attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct QuickPrefEntryJson {
    /// The setting key.
    control_name: String,
    /// A literal label (used when `label_key` is absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    /// A Fluent key for the label (the default entries carry this so they stay
    /// localised even once written to the file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label_key: Option<String>,
    /// `"slider"` or `"checkbox"`.
    #[serde(default = "default_control_type")]
    control_type: String,
    /// Whether the edit writes to the per-avatar (account) scope rather than the
    /// machine-wide (global) one.
    #[serde(default)]
    account_scope: bool,
    /// Whether a slider is integer-valued.
    #[serde(default)]
    integer: bool,
    /// A slider's minimum.
    #[serde(default)]
    min: f32,
    /// A slider's maximum.
    #[serde(default = "default_max")]
    max: f32,
    /// A slider's increment.
    #[serde(default = "default_increment")]
    increment: f32,
}

/// The default `control_type` when the JSON omits it.
fn default_control_type() -> String {
    "slider".to_owned()
}

/// The default slider maximum when the JSON omits it.
const fn default_max() -> f32 {
    1.0
}

/// The default slider increment when the JSON omits it.
const fn default_increment() -> f32 {
    1.0
}

impl QuickPrefEntry {
    /// Convert to the JSON shape for writing the template / round-tripping.
    fn to_json(&self) -> QuickPrefEntryJson {
        let (label, label_key) = match &self.label {
            QuickPrefLabel::Key(key) => (None, Some(key.clone())),
            QuickPrefLabel::Text(text) => (Some(text.clone()), None),
        };
        QuickPrefEntryJson {
            control_name: self.control_name.clone(),
            label,
            label_key,
            control_type: match self.kind {
                QuickPrefKind::Checkbox => "checkbox".to_owned(),
                QuickPrefKind::Slider => "slider".to_owned(),
            },
            account_scope: self.scope == Scope::Account,
            integer: self.integer,
            min: self.min,
            max: self.max,
            increment: self.increment,
        }
    }

    /// Build a runtime entry from a JSON record, or `None` for an unrecognised
    /// control type (a hand-edit typo is skipped, not fatal).
    fn from_json(json: QuickPrefEntryJson) -> Option<Self> {
        let kind = match json.control_type.as_str() {
            "checkbox" => QuickPrefKind::Checkbox,
            "slider" => QuickPrefKind::Slider,
            _ => return None,
        };
        let label = match (json.label_key, json.label) {
            (Some(key), _) => QuickPrefLabel::Key(key),
            (None, Some(text)) => QuickPrefLabel::Text(text),
            // No label at all: fall back to the control name so the row is at
            // least identifiable.
            (None, None) => QuickPrefLabel::Text(json.control_name.clone()),
        };
        Some(Self {
            control_name: json.control_name,
            label,
            scope: if json.account_scope {
                Scope::Account
            } else {
                Scope::Global
            },
            kind,
            integer: json.integer,
            min: json.min,
            max: json.max,
            increment: json.increment,
        })
    }
}

/// Load the entry list for the current avatar: the per-avatar
/// `quick_preferences.json` if present and parseable, else the built-in
/// [`default_entries`]. A missing file is the first-run case, not an error.
fn load_entries(settings: &ViewerSettings) -> Vec<QuickPrefEntry> {
    let Some(dir) = settings.account_dir() else {
        return default_entries();
    };
    let path = dir.join(ENTRIES_FILE);
    if !path.exists() {
        return default_entries();
    }
    match fs_err::read_to_string(&path) {
        Ok(contents) => match serde_json::from_str::<Vec<QuickPrefEntryJson>>(&contents) {
            Ok(records) => {
                let entries: Vec<QuickPrefEntry> = records
                    .into_iter()
                    .filter_map(QuickPrefEntry::from_json)
                    .collect();
                if entries.is_empty() {
                    default_entries()
                } else {
                    entries
                }
            }
            Err(error) => {
                warn!("quick-prefs: could not parse {}: {error}", path.display());
                default_entries()
            }
        },
        Err(error) => {
            warn!("quick-prefs: could not read {}: {error}", path.display());
            default_entries()
        }
    }
}

/// Write the default entry list to the per-avatar file once (post-login) if it is
/// absent, so a power user has a self-describing template to edit. Best-effort: a
/// write failure is logged, never fatal.
fn write_template(settings: Option<Res<ViewerSettings>>, mut written: Local<bool>) {
    if *written {
        return;
    }
    let Some(settings) = settings else {
        return;
    };
    if !settings.account_loaded() {
        return;
    }
    *written = true;
    let Some(dir) = settings.account_dir() else {
        return;
    };
    let path = dir.join(ENTRIES_FILE);
    if path.exists() {
        return;
    }
    let records: Vec<QuickPrefEntryJson> = default_entries()
        .iter()
        .map(QuickPrefEntry::to_json)
        .collect();
    match serde_json::to_string_pretty(&records) {
        Ok(json) => {
            if let Err(error) = fs_err::write(&path, json) {
                warn!(
                    "quick-prefs: could not write template {}: {error}",
                    path.display()
                );
            } else {
                info!("quick-prefs: wrote entry template {}", path.display());
            }
        }
        Err(error) => warn!("quick-prefs: could not serialise template: {error}"),
    }
}

// ---------------------------------------------------------------------------
// The environment section (drives EnvironmentState, not a setting).
// ---------------------------------------------------------------------------

/// The environment preset groups the group combo offers, in option order —
/// mapping Firestorm's sky / day-cycle preset choices onto our
/// [`FixedEnvironment`] model. The first is "shared" (un-pinned).
const ENV_GROUP_KEYS: [&str; 4] = [
    "quick-prefs-env-shared",
    "quick-prefs-env-daycycle",
    "quick-prefs-env-legacy",
    "quick-prefs-env-modern",
];

/// The times of day the time combo offers, in option order.
const ENV_TIME_KEYS: [&str; 4] = [
    "quick-prefs-time-sunrise",
    "quick-prefs-time-midday",
    "quick-prefs-time-sunset",
    "quick-prefs-time-midnight",
];

/// The two environment combos' anchor entities, so the apply/sync systems can
/// read both selections at once.
#[derive(Resource, Debug, Clone, Copy)]
struct QuickPrefEnvCombos {
    /// The preset-group combo anchor.
    group: Entity,
    /// The time-of-day combo anchor.
    time: Entity,
}

/// The element id (and [`ComboChanged`] tag) of the environment group combo.
const ENV_GROUP_ELEMENT: &str = "quick-prefs-env-group";
/// The element id of the environment time combo.
const ENV_TIME_ELEMENT: &str = "quick-prefs-env-time";

/// The [`FixedSky`] for a time-combo option index.
const fn sky_for_time_index(index: usize) -> FixedSky {
    match index {
        1 => FixedSky::Midday,
        2 => FixedSky::Sunset,
        3 => FixedSky::Midnight,
        // Index 0 and any out-of-range value fall back to sunrise.
        _ => FixedSky::Sunrise,
    }
}

/// The time-combo option index for a [`FixedSky`].
const fn time_index_for_sky(sky: FixedSky) -> usize {
    match sky {
        FixedSky::Sunrise => 0,
        FixedSky::Midday => 1,
        FixedSky::Sunset => 2,
        FixedSky::Midnight => 3,
    }
}

/// The [`FixedEnvironment`] for a (group index, time index) pair, or `None` for
/// the shared (un-pinned) group.
const fn fixed_for(group_index: usize, time_index: usize) -> Option<FixedEnvironment> {
    let sky = sky_for_time_index(time_index);
    match group_index {
        1 => Some(FixedEnvironment::DayCycle(sky)),
        2 => Some(FixedEnvironment::Legacy(sky)),
        3 => Some(FixedEnvironment::Modern(sky)),
        // Index 0 (shared) and out-of-range: un-pin.
        _ => None,
    }
}

/// The (group index, time index) pair for the current fixed environment. A
/// shared environment keeps `time` at midday so switching to a group starts
/// somewhere sensible.
const fn combo_indices(fixed: Option<FixedEnvironment>) -> (usize, usize) {
    match fixed {
        None => (0, time_index_for_sky(FixedSky::Midday)),
        Some(FixedEnvironment::DayCycle(sky)) => (1, time_index_for_sky(sky)),
        Some(FixedEnvironment::Legacy(sky)) => (2, time_index_for_sky(sky)),
        Some(FixedEnvironment::Modern(sky)) => (3, time_index_for_sky(sky)),
    }
}

// ---------------------------------------------------------------------------
// The plugin.
// ---------------------------------------------------------------------------

/// A marker on the quick-prefs floater root, so [`anchor_quick_prefs`] finds it.
#[derive(Component, Debug, Clone, Copy)]
struct QuickPrefsFloaterRoot;

/// A marker on a setting slider's thumb, so [`drive_quick_pref_thumbs`] slides it.
#[derive(Component, Debug, Clone, Copy)]
struct QuickPrefSliderThumb;

/// A marker on a setting checkbox's box, so [`drive_quick_pref_checkboxes`]
/// colours it.
#[derive(Component, Debug, Clone, Copy)]
struct QuickPrefCheckboxBox;

/// A setting row's trailing value readout, tagged with what it displays.
#[derive(Component, Debug, Clone)]
struct QuickPrefValueLabel {
    /// The setting whose value this label shows.
    control_name: String,
    /// Whether to format as an integer (no decimals).
    integer: bool,
}

/// Owns the Quick Preferences panel: the floater chrome + deferred content, the
/// toolbar button, the environment combos, and the control visuals.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct QuickPreferencesPlugin;

impl Plugin for QuickPreferencesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            spawn_quick_prefs_floater.after(UiScaffoldSystems::SpawnRoot),
        )
        .add_systems(
            Update,
            (
                // Trailing-most in the bottom area's upper-trailing slot, so it
                // spawns after the parcel-audio and volume controls it follows.
                spawn_quick_prefs_button.after(crate::volume_panel::spawn_volume_controls),
                write_template,
                anchor_quick_prefs,
                apply_env_combos,
                sync_env_combos.after(apply_env_combos),
                update_quick_pref_values,
                drive_quick_pref_thumbs,
                drive_quick_pref_checkboxes,
            ),
        );
    }
}

/// Startup: spawn the floater chrome (hidden), its content deferred to first open.
fn spawn_quick_prefs_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: QUICK_PREFS_FLOATER_ID,
            title: "Quick Preferences".to_owned(),
            position: SPAWN_POSITION,
            default_size: Some(DEFAULT_SIZE),
            min_size: Some(MIN_SIZE),
            dock_host: None,
            caps: FloaterCaps {
                resizable: true,
                minimizable: true,
                closable: true,
                dockable: false,
            },
        },
    );
    commands
        .entity(handle.title_text)
        .insert(Translated::new("quick-prefs-title"));
    commands.entity(handle.root).insert(QuickPrefsFloaterRoot);
    let builder = commands.register_system(build_quick_prefs_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build: the environment section, a divider, then one row per
/// entry (loaded from the per-avatar file or the defaults).
fn build_quick_prefs_content(
    In(handle): In<FloaterHandle>,
    mut commands: Commands,
    settings: Option<Res<ViewerSettings>>,
) {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                padding: UiRect::all(Val::Px(4.0)),
                ..column(Val::Px(ROW_GAP))
            },
            Name::new("quick-prefs:content"),
            ChildOf(handle.content),
        ))
        .id();

    spawn_section(&mut commands, content, "quick-prefs-environment");
    let (group, time) = spawn_env_rows(&mut commands, content);
    commands.insert_resource(QuickPrefEnvCombos { group, time });

    spawn_divider(&mut commands, content);

    spawn_quality_row(&mut commands, content);

    let entries = settings
        .as_deref()
        .map_or_else(default_entries, load_entries);
    for entry in &entries {
        let Some(kind) = binding_kind(settings.as_deref(), entry) else {
            continue;
        };
        spawn_entry_row(&mut commands, content, entry, kind);
    }
}

/// The setting kind a row would bind, or `None` when the setting is unregistered
/// or its type does not match the control (a hand-edit mistake is skipped, not
/// bound to nothing). The gallery app (no store) admits every entry so a specimen
/// can render.
fn binding_kind(
    settings: Option<&ViewerSettings>,
    entry: &QuickPrefEntry,
) -> Option<QuickPrefKind> {
    let Some(settings) = settings else {
        return Some(entry.kind);
    };
    let kind = settings.store().declaration(&entry.control_name)?.kind();
    let ok = match entry.kind {
        QuickPrefKind::Checkbox => kind == SettingKind::Bool,
        QuickPrefKind::Slider => {
            matches!(kind, SettingKind::F32 | SettingKind::I32 | SettingKind::U32)
        }
    };
    if ok {
        Some(entry.kind)
    } else {
        warn!(
            "quick-prefs: setting {} is {kind:?}, not usable as {:?}; skipping",
            entry.control_name, entry.kind
        );
        None
    }
}

/// Spawn a section heading.
fn spawn_section(commands: &mut Commands, parent: Entity, key: &'static str) {
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(SECTION_FONT),
        TextColor(SECTION_COLOR),
        Name::new(format!("quick-prefs:section:{key}")),
        ChildOf(parent),
    ));
}

/// Spawn a thin horizontal divider.
fn spawn_divider(commands: &mut Commands, parent: Entity) {
    commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(1.0),
            ..default()
        },
        BackgroundColor(DIVIDER_COLOR),
        Name::new("quick-prefs:divider"),
        ChildOf(parent),
    ));
}

/// A label + control row node.
fn row_node() -> Node {
    Node {
        align_items: AlignItems::Center,
        justify_content: JustifyContent::SpaceBetween,
        width: Val::Percent(100.0),
        ..row(Val::Px(ROW_GAP))
    }
}

/// Spawn the two environment combos (preset group + time of day), returning their
/// anchor entities.
fn spawn_env_rows(commands: &mut Commands, parent: Entity) -> (Entity, Entity) {
    let group = spawn_env_combo_row(
        commands,
        parent,
        "quick-prefs-env-preset",
        ENV_GROUP_ELEMENT,
        &ENV_GROUP_KEYS,
        1,
    );
    let time = spawn_env_combo_row(
        commands,
        parent,
        "quick-prefs-env-time",
        ENV_TIME_ELEMENT,
        &ENV_TIME_KEYS,
        2,
    );
    (group, time)
}

/// Spawn the quality-preset combo row: a combo bound to
/// [`crate::preferences_graphics::SETTING_RENDER_QUALITY`] whose anchor
/// carries [`crate::preferences_graphics::QualityTierControl`], so a user
/// pick here applies the tier exactly like the graphics tab's row (one
/// applier serves both surfaces). Quick prefs has no snapshot / revert, so
/// the pick simply applies live.
fn spawn_quality_row(commands: &mut Commands, parent: Entity) {
    let row = commands
        .spawn((
            row_node(),
            Name::new("quick-prefs:row:quality"),
            ChildOf(parent),
        ))
        .id();
    spawn_label(commands, row, "quick-prefs-quality");
    let labels: Vec<String> = crate::preferences_graphics::QUALITY_OPTION_KEYS
        .iter()
        .map(|key| (*key).to_owned())
        .collect();
    let anchor = spawn_combo(
        commands,
        row,
        &ComboSpec {
            element: "quick-prefs:quality",
            labels: &labels,
            active: 0,
            tab_index: 3,
            font_size: FONT,
            translate_labels: true,
        },
    );
    commands.entity(anchor).insert((
        SettingBinding::global(crate::preferences_graphics::SETTING_RENDER_QUALITY),
        crate::settings_binding::ComboBindingValues(
            crate::preferences_graphics::quality_option_values(),
        ),
        crate::preferences_graphics::QualityTierControl,
    ));
}

/// Spawn one labelled environment combo, returning its anchor.
fn spawn_env_combo_row(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    element: &'static str,
    option_keys: &[&str],
    tab_index: i32,
) -> Entity {
    let row = commands
        .spawn((
            row_node(),
            Name::new(format!("quick-prefs:env-row:{element}")),
            ChildOf(parent),
        ))
        .id();
    spawn_label(commands, row, label_key);
    let labels: Vec<String> = option_keys.iter().map(|key| (*key).to_owned()).collect();
    spawn_combo(
        commands,
        row,
        &ComboSpec {
            element,
            labels: &labels,
            active: 0,
            tab_index,
            font_size: FONT,
            translate_labels: true,
        },
    )
}

/// Spawn a row's translated label.
fn spawn_label(commands: &mut Commands, parent: Entity, label_key: &'static str) {
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(parent),
    ));
}

/// Spawn a setting entry's row: a checkbox row or a slider row.
fn spawn_entry_row(
    commands: &mut Commands,
    parent: Entity,
    entry: &QuickPrefEntry,
    kind: QuickPrefKind,
) {
    match kind {
        QuickPrefKind::Checkbox => spawn_checkbox_row(commands, parent, entry),
        QuickPrefKind::Slider => spawn_slider_row(commands, parent, entry),
    }
}

/// The binding for an entry, at its declared scope.
fn entry_binding(entry: &QuickPrefEntry) -> SettingBinding {
    match entry.scope {
        Scope::Account => SettingBinding::account(&entry.control_name),
        Scope::Global => SettingBinding::global(&entry.control_name),
    }
}

/// Spawn an entry's label node (translated key or literal text).
fn spawn_entry_label(commands: &mut Commands, parent: Entity, entry: &QuickPrefEntry) {
    let mut label = commands.spawn((
        Text::default(),
        UiFont::Sans.at(FONT),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(parent),
    ));
    match &entry.label {
        QuickPrefLabel::Key(key) => {
            label.insert(Translated::new(key.clone()));
        }
        QuickPrefLabel::Text(text) => {
            label.insert(Text::new(text.clone()));
        }
    }
}

/// Spawn a checkbox row bound to a boolean setting.
fn spawn_checkbox_row(commands: &mut Commands, parent: Entity, entry: &QuickPrefEntry) {
    let row = commands
        .spawn((
            row_node(),
            Name::new(format!("quick-prefs:row:{}", entry.control_name)),
            ChildOf(parent),
        ))
        .id();
    spawn_entry_label(commands, row, entry);
    commands.spawn((
        bound_checkbox(entry_binding(entry)),
        Node {
            width: Val::Px(CHECK_SIZE),
            height: Val::Px(CHECK_SIZE),
            border: UiRect::all(Val::Px(2.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BorderColor::all(CONTROL_BORDER),
        BackgroundColor(CHECK_OFF),
        TabIndex(0),
        QuickPrefCheckboxBox,
        ChildOf(row),
    ));
}

/// Spawn a slider row bound to a numeric setting, with a trailing value readout.
fn spawn_slider_row(commands: &mut Commands, parent: Entity, entry: &QuickPrefEntry) {
    let row_entity = commands
        .spawn((
            row_node(),
            Name::new(format!("quick-prefs:row:{}", entry.control_name)),
            ChildOf(parent),
        ))
        .id();
    spawn_entry_label(commands, row_entity, entry);
    // A trailing group holds the slider and its numeric readout together, so the
    // label sits at the leading edge and the control at the trailing edge.
    let group = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..row(Val::Px(6.0))
            },
            ChildOf(row_entity),
        ))
        .id();
    commands
        .spawn((
            bound_slider(
                entry_binding(entry),
                SliderRange::new(entry.min, entry.max),
                SliderStep(entry.increment),
            ),
            Node {
                width: Val::Px(TRACK_WIDTH),
                height: Val::Px(TRACK_HEIGHT),
                border: UiRect::all(Val::Px(2.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(TRACK_FILL),
            TabIndex(0),
            ChildOf(group),
        ))
        .with_children(|track| {
            track.spawn((
                SliderThumb,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(THUMB_WIDTH),
                    height: Val::Px(TRACK_HEIGHT),
                    ..default()
                },
                LogicalInset(LogicalRect {
                    inline_start: Val::Px(0.0),
                    ..LogicalRect::ZERO
                }),
                BackgroundColor(THUMB_FILL),
                QuickPrefSliderThumb,
            ));
        });
    // A right-aligning slot with a minimum width (not a fixed one) so the readout
    // column lines up but a long value / large UI scale grows it rather than
    // clipping or wrapping the text. The value Text itself is content-sized (a
    // width on the Text leaf makes bevy_text wrap instead of growing the box).
    let value_slot = commands
        .spawn((
            Node {
                min_width: Val::Px(VALUE_WIDTH),
                justify_content: JustifyContent::FlexEnd,
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(group),
        ))
        .id();
    commands.spawn((
        Text::default(),
        UiFont::Sans.at(FONT),
        TextColor(VALUE_COLOR),
        QuickPrefValueLabel {
            control_name: entry.control_name.clone(),
            integer: entry.integer,
        },
        Pickable::IGNORE,
        ChildOf(value_slot),
    ));
}

// ---------------------------------------------------------------------------
// The toolbar button.
// ---------------------------------------------------------------------------

/// Spawn the gear button into the bottom area's trailing slot, once (the
/// [`Local`] latch waits for the toolbar host to exist).
pub(crate) fn spawn_quick_prefs_button(
    mut commands: Commands,
    area: Option<Res<BottomArea>>,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }
    let Some(area) = area else {
        return;
    };
    *spawned = true;
    let wrapper = commands
        .spawn((
            Node {
                align_items: AlignItems::FlexEnd,
                ..row(Val::ZERO)
            },
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Name::new("quick-prefs-button-wrapper"),
            ChildOf(area.upper_trailing),
        ))
        .id();
    commands
        .spawn((
            Button,
            TabIndex(0),
            Node {
                padding: UiRect::axes(Val::Px(7.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_FILL),
            Name::new("quick-prefs-button"),
            ChildOf(wrapper),
        ))
        .observe(on_quick_prefs_button)
        .with_child((
            Text::new("⚙"),
            UiFont::Sans.at(BUTTON_FONT),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ));
}

/// Observer: toggle the quick-prefs floater, raising it when it opens.
fn on_quick_prefs_button(
    _activate: On<Activate>,
    floaters: Query<(Entity, &Floater)>,
    mut panels: Query<&mut UiPanelShown>,
    mut commands: MessageWriter<FloaterCommand>,
) {
    let Some(panel) = floater_panel(&floaters, QUICK_PREFS_FLOATER_ID) else {
        return;
    };
    let Ok(mut shown) = panels.get_mut(panel) else {
        return;
    };
    shown.0 = !shown.0;
    if shown.0 {
        commands.write(FloaterCommand {
            floater: panel,
            op: FloaterOp::BringToFront,
        });
    }
}

/// Anchor the floater to the bottom-right corner the first time it is shown,
/// unless a persisted position already moved it off the spawn sentinel (saved
/// geometry wins). Runs once, latched.
fn anchor_quick_prefs(
    mut done: Local<bool>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut floaters: Query<(&mut Floater, &UiPanelShown), With<QuickPrefsFloaterRoot>>,
) {
    if *done {
        return;
    }
    let Ok((mut floater, shown)) = floaters.single_mut() else {
        return;
    };
    if !shown.0 {
        return;
    }
    *done = true;
    // A saved position (seeded by floater persistence at login) has already moved
    // the floater off the sentinel; respect it.
    if floater.geometry().position.distance(SPAWN_POSITION) > 1.0 {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let width = window.width();
    let height = window.height();
    let position = Vec2::new(
        (width - DEFAULT_SIZE.x - ANCHOR_MARGIN).max(ANCHOR_MARGIN),
        (height - DEFAULT_SIZE.y - ANCHOR_MARGIN).max(ANCHOR_MARGIN),
    );
    floater.set_position(position);
}

// ---------------------------------------------------------------------------
// The environment combos ↔ EnvironmentState.
// ---------------------------------------------------------------------------

/// Apply a user pick on either environment combo to [`EnvironmentState`]: recompute
/// the fixed environment from both combos' current selections and pin it (or
/// un-pin for the shared group).
fn apply_env_combos(
    mut changes: MessageReader<ComboChanged>,
    combos: Option<Res<QuickPrefEnvCombos>>,
    selections: Query<&ComboSelection>,
    environment: Option<ResMut<EnvironmentState>>,
) {
    // The combos resource is published when the floater's content is first built;
    // until then there is nothing to apply.
    let Some(combos) = combos else {
        return;
    };
    // Take the changed combo's new index straight from the message (authoritative,
    // no dependency on when the widget commits `ComboSelection`); read the other
    // combo's current selection.
    let mut new_group: Option<usize> = None;
    let mut new_time: Option<usize> = None;
    for change in changes.read() {
        if change.combo == combos.group {
            new_group = Some(change.active);
        } else if change.combo == combos.time {
            new_time = Some(change.active);
        }
    }
    if new_group.is_none() && new_time.is_none() {
        return;
    }
    let Some(mut environment) = environment else {
        return;
    };
    let group = new_group.unwrap_or_else(|| selections.get(combos.group).map_or(0, |s| s.active));
    let time = new_time.unwrap_or_else(|| {
        selections
            .get(combos.time)
            .map_or_else(|_| time_index_for_sky(FixedSky::Midday), |s| s.active)
    });
    environment.set_fixed(fixed_for(group, time));
}

/// Keep the environment combos showing the current [`EnvironmentState`] (so an
/// external change — the World ▸ Environment menu — is reflected), and disable the
/// time combo while the shared group is selected (time has no effect then).
fn sync_env_combos(
    combos: Option<Res<QuickPrefEnvCombos>>,
    environment: Option<Res<EnvironmentState>>,
    mut selections: Query<&mut ComboSelection>,
    time_disabled: Query<Has<InteractionDisabled>>,
    mut commands: Commands,
) {
    let Some(combos) = combos else {
        return;
    };
    let Some(environment) = environment else {
        return;
    };
    let (group_index, time_index) = combo_indices(environment.fixed());
    if let Ok(mut group) = selections.get_mut(combos.group)
        && group.active != group_index
    {
        group.active = group_index;
    }
    if let Ok(mut time) = selections.get_mut(combos.time)
        && time.active != time_index
    {
        time.active = time_index;
    }
    // Disable the time combo while shared (group 0): a time only means something
    // once a preset group pins the sky.
    let want_disabled = group_index == 0;
    if time_disabled.get(combos.time).unwrap_or(false) != want_disabled {
        if want_disabled {
            commands.entity(combos.time).insert(InteractionDisabled);
        } else {
            commands.entity(combos.time).remove::<InteractionDisabled>();
        }
    }
}

// ---------------------------------------------------------------------------
// Control visuals (thumb position, checkbox fill, value readout).
// ---------------------------------------------------------------------------

/// Keep each setting slider's value readout current from the store.
fn update_quick_pref_values(
    settings: Option<Res<ViewerSettings>>,
    mut labels: Query<(&mut Text, &QuickPrefValueLabel)>,
) {
    let Some(settings) = settings else {
        return;
    };
    for (mut text, label) in &mut labels {
        let Some(value) = settings
            .store()
            .get(&label.control_name)
            .and_then(setting_as_f32)
        else {
            continue;
        };
        let wanted = if label.integer {
            format!("{}", value.round())
        } else {
            format!("{value:.2}")
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

/// A setting value as the `f32` a slider readout shows, or `None` for a
/// non-numeric setting.
const fn setting_as_f32(value: &sl_settings::SettingValue) -> Option<f32> {
    match value {
        sl_settings::SettingValue::F32(v) => Some(*v),
        sl_settings::SettingValue::I32(v) => Some(i32_to_f32(*v)),
        sl_settings::SettingValue::U32(v) => Some(u32_to_f32(*v)),
        _ => None,
    }
}

/// Widen an `i32` setting to the `f32` a readout shows.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "a quick-prefs integer setting's magnitude is small; the readout only shows whole numbers"
)]
const fn i32_to_f32(value: i32) -> f32 {
    value as f32
}

/// Widen a `u32` setting to the `f32` a readout shows.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "a quick-prefs integer setting's magnitude is small; the readout only shows whole numbers"
)]
const fn u32_to_f32(value: u32) -> f32 {
    value as f32
}

/// Slide each setting slider's thumb to its value within the range.
fn drive_quick_pref_thumbs(
    sliders: Query<(&SliderValue, &SliderRange, &Children), With<Slider>>,
    mut thumbs: Query<&mut LogicalInset, With<QuickPrefSliderThumb>>,
) {
    for (value, range, children) in &sliders {
        let span = range.span();
        let fraction = if span > f32::EPSILON {
            ((value.0 - range.start()) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let offset = fraction * (TRACK_WIDTH - THUMB_WIDTH);
        for child in children {
            if let Ok(mut inset) = thumbs.get_mut(*child) {
                inset.0.inline_start = Val::Px(offset);
            }
        }
    }
}

/// Colour each setting checkbox's box from its `Checked` state.
fn drive_quick_pref_checkboxes(
    mut boxes: Query<(&mut BackgroundColor, Has<Checked>), With<QuickPrefCheckboxBox>>,
) {
    for (mut fill, checked) in &mut boxes {
        let target = if checked { CHECK_ON } else { CHECK_OFF };
        if fill.0 != target {
            fill.0 = target;
        }
    }
}

// ---------------------------------------------------------------------------
// The gallery specimen.
// ---------------------------------------------------------------------------

/// The static quick-prefs specimen for the gallery / headless harness: the
/// environment section over a divider and two setting slider rows — the layout,
/// with none of the live behaviour (per the element registry's rule: no plugin,
/// no store, no observers).
pub(crate) fn spawn_quick_prefs_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let card = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(10.0)),
                min_width: Val::Px(280.0),
                ..column(Val::Px(ROW_GAP))
            },
            Name::new("quick-prefs-specimen"),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(cx.text("Environment")),
        cx.font(UiFont::Sans),
        TextColor(SECTION_COLOR),
        ChildOf(card),
    ));
    spawn_specimen_combo_row(commands, card, &cx, "Preset", "Legacy WindLight");
    spawn_specimen_combo_row(commands, card, &cx, "Time of day", "Midday");
    spawn_divider(commands, card);
    spawn_specimen_slider_row(commands, card, &cx, "Draw distance", "512", 0.5);
    spawn_specimen_slider_row(commands, card, &cx, "Max particles", "4096", 0.5);
    card
}

/// A content-sized specimen row (unlike the live [`row_node`], which fills the
/// floater with `width: 100%` + space-between): the card grows to the widest row,
/// so no fixed-width child can overflow its box across scripts / scales.
fn specimen_row() -> Node {
    Node {
        align_items: AlignItems::Center,
        ..row(Val::Px(ROW_GAP))
    }
}

/// A static combo-looking row for the specimen: a label and a bordered value box.
fn spawn_specimen_combo_row(
    commands: &mut Commands,
    parent: Entity,
    cx: &ElementCx,
    label: &str,
    value: &str,
) {
    let row_entity = commands.spawn((specimen_row(), ChildOf(parent))).id();
    commands.spawn((
        Text::new(cx.text(label)),
        cx.font(UiFont::Sans),
        TextColor(LABEL_COLOR),
        ChildOf(row_entity),
    ));
    commands
        .spawn((
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(TRACK_FILL),
            ChildOf(row_entity),
        ))
        .with_child((
            Text::new(cx.text(value)),
            cx.font(UiFont::Sans),
            TextColor(VALUE_COLOR),
        ));
}

/// A static slider row for the specimen: a label, a track with a thumb at
/// `fraction`, and a value readout.
fn spawn_specimen_slider_row(
    commands: &mut Commands,
    parent: Entity,
    cx: &ElementCx,
    label: &str,
    value: &str,
    fraction: f32,
) {
    let row_entity = commands.spawn((specimen_row(), ChildOf(parent))).id();
    commands.spawn((
        Text::new(cx.text(label)),
        cx.font(UiFont::Sans),
        TextColor(LABEL_COLOR),
        ChildOf(row_entity),
    ));
    let group = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(row_entity),
        ))
        .id();
    commands
        .spawn((
            Node {
                width: Val::Px(TRACK_WIDTH),
                height: Val::Px(TRACK_HEIGHT),
                border: UiRect::all(Val::Px(2.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(TRACK_FILL),
            ChildOf(group),
        ))
        .with_child((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(THUMB_WIDTH),
                height: Val::Px(TRACK_HEIGHT),
                ..default()
            },
            LogicalInset(LogicalRect {
                inline_start: Val::Px(fraction * (TRACK_WIDTH - THUMB_WIDTH)),
                ..LogicalRect::ZERO
            }),
            BackgroundColor(THUMB_FILL),
        ));
    commands.spawn((
        Text::new(cx.text(value)),
        cx.font(UiFont::Sans),
        TextColor(VALUE_COLOR),
        ChildOf(group),
    ));
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_settings::{Scope, SettingValue, SettingsStore};

    use super::{
        ENV_GROUP_ELEMENT, ENV_TIME_ELEMENT, QuickPrefEntry, QuickPrefEnvCombos, QuickPrefKind,
        QuickPrefLabel, QuickPrefValueLabel, apply_env_combos, binding_kind, combo_indices,
        default_entries, fixed_for, sky_for_time_index, sync_env_combos, time_index_for_sky,
        update_quick_pref_values,
    };
    use crate::environment::{EnvironmentState, FixedEnvironment};
    use crate::settings::ViewerSettings;
    use crate::sky_presets::FixedSky;
    use crate::ui_combo::{ComboChanged, ComboSelection};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// Each time-of-day index round-trips through its [`FixedSky`].
    #[test]
    fn time_index_round_trips() {
        for index in 0..4 {
            assert_eq!(time_index_for_sky(sky_for_time_index(index)), index);
        }
    }

    /// The group / time combo indices map to (and back from) the fixed
    /// environment, including the shared (un-pinned) group.
    #[test]
    fn combo_indices_round_trip() {
        assert_eq!(fixed_for(0, 1), None);
        assert_eq!(
            fixed_for(2, 2),
            Some(FixedEnvironment::Legacy(FixedSky::Sunset))
        );
        assert_eq!(
            combo_indices(Some(FixedEnvironment::Modern(FixedSky::Sunrise))),
            (3, 0)
        );
        // A shared environment reports the shared group and a sensible default
        // time (midday), so switching to a group starts somewhere lit.
        assert_eq!(
            combo_indices(None),
            (0, time_index_for_sky(FixedSky::Midday))
        );
    }

    /// The default entries surface the load-bearing render settings by their
    /// real keys, so the panel binds to something that exists.
    #[test]
    fn defaults_name_real_settings() {
        let entries = default_entries();
        let names: Vec<&str> = entries
            .iter()
            .map(|entry| entry.control_name.as_str())
            .collect();
        assert!(names.contains(&crate::session::SETTING_DRAW_DISTANCE));
        assert!(names.contains(&crate::particles::SETTING_MAX_PARTICLES));
    }

    /// A JSON entry round-trips through the runtime model, preserving scope, kind
    /// and label source.
    #[test]
    fn entry_json_round_trips() -> Result<(), TestError> {
        let entry = QuickPrefEntry {
            control_name: "SomeFlag".to_owned(),
            label: QuickPrefLabel::Text("Some flag".to_owned()),
            scope: Scope::Account,
            kind: QuickPrefKind::Checkbox,
            integer: false,
            min: 0.0,
            max: 1.0,
            increment: 1.0,
        };
        let restored =
            QuickPrefEntry::from_json(entry.to_json()).ok_or("checkbox entry should round-trip")?;
        assert_eq!(restored.control_name, "SomeFlag");
        assert_eq!(restored.scope, Scope::Account);
        assert_eq!(restored.kind, QuickPrefKind::Checkbox);
        matches!(restored.label, QuickPrefLabel::Text(_))
            .then_some(())
            .ok_or("a literal label should stay literal")?;
        Ok(())
    }

    /// An unrecognised control type in the JSON is skipped, not fatal.
    #[test]
    fn unknown_control_type_is_skipped() {
        let entry = QuickPrefEntry {
            control_name: "X".to_owned(),
            label: QuickPrefLabel::Text("X".to_owned()),
            scope: Scope::Global,
            kind: QuickPrefKind::Slider,
            integer: false,
            min: 0.0,
            max: 1.0,
            increment: 1.0,
        };
        let mut json = entry.to_json();
        json.control_type = "colorswatch".to_owned();
        assert!(QuickPrefEntry::from_json(json).is_none());
    }

    /// A test store with the two render settings registered.
    fn render_store() -> SettingsStore {
        let mut store = SettingsStore::new();
        store
            .register(
                crate::session::SETTING_DRAW_DISTANCE,
                SettingValue::F32(512.0),
                "draw distance",
            )
            .ok();
        store
            .register(
                crate::particles::SETTING_MAX_PARTICLES,
                SettingValue::U32(4096),
                "particle cap",
            )
            .ok();
        store
    }

    /// A slider entry binds a numeric setting, but not a mismatched or absent one.
    #[test]
    fn binding_kind_checks_type() {
        let settings = ViewerSettings::from_store_for_test(render_store());
        let slider = QuickPrefEntry {
            control_name: crate::session::SETTING_DRAW_DISTANCE.to_owned(),
            label: QuickPrefLabel::Text("d".to_owned()),
            scope: Scope::Global,
            kind: QuickPrefKind::Slider,
            integer: true,
            min: 32.0,
            max: 1024.0,
            increment: 8.0,
        };
        assert_eq!(
            binding_kind(Some(&settings), &slider),
            Some(QuickPrefKind::Slider)
        );
        // A checkbox over a numeric setting is a type mismatch: skipped.
        let mismatched = QuickPrefEntry {
            kind: QuickPrefKind::Checkbox,
            ..slider.clone()
        };
        assert_eq!(binding_kind(Some(&settings), &mismatched), None);
        // An unregistered setting: skipped.
        let absent = QuickPrefEntry {
            control_name: "NoSuchSetting".to_owned(),
            ..slider
        };
        assert_eq!(binding_kind(Some(&settings), &absent), None);
    }

    /// A headless app wired for the environment-combo systems, with the two combo
    /// anchors and the environment state.
    fn env_app(group_active: usize, time_active: usize) -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<ComboChanged>()
            .insert_resource(EnvironmentState::default());
        let group = app
            .world_mut()
            .spawn(ComboSelection {
                element: ENV_GROUP_ELEMENT,
                active: group_active,
            })
            .id();
        let time = app
            .world_mut()
            .spawn(ComboSelection {
                element: ENV_TIME_ELEMENT,
                active: time_active,
            })
            .id();
        app.insert_resource(QuickPrefEnvCombos { group, time });
        app.add_systems(Update, apply_env_combos);
        (app, group, time)
    }

    /// Picking a preset group (with the widget having moved the selection) pins the
    /// matching fixed environment at the time combo's current time.
    #[test]
    fn env_combo_pick_pins_environment() -> Result<(), TestError> {
        // The widget has already moved the group selection to Legacy (index 2);
        // the ComboChanged is the signal that a user pick happened.
        let (mut app, group, _time) = env_app(2, time_index_for_sky(FixedSky::Sunset));
        app.world_mut().write_message(ComboChanged {
            combo: group,
            active: 2,
        });
        app.update();
        let state = app
            .world()
            .get_resource::<EnvironmentState>()
            .ok_or("environment state present")?;
        assert_eq!(
            state.fixed(),
            Some(FixedEnvironment::Legacy(FixedSky::Sunset))
        );
        Ok(())
    }

    /// A ComboChanged from an unrelated combo does not touch the environment.
    #[test]
    fn foreign_combo_change_is_ignored() -> Result<(), TestError> {
        let (mut app, _group, _time) = env_app(0, 1);
        let other = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(ComboChanged {
            combo: other,
            active: 3,
        });
        app.update();
        let state = app
            .world()
            .get_resource::<EnvironmentState>()
            .ok_or("environment state present")?;
        assert_eq!(state.fixed(), None);
        Ok(())
    }

    /// The sync pass reflects an external environment change onto the combos and
    /// disables the time combo only while the shared group is selected.
    #[test]
    fn sync_reflects_environment_and_gates_time() -> Result<(), TestError> {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(EnvironmentState::default());
        let group = app
            .world_mut()
            .spawn(ComboSelection {
                element: ENV_GROUP_ELEMENT,
                active: 0,
            })
            .id();
        let time = app
            .world_mut()
            .spawn(ComboSelection {
                element: ENV_TIME_ELEMENT,
                active: 0,
            })
            .id();
        app.insert_resource(QuickPrefEnvCombos { group, time });
        app.add_systems(Update, sync_env_combos);

        // Shared by default: time combo disabled, group index 0.
        app.update();
        assert!(
            app.world()
                .entity(time)
                .contains::<bevy::ui::InteractionDisabled>(),
            "time combo disabled while shared",
        );

        // Pin Modern/Sunset externally (the World menu path): the combos follow and
        // the time combo re-enables.
        if let Some(mut state) = app.world_mut().get_resource_mut::<EnvironmentState>() {
            state.set_fixed(Some(FixedEnvironment::Modern(FixedSky::Sunset)));
        }
        app.update();
        assert_eq!(
            app.world()
                .entity(group)
                .get::<ComboSelection>()
                .map(|s| s.active),
            Some(3)
        );
        assert_eq!(
            app.world()
                .entity(time)
                .get::<ComboSelection>()
                .map(|s| s.active),
            Some(time_index_for_sky(FixedSky::Sunset))
        );
        assert!(
            !app.world()
                .entity(time)
                .contains::<bevy::ui::InteractionDisabled>(),
            "time combo enabled once a group is pinned",
        );
        Ok(())
    }

    /// A slider value readout tracks its setting's stored value.
    #[test]
    fn value_readout_follows_store() -> Result<(), TestError> {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(ViewerSettings::from_store_for_test(render_store()));
        let label = app
            .world_mut()
            .spawn((
                Text::default(),
                QuickPrefValueLabel {
                    control_name: crate::session::SETTING_DRAW_DISTANCE.to_owned(),
                    integer: true,
                },
            ))
            .id();
        app.add_systems(Update, update_quick_pref_values);
        app.update();
        assert_eq!(
            app.world().entity(label).get::<Text>().map(|t| t.0.clone()),
            Some("512".to_owned())
        );
        Ok(())
    }
}
