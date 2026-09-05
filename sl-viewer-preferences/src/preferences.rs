//! The **Preferences floater shell** (`viewer-preferences-floater`): the tabbed
//! settings window and the wiring that connects its controls to the persistent
//! typed settings store ([`crate::settings`]) — the root of the preferences
//! cluster the per-tab tasks plug into.
//!
//! # What the shell owns
//!
//! - **The window**: a resizable floater hosting a search box, a **leading**
//!   ([`TabPlacement::InlineStart`], so it mirrors under RTL) tab strip, one
//!   panel per registered tab, and an OK / Cancel footer.
//! - **The tab registry** (`PREF_TABS`): a static list in the pattern of
//!   `crate::menu_bar`'s menus and [`crate::ui_element`]'s `ELEMENTS` — a
//!   sibling tab task appends one `PreferencesTabDef` and provides its build
//!   `fn`; the shell lays the tab out, fills it on first open, and gives its
//!   controls snapshot / revert, search and the account guard for free.
//! - **Commit semantics**, faithful to the reference viewer: controls edit the
//!   store **live** (through [`crate::settings_binding`]), the shell snapshots
//!   every bound setting's override state when the window opens, **Cancel and
//!   closing the window revert** to that snapshot, and **OK** re-snapshots
//!   (so the close-revert becomes a no-op), saves both scopes to disk and
//!   closes. A tab whose settings need a non-live side effect on OK listens for
//!   `PreferencesApplied`.
//! - **The search / filter box**: typing filters every tab's labelled rows
//!   (`spawn_pref_checkbox` / `spawn_pref_slider` tag them), hides the
//!   misses, highlights
//!   the hits, dims the tabs left empty and jumps to the first tab that still
//!   has a match. Matching is against the **resolved translated** label text,
//!   so it works in any locale.
//! - **The account guard**: a control bound to a per-avatar
//!   ([`Scope::Account`]) setting is disabled until the account scope has
//!   loaded at login, since before that an edit could not be persisted.
//!
//! Deliberate deviation from the reference: a tab left empty by the filter is
//! **dimmed**, not hidden — hiding strip buttons would shift the tab indices
//! under the selection state. The floater always opens on the first tab (no
//! remembered last tab); within a session the strip keeps its selection across
//! close / reopen because content is built once and only hidden.
//!
//! Reference (Firestorm, read-only): `llfloaterpreference.{h,cpp}`,
//! `floater_preferences.xml`, `llpanelpreference` (the generic
//! `saveSettings` / `cancel` snapshot), `fssearchablecontrol.h` (the filter).

use std::collections::HashMap;

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::ui_widgets::{Activate, Button, SliderRange, SliderStep, SliderThumb, SliderValue};
use bevy_flair::style::components::ClassList;
use sl_settings::{Scope, SettingValue, SettingsStore};

use crate::floater::{
    DeferredFloaterContent, FloaterCaps, FloaterCommand, FloaterHandle, FloaterOp, FloaterSpec,
    spawn_floater,
};
use crate::i18n::Translated;
use crate::settings::ViewerSettings;
use crate::settings_binding::{ComboBindingValues, SettingBinding, bound_checkbox, bound_slider};
use crate::ui::{LogicalInset, LogicalRect, UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_color_picker::spawn_color_swatch;
use crate::ui_combo::{ComboSpec, spawn_combo};
use crate::ui_element::ElementCx;
use crate::ui_font::UiFont;
use crate::ui_search::{SearchFieldSpec, spawn_search_field};
use crate::ui_tab::{
    DEFAULT_ELLIPSIS, TAB_LABEL_COLOR, TabButton, TabPanel, TabPlacement, TabSpec, TabStrip,
    fill_tab_container, spawn_tab_container,
};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};

/// The floater's stable id (geometry persistence, menu toggle, tests).
pub const PREFERENCES_FLOATER_ID: &str = "preferences";

/// The shell's body font size, in logical pixels.
pub const FONT: f32 = 13.0;

/// A section heading's font size, a step up from the rows it heads.
const SECTION_FONT: f32 = 14.0;

/// A row label's resting colour (the shared panel label tone).
pub const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A section heading's colour — same tone as the labels; the size difference
/// carries the hierarchy.
const SECTION_COLOR: Color = Color::srgb(0.75, 0.80, 0.88);

/// The muted tone for a filtered-empty tab's label (and the search glyphs).
const MUTED_COLOR: Color = Color::srgb(0.55, 0.60, 0.68);

/// A filter-matched row label's highlight — the same warm accent
/// [`crate::menu`] paints its menu-search hits with.
const FILTER_MATCH_COLOR: Color = Color::srgb(0.98, 0.82, 0.40);

/// A control's border tone (the settings-binding demo's, kept for continuity).
pub const CONTROL_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// A checkbox box's fill while unchecked.
pub(crate) const CHECK_OFF: Color = Color::srgb(0.12, 0.14, 0.18);

/// A checkbox box's fill while checked.
const CHECK_ON: Color = Color::srgb(0.30, 0.70, 0.45);

/// A checkbox box's fill while its binding is account-guarded (disabled).
const CHECK_DISABLED: Color = Color::srgb(0.20, 0.22, 0.26);

/// A slider track's fill.
const TRACK_FILL: Color = Color::srgb(0.16, 0.19, 0.25);

/// A slider thumb's fill.
const THUMB_FILL: Color = Color::srgb(0.62, 0.72, 0.86);

/// A footer button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// The skin class the footer buttons carry (hover styling).
const BUTTON_CLASS: &str = "sk-button";

/// A checkbox box's side length, in logical pixels.
pub(crate) const CHECK_SIZE: f32 = 18.0;

/// A slider track's width, in logical pixels.
const TRACK_WIDTH: f32 = 180.0;

/// A slider thumb's width, in logical pixels.
const THUMB_WIDTH: f32 = 14.0;

/// A slider track's (and thumb's) height, in logical pixels.
const TRACK_HEIGHT: f32 = 16.0;

/// The leading tab strip's fixed width, in logical pixels — near the
/// reference's 114 px tab column, with room for the divider. Resizable by its
/// divider and persisted per user by [`crate::floater_persist`].
const STRIP_WIDTH: f32 = 140.0;

/// The gap between a row's control and its label, in logical pixels.
const ROW_GAP: f32 = 8.0;

// ---------------------------------------------------------------------------
// The tab registry.
// ---------------------------------------------------------------------------

/// One tab of the preferences floater.
///
/// A sibling tab task appends an entry to `PREF_TABS` and provides the two
/// pieces in its own module; the shell does the rest (layout, deferred build,
/// snapshot / revert, search, the account guard).
pub(crate) struct PreferencesTabDef {
    /// A stable id, naming the tab in node [`Name`]s and tests.
    pub(crate) id: &'static str,
    /// The Fluent key of the tab's strip label.
    pub(crate) label_key: &'static str,
    /// Build this tab's content into its (empty) panel entity. A plain `fn` so
    /// the registry stays a `const` (the `ELEMENTS` rationale); a tab needing
    /// runtime data at build time spawns placeholders and populates from its
    /// own module's systems.
    pub(crate) build: fn(&mut Commands, Entity),
}

/// The registered preference tabs, in strip order.
pub(crate) const PREF_TABS: &[PreferencesTabDef] = &[
    PreferencesTabDef {
        id: "general",
        label_key: "preferences-tab-general",
        build: crate::preferences_general::build_general_tab,
    },
    PreferencesTabDef {
        id: crate::preferences_graphics::TAB_ID,
        label_key: "preferences-tab-graphics",
        build: crate::preferences_graphics::build_graphics_tab,
    },
    PreferencesTabDef {
        id: crate::preferences_audio::TAB_ID,
        label_key: "preferences-tab-audio",
        build: crate::preferences_audio::build_audio_tab,
    },
    PreferencesTabDef {
        id: crate::preferences_chat::TAB_ID,
        label_key: "preferences-tab-chat",
        build: crate::preferences_chat::build_chat_tab,
    },
    PreferencesTabDef {
        id: crate::preferences_camera_move::TAB_ID,
        label_key: "preferences-tab-camera-move",
        build: crate::preferences_camera_move::build_camera_move_tab,
    },
    PreferencesTabDef {
        id: crate::preferences_colors_skins::TAB_ID,
        label_key: "preferences-tab-colors-skins",
        build: crate::preferences_colors_skins::build_colors_skins_tab,
    },
    PreferencesTabDef {
        id: crate::preferences_network_cache::TAB_ID,
        label_key: "preferences-tab-network-cache",
        build: crate::preferences_network_cache::build_network_cache_tab,
    },
    PreferencesTabDef {
        id: "world-ui",
        label_key: "preferences-tab-world-ui",
        build: build_world_ui_tab,
    },
    PreferencesTabDef {
        id: "alerts",
        label_key: "preferences-tab-alerts",
        build: crate::preferences_alerts::build_alerts_tab,
    },
];

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// The retained entities of the built preferences floater, inserted by
/// [`build_preferences_content`] on first open (the `XUi` idiom — consumers
/// tolerate the pre-build `None`).
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct PreferencesUi {
    /// The floater root (carries [`UiPanelShown`]).
    pub(crate) root: Entity,
    /// The tab strip; its [`TabStrip::active`] is the current tab.
    pub(crate) tab_strip: Entity,
    /// The filter field's [`EditableText`] entity.
    pub(crate) search_field: Entity,
}

/// The shell's open / close and snapshot state.
#[derive(Resource, Debug, Default)]
pub(crate) struct PreferencesState {
    /// Whether the floater was open last frame (edge detection).
    open: bool,
    /// Whether the account scope was already loaded when the snapshot was
    /// taken — if it loads *while* the floater is open, the snapshot's account
    /// entries are refreshed so a later Cancel cannot wipe the just-loaded
    /// per-avatar overrides.
    account_was_loaded: bool,
    /// Per bound `(scope, name)`: the override that scope held when the
    /// floater was opened (or when OK last re-snapshotted). `None` means no
    /// override existed, so the revert *resets* instead of setting.
    snapshot: HashMap<(Scope, String), Option<SettingValue>>,
    /// The lowercased active filter term; empty = no filter.
    filter: String,
}

impl PreferencesState {
    /// The lowercased active filter term (empty = no filter) — read by tab
    /// modules with their own searchable surface (the alerts list) so they
    /// match against the same term as the [`PrefSearchRow`]s.
    pub(crate) fn filter(&self) -> &str {
        &self.filter
    }
}

/// Per tab index: whether a tab-owned searchable surface that is *not* made of
/// [`PrefSearchRow`]s (the alerts tab's virtualized list) currently has filter
/// hits. [`apply_preferences_filter`] ORs these into its per-tab hit counts,
/// so such a tab is dimmed / jumped-to like any other; an entry is only
/// meaningful while a filter term is active. Written by the owning tab's
/// module (the alerts view refresh), keyed by the tab's `PREF_TABS` index.
#[derive(Resource, Debug, Default)]
pub(crate) struct PreferencesExtraHits(pub(crate) HashMap<usize, bool>);

/// Debug: the `PREF_TABS` id to select once the floater's content builds —
/// for the offline screenshot harness, which cannot click the tab strip (the
/// `SL_VIEWER_UI_DEMO` idiom). Combine with the persisted open state
/// (`preferences_visible`) to land a headless run on a chosen tab.
const PREFERENCES_TAB_ENV: &str = "SL_VIEWER_PREFERENCES_TAB";

/// Select the [`PREFERENCES_TAB_ENV`] tab once the shell's content exists; a
/// missing / unknown value does nothing. One-shot.
fn select_env_preferences_tab(
    ui: Option<Res<PreferencesUi>>,
    mut strips: Query<&mut TabStrip>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(wanted) = std::env::var_os(PREFERENCES_TAB_ENV) else {
        *done = true;
        return;
    };
    let Some(ui) = ui else {
        return;
    };
    let Some(index) = PREF_TABS
        .iter()
        .position(|tab| Some(tab.id) == wanted.to_str())
    else {
        *done = true;
        return;
    };
    if let Ok(mut strip) = strips.get_mut(ui.tab_strip) {
        strip.active = index;
        *done = true;
    }
}

/// Written once per OK press, after the settings have been saved — the per-tab
/// **apply hook**: a tab whose settings need a non-live side effect on commit
/// (the reference's `LLPanelPreference::apply`) reads this from its own module
/// instead of registering anything with the shell.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct PreferencesApplied;

// ---------------------------------------------------------------------------
// Searchable rows — the helpers tab builders use.
// ---------------------------------------------------------------------------

/// Tags a row the preferences filter matches against, naming its label node.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PrefSearchRow {
    /// The row's [`Translated`] label — the text matched and highlighted.
    pub(crate) label: Entity,
}

/// Marks a [`PrefSearchRow`]'s label node, so the filter re-runs when a locale
/// switch (or the bundle first loading) rewrites the resolved label text.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PrefRowLabel;

/// Marks a preference checkbox's box node, so its fill tracks `Checked` (and
/// the account guard).
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PrefCheckboxBox;

/// Marks a preference slider's thumb node, so it slides to the bound value.
#[derive(Component, Debug, Clone, Copy)]
struct PrefSliderThumb;

/// The bare row node every preference row starts from.
fn pref_row_node() -> Node {
    Node {
        align_items: AlignItems::Center,
        ..row(Val::Px(ROW_GAP))
    }
}

/// Spawn a row's translated label, tagged for the filter.
fn spawn_row_label(commands: &mut Commands, parent: Entity, label_key: &'static str) -> Entity {
    commands
        .spawn((
            Text::default(),
            Translated::new(label_key),
            UiFont::Sans.at(FONT),
            TextColor(LABEL_COLOR),
            PrefRowLabel,
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id()
}

/// Spawn a searchable row holding a settings-bound **checkbox** and its
/// translated label, returning the row node (the filter's hide / show target;
/// a tab builder may parent further controls into it).
pub(crate) fn spawn_pref_checkbox(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    binding: SettingBinding,
) -> Entity {
    let row = commands
        .spawn((
            pref_row_node(),
            Name::new(format!("preferences:row:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        bound_checkbox(binding),
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
        PrefCheckboxBox,
        ChildOf(row),
    ));
    let label = spawn_row_label(commands, row, label_key);
    commands.entity(row).insert(PrefSearchRow { label });
    row
}

/// Spawn a searchable row holding a translated label and a settings-bound
/// **slider**, returning the row node (see `spawn_pref_checkbox`).
pub(crate) fn spawn_pref_slider(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    binding: SettingBinding,
    range: SliderRange,
    step: SliderStep,
) -> Entity {
    let row = commands
        .spawn((
            pref_row_node(),
            Name::new(format!("preferences:row:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    let label = spawn_row_label(commands, row, label_key);
    commands
        .spawn((
            bound_slider(binding, range, step),
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
            ChildOf(row),
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
                PrefSliderThumb,
            ));
        });
    commands.entity(row).insert(PrefSearchRow { label });
    row
}

/// Spawn a searchable row holding a translated label and a settings-bound
/// **combo**: each `(label_key, value)` option pair maps one translated option
/// label to the [`SettingValue`] it writes / matches. Returns the row node
/// (see `spawn_pref_checkbox`).
pub(crate) fn spawn_pref_combo(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    binding: SettingBinding,
    options: &[(&str, SettingValue)],
) -> Entity {
    let (row, _anchor) =
        spawn_pref_combo_with_anchor(commands, parent, label_key, binding, options);
    row
}

/// [`spawn_pref_combo`], also returning the combo **anchor** entity (the one
/// carrying the [`SettingBinding`] and emitting `ComboChanged`), for a tab that
/// must mark the anchor with its own component — e.g. the graphics tab's
/// quality-tier driver.
pub(crate) fn spawn_pref_combo_with_anchor(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    binding: SettingBinding,
    options: &[(&str, SettingValue)],
) -> (Entity, Entity) {
    let row = commands
        .spawn((
            pref_row_node(),
            Name::new(format!("preferences:row:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    let label = spawn_row_label(commands, row, label_key);
    let option_labels: Vec<String> = options.iter().map(|(key, _)| String::from(*key)).collect();
    let anchor = spawn_combo(
        commands,
        row,
        &ComboSpec {
            element: label_key,
            labels: &option_labels,
            active: 0,
            tab_index: 0,
            font_size: FONT,
            translate_labels: true,
        },
    );
    commands.entity(anchor).insert((
        binding,
        ComboBindingValues(options.iter().map(|(_, value)| value.clone()).collect()),
    ));
    commands.entity(row).insert(PrefSearchRow { label });
    (row, anchor)
}

/// Spawn a searchable **text-field** row: a translated label above a
/// settings-bound text input (single-line or multiline per `kind` /
/// `visible_lines`). Returns the outer node the filter hides / shows (see
/// `spawn_pref_checkbox`).
pub(crate) fn spawn_pref_text(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    binding: SettingBinding,
    kind: TextInputKind,
    visible_lines: f32,
) -> Entity {
    let row = commands
        .spawn((
            Node {
                align_items: AlignItems::Start,
                ..column(Val::Px(ROW_GAP / 2.0))
            },
            Name::new(format!("preferences:row:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    let label = spawn_row_label(commands, row, label_key);
    let field = spawn_text_input(
        commands,
        row,
        &TextInputSpec {
            visible_lines,
            fill: true,
            ..TextInputSpec::new(label_key, kind)
        },
    );
    commands.entity(field).insert(binding);
    commands.entity(row).insert(PrefSearchRow { label });
    row
}

/// Spawn a searchable row holding a translated label and a settings-bound
/// **colour swatch** (a [`SettingValue::Color3`] setting): clicking the swatch
/// opens the shared colour picker, every pick writes through the binding, and
/// the swatch follows the store (see the colour-swatch binding in
/// [`crate::settings_binding`]). Returns the row node (see
/// `spawn_pref_checkbox`).
pub(crate) fn spawn_pref_color(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    binding: SettingBinding,
) -> Entity {
    let row = commands
        .spawn((
            pref_row_node(),
            Name::new(format!("preferences:row:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    // Seeded black; the swatch sync pass paints the stored colour on the next
    // frame (the slider idiom — the initial value is a placeholder).
    let swatch = spawn_color_swatch(commands, row, label_key, 0, Color::BLACK);
    commands.entity(swatch).insert(binding);
    let label = spawn_row_label(commands, row, label_key);
    commands.entity(row).insert(PrefSearchRow { label });
    row
}

/// Spawn a searchable **action** row: a translated label plus a button the
/// caller attaches behaviour to (`.observe(On<Activate>)`). Returns the
/// button entity; the row is the button's parent.
pub(crate) fn spawn_pref_action(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    button_key: &'static str,
) -> Entity {
    let row = commands
        .spawn((
            pref_row_node(),
            Name::new(format!("preferences:row:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    let label = spawn_row_label(commands, row, label_key);
    let button = spawn_footer_button(commands, row, button_key, 0);
    commands.entity(row).insert(PrefSearchRow { label });
    button
}

/// Spawn a (non-searchable) section heading over a group of rows.
pub(crate) fn spawn_pref_section(commands: &mut Commands, parent: Entity, key: &'static str) {
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(SECTION_FONT),
        TextColor(SECTION_COLOR),
        Name::new(format!("preferences:section:{key}")),
        ChildOf(parent),
    ));
}

// ---------------------------------------------------------------------------
// The plugin.
// ---------------------------------------------------------------------------

/// Owns the preferences floater: the chrome spawn, the deferred content build,
/// the snapshot / revert lifecycle, the filter and the account guard.
#[derive(Debug, Clone, Copy, Default)]
pub struct PreferencesPlugin;

impl Plugin for PreferencesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PreferencesState>()
            .init_resource::<PreferencesExtraHits>()
            .add_message::<PreferencesApplied>()
            .add_systems(
                Startup,
                spawn_preferences_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    // After the deferred build, so the first open's snapshot
                    // already sees the freshly-built tabs' bindings (the
                    // builder runs, and flushes, inside that system's command
                    // application).
                    track_preferences_open_close
                        .after(crate::floater::build_deferred_floater_content),
                    guard_account_bindings,
                    select_env_preferences_tab,
                    mirror_preferences_filter,
                    apply_preferences_filter.after(mirror_preferences_filter),
                    drive_pref_checkbox_visual,
                    drive_pref_slider_visual,
                ),
            );
    }
}

/// Startup: spawn the floater's chrome, hidden; the content is built on first
/// open ([`DeferredFloaterContent`]).
fn spawn_preferences_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: PREFERENCES_FLOATER_ID,
            title: "Preferences".to_owned(),
            position: Vec2::new(160.0, 80.0),
            default_size: Some(Vec2::new(760.0, 520.0)),
            min_size: Some(Vec2::new(560.0, 380.0)),
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
        .insert(Translated::new("preferences-title"));
    let builder = commands.register_system(build_preferences_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build: the search row, the leading tab container (one
/// panel per `PREF_TABS` entry, each filled by its tab's builder), and the
/// OK / Cancel footer — ending with the [`PreferencesUi`] insert.
fn build_preferences_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    // The content column: search on top, tabs filling the middle, footer last.
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(8.0))
            },
            Name::new("preferences:content"),
            ChildOf(handle.content),
        ))
        .id();

    // The filter box. The placeholder string must be non-empty for the node to
    // exist; the `Translated` then keeps it locale-resolved.
    let search = spawn_search_field(
        &mut commands,
        content,
        &SearchFieldSpec {
            tab_index: 0,
            font_size: FONT,
            min_width: 220.0,
            placeholder: "Search settings".to_owned(),
            search_glyph: true,
            ..SearchFieldSpec::new("preferences-search")
        },
    );
    if let Some(placeholder) = search.placeholder {
        commands
            .entity(placeholder)
            .insert(Translated::new("preferences-search-placeholder"));
    }

    // The leading tab strip and its panels.
    let labels: Vec<String> = PREF_TABS
        .iter()
        .map(|tab| tab.label_key.to_owned())
        .collect();
    let tabs = spawn_tab_container(
        &mut commands,
        content,
        &TabSpec {
            element: "preferences-tabs",
            placement: TabPlacement::InlineStart,
            labels: &labels,
            active: 0,
            tab_index: 1,
            font_size: FONT,
            strip_width: Some(STRIP_WIDTH),
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );
    fill_tab_container(&mut commands, TabPlacement::InlineStart, &tabs);
    for (tab, panel) in PREF_TABS.iter().zip(tabs.panels.iter().copied()) {
        commands
            .entity(panel)
            .insert(Name::new(format!("preferences:tab:{}", tab.id)));
        (tab.build)(&mut commands, panel);
    }

    // The footer: OK, then Cancel, trailing-aligned (the reference's order).
    let footer = commands
        .spawn((
            Node {
                justify_content: JustifyContent::FlexEnd,
                ..row(Val::Px(8.0))
            },
            Name::new("preferences:footer"),
            ChildOf(content),
        ))
        .id();
    let ok = spawn_footer_button(&mut commands, footer, "preferences-ok", 2);
    commands.entity(ok).observe(on_preferences_ok);
    let cancel = spawn_footer_button(&mut commands, footer, "preferences-cancel", 3);
    commands.entity(cancel).observe(on_preferences_cancel);

    commands.insert_resource(PreferencesUi {
        root: handle.root,
        tab_strip: tabs.strip,
        search_field: search.field,
    });
}

/// Spawn a translated-label footer button, returning its clickable box.
pub(crate) fn spawn_footer_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    tab: i32,
) -> Entity {
    let button = commands
        .spawn((
            Button,
            TabIndex(tab),
            Node {
                padding: UiRect::axes(Val::Px(14.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            ClassList::new_with_classes([BUTTON_CLASS]),
            Name::new(format!("preferences:button:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    button
}

// ---------------------------------------------------------------------------
// Snapshot / revert.
// ---------------------------------------------------------------------------

/// Whether `entity` is `ancestor` or sits anywhere below it.
fn is_descendant_of(entity: Entity, ancestor: Entity, parents: &Query<&ChildOf>) -> bool {
    core::iter::successors(Some(entity), |entity| {
        parents.get(*entity).ok().map(ChildOf::parent)
    })
    .any(|entity| entity == ancestor)
}

/// Capture the override every binding under `root` currently has at its own
/// scope — `None` for a setting resting on a lower layer, so the restore can
/// tell "set the old override back" from "reset to default". Two widgets bound
/// to one setting collapse to one entry (same key, same value).
fn snapshot_bindings(
    store: &SettingsStore,
    root: Entity,
    bindings: &Query<(Entity, &SettingBinding)>,
    parents: &Query<&ChildOf>,
) -> HashMap<(Scope, String), Option<SettingValue>> {
    let mut snapshot = HashMap::new();
    for (entity, binding) in bindings {
        if !is_descendant_of(entity, root, parents) {
            continue;
        }
        snapshot.insert(
            (binding.scope(), binding.name().to_owned()),
            store.get_override(binding.scope(), binding.name()).cloned(),
        );
    }
    snapshot
}

/// Put every snapshotted override back: a remembered value is set again, an
/// absent one is reset. The binding sync passes then move the widgets.
fn restore_snapshot(
    settings: &mut ViewerSettings,
    snapshot: &HashMap<(Scope, String), Option<SettingValue>>,
) {
    for ((scope, name), saved) in snapshot {
        match saved {
            Some(value) => settings.set(*scope, name, value.clone()),
            None => {
                settings.reset(*scope, name);
            }
        }
    }
}

/// The open / close lifecycle: snapshot on the open edge, revert on the close
/// edge — which Cancel, the window's close button, `Ctrl+W` and OK all share
/// (after OK's re-snapshot the revert is a no-op). If the account scope loads
/// *while* the floater is open, the snapshot's account entries are refreshed
/// so a later Cancel keeps the just-loaded overrides.
fn track_preferences_open_close(
    ui: Option<Res<PreferencesUi>>,
    mut state: ResMut<PreferencesState>,
    settings: Option<ResMut<ViewerSettings>>,
    panels: Query<&UiPanelShown>,
    bindings: Query<(Entity, &SettingBinding)>,
    parents: Query<&ChildOf>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Some(mut settings) = settings else {
        return;
    };
    let shown = panels.get(ui.root).is_ok_and(|shown| shown.0);
    if shown && !state.open {
        state.snapshot = snapshot_bindings(settings.store(), ui.root, &bindings, &parents);
        state.account_was_loaded = settings.account_loaded();
    } else if !shown && state.open {
        restore_snapshot(&mut settings, &state.snapshot);
    } else if shown && !state.account_was_loaded && settings.account_loaded() {
        for ((scope, name), saved) in &mut state.snapshot {
            if *scope == Scope::Account {
                *saved = settings.store().get_override(Scope::Account, name).cloned();
            }
        }
        state.account_was_loaded = true;
    }
    state.open = shown;
}

/// Observer: **OK** — re-snapshot the current values (so the close-edge revert
/// becomes a no-op and a later Cancel reverts to *these* values), save both
/// scopes to disk, fire the per-tab apply hook, and close.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the shell's state and \
              store, the binding walk (bindings + parents), and the two outgoing messages"
)]
fn on_preferences_ok(
    _activate: On<Activate>,
    ui: Option<Res<PreferencesUi>>,
    mut state: ResMut<PreferencesState>,
    settings: Option<Res<ViewerSettings>>,
    bindings: Query<(Entity, &SettingBinding)>,
    parents: Query<&ChildOf>,
    mut floater_commands: MessageWriter<FloaterCommand>,
    mut applied: MessageWriter<PreferencesApplied>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Some(settings) = settings else {
        return;
    };
    state.snapshot = snapshot_bindings(settings.store(), ui.root, &bindings, &parents);
    settings.save_async();
    applied.write(PreferencesApplied);
    floater_commands.write(FloaterCommand {
        floater: ui.root,
        op: FloaterOp::Close,
    });
}

/// Observer: **Cancel** — just close; the revert rides the shared close edge
/// ([`track_preferences_open_close`]), exactly like the window's own close
/// button.
fn on_preferences_cancel(
    _activate: On<Activate>,
    ui: Option<Res<PreferencesUi>>,
    mut floater_commands: MessageWriter<FloaterCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    floater_commands.write(FloaterCommand {
        floater: ui.root,
        op: FloaterOp::Close,
    });
}

// ---------------------------------------------------------------------------
// The account guard.
// ---------------------------------------------------------------------------

/// Disable every control under the floater bound to a per-avatar setting while
/// the account scope has not loaded (pre-login) — an edit then could not be
/// persisted. The widgets natively refuse input under [`InteractionDisabled`];
/// insert / remove only on a state mismatch to avoid archetype churn.
fn guard_account_bindings(
    ui: Option<Res<PreferencesUi>>,
    settings: Option<Res<ViewerSettings>>,
    bindings: Query<(Entity, &SettingBinding, Has<InteractionDisabled>)>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    let Some(ui) = ui else {
        return;
    };
    let Some(settings) = settings else {
        return;
    };
    let want_disabled = !settings.account_loaded();
    for (entity, binding, disabled) in &bindings {
        if binding.scope() != Scope::Account {
            continue;
        }
        if !is_descendant_of(entity, ui.root, &parents) {
            continue;
        }
        if want_disabled && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        } else if !want_disabled && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
}

// ---------------------------------------------------------------------------
// The search / filter.
// ---------------------------------------------------------------------------

/// Mirror the filter field's live text into [`PreferencesState::filter`]
/// (lowercased, trimmed), the [`crate::menu_search`] idiom — written only on a
/// real change so the filter pass's change guard works.
pub(crate) fn mirror_preferences_filter(
    ui: Option<Res<PreferencesUi>>,
    fields: Query<&EditableText>,
    mut state: ResMut<PreferencesState>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Ok(field) = fields.get(ui.search_field) else {
        return;
    };
    let term = field.value().to_string().trim().to_lowercase();
    if state.filter != term {
        state.filter = term;
    }
}

/// Apply the filter to every searchable row: a miss collapses
/// (`Display::None`), a hit stays and its label is highlighted; a tab left
/// with no hit is dimmed in the strip, and if the *active* tab has no hit the
/// first tab that does is selected. An empty term restores everything (and the
/// selection stays where the filter left it, like the reference).
///
/// Runs when the term changes **or** any row label's resolved text changes (a
/// locale switch, the bundle first loading), so the match set is always
/// against the text the user actually sees.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the filter term, the rows \
              with their labels and colours, and the tab tree (panels, strip, buttons) the \
              per-tab hit counts and the selection jump need"
)]
pub(crate) fn apply_preferences_filter(
    ui: Option<Res<PreferencesUi>>,
    state: Res<PreferencesState>,
    extra_hits: Res<PreferencesExtraHits>,
    changed_labels: Query<(), (Changed<Text>, With<PrefRowLabel>)>,
    mut rows: Query<(Entity, &PrefSearchRow, &mut Node)>,
    labels: Query<&Text>,
    mut colors: Query<&mut TextColor>,
    parents: Query<&ChildOf>,
    tab_panels: Query<&TabPanel>,
    mut strips: Query<&mut TabStrip>,
    tab_buttons: Query<(Entity, &TabButton)>,
    children: Query<&Children>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !state.is_changed() && !extra_hits.is_changed() && changed_labels.is_empty() {
        return;
    }
    let filtering = !state.filter.is_empty();

    // Pass over the rows: show / hide, highlight, and count hits per tab.
    let mut tab_has_match: HashMap<usize, bool> = HashMap::new();
    let mut first_match: Option<usize> = None;
    for (row_entity, search_row, mut node) in &mut rows {
        let matched = !filtering
            || labels
                .get(search_row.label)
                .is_ok_and(|text| text.0.to_lowercase().contains(&state.filter));
        let display = if matched {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if let Ok(mut color) = colors.get_mut(search_row.label) {
            let target = if filtering && matched {
                FILTER_MATCH_COLOR
            } else {
                LABEL_COLOR
            };
            if color.0 != target {
                color.0 = target;
            }
        }
        // The tab this row lives on (only rows of *our* strip count).
        let panel_index = core::iter::successors(Some(row_entity), |entity| {
            parents.get(*entity).ok().map(ChildOf::parent)
        })
        .find_map(|entity| tab_panels.get(entity).ok())
        .and_then(|panel| (panel.strip == ui.tab_strip).then_some(panel.index));
        if let Some(index) = panel_index {
            let hit = filtering && matched;
            let entry = tab_has_match.entry(index).or_insert(false);
            *entry = *entry || hit;
            if hit {
                first_match = Some(first_match.map_or(index, |first| first.min(index)));
            }
        }
    }

    // Merge the tab-owned extra surfaces (the alerts list) into the hit set,
    // so their tabs dim and attract the jump like PrefSearchRow tabs.
    if filtering {
        for (&index, &hit) in &extra_hits.0 {
            if hit {
                let entry = tab_has_match.entry(index).or_insert(false);
                *entry = true;
                first_match = Some(first_match.map_or(index, |first| first.min(index)));
            }
        }
    }

    // Jump to the first tab that still has a hit when the active one has none.
    if filtering
        && let Ok(mut strip) = strips.get_mut(ui.tab_strip)
        && !tab_has_match.get(&strip.active).copied().unwrap_or(false)
        && let Some(first) = first_match
        && strip.active != first
    {
        strip.active = first;
    }

    // Dim the strip labels of tabs the filter left empty; restore otherwise.
    for (button_entity, button) in &tab_buttons {
        if button.strip != ui.tab_strip {
            continue;
        }
        let dim = filtering && !tab_has_match.get(&button.index).copied().unwrap_or(false);
        let target = if dim { MUTED_COLOR } else { TAB_LABEL_COLOR };
        if let Some(label) = first_text_descendant(button_entity, &children, &labels)
            && let Ok(mut color) = colors.get_mut(label)
            && color.0 != target
        {
            color.0 = target;
        }
    }
}

/// The first descendant of `root` (breadth-first) that has a [`Text`] — a tab
/// button's label node.
fn first_text_descendant(
    root: Entity,
    children: &Query<&Children>,
    texts: &Query<&Text>,
) -> Option<Entity> {
    let mut queue: Vec<Entity> = children
        .get(root)
        .map(|direct| direct.iter().collect())
        .unwrap_or_default();
    let mut cursor = 0;
    while let Some(entity) = queue.get(cursor).copied() {
        cursor = cursor.saturating_add(1);
        if texts.get(entity).is_ok() {
            return Some(entity);
        }
        if let Ok(more) = children.get(entity) {
            queue.extend(more.iter());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Control visuals (the settings-binding demo's look, on the shell's markers).
// ---------------------------------------------------------------------------

/// One preference checkbox box's paint inputs: its fill, whether it is
/// checked, and whether the account guard disables it (a type alias per
/// `clippy::type_complexity`).
type PrefCheckboxPaint<'world, 'state> = Query<
    'world,
    'state,
    (
        &'static mut BackgroundColor,
        Has<Checked>,
        Has<InteractionDisabled>,
    ),
    With<PrefCheckboxBox>,
>;

/// Colour each preference checkbox's box from its `Checked` state (muted while
/// the account guard disables it).
fn drive_pref_checkbox_visual(mut boxes: PrefCheckboxPaint) {
    for (mut fill, checked, disabled) in &mut boxes {
        let target = if disabled {
            CHECK_DISABLED
        } else if checked {
            CHECK_ON
        } else {
            CHECK_OFF
        };
        if fill.0 != target {
            fill.0 = target;
        }
    }
}

/// Slide each preference slider's thumb to its [`SliderValue`] within its
/// range.
fn drive_pref_slider_visual(
    sliders: Query<(&SliderValue, &SliderRange, &Children), With<SettingBinding>>,
    mut thumbs: Query<&mut LogicalInset, With<PrefSliderThumb>>,
) {
    for (value, range, slider_children) in &sliders {
        let span = range.span();
        let fraction = if span > f32::EPSILON {
            ((value.0 - range.start()) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let offset = fraction * (TRACK_WIDTH - THUMB_WIDTH);
        for child in slider_children {
            if let Ok(mut inset) = thumbs.get_mut(*child) {
                inset.0.inline_start = Val::Px(offset);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The first tab: UI & world display.
// ---------------------------------------------------------------------------

/// The **UI & world display** tab — already-registered, live-consumed global
/// settings no sibling tab task claims: the in-world overlays (property lines,
/// status-bar coordinates) and the mini-map / world-map display toggles.
fn build_world_ui_tab(commands: &mut Commands, panel: Entity) {
    spawn_pref_section(commands, panel, "preferences-section-world");
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-property-lines",
        SettingBinding::global(crate::parcel_borders::SETTING_SHOW_PROPERTY_LINES),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-status-coordinates",
        SettingBinding::global(crate::world_api::SHOW_COORDINATES_KEY),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-hover-text",
        SettingBinding::global(crate::hover_text::SETTING_SHOW_HOVER_TEXT),
    );

    spawn_pref_section(commands, panel, "preferences-section-maps");
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-minimap-rotate",
        SettingBinding::global(crate::minimap::SETTING_ROTATE),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-minimap-auto-center",
        SettingBinding::global(crate::minimap::SETTING_AUTO_CENTER),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-minimap-objects",
        SettingBinding::global(crate::minimap::SETTING_OBJECTS),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-minimap-property-lines",
        SettingBinding::global(crate::minimap::SETTING_PROPERTY_LINES),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-minimap-for-sale",
        SettingBinding::global(crate::minimap::SETTING_FOR_SALE),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-minimap-chat-ring",
        SettingBinding::global(crate::minimap::SETTING_CHAT_RING),
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-minimap-scale",
        SettingBinding::global(crate::minimap::SETTING_SCALE),
        SliderRange::new(
            crate::minimap_math::MAP_SCALE_MIN,
            crate::minimap_math::MAP_SCALE_MAX,
        ),
        SliderStep(32.0),
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-minimap-opacity",
        SettingBinding::global(crate::minimap::SETTING_OPACITY),
        SliderRange::new(0.0, 1.0),
        SliderStep(0.05),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-worldmap-people",
        SettingBinding::global(crate::world_map::SETTING_PEOPLE),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-worldmap-infohubs",
        SettingBinding::global(crate::world_map::SETTING_INFOHUBS),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-worldmap-land-sale",
        SettingBinding::global(crate::world_map::SETTING_LAND_SALE),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-worldmap-events",
        SettingBinding::global(crate::world_map::SETTING_EVENTS),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-worldmap-region-names",
        SettingBinding::global(crate::world_map::SETTING_REGION_NAMES),
    );
}

// ---------------------------------------------------------------------------
// The gallery specimen.
// ---------------------------------------------------------------------------

/// The static preferences-shell specimen for the gallery / headless harness: a
/// search box, a leading two-tab container, two stand-in setting rows and
/// the OK / Cancel footer — the layout, with none of the live behaviour (per
/// the element registry's rule: no plugin, no store, no observers).
pub fn spawn_preferences_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let card = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(10.0)),
                ..column(Val::Px(8.0))
            },
            Name::new("preferences-specimen"),
            ChildOf(parent),
        ))
        .id();

    spawn_search_field(
        commands,
        card,
        &SearchFieldSpec {
            font_size: cx.font_size,
            search_glyph: true,
            ..SearchFieldSpec::new("preferences-specimen")
        },
    );

    let labels = [cx.text("General"), cx.text("Alerts")];
    let tabs = spawn_tab_container(
        commands,
        card,
        &TabSpec {
            element: "preferences-specimen-tabs",
            placement: TabPlacement::InlineStart,
            labels: &labels,
            active: 0,
            tab_index: 0,
            font_size: cx.font_size,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: false,
        },
    );
    if let Some(panel) = tabs.panels.first().copied() {
        // A checkbox row and a slider row, as static stand-ins.
        let check_row = commands.spawn((pref_row_node(), ChildOf(panel))).id();
        commands.spawn((
            Node {
                width: Val::Px(CHECK_SIZE),
                height: Val::Px(CHECK_SIZE),
                border: UiRect::all(Val::Px(2.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(CHECK_ON),
            ChildOf(check_row),
        ));
        commands.spawn((
            Text::new(cx.text("Show property lines")),
            cx.font(UiFont::Sans),
            TextColor(LABEL_COLOR),
            ChildOf(check_row),
        ));
        let slider_row = commands.spawn((pref_row_node(), ChildOf(panel))).id();
        commands.spawn((
            Text::new(cx.text("Mini-map opacity")),
            cx.font(UiFont::Sans),
            TextColor(LABEL_COLOR),
            ChildOf(slider_row),
        ));
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
                ChildOf(slider_row),
            ))
            .with_child((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(THUMB_WIDTH),
                    height: Val::Px(TRACK_HEIGHT),
                    ..default()
                },
                LogicalInset(LogicalRect {
                    inline_start: Val::Px(TRACK_WIDTH * 0.6),
                    ..LogicalRect::ZERO
                }),
                BackgroundColor(THUMB_FILL),
            ));
    }
    if let Some(panel) = tabs.panels.get(1).copied() {
        // The alerts tab stand-in: a headline toggle row over a static
        // two-column popup list (checkbox column | ignoretext label), the
        // live layout's shape without the virtualized table.
        let headline_row = commands.spawn((pref_row_node(), ChildOf(panel))).id();
        commands.spawn((
            Node {
                width: Val::Px(CHECK_SIZE),
                height: Val::Px(CHECK_SIZE),
                border: UiRect::all(Val::Px(2.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(CHECK_ON),
            ChildOf(headline_row),
        ));
        commands.spawn((
            Text::new(cx.text("Notify me when my friends log in or out")),
            cx.font(UiFont::Sans),
            TextColor(LABEL_COLOR),
            ChildOf(headline_row),
        ));
        let list_rows: [(&str, bool); 3] = [
            ("About Land: unsaved changes", true),
            ("Confirm before I pay an object", false),
            ("Warn about script permissions", true),
        ];
        let header_row = commands.spawn((pref_row_node(), ChildOf(panel))).id();
        commands.spawn((
            Text::new(cx.text("Show")),
            cx.font(UiFont::Sans),
            TextColor(SECTION_COLOR),
            ChildOf(header_row),
        ));
        commands.spawn((
            Text::new(cx.text("Alert")),
            cx.font(UiFont::Sans),
            TextColor(SECTION_COLOR),
            ChildOf(header_row),
        ));
        for (label, shown) in list_rows {
            let list_row = commands.spawn((pref_row_node(), ChildOf(panel))).id();
            commands.spawn((
                Node {
                    width: Val::Px(CHECK_SIZE),
                    height: Val::Px(CHECK_SIZE),
                    border: UiRect::all(Val::Px(2.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BorderColor::all(CONTROL_BORDER),
                BackgroundColor(if shown { CHECK_ON } else { CHECK_OFF }),
                ChildOf(list_row),
            ));
            commands.spawn((
                Text::new(cx.text(label)),
                cx.font(UiFont::Sans),
                TextColor(LABEL_COLOR),
                ChildOf(list_row),
            ));
        }
    }

    let footer = commands
        .spawn((
            Node {
                justify_content: JustifyContent::FlexEnd,
                ..row(Val::Px(8.0))
            },
            ChildOf(card),
        ))
        .id();
    for label in ["OK", "Cancel"] {
        let button = commands
            .spawn((
                Node {
                    padding: UiRect::axes(Val::Px(14.0), Val::Px(5.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BorderColor::all(CONTROL_BORDER),
                BackgroundColor(BUTTON_BACKGROUND),
                ChildOf(footer),
            ))
            .id();
        commands.spawn((
            Text::new(cx.text(label)),
            cx.font(UiFont::Sans),
            TextColor(LABEL_COLOR),
            ChildOf(button),
        ));
    }

    card
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use bevy::ui::InteractionDisabled;
    use bevy::ui_widgets::Activate;
    use pretty_assertions::assert_eq;
    use sl_settings::{Scope, SettingValue, SettingsStore};

    use super::{
        FILTER_MATCH_COLOR, LABEL_COLOR, PrefRowLabel, PrefSearchRow, PreferencesApplied,
        PreferencesExtraHits, PreferencesState, PreferencesUi, apply_preferences_filter,
        guard_account_bindings, on_preferences_cancel, on_preferences_ok,
        track_preferences_open_close,
    };
    use crate::floater::FloaterCommand;
    use crate::settings::ViewerSettings;
    use crate::settings_binding::SettingBinding;
    use crate::ui::UiPanelShown;
    use crate::ui_tab::{TabPanel, TabStrip};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The headless shell fixture: a fake floater root with a two-tab layout,
    /// one searchable bound row per tab. The tests drive open / close by
    /// flipping the root's [`UiPanelShown`] directly — exactly what the real
    /// close command path does — so no floater manager is needed.
    struct Fixture {
        /// The fake floater root (carries [`UiPanelShown`]).
        root: Entity,
        /// The tab strip entity (carries [`TabStrip`]).
        strip: Entity,
        /// Row / label / bound-control entities, one triple per tab.
        rows: [(Entity, Entity, Entity); 2],
    }

    /// A headless app with the shell's systems over a store populated by
    /// `register`.
    fn app(register: impl FnOnce(&mut SettingsStore)) -> App {
        let mut store = SettingsStore::new();
        register(&mut store);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(ViewerSettings::from_store_for_test(store))
            .init_resource::<PreferencesState>()
            .init_resource::<PreferencesExtraHits>()
            .add_message::<PreferencesApplied>()
            .add_message::<FloaterCommand>()
            .add_systems(
                Update,
                (
                    track_preferences_open_close,
                    guard_account_bindings,
                    apply_preferences_filter,
                ),
            );
        app
    }

    /// Build the fixture: root → two [`TabPanel`]s → one labelled, bound row
    /// each, plus the [`PreferencesUi`] resource pointing at it all.
    fn spawn_fixture(app: &mut App, bindings: [SettingBinding; 2], labels: [&str; 2]) -> Fixture {
        let world = app.world_mut();
        let root = world.spawn((Node::default(), UiPanelShown(false))).id();
        let strip = world
            .spawn(TabStrip {
                element: "preferences-tabs",
                active: 0,
            })
            .id();
        let mut rows = [(root, root, root); 2];
        for (index, (binding, label_text)) in bindings.into_iter().zip(labels).enumerate() {
            let panel = world
                .spawn((Node::default(), TabPanel { strip, index }, ChildOf(root)))
                .id();
            let row = world.spawn((Node::default(), ChildOf(panel))).id();
            let label = world
                .spawn((
                    Text::new(label_text),
                    TextColor(LABEL_COLOR),
                    PrefRowLabel,
                    ChildOf(row),
                ))
                .id();
            let control = world.spawn((binding, ChildOf(row))).id();
            world.entity_mut(row).insert(PrefSearchRow { label });
            if let Some(slot) = rows.get_mut(index) {
                *slot = (row, label, control);
            }
        }
        // A stand-in filter-field entity; the filter tests write the term into
        // the state directly (the mirror system is thin glue over the field).
        let search_field = world.spawn_empty().id();
        world.insert_resource(PreferencesUi {
            root,
            tab_strip: strip,
            search_field,
        });
        Fixture { root, strip, rows }
    }

    /// Flip the fixture's floater open or closed and run a frame.
    fn set_open(app: &mut App, root: Entity, open: bool) {
        if let Some(mut shown) = app.world_mut().entity_mut(root).get_mut::<UiPanelShown>() {
            shown.0 = open;
        }
        app.update();
    }

    /// A read-only view of the store under test.
    fn store(app: &App) -> &sl_settings::SettingsStore {
        app.world().resource::<ViewerSettings>().store()
    }

    /// Closing without OK reverts every bound setting to its open-time state:
    /// a pre-existing override is set back, a bare default is reset (not
    /// overridden with the old value).
    #[test]
    fn close_without_ok_reverts_set_back_and_reset() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Flag", SettingValue::Bool(false), "a toggle")
                .ok();
            store
                .register("Level", SettingValue::F32(10.0), "a level")
                .ok();
        });
        // "Flag" is already overridden before the floater opens; "Level" rests
        // on its default.
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            "Flag",
            SettingValue::Bool(true),
        );
        let fixture = spawn_fixture(
            &mut app,
            [
                SettingBinding::global("Flag"),
                SettingBinding::global("Level"),
            ],
            ["Show property lines", "Mini-map opacity"],
        );
        set_open(&mut app, fixture.root, true);

        // Edit both while open (as the live binding observers would).
        {
            let mut settings = app.world_mut().resource_mut::<ViewerSettings>();
            settings.set(Scope::Global, "Flag", SettingValue::Bool(false));
            settings.set(Scope::Global, "Level", SettingValue::F32(55.0));
        }
        set_open(&mut app, fixture.root, false);

        assert_eq!(
            store(&app).get_override(Scope::Global, "Flag"),
            Some(&SettingValue::Bool(true)),
            "a pre-existing override is set back"
        );
        assert_eq!(
            store(&app).get_override(Scope::Global, "Level"),
            None,
            "a setting that rested on its default is reset, not re-overridden"
        );
        assert!((store(&app).get_f32("Level")? - 10.0).abs() < 1.0e-4);
        Ok(())
    }

    /// A **widgetless** binding marker inside a `Display::None` container —
    /// the alerts tab's snapshot markers for its virtualized popup list —
    /// participates in the open-edge snapshot exactly like a live widget: an
    /// account-scope suppression written while the floater is open is
    /// reverted by a close without OK.
    #[test]
    fn hidden_widgetless_markers_participate_in_the_snapshot() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("SomePopup", SettingValue::Bool(true), "show")
                .ok();
        });
        app.world_mut()
            .resource_mut::<ViewerSettings>()
            .mark_account_loaded_for_test();
        let fixture = spawn_fixture(
            &mut app,
            [
                SettingBinding::global("SomePopup"),
                SettingBinding::global("SomePopup"),
            ],
            ["row a", "row b"],
        );
        // The alerts-tab shape: a hidden container under the floater holding a
        // bare (Node, SettingBinding) marker — no widget components at all.
        let hidden = app
            .world_mut()
            .spawn((
                Node {
                    display: Display::None,
                    ..Default::default()
                },
                ChildOf(fixture.root),
            ))
            .id();
        app.world_mut().spawn((
            Node::default(),
            SettingBinding::account("SomePopup"),
            ChildOf(hidden),
        ));
        set_open(&mut app, fixture.root, true);
        // The list's checkbox write while open: an account-scope suppression.
        app.world_mut()
            .resource_mut::<ViewerSettings>()
            .set_account("SomePopup", SettingValue::Bool(false));
        set_open(&mut app, fixture.root, false);
        assert_eq!(
            store(&app).get_override(Scope::Account, "SomePopup"),
            None,
            "the close-without-OK revert reaches the marker's setting"
        );
        Ok(())
    }

    /// OK re-snapshots and saves: the close that follows reverts nothing, a
    /// later open + cancel reverts to the OK'd values, and the per-tab apply
    /// hook fires exactly once.
    #[test]
    fn ok_persists_and_resnapshots() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Level", SettingValue::F32(10.0), "a level")
                .ok();
            store
                .register("Flag", SettingValue::Bool(false), "a toggle")
                .ok();
        });
        let fixture = spawn_fixture(
            &mut app,
            [
                SettingBinding::global("Level"),
                SettingBinding::global("Flag"),
            ],
            ["Mini-map opacity", "Show property lines"],
        );
        let ok_button = {
            let mut entity = app.world_mut().spawn_empty();
            entity.observe(on_preferences_ok);
            entity.id()
        };
        set_open(&mut app, fixture.root, true);
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            "Level",
            SettingValue::F32(55.0),
        );
        app.world_mut().trigger(Activate {
            entity: ok_button,
            button: None,
        });
        app.update();
        assert_eq!(
            app.world()
                .resource::<Messages<PreferencesApplied>>()
                .iter_current_update_messages()
                .count(),
            1,
            "the apply hook fires once per OK"
        );
        // The close that follows OK (here: the manual flip) must not revert.
        set_open(&mut app, fixture.root, false);
        assert!((store(&app).get_f32("Level")? - 55.0).abs() < 1.0e-4);

        // A later open + close without edits is a clean no-op too.
        set_open(&mut app, fixture.root, true);
        set_open(&mut app, fixture.root, false);
        assert!((store(&app).get_f32("Level")? - 55.0).abs() < 1.0e-4);
        Ok(())
    }

    /// Cancel routes through the shared close edge: the observer only writes
    /// the close command, and the revert happens when the panel actually
    /// hides.
    #[test]
    fn cancel_requests_close() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Flag", SettingValue::Bool(false), "a toggle")
                .ok();
            store
                .register("Level", SettingValue::F32(1.0), "a level")
                .ok();
        });
        let fixture = spawn_fixture(
            &mut app,
            [
                SettingBinding::global("Flag"),
                SettingBinding::global("Level"),
            ],
            ["a", "b"],
        );
        let cancel_button = {
            let mut entity = app.world_mut().spawn_empty();
            entity.observe(on_preferences_cancel);
            entity.id()
        };
        set_open(&mut app, fixture.root, true);
        app.world_mut().trigger(Activate {
            entity: cancel_button,
            button: None,
        });
        app.update();
        let close_requested = app
            .world()
            .resource::<Messages<FloaterCommand>>()
            .iter_current_update_messages()
            .any(|command| command.floater == fixture.root);
        assert!(close_requested, "cancel asks the floater manager to close");
        Ok(())
    }

    /// A control bound to a per-avatar setting is disabled until the account
    /// scope loads, then enabled.
    #[test]
    fn account_rows_disabled_until_login() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("PerAvatar", SettingValue::Bool(false), "an account toggle")
                .ok();
            store
                .register("Machine", SettingValue::Bool(false), "a global toggle")
                .ok();
        });
        let fixture = spawn_fixture(
            &mut app,
            [
                SettingBinding::account("PerAvatar"),
                SettingBinding::global("Machine"),
            ],
            ["a", "b"],
        );
        app.update();
        let (_, _, account_control) = fixture.rows[0];
        let (_, _, global_control) = fixture.rows[1];
        assert!(
            app.world()
                .entity(account_control)
                .contains::<InteractionDisabled>(),
            "an account-bound control is guarded pre-login"
        );
        assert!(
            !app.world()
                .entity(global_control)
                .contains::<InteractionDisabled>(),
            "a global-bound control is never guarded"
        );
        app.world_mut()
            .resource_mut::<ViewerSettings>()
            .mark_account_loaded_for_test();
        app.update();
        assert!(
            !app.world()
                .entity(account_control)
                .contains::<InteractionDisabled>(),
            "the guard lifts once the account scope has loaded"
        );
        Ok(())
    }

    /// A filter term hides the rows that do not match, highlights the labels
    /// that do, and jumps the strip to the first tab that still has a hit.
    #[test]
    fn filter_hides_highlights_and_selects_matching_tab() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Flag", SettingValue::Bool(false), "a toggle")
                .ok();
            store
                .register("Level", SettingValue::F32(1.0), "a level")
                .ok();
        });
        let fixture = spawn_fixture(
            &mut app,
            [
                SettingBinding::global("Flag"),
                SettingBinding::global("Level"),
            ],
            ["Show property lines", "Mini-map opacity"],
        );
        set_open(&mut app, fixture.root, true);
        app.world_mut().resource_mut::<PreferencesState>().filter = "opacity".to_owned();
        app.update();

        let (miss_row, miss_label, _) = fixture.rows[0];
        let (hit_row, hit_label, _) = fixture.rows[1];
        assert_eq!(
            app.world()
                .entity(miss_row)
                .get::<Node>()
                .map(|n| n.display),
            Some(Display::None),
            "a row that does not match collapses"
        );
        assert_eq!(
            app.world().entity(hit_row).get::<Node>().map(|n| n.display),
            Some(Display::Flex),
            "a matching row stays"
        );
        assert_eq!(
            app.world()
                .entity(hit_label)
                .get::<TextColor>()
                .map(|c| c.0),
            Some(FILTER_MATCH_COLOR),
            "a matching label is highlighted"
        );
        assert_eq!(
            app.world()
                .entity(miss_label)
                .get::<TextColor>()
                .map(|c| c.0),
            Some(LABEL_COLOR),
            "a missing label keeps the resting colour"
        );
        assert_eq!(
            app.world()
                .entity(fixture.strip)
                .get::<TabStrip>()
                .map(|s| s.active),
            Some(1),
            "the strip jumps to the first tab with a hit"
        );
        Ok(())
    }

    /// Clearing the term restores every row and colour; the strip stays where
    /// the filter left it (the reference's behaviour).
    #[test]
    fn clearing_filter_restores_rows() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Flag", SettingValue::Bool(false), "a toggle")
                .ok();
            store
                .register("Level", SettingValue::F32(1.0), "a level")
                .ok();
        });
        let fixture = spawn_fixture(
            &mut app,
            [
                SettingBinding::global("Flag"),
                SettingBinding::global("Level"),
            ],
            ["Show property lines", "Mini-map opacity"],
        );
        set_open(&mut app, fixture.root, true);
        app.world_mut().resource_mut::<PreferencesState>().filter = "opacity".to_owned();
        app.update();
        app.world_mut().resource_mut::<PreferencesState>().filter = String::new();
        app.update();

        for (row, label, _) in fixture.rows {
            assert_eq!(
                app.world().entity(row).get::<Node>().map(|n| n.display),
                Some(Display::Flex),
                "every row is restored"
            );
            assert_eq!(
                app.world().entity(label).get::<TextColor>().map(|c| c.0),
                Some(LABEL_COLOR),
                "every label returns to the resting colour"
            );
        }
        assert_eq!(
            app.world()
                .entity(fixture.strip)
                .get::<TabStrip>()
                .map(|s| s.active),
            Some(1),
            "the selection stays where the filter left it"
        );
        Ok(())
    }
}
