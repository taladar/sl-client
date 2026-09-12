//! The **settings-asset preset combos**
//! (`viewer-quick-prefs-environment-presets`): one combo each for the sky, the
//! water and the day cycle, listing every settings asset in inventory, with
//! prev / next buttons either side.
//!
//! # Two windows carry them
//!
//! The Quick Preferences panel was the first host and is why this module is
//! named for it; the Phototools window (`crate::phototools`) is the second, and
//! both can be open at once over one environment. A window that wants the rows
//! declares a [`PresetHost`] — its element ids and its scope tag — and calls
//! [`spawn_preset_rows`]; everything downstream addresses a combo through the
//! [`PresetCombo`] its anchor carries rather than through a table of "the three
//! anchors", which is what a singleton resource made this and could only ever
//! describe one window.
//!
//! # Three tracks, not one choice
//!
//! The panel's older environment row models the environment as a single choice
//! — a preset *group* crossed with a *time of day*, driving
//! [`EnvironmentState::set_fixed`]. The reference models it as three independent
//! tracks: `FloaterQuickPrefs::loadPresets` fills a sky, a water and a
//! day-cycle combo from the same inventory walk, and picking in one of them
//! writes only its own track of `ENV_LOCAL` (see
//! [`LocalEnvironment`](sl_viewer_world_scene::environment::LocalEnvironment), which is
//! the layer this panel is a view of). Both rows are here, and they are not
//! redundant:
//!
//! - the **group / time** pair is the only environment control that works
//!   before inventory loads — the reference's three combos are empty until the
//!   walk finds something — and it is the only way to the Day-Cycle-frozen and
//!   Modern-library groups, which the reference has no row for at all;
//! - the **three preset combos** are the tracks: a water asset the group/time
//!   pair cannot express, a day cycle it cannot install, and the user's own
//!   saved skies.
//!
//! The two agree where they overlap. The sky combo appends the four ported
//! legacy WindLight presets after a second separator — exactly what the
//! reference appends from `LLEnvironment::mLegacySkies`, with the same `[WL]`
//! suffix — and picking one is [`FixedEnvironment::Legacy`], the same thing the
//! group/time pair pins, so the sky combo shows the group/time pair's choice
//! and vice versa. (Ours are not gated on OpenSim as the reference's are: the
//! World ▸ Environment menu offers them on every grid, so a grid test here
//! would take away a control the viewer already has.)
//!
//! # The two sentinel rows
//!
//! Each combo prepends two rows before a separator: *Region default* and
//! *Day-cycle based* for sky and water, *Region default* and *No day cycle* for
//! the day cycle. They are **states, not choices** — `setDefaultPresetsEnabled`
//! enables them only long enough for `setSelectedEnvironment` to select one and
//! disables them again, and `isValidPreset` refuses them, so the prev / next
//! walk steps over them. They are [`ComboRow::Disabled`] here for the same
//! reason: the combo has to be able to *show* "the region's environment is what
//! you are looking at" without offering it as a thing to pick, because picking
//! it is what the World ▸ Environment menu's "Use Shared Environment" is for.
//!
//! Reference (Firestorm, read-only): `indra/newview/quickprefs.cpp`
//! (`FloaterQuickPrefs::loadPresets`, `loadSkyPresets`, `loadWaterPresets`,
//! `loadDayCyclePresets`, `setDefaultPresetsEnabled`, `setSelectedEnvironment`,
//! `stepComboBox`, `isValidPreset`, `onChangeSkyPreset` and siblings),
//! `panel_quick_prefs.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use sl_client_bevy::{AssetKey, SettingsKind, Uuid};
use std::collections::HashSet;

use crate::environment::{EnvironmentState, FixedEnvironment, LocalEnvironmentPick};
use crate::i18n::Translator;
use crate::notifications::ShowNotification;
use crate::settings_index::{SettingsAsset, SettingsIndex};
use crate::sky_presets::FixedSky;
use crate::ui::row;
use crate::ui_combo::{
    ComboChanged, ComboRow, ComboSelection, ComboSpec, SetComboOptions, spawn_combo,
};
use crate::ui_font::UiFont;

/// The Fluent key of the *Region default* sentinel (`QP_WL_Region_Default`).
pub const KEY_REGION_DEFAULT: &str = "quick-prefs-env-region-default";
/// The Fluent key of the *Day-cycle based* sentinel (`QP_WL_Day_Cycle_Based`).
const KEY_DAY_CYCLE_BASED: &str = "quick-prefs-env-day-cycle-based";
/// The Fluent key of the day-cycle combo's *None* sentinel (`QP_WL_None`).
const KEY_NO_DAY_CYCLE: &str = "quick-prefs-env-no-day-cycle";

/// The Fluent keys of the three combos' row labels, in kind order.
const ROW_LABEL_KEYS: [&str; 3] = [
    "quick-prefs-env-sky",
    "quick-prefs-env-water",
    "quick-prefs-env-day-cycle",
];

/// The suffix the reference appends to a legacy WindLight sky's label so it
/// reads apart from an inventory asset of the same name.
const LEGACY_SUFFIX: &str = " [WL]";

/// The three kinds, in the order the rows are laid out — the reference's
/// (sky, water, day cycle) top to bottom.
const KINDS: [SettingsKind; 3] = [
    SettingsKind::Sky,
    SettingsKind::Water,
    SettingsKind::DayCycle,
];

/// The four times of day the legacy WindLight rows offer, in day order, with
/// the Fluent key of each one's label — the same keys the time-of-day combo
/// uses, so the two rows name a preset the same way.
const LEGACY_SKIES: [(FixedSky, &str); 4] = [
    (FixedSky::Sunrise, "quick-prefs-time-sunrise"),
    (FixedSky::Midday, "quick-prefs-time-midday"),
    (FixedSky::Sunset, "quick-prefs-time-sunset"),
    (FixedSky::Midnight, "quick-prefs-time-midnight"),
];

/// The prev / next buttons' glyphs.
const PREV_GLYPH: &str = "\u{2039}";
/// The next button's glyph.
const NEXT_GLYPH: &str = "\u{203a}";

/// The combos' and buttons' font size, in logical pixels.
const FONT: f32 = 13.0;
/// A row's label colour.
const LABEL_COLOR: Color = Color::srgb(0.86, 0.88, 0.92);
/// A step button's border.
const BUTTON_BORDER: Color = Color::srgb(0.3, 0.34, 0.42);
/// A step button's fill.
const BUTTON_FILL: Color = Color::srgb(0.16, 0.17, 0.2);

/// One row of a preset combo.
///
/// The list is built as rows rather than as labels because what a row *means* is
/// what decides both halves of its behaviour: whether the user may pick it, and
/// what picking it does. A label is only how it is spelled in the current
/// locale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetRow {
    /// *Region default*: this track has no local override.
    RegionDefault,
    /// *Day-cycle based*: a local day cycle supplies this track (sky / water).
    DayCycleBased,
    /// *None*: no local day cycle (the day-cycle combo's second sentinel).
    NoDayCycle,
    /// The divider between groups of rows.
    Separator,
    /// A settings asset in inventory.
    Asset {
        /// The settings asset a pick installs.
        asset: Uuid,
        /// The inventory item's name, as the list shows it.
        name: String,
    },
    /// A ported legacy WindLight sky preset.
    Legacy(FixedSky),
}

impl PresetRow {
    /// Whether a **user** may pick this row — the reference's `isValidPreset`,
    /// which is the same predicate the prev / next walk steps by.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        matches!(*self, Self::Asset { .. } | Self::Legacy(_))
    }

    /// How this row is drawn in the combo's list.
    #[must_use]
    pub const fn combo_row(&self) -> ComboRow {
        match *self {
            Self::Separator => ComboRow::Separator,
            Self::Asset { .. } | Self::Legacy(_) => ComboRow::Selectable,
            Self::RegionDefault | Self::DayCycleBased | Self::NoDayCycle => ComboRow::Disabled,
        }
    }

    /// This row's label in the current locale.
    #[must_use]
    pub fn label(&self, translator: &Translator) -> String {
        match *self {
            Self::RegionDefault => translator.get(KEY_REGION_DEFAULT),
            Self::DayCycleBased => translator.get(KEY_DAY_CYCLE_BASED),
            Self::NoDayCycle => translator.get(KEY_NO_DAY_CYCLE),
            Self::Separator => String::new(),
            Self::Asset { ref name, .. } => name.clone(),
            Self::Legacy(time) => {
                let key = LEGACY_SKIES
                    .iter()
                    .find_map(|(sky, key)| (*sky == time).then_some(*key))
                    .unwrap_or("quick-prefs-time-midday");
                format!("{}{LEGACY_SUFFIX}", translator.get(key))
            }
        }
    }
}

/// The rows each of the three combos is currently showing.
///
/// Held rather than rebuilt per query because two systems read it — the pick
/// applies the row at an index, the sync finds the index of a row — and both
/// have to agree with the list the widget is actually displaying.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct PresetLists {
    /// The sky combo's rows.
    sky: Vec<PresetRow>,
    /// The water combo's rows.
    water: Vec<PresetRow>,
    /// The day-cycle combo's rows.
    day_cycle: Vec<PresetRow>,
}

impl PresetLists {
    /// One kind's rows.
    #[must_use]
    pub fn of_kind(&self, kind: SettingsKind) -> &[PresetRow] {
        match kind {
            SettingsKind::Sky => &self.sky,
            SettingsKind::Water => &self.water,
            SettingsKind::DayCycle => &self.day_cycle,
        }
    }

    /// One kind's rows, for writing.
    const fn of_kind_mut(&mut self, kind: SettingsKind) -> &mut Vec<PresetRow> {
        match kind {
            SettingsKind::Sky => &mut self.sky,
            SettingsKind::Water => &mut self.water,
            SettingsKind::DayCycle => &mut self.day_cycle,
        }
    }
}

/// One kind's position in [`KINDS`] — the index the per-kind arrays are read by.
const fn kind_index(kind: SettingsKind) -> usize {
    match kind {
        SettingsKind::Sky => 0,
        SettingsKind::Water => 1,
        SettingsKind::DayCycle => 2,
    }
}

/// A window that carries a set of preset rows.
///
/// Two do — the Quick Preferences panel and the Phototools window
/// (`crate::phototools`) — and both can be open at once over the same
/// environment, so **everything that addresses a combo is scoped by its host**:
/// the element id the widget reports, the node names, and the tag the systems
/// query the anchors and step buttons by. A shared element id would make the two
/// windows' combos indistinguishable to the interaction harness, and a shared
/// anchor tag would make a prev / next press step both.
///
/// The row *lists* are deliberately **not** per host. What settings assets
/// inventory holds is one answer to one question, so [`PresetLists`] stays a
/// resource and both hosts are views of it.
#[derive(Debug, Clone, Copy)]
pub struct PresetHost {
    /// The tag carried on this host's anchors and step buttons.
    pub scope: &'static str,
    /// The element ids of the sky, water and day-cycle combos, in sky, water,
    /// day-cycle order.
    pub elements: [&'static str; 3],
}

impl PresetHost {
    /// One kind's element id.
    fn element(&self, kind: SettingsKind) -> &'static str {
        self.elements.get(kind_index(kind)).copied().unwrap_or("")
    }
}

/// The Quick Preferences panel's set of preset rows.
pub static QUICK_PREFS_HOST: PresetHost = PresetHost {
    scope: "quick-prefs",
    elements: [
        "quick-prefs-preset-sky",
        "quick-prefs-preset-water",
        "quick-prefs-preset-day-cycle",
    ],
};

/// The Phototools window's set of preset rows.
pub static PHOTOTOOLS_HOST: PresetHost = PresetHost {
    scope: "phototools",
    elements: [
        "phototools-preset-sky",
        "phototools-preset-water",
        "phototools-preset-day-cycle",
    ],
};

/// A preset combo's anchor: which track it drives and which window it is in.
#[derive(Component, Debug, Clone, Copy)]
pub struct PresetCombo {
    /// The track this combo picks for.
    kind: SettingsKind,
    /// The host window's [`PresetHost::scope`].
    scope: &'static str,
}

/// A prev / next button press: step one combo's selection to the next valid row
/// in that direction and apply it.
///
/// A message rather than the observer doing the work, because the walk needs the
/// row lists, the environment and the pick queue at once, and an observer that
/// took all three would run them as an exclusive stretch of the schedule for a
/// button click.
#[derive(Message, Debug, Clone, Copy)]
pub struct StepPreset {
    /// Which track to step.
    pub kind: SettingsKind,
    /// Which window's combo — a press in one window must not step the other's.
    pub scope: &'static str,
    /// Forward through the list (the next button) rather than back.
    pub forward: bool,
}

/// A prev / next button, tagged with the combo it steps and the direction.
#[derive(Component, Debug, Clone, Copy)]
struct PresetStepButton {
    /// The track this button steps.
    kind: SettingsKind,
    /// The host window's [`PresetHost::scope`].
    scope: &'static str,
    /// Whether it steps forward.
    forward: bool,
}

/// Build the three preset rows of one `host` under `parent`, tagging each
/// anchor with a [`PresetCombo`].
///
/// Called from a hosting window's content build. `tab_index` is the first of
/// three consecutive focus stops the rows take.
///
/// `seed` is the label each combo opens showing, before `refresh_preset_lists`
/// has run once — the *Region default* sentinel, which is both what the panel
/// will almost always be showing and what a combo with nothing in it means.
pub fn spawn_preset_rows(
    commands: &mut Commands,
    parent: Entity,
    host: &PresetHost,
    tab_index: i32,
    seed: &str,
) {
    for (offset, kind) in KINDS.into_iter().enumerate() {
        let index = i32::try_from(offset).unwrap_or(0);
        spawn_preset_row(
            commands,
            parent,
            host,
            kind,
            tab_index.saturating_add(index),
            seed,
        );
    }
}

/// The Fluent key of one kind's row label.
fn label_key_of(kind: SettingsKind) -> &'static str {
    ROW_LABEL_KEYS.get(kind_index(kind)).copied().unwrap_or("")
}

/// Spawn one labelled row: `label  ‹ [combo] ›`, returning the combo's anchor.
fn spawn_preset_row(
    commands: &mut Commands,
    parent: Entity,
    host: &PresetHost,
    kind: SettingsKind,
    tab_index: i32,
    seed: &str,
) -> Entity {
    let element = host.element(kind);
    let row_entity = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                width: Val::Percent(100.0),
                ..row(Val::Px(8.0))
            },
            Name::new(format!("{}:preset-row:{element}", host.scope)),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        crate::i18n::Translated::new(label_key_of(kind)),
        UiFont::Sans.at(FONT),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(row_entity),
    ));
    let group = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..row(Val::Px(4.0))
            },
            ChildOf(row_entity),
        ))
        .id();
    spawn_step_button(commands, group, host, kind, false);
    // One row to start with, so the combo has something to show before the
    // inventory walk has found anything: the sentinel that says exactly that.
    let labels = vec![seed.to_owned()];
    let anchor = spawn_combo(
        commands,
        group,
        &ComboSpec {
            element,
            labels: &labels,
            active: 0,
            tab_index,
            font_size: FONT,
            translate_labels: false,
        },
    );
    commands.entity(anchor).insert(PresetCombo {
        kind,
        scope: host.scope,
    });
    spawn_step_button(commands, group, host, kind, true);
    anchor
}

/// Spawn one prev / next button beside a combo.
fn spawn_step_button(
    commands: &mut Commands,
    parent: Entity,
    host: &PresetHost,
    kind: SettingsKind,
    forward: bool,
) {
    let element = host.element(kind);
    let side = if forward { "next" } else { "prev" };
    commands
        .spawn((
            Button,
            TabIndex(0),
            PresetStepButton {
                kind,
                scope: host.scope,
                forward,
            },
            Node {
                padding: UiRect::axes(Val::Px(5.0), Val::Px(1.0)),
                border: UiRect::all(Val::Px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_FILL),
            Name::new(format!("{element}:{side}")),
            ChildOf(parent),
        ))
        .observe(on_step_button)
        .with_child((
            Text::new(if forward { NEXT_GLYPH } else { PREV_GLYPH }),
            UiFont::Sans.at(FONT),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ));
}

/// Observer: turn a prev / next press into a [`StepPreset`].
fn on_step_button(
    activate: On<Activate>,
    buttons: Query<&PresetStepButton>,
    mut steps: MessageWriter<StepPreset>,
) {
    if let Ok(button) = buttons.get(activate.entity) {
        steps.write(StepPreset {
            kind: button.kind,
            scope: button.scope,
            forward: button.forward,
        });
    }
}

/// Build one kind's row list from that kind's settings assets — the reference's
/// `loadSkyPresets` / `loadWaterPresets` / `loadDayCyclePresets`.
///
/// Over the asset slice rather than the whole index because that is all the list
/// is: the index's job is the inventory walk, and this one's is the row order
/// around its answer.
#[must_use]
pub fn build_rows(kind: SettingsKind, assets: &[SettingsAsset]) -> Vec<PresetRow> {
    let mut rows = vec![
        PresetRow::RegionDefault,
        if kind == SettingsKind::DayCycle {
            PresetRow::NoDayCycle
        } else {
            PresetRow::DayCycleBased
        },
        PresetRow::Separator,
    ];
    // An unnamed item is skipped, as `if (!preset_name.empty())` does: a row
    // with no label is one the user cannot tell from the separator above it.
    //
    // **De-duplicated by asset id**, which is `FSSettingsCollector`'s own rule
    // and belongs here rather than in the index: a combo is choosing an
    // *environment*, so two inventory items of one asset are one choice, and
    // offering both would be two rows that do exactly the same thing. The
    // windows that show *inventory* — the My Environments library, the settings
    // picker — want every item and so read the index undeduplicated.
    let mut seen: HashSet<Uuid> = HashSet::new();
    rows.extend(
        assets
            .iter()
            .filter(|asset| !asset.name.is_empty())
            .filter(|asset| seen.insert(asset.asset_id))
            .map(|asset| PresetRow::Asset {
                asset: asset.asset_id,
                name: asset.name.clone(),
            }),
    );
    if kind == SettingsKind::Sky {
        // The second separator is the reference's, and so is its condition: it
        // only appears between two lists that both have something in them.
        if !assets.is_empty() {
            rows.push(PresetRow::Separator);
        }
        rows.extend(
            LEGACY_SKIES
                .iter()
                .map(|(sky, _key)| PresetRow::Legacy(*sky)),
        );
    }
    rows
}

/// The row each combo should be showing for the current environment — the
/// reference's `setSelectedEnvironment`, which walks the local layer in the same
/// order for the same reason: a day cycle decides what the sky and water combos
/// say *unless* a fixed sky or water is pinned over it.
#[must_use]
pub fn selected_rows(state: &EnvironmentState) -> [PresetRow; 3] {
    let local = state.local();
    // Nothing local and no pinned preset: the region's environment is what is
    // rendering, and all three combos say so.
    if local.is_empty() && state.fixed().is_none() {
        return [
            PresetRow::RegionDefault,
            PresetRow::RegionDefault,
            PresetRow::RegionDefault,
        ];
    }
    let mut sky = PresetRow::RegionDefault;
    let mut water = PresetRow::RegionDefault;
    let mut day_cycle = PresetRow::NoDayCycle;
    if let Some(day) = local.day() {
        if let Some(asset) = day.asset {
            day_cycle = PresetRow::Asset {
                asset,
                name: String::new(),
            };
        }
        // A cycle animates both, so both combos say where their frames come
        // from — until one is pinned over it below.
        sky = PresetRow::DayCycleBased;
        water = PresetRow::DayCycleBased;
    }
    if let Some(asset) = local.sky().and_then(|track| track.asset) {
        sky = PresetRow::Asset {
            asset,
            name: String::new(),
        };
    }
    if let Some(asset) = local.water().and_then(|track| track.asset) {
        water = PresetRow::Asset {
            asset,
            name: String::new(),
        };
    }
    // Ours: the World ▸ Environment menu's legacy pin *is* one of the `[WL]`
    // rows, so the sky combo shows it. The other two groups (the region's own
    // cycle frozen, and a Modern library sky) have no row here, and fall back to
    // whatever the local layer said.
    if let Some(FixedEnvironment::Legacy(time)) = state.fixed() {
        sky = PresetRow::Legacy(time);
    }
    [sky, water, day_cycle]
}

/// The index of `wanted` in `rows`, matching an asset row by its **asset id**
/// alone (the name a list shows is not part of the identity a selection has).
#[must_use]
pub fn index_of(rows: &[PresetRow], wanted: &PresetRow) -> Option<usize> {
    rows.iter().position(|row| match (row, wanted) {
        (PresetRow::Asset { asset: held, .. }, PresetRow::Asset { asset: sought, .. }) => {
            held == sought
        }
        (held, sought) => held == sought,
    })
}

/// The next index in `rows` that a user may pick, walking from `from` in the
/// given direction and wrapping — the reference's `stepComboBox`, which stops
/// when it comes back to where it started.
///
/// `None` when the list has no valid row at all, which is what the reference
/// answers with the `NoValidEnvSettingFound` alert.
#[must_use]
pub fn step_index(rows: &[PresetRow], from: usize, forward: bool) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    let last = rows.len().saturating_sub(1);
    let mut current = from.min(last);
    for _ in 0..rows.len() {
        current = if forward {
            if current >= last {
                0
            } else {
                current.saturating_add(1)
            }
        } else if current == 0 {
            last
        } else {
            current.saturating_sub(1)
        };
        if rows.get(current).is_some_and(PresetRow::is_valid) {
            return Some(current);
        }
    }
    None
}

/// Rebuild the three row lists whenever the settings index or the locale
/// changes, and push the labels and row states into the combos.
///
/// The locale is in here because a sentinel's *label* is localised while its
/// *identity* is not: the list does not change when the language does, but every
/// row's text does.
fn refresh_preset_lists(
    combos: Query<(Entity, &PresetCombo)>,
    spawned: Query<(), Added<PresetCombo>>,
    index: Option<Res<SettingsIndex>>,
    translator: Translator,
    mut lists: ResMut<PresetLists>,
    mut set_options: MessageWriter<SetComboOptions>,
) {
    if combos.is_empty() {
        return;
    }
    let index_changed = index.as_ref().is_some_and(|index| index.is_changed());
    // A combo added this frame is a host whose content was just built: its
    // combos exist but hold the placeholder row `spawn_preset_row` seeded. That
    // is why this is `Added` rather than "the resource appeared" — a second
    // host can open long after the first, and its combos need filling then.
    if !index_changed && !translator.changed() && spawned.is_empty() {
        return;
    }
    let empty = SettingsIndex::default();
    let index = index.as_deref().unwrap_or(&empty);
    for kind in KINDS {
        let rows = build_rows(kind, index.of_kind(kind));
        let labels: Vec<String> = rows.iter().map(|row| row.label(&translator)).collect();
        let row_states: Vec<ComboRow> = rows.iter().map(PresetRow::combo_row).collect();
        for (anchor, combo) in &combos {
            if combo.kind != kind {
                continue;
            }
            set_options.write(SetComboOptions {
                combo: anchor,
                labels: labels.clone(),
                rows: row_states.clone(),
            });
        }
        let held = lists.of_kind_mut(kind);
        if *held != rows {
            *held = rows;
        }
    }
}

/// Keep the three combos showing what the local environment layer holds.
///
/// Skipped while a pick is still fetching: the user's click is the truth until
/// the asset lands, and snapping the row back to the old one in the meantime
/// would read as the click having been refused.
fn sync_preset_combos(
    lists: Res<PresetLists>,
    environment: Option<Res<EnvironmentState>>,
    pick: Option<Res<LocalEnvironmentPick>>,
    mut combos: Query<(&PresetCombo, &mut ComboSelection)>,
) {
    let Some(environment) = environment else {
        return;
    };
    if pick.is_some_and(|pick| pick.pending().is_some()) {
        return;
    }
    let wanted = selected_rows(&environment);
    // Every host's combo for a kind shows the same row, because there is one
    // environment: two windows open at once agree by construction rather than
    // by being kept in step.
    for (combo, mut selection) in &mut combos {
        let Some(row) = wanted.get(kind_index(combo.kind)) else {
            continue;
        };
        let rows = lists.of_kind(combo.kind);
        // A row the list does not hold (an asset outside inventory, a preset the
        // walk has not found yet) falls back to the first sentinel, which is
        // where the reference's failed `selectByValue` leaves the combo too.
        let index = index_of(rows, row).unwrap_or(0);
        if selection.active != index {
            selection.active = index;
        }
    }
}

/// Apply a user pick on one of the three combos.
fn apply_preset_pick(
    mut changes: MessageReader<ComboChanged>,
    combos: Query<&PresetCombo>,
    lists: Res<PresetLists>,
    mut pick: Option<ResMut<LocalEnvironmentPick>>,
    mut environment: Option<ResMut<EnvironmentState>>,
) {
    for change in changes.read() {
        // The message names the anchor, and the anchor carries which track it
        // drives — so a change from any other combo in the world (or from the
        // other host's) needs no list of "ours" to be checked against.
        let Ok(combo) = combos.get(change.combo) else {
            continue;
        };
        let Some(row) = lists.of_kind(combo.kind).get(change.active) else {
            continue;
        };
        apply_row(row, pick.as_deref_mut(), environment.as_deref_mut());
    }
}

/// Install what one row selects. A row the user cannot pick installs nothing —
/// the widget refuses those, and this is the second half of the same rule.
fn apply_row(
    row: &PresetRow,
    pick: Option<&mut LocalEnvironmentPick>,
    environment: Option<&mut EnvironmentState>,
) {
    match *row {
        PresetRow::Asset { asset, .. } => {
            if let Some(pick) = pick {
                pick.request(AssetKey::from(asset));
            }
        }
        PresetRow::Legacy(time) => {
            if let Some(environment) = environment {
                environment.set_fixed(Some(FixedEnvironment::Legacy(time)));
            }
        }
        PresetRow::RegionDefault
        | PresetRow::DayCycleBased
        | PresetRow::NoDayCycle
        | PresetRow::Separator => {}
    }
}

/// Step one combo on a prev / next press, then apply the row it landed on.
fn step_preset_combo(
    mut steps: MessageReader<StepPreset>,
    lists: Res<PresetLists>,
    mut combos: Query<(&PresetCombo, &mut ComboSelection)>,
    mut pick: Option<ResMut<LocalEnvironmentPick>>,
    mut environment: Option<ResMut<EnvironmentState>>,
    mut notify: MessageWriter<ShowNotification>,
) {
    for step in steps.read() {
        let rows = lists.of_kind(step.kind);
        // The pressed button's own window, not every window's combo of that
        // kind: the step is a gesture on one control. The others follow through
        // `sync_preset_combos` once the pick lands, which is how they would
        // follow any other way of changing the environment.
        let Some((_combo, mut selection)) = combos
            .iter_mut()
            .find(|(combo, _selection)| combo.kind == step.kind && combo.scope == step.scope)
        else {
            continue;
        };
        let Some(next) = step_index(rows, selection.active, step.forward) else {
            // Nothing in the list is pickable — an inventory with no settings
            // assets, which for water and day cycles is the ordinary first-login
            // case. The reference says so rather than doing nothing silently.
            notify.write(ShowNotification::new("NoValidEnvSettingFound"));
            continue;
        };
        if selection.active != next {
            selection.active = next;
        }
        let Some(row) = rows.get(next) else {
            continue;
        };
        apply_row(row, pick.as_deref_mut(), environment.as_deref_mut());
    }
}

/// The preset combos' runtime: the row lists, the pick, the prev / next walk and
/// the sync back from the environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct QuickPrefsEnvironmentPlugin;

impl Plugin for QuickPrefsEnvironmentPlugin {
    fn build(&self, app: &mut App) {
        // The combo widget's and the notification host's own plugins register
        // these too; doing it here as well (both are idempotent) keeps this
        // plugin safe to add on its own, in a test or a harness that has
        // neither.
        app.init_resource::<PresetLists>()
            .add_message::<StepPreset>()
            .add_message::<SetComboOptions>()
            .add_message::<ShowNotification>()
            .add_systems(
                Update,
                (
                    refresh_preset_lists,
                    apply_preset_pick,
                    step_preset_combo,
                    sync_preset_combos,
                )
                    .chain(),
            );
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{
        EnvironmentAsset, EnvironmentSettings, InventoryKey, SettingsKind, SkySettings, Uuid,
        WaterSettings,
    };

    use bevy::prelude::*;

    use super::{PresetRow, build_rows, index_of, selected_rows, step_index};
    use crate::environment::{EnvironmentState, FixedEnvironment};
    use crate::settings_index::SettingsAsset;
    use crate::sky_presets::FixedSky;
    use crate::ui_combo::{ComboChanged, ComboSelection};

    /// A boxed error so tests avoid `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// One settings asset of `kind`, as the index would hand it over.
    fn asset(name: &str, id: u128, kind: SettingsKind) -> SettingsAsset {
        SettingsAsset {
            item: InventoryKey::from(Uuid::from_u128(id)),
            asset_id: Uuid::from_u128(id),
            name: name.to_owned(),
            kind,
            library: false,
        }
    }

    /// **Two inventory items of one asset are one combo row.**
    ///
    /// `FSSettingsCollector` de-duplicates by asset id, and since the index
    /// stopped doing it (the library window needs every *item*, or an item with
    /// no row cannot be renamed or deleted) this is where that rule lives. A
    /// combo picks an environment, so two rows installing the identical asset
    /// would be two ways to do one thing — and every freshly created sky shares
    /// the simulator's default asset, so it is not a rare case.
    #[test]
    fn one_asset_held_twice_is_one_row() {
        let twice = [
            asset("A copy", 0xA1, SettingsKind::Sky),
            asset("Another copy", 0xA1, SettingsKind::Sky),
            asset("A different sky", 0xA2, SettingsKind::Sky),
        ];
        let rows = build_rows(SettingsKind::Sky, &twice);
        let assets: Vec<Uuid> = rows
            .iter()
            .filter_map(|row| match row {
                PresetRow::Asset { asset, .. } => Some(*asset),
                _other => None,
            })
            .collect();
        assert_eq!(assets, [Uuid::from_u128(0xA1), Uuid::from_u128(0xA2)]);
        // The first met wins its name, as the collector's insertion order does.
        let names: Vec<&str> = rows
            .iter()
            .filter_map(|row| match row {
                PresetRow::Asset { name, .. } => Some(name.as_str()),
                _other => None,
            })
            .collect();
        assert_eq!(names, ["A copy", "A different sky"]);
    }

    /// **The sky list is the reference's**: two sentinels, a separator, the
    /// inventory assets, a second separator, then the legacy WindLight presets.
    #[test]
    fn the_sky_list_has_both_sentinels_and_the_legacy_tail() {
        let rows = build_rows(
            SettingsKind::Sky,
            &[asset("Bright", 0xA1, SettingsKind::Sky)],
        );
        assert_eq!(
            rows,
            vec![
                PresetRow::RegionDefault,
                PresetRow::DayCycleBased,
                PresetRow::Separator,
                PresetRow::Asset {
                    asset: Uuid::from_u128(0xA1),
                    name: "Bright".to_owned(),
                },
                PresetRow::Separator,
                PresetRow::Legacy(FixedSky::Sunrise),
                PresetRow::Legacy(FixedSky::Midday),
                PresetRow::Legacy(FixedSky::Sunset),
                PresetRow::Legacy(FixedSky::Midnight),
            ]
        );
    }

    /// **The second separator only appears between two non-empty lists** —
    /// `if (!sky_map.empty() && !mLegacySkies.empty())`. With no sky assets the
    /// legacy presets follow the first separator directly, so the list never
    /// shows two rules in a row.
    #[test]
    fn an_empty_inventory_does_not_add_a_second_separator() {
        assert_eq!(
            build_rows(SettingsKind::Sky, &[]),
            vec![
                PresetRow::RegionDefault,
                PresetRow::DayCycleBased,
                PresetRow::Separator,
                PresetRow::Legacy(FixedSky::Sunrise),
                PresetRow::Legacy(FixedSky::Midday),
                PresetRow::Legacy(FixedSky::Sunset),
                PresetRow::Legacy(FixedSky::Midnight),
            ]
        );
    }

    /// **The day-cycle combo's second sentinel is *None*, not *Day-cycle
    /// based*** — a day cycle cannot be based on itself — and no legacy rows
    /// follow it, because the ported presets are skies.
    #[test]
    fn the_day_cycle_list_has_its_own_sentinel() {
        assert_eq!(
            build_rows(
                SettingsKind::DayCycle,
                &[asset("A day", 0xC1, SettingsKind::DayCycle)],
            ),
            vec![
                PresetRow::RegionDefault,
                PresetRow::NoDayCycle,
                PresetRow::Separator,
                PresetRow::Asset {
                    asset: Uuid::from_u128(0xC1),
                    name: "A day".to_owned(),
                },
            ]
        );
    }

    /// **An item with no name is not a row** — `if (!preset_name.empty())`.
    #[test]
    fn an_unnamed_asset_is_dropped() {
        let rows = build_rows(
            SettingsKind::Water,
            &[
                asset("", 0xB1, SettingsKind::Water),
                asset("Deep", 0xB2, SettingsKind::Water),
            ],
        );
        assert_eq!(
            rows,
            vec![
                PresetRow::RegionDefault,
                PresetRow::DayCycleBased,
                PresetRow::Separator,
                PresetRow::Asset {
                    asset: Uuid::from_u128(0xB2),
                    name: "Deep".to_owned(),
                },
            ]
        );
    }

    /// **Only the two sentinel rows and the separators are unpickable.**
    #[test]
    fn the_sentinels_and_separators_are_not_choices() {
        let valid: Vec<bool> = build_rows(
            SettingsKind::Sky,
            &[asset("Bright", 0xA1, SettingsKind::Sky)],
        )
        .iter()
        .map(PresetRow::is_valid)
        .collect();
        assert_eq!(
            valid,
            vec![false, false, false, true, false, true, true, true, true]
        );
    }

    /// **Prev / next steps over everything the user may not pick, and wraps.**
    #[test]
    fn stepping_skips_the_sentinels_and_wraps() {
        let rows = build_rows(
            SettingsKind::Sky,
            &[asset("Bright", 0xA1, SettingsKind::Sky)],
        );
        // From the first sentinel, forward lands on the one asset (index 3),
        // skipping the second sentinel and the separator.
        assert_eq!(step_index(&rows, 0, true), Some(3));
        // Forward from the asset skips the second separator to the first legacy
        // preset.
        assert_eq!(step_index(&rows, 3, true), Some(5));
        // Backwards from the first sentinel wraps to the last legacy preset.
        assert_eq!(step_index(&rows, 0, false), Some(8));
        // Forward from the last legacy preset wraps past the sentinels to the
        // asset.
        assert_eq!(step_index(&rows, 8, true), Some(3));
    }

    /// **A list with nothing pickable steps nowhere** — the case the reference
    /// raises `NoValidEnvSettingFound` for, and the ordinary state of the water
    /// and day-cycle combos on an inventory that holds no settings assets.
    #[test]
    fn a_list_with_no_valid_row_steps_nowhere() {
        let rows = build_rows(SettingsKind::Water, &[]);
        assert_eq!(step_index(&rows, 0, true), None);
        assert_eq!(step_index(&rows, 0, false), None);
    }

    /// A sky asset with a recognisable frame name.
    fn sky_asset(name: &str) -> EnvironmentAsset {
        EnvironmentAsset::Sky(Box::new(SkySettings::legacy_windlight_default(name)))
    }

    /// **Nothing local: all three combos say "Region default".**
    #[test]
    fn an_untouched_environment_selects_the_region_default() {
        assert_eq!(
            selected_rows(&EnvironmentState::default()),
            [
                PresetRow::RegionDefault,
                PresetRow::RegionDefault,
                PresetRow::RegionDefault,
            ]
        );
    }

    /// **A local day cycle puts the sky and water combos on "Day-cycle
    /// based"**, and a fixed sky pinned over it takes the sky combo back — the
    /// order `setSelectedEnvironment` walks the layer in.
    #[test]
    fn a_day_cycle_decides_the_other_two_until_one_is_pinned() -> Result<(), TestError> {
        let mut state = EnvironmentState::default();
        let cycle = EnvironmentSettings::legacy_windlight_default().day_cycle;
        state.set_local(
            EnvironmentAsset::DayCycle(Box::new(cycle)),
            Some(Uuid::from_u128(0xC1)),
        );
        let rows = selected_rows(&state);
        assert_eq!(rows.first(), Some(&PresetRow::DayCycleBased));
        assert_eq!(rows.get(1), Some(&PresetRow::DayCycleBased));
        assert_eq!(
            rows.get(2),
            Some(&PresetRow::Asset {
                asset: Uuid::from_u128(0xC1),
                name: String::new(),
            })
        );

        state.set_local(sky_asset("mine"), Some(Uuid::from_u128(0xA1)));
        let rows = selected_rows(&state);
        assert_eq!(
            rows.first(),
            Some(&PresetRow::Asset {
                asset: Uuid::from_u128(0xA1),
                name: String::new(),
            }),
            "the pinned sky wins the sky combo"
        );
        // The sky is pinned *over* the cycle rather than instead of it, so the
        // water still follows the cycle and the day combo still names it.
        assert_eq!(rows.get(1), Some(&PresetRow::DayCycleBased));
        assert_eq!(
            rows.get(2),
            Some(&PresetRow::Asset {
                asset: Uuid::from_u128(0xC1),
                name: String::new(),
            })
        );
        Ok(())
    }

    /// **A water pick shows in the water combo and nowhere else** — the whole
    /// point of the three tracks being independent.
    #[test]
    fn a_water_pick_touches_only_the_water_combo() {
        let mut state = EnvironmentState::default();
        state.set_local(
            EnvironmentAsset::Water(WaterSettings::legacy_default("deep")),
            Some(Uuid::from_u128(0xB1)),
        );
        assert_eq!(
            selected_rows(&state),
            [
                PresetRow::RegionDefault,
                PresetRow::Asset {
                    asset: Uuid::from_u128(0xB1),
                    name: String::new(),
                },
                PresetRow::NoDayCycle,
            ]
        );
    }

    /// **The World ▸ Environment menu's legacy pin is a row of the sky combo**,
    /// so the panel's two environment surfaces never contradict each other.
    #[test]
    fn a_pinned_legacy_preset_selects_its_row() {
        let mut state = EnvironmentState::default();
        state.set_fixed(Some(FixedEnvironment::Legacy(FixedSky::Sunset)));
        let rows = selected_rows(&state);
        assert_eq!(rows.first(), Some(&PresetRow::Legacy(FixedSky::Sunset)));
        let list = build_rows(SettingsKind::Sky, &[]);
        assert_eq!(
            index_of(&list, &PresetRow::Legacy(FixedSky::Sunset)),
            Some(5)
        );
    }

    /// **A sky a script edited is nobody's asset**, so no row claims to be it:
    /// the combo falls back to the sentinel rather than pointing at whichever
    /// preset happens to share its name.
    #[test]
    fn a_script_sky_selects_no_asset_row() {
        let mut state = EnvironmentState::default();
        state.set_local(sky_asset("script"), None);
        let rows = selected_rows(&state);
        assert_eq!(rows.first(), Some(&PresetRow::RegionDefault));
        let list = build_rows(SettingsKind::Sky, &[]);
        assert_eq!(index_of(&list, &PresetRow::RegionDefault), Some(0));
    }

    /// **An asset row is found by its asset id, not by its name** — two skies
    /// really can share a name, and what a selection names is the asset.
    #[test]
    fn an_asset_row_matches_on_the_asset_id() {
        let rows = build_rows(
            SettingsKind::Sky,
            &[
                asset("Sunrise", 0xA1, SettingsKind::Sky),
                asset("Sunrise", 0xA2, SettingsKind::Sky),
            ],
        );
        assert_eq!(
            index_of(
                &rows,
                &PresetRow::Asset {
                    asset: Uuid::from_u128(0xA2),
                    name: String::new(),
                }
            ),
            Some(4),
            "the second of the two same-named skies"
        );
    }

    // -----------------------------------------------------------------------
    // Two hosts.
    // -----------------------------------------------------------------------

    /// An app with the row lists seeded and one sky combo per host, both parked
    /// on the *Region default* sentinel. Returns the two sky anchors,
    /// (quick-prefs, phototools).
    fn two_host_app() -> Result<(App, Entity, Entity), TestError> {
        let mut app = App::new();
        app.add_message::<super::SetComboOptions>()
            .add_message::<super::ShowNotification>()
            .add_message::<super::StepPreset>()
            .init_resource::<super::PresetLists>();
        // The list refresh resolves every row's label, so it wants the
        // translator's three resources; `install_untranslated` is the harness
        // that gives a key back as itself.
        crate::i18n::install_untranslated(&mut app);
        let sky = build_rows(
            SettingsKind::Sky,
            &[asset("A sky", 0xA1, SettingsKind::Sky)],
        );
        if let Some(mut lists) = app.world_mut().get_resource_mut::<super::PresetLists>() {
            *lists.of_kind_mut(SettingsKind::Sky) = sky;
        }
        let anchor = |app: &mut App, host: &super::PresetHost| {
            app.world_mut()
                .spawn((
                    super::PresetCombo {
                        kind: SettingsKind::Sky,
                        scope: host.scope,
                    },
                    ComboSelection {
                        element: host.element(SettingsKind::Sky),
                        active: 0,
                    },
                ))
                .id()
        };
        let quick = anchor(&mut app, &super::QUICK_PREFS_HOST);
        let photo = anchor(&mut app, &super::PHOTOTOOLS_HOST);
        Ok((app, quick, photo))
    }

    /// **A prev / next press steps its own window's combo, not both.**
    ///
    /// The step is a gesture on one control. Before the combos were addressed by
    /// a per-anchor tag there could only ever be one window, so "the sky combo"
    /// was unambiguous; with two open at once, a scope-blind step would move a
    /// control the hand was nowhere near.
    #[test]
    fn a_step_moves_only_its_own_host() -> Result<(), TestError> {
        let (mut app, quick, photo) = two_host_app()?;
        app.add_systems(Update, super::step_preset_combo);
        app.world_mut().write_message(super::StepPreset {
            kind: SettingsKind::Sky,
            scope: super::PHOTOTOOLS_HOST.scope,
            forward: true,
        });
        app.update();
        let photo_active = app
            .world()
            .get::<ComboSelection>(photo)
            .ok_or("the phototools combo should still exist")?
            .active;
        let quick_active = app
            .world()
            .get::<ComboSelection>(quick)
            .ok_or("the quick-prefs combo should still exist")?
            .active;
        assert_ne!(photo_active, 0, "the pressed window's combo stepped");
        assert_eq!(quick_active, 0, "the other window's combo did not");
        Ok(())
    }

    /// **Both hosts' combos are filled from the one list.**
    ///
    /// Three kinds times two windows is six pushes, and every one of them
    /// carries the same labels — the lists are inventory's answer, not a
    /// window's.
    #[test]
    fn every_host_combo_is_filled() -> Result<(), TestError> {
        let (mut app, _quick, _photo) = two_host_app()?;
        app.add_systems(Update, super::refresh_preset_lists);
        app.update();
        let pushed = app
            .world()
            .resource::<Messages<super::SetComboOptions>>()
            .iter_current_update_messages()
            .count();
        // Only the sky combos exist in this fixture, so one push each.
        assert_eq!(pushed, 2);
        Ok(())
    }

    /// **A pick on either window's combo installs the row.**
    ///
    /// The anchor carries which track it drives, so a change from a combo this
    /// module does not own is ignored without a roster to check it against.
    #[test]
    fn a_pick_on_the_second_host_applies() -> Result<(), TestError> {
        let (mut app, _quick, photo) = two_host_app()?;
        app.insert_resource(EnvironmentState::default());
        app.add_message::<ComboChanged>()
            .add_systems(Update, super::apply_preset_pick);
        // Row 3 of the sky list is the one inventory asset (two sentinels and a
        // separator come first).
        let legacy = app
            .world()
            .resource::<super::PresetLists>()
            .of_kind(SettingsKind::Sky)
            .iter()
            .position(|row| matches!(*row, PresetRow::Legacy(_)))
            .ok_or("the sky list should carry the legacy presets")?;
        app.world_mut().write_message(ComboChanged {
            combo: photo,
            active: legacy,
        });
        app.update();
        assert!(
            app.world().resource::<EnvironmentState>().fixed().is_some(),
            "a legacy pick from the second host pins a fixed environment"
        );
        Ok(())
    }
}
