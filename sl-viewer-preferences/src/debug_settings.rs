//! The raw **debug-settings editor** (`viewer-preferences-debug-settings-editor`):
//! a searchable, type-aware editor over **every** entry in the typed settings
//! store — pick a setting by name, see its type / default / current value, and
//! edit it directly. The escape hatch for settings that have no dedicated
//! preferences control.
//!
//! # Shape
//!
//! A separate floater (not a preferences tab), opened from the **Advanced**
//! menu or `Ctrl+Alt+Shift+S`, mirroring the reference viewer's
//! `llfloatersettingsdebug`. Two panes:
//!
//! - **Left**: a search box (matching name *and* comment, case-insensitively),
//!   a "changed settings only" toggle (itself a registered setting,
//!   `SETTING_HIDE_DEFAULT`, the reference's `DebugSettingsHideDefault`),
//!   and a virtualized two-column table — a `*` changed-marker column and the
//!   setting name, in the store's sorted-name order.
//! - **Right**: the selected setting's comment, type, and its value at every
//!   layer (declared default, Global override, Account override, effective),
//!   a **scope selector** choosing which layer edits and resets write to, one
//!   build-once editor stack per [`SettingKind`] (`Display`-toggled, never
//!   respawned), a copy-name button and a reset-to-default button.
//!
//! # Semantics
//!
//! Unlike the preferences floater there is no OK / Cancel snapshot: edits
//! apply **live**, exactly like the reference floater. Deliberate departure
//! from the reference: our store layers Global and Account overrides per
//! setting (the reference merges two disjoint control groups), so the editor
//! shows both layers and lets the user pick the write target — the Account
//! layer guarded until the account scope loads at login. Edits are not
//! flushed to disk per keystroke; they ride the existing persistence edges
//! (logout, a preferences OK, the floater-geometry flush), the same model as
//! the quick-preferences panel.
//!
//! Reference (Firestorm, read-only): `llfloatersettingsdebug.{h,cpp}`,
//! `floater_settings_debug.xml`, `llcontrol.h`.

use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::ui_widgets::{Activate, Checkbox, ValueChange};
use sl_settings::{Scope, SettingDecl, SettingKind, SettingValue};

use crate::clipboard::{ViewerClipboard, copy_to_clipboard};
use crate::floater::{
    DeferredFloaterContent, Floater, FloaterCaps, FloaterHandle, FloaterSpec, floater_shown,
    spawn_floater, toggle_floater,
};
use crate::i18n::Translated;
use crate::preferences::{CHECK_OFF, CHECK_SIZE, CONTROL_BORDER, PrefCheckboxBox};
use crate::settings::ViewerSettings;
use crate::settings_binding::{SettingBinding, bound_checkbox};
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_color_picker::{ColorPicked, ColorSwatchValue, spawn_color_swatch};
use crate::ui_combo::{ComboChanged, ComboSelection, ComboSpec, spawn_combo};
use crate::ui_element::ElementCx;
use crate::ui_font::UiFont;
use crate::ui_search::{SearchFieldSpec, spawn_search_field};
use crate::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableSelectionMode, TableSpec,
    TableState, set_table_cell, spawn_table, spawn_table_row,
};
use crate::ui_text_input::{TextInputKind, TextInputSpec, TextInputValue, spawn_text_input};
use crate::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists};

/// The floater's stable id (geometry persistence, menu toggle, tests).
pub const DEBUG_SETTINGS_FLOATER_ID: &str = "debug_settings";

/// The "changed settings only" toggle's setting — a real registered setting,
/// like the reference's `DebugSettingsHideDefault` (its name kept for
/// familiarity).
pub(crate) const SETTING_HIDE_DEFAULT: &str = "DebugSettingsHideDefault";

/// The editor's body font size, in logical pixels.
const FONT: f32 = 13.0;

/// The settings list's uniform row height, in logical pixels.
const ROW_HEIGHT: f32 = 22.0;

/// The left (search + list) pane's fixed width, in logical pixels.
const LIST_PANE_WIDTH: f32 = 300.0;

/// The fixed width of the changed-marker column, in logical pixels.
const CHANGED_COL_WIDTH: f32 = 22.0;

/// The least width of a detail row's leading label, in logical pixels — keeps
/// the value column aligned across the rows (a longer translation may widen
/// its own row rather than overflow).
const DETAIL_LABEL_WIDTH: f32 = 84.0;

/// The changed-marker glyph shown beside an overridden setting.
const CHANGED_MARK: &str = "*";

/// The value shown for a layer holding no override.
const NO_OVERRIDE: &str = "–";

/// The list header's label colour (the preferences section palette).
const HEADER_COLOR: Color = Color::srgb(0.75, 0.80, 0.88);

/// The list cell / detail value colour (the preferences row-label palette).
const CELL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// The muted tone for the comment text and the detail row labels.
const MUTED_COLOR: Color = Color::srgb(0.65, 0.70, 0.78);

/// The largest per-channel disagreement the swatch seed treats as "already in
/// sync" (half the picker's 8-bit quantisation step, the
/// [`crate::settings_binding`] epsilon).
const COLOR_SEED_EPSILON: f32 = 0.5 / 255.0;

/// The settings list: the changed-marker column and the name column, in the
/// store's sorted-name order (no widget sort, no persisted geometry — the
/// order is canonical).
const DEBUG_TABLE: TableSpec = TableSpec {
    element: "debug-settings",
    selection: TableSelectionMode::Single,
    columns: &[
        TableColumn {
            header_key: "debug-settings-col-changed",
            token: "changed",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed {
                default: CHANGED_COL_WIDTH,
            },
            align: TableAlign::Center,
            sortable: false,
        },
        TableColumn {
            header_key: "debug-settings-col-name",
            token: "name",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: false,
        },
    ],
    default_sort: &[],
    builtin_sort: false,
    row_height: ROW_HEIGHT,
    font_size: FONT,
    header_color: HEADER_COLOR,
    cell_color: CELL_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

/// Register this module's settings (`SETTING_HIDE_DEFAULT`).
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        &["debug_settings"],
        SETTING_HIDE_DEFAULT,
        SettingValue::Bool(false),
        "Show only settings with an override in the debug-settings editor",
    );
}

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// The editor's retained entities, inserted at the end of the deferred
/// content build.
#[derive(Resource, Debug, Clone, Copy)]
struct DebugSettingsUi {
    /// The search field's [`EditableText`] entity.
    search_field: Entity,
    /// The table root (carries [`TableState`]).
    table: Entity,
    /// The virtualized viewport (carries [`VirtualList`]).
    viewport: Entity,
    /// The selected setting's name read-out.
    name_text: Entity,
    /// The selected setting's comment read-out.
    comment_text: Entity,
    /// The selected setting's type (+ transient marker) read-out.
    type_text: Entity,
    /// The declared-default value read-out.
    default_value: Entity,
    /// The Global-override value read-out ([`NO_OVERRIDE`] when absent).
    global_value: Entity,
    /// The Account-override value read-out ([`NO_OVERRIDE`] when absent).
    account_value: Entity,
    /// The effective (resolved) value read-out.
    effective_value: Entity,
    /// The scope selector's combo anchor (carries [`ComboSelection`]).
    scope_combo: Entity,
    /// The "select a setting" placeholder row, shown while nothing is
    /// selected.
    placeholder_row: Entity,
    /// The Bool editor row.
    bool_row: Entity,
    /// The Bool editor's checkbox.
    bool_checkbox: Entity,
    /// The String editor row.
    string_row: Entity,
    /// The String editor's line field.
    string_field: Entity,
    /// The F32 editor row.
    f32_row: Entity,
    /// The F32 editor's numeric field.
    f32_field: Entity,
    /// The I32 editor row.
    i32_row: Entity,
    /// The I32 editor's numeric field.
    i32_field: Entity,
    /// The U32 editor row.
    u32_row: Entity,
    /// The U32 editor's numeric field.
    u32_field: Entity,
    /// The 3-vector editor row (serves both `Vec3` and `Vec3d`).
    vec_row: Entity,
    /// The vector editor's X / Y / Z fields.
    vec_fields: [Entity; 3],
    /// The rectangle editor row.
    rect_row: Entity,
    /// The rectangle editor's left / top / right / bottom fields.
    rect_fields: [Entity; 4],
    /// The colour editor row (serves both `Color3` and `Color4`).
    color_row: Entity,
    /// The colour editor's swatch.
    color_swatch: Entity,
    /// The alpha editor row, shown only for `Color4`.
    alpha_row: Entity,
    /// The alpha editor's numeric field.
    alpha_field: Entity,
}

/// One registered setting, projected for the list.
#[derive(Debug, Clone)]
struct DebugEntry {
    /// The setting's name (the store key).
    name: String,
    /// [`name`](Self::name) lowercased, a filter match target.
    name_lower: String,
    /// The declaration comment lowercased, the other filter match target.
    comment_lower: String,
}

/// The list model: every registered setting and the filtered view the pooled
/// rows present. Rebuilt by [`refresh_debug_view`]; its change tick tells
/// [`bind_debug_rows`] to re-project every visible row.
#[derive(Resource, Debug, Default)]
struct DebugSettingsModel {
    /// Every registered setting, in the store's sorted-name order.
    entries: Vec<DebugEntry>,
    /// Indices into [`entries`](Self::entries): the rows currently presented
    /// (name order preserved).
    view: Vec<usize>,
    /// The lowercased, trimmed mirror of the search field's text.
    filter: String,
}

/// What the detail pane edits: the selected setting and the override layer
/// edits / resets write to.
#[derive(Resource, Debug, Clone)]
struct DebugEditorState {
    /// The selected setting's name, or `None` while nothing is selected.
    selected: Option<String>,
    /// The override layer an edit or reset targets.
    scope: Scope,
    /// The last table selection revision acted on (see
    /// [`TableState::selection_revision`]).
    last_selection_revision: u64,
}

impl Default for DebugEditorState {
    /// Nothing selected, editing the Global layer.
    fn default() -> Self {
        Self {
            selected: None,
            scope: Scope::Global,
            last_selection_revision: 0,
        }
    }
}

/// Which editor field held keyboard focus last frame, to commit on blur (the
/// build window's `ParamFieldFocus` idiom).
#[derive(Resource, Debug, Default)]
struct DebugFieldFocus {
    /// The field entity focused last frame, if any.
    last: Option<Entity>,
}

/// The cell entities of one pooled list row, for the bind pass.
#[derive(Component, Debug, Clone, Copy)]
struct DebugRowParts {
    /// The changed-marker column's text node.
    changed_cell: Entity,
    /// The name column's text node.
    name_cell: Entity,
}

/// Marks the Bool editor's checkbox, so [`on_debug_bool_toggle`] recognises
/// its `ValueChange` (the box deliberately carries **no** [`SettingBinding`] —
/// its write target is [`DebugEditorState`], which is dynamic).
#[derive(Component, Debug, Clone, Copy)]
struct DebugBoolCheckbox;

/// Marks every detail-pane text field the blur / Enter commit pass reads.
#[derive(Component, Debug, Clone, Copy)]
struct DebugEditField;

// ---------------------------------------------------------------------------
// The plugin.
// ---------------------------------------------------------------------------

/// Owns the debug-settings floater: the chrome spawn, the deferred content
/// build, the list model, the per-kind detail editor and its commit paths.
#[derive(Debug, Clone, Copy, Default)]
pub struct DebugSettingsPlugin;

impl Plugin for DebugSettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugSettingsModel>()
            .init_resource::<DebugEditorState>()
            .init_resource::<DebugFieldFocus>()
            .add_observer(on_debug_bool_toggle)
            .add_systems(
                Startup,
                spawn_debug_settings_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            // The shortcut stays ungated so it can perform the *first* open.
            .add_systems(Update, toggle_debug_settings_shortcut)
            .add_systems(
                Update,
                (
                    refresh_debug_view.before(layout_virtual_lists),
                    (populate_debug_rows, bind_debug_rows)
                        .chain()
                        .after(layout_virtual_lists),
                    // Commit **before** the selection tracker: a click on
                    // another row blurs a focused field and moves the
                    // selection in the same frame, and the blur commit must
                    // land on the setting the field was editing.
                    (
                        commit_debug_text_fields,
                        track_debug_selection,
                        apply_debug_scope_picks,
                        guard_debug_account_scope,
                        handle_debug_color_picks,
                        sync_debug_detail,
                    )
                        .chain(),
                )
                    .run_if(floater_shown(DEBUG_SETTINGS_FLOATER_ID)),
            );
    }
}

// ---------------------------------------------------------------------------
// Spawn.
// ---------------------------------------------------------------------------

/// Startup: spawn the floater's chrome, hidden; the content is built on first
/// open ([`DeferredFloaterContent`]).
fn spawn_debug_settings_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: DEBUG_SETTINGS_FLOATER_ID,
            title: "Debug settings".to_owned(),
            position: Vec2::new(200.0, 100.0),
            default_size: Some(Vec2::new(720.0, 480.0)),
            min_size: Some(Vec2::new(520.0, 340.0)),
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
        .insert(Translated::new("debug-settings-title"));
    let builder = commands.register_system(build_debug_settings_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// `Ctrl+Alt+Shift+S` opens / closes the editor, the reference viewer's
/// shortcut. Resolved by floater id (not a module resource), since the chrome
/// exists from startup but the content only builds on first open.
fn toggle_debug_settings_shortcut(
    keyboard: Res<ButtonInput<KeyCode>>,
    floaters: Query<(Entity, &Floater)>,
    mut panels: Query<&mut UiPanelShown>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let alt = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    if ctrl && alt && shift && keyboard.just_pressed(KeyCode::KeyS) {
        toggle_floater(&floaters, &mut panels, DEBUG_SETTINGS_FLOATER_ID);
    }
}

/// First-open content build: the left search + list pane, the right detail
/// pane with one editor stack per [`SettingKind`] — ending with the
/// [`DebugSettingsUi`] insert.
fn build_debug_settings_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    // Two panes side by side, filling the content slot.
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                ..row(Val::Px(10.0))
            },
            Name::new("debug-settings:content"),
            ChildOf(handle.content),
        ))
        .id();

    // --- Left pane: search, the changed-only toggle, the list. ---
    let left = commands
        .spawn((
            Node {
                width: Val::Px(LIST_PANE_WIDTH),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                flex_shrink: 0.0,
                ..column(Val::Px(6.0))
            },
            Name::new("debug-settings:list-pane"),
            ChildOf(content),
        ))
        .id();
    let search = spawn_search_field(
        &mut commands,
        left,
        &SearchFieldSpec {
            tab_index: 0,
            font_size: FONT,
            min_width: 200.0,
            placeholder: "Search settings".to_owned(),
            search_glyph: true,
            ..SearchFieldSpec::new("debug-settings")
        },
    );
    if let Some(placeholder) = search.placeholder {
        commands
            .entity(placeholder)
            .insert(Translated::new("debug-settings-search-placeholder"));
    }
    // The changed-only toggle: a *statically* bound checkbox — its setting and
    // scope are fixed, so the generic binding layer covers it entirely.
    let changed_only_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(8.0))
            },
            Name::new("debug-settings:changed-only"),
            ChildOf(left),
        ))
        .id();
    commands.spawn((
        bound_checkbox(SettingBinding::global(SETTING_HIDE_DEFAULT)),
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
        ChildOf(changed_only_row),
    ));
    commands.spawn((
        Text::default(),
        Translated::new("debug-settings-changed-only"),
        UiFont::Sans.at(FONT),
        TextColor(CELL_COLOR),
        Pickable::IGNORE,
        ChildOf(changed_only_row),
    ));
    let table = spawn_table(&mut commands, left, &DEBUG_TABLE);

    // --- Right pane: the detail read-outs and the per-kind editor stacks. ---
    let right = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(6.0))
            },
            Name::new("debug-settings:detail-pane"),
            ChildOf(content),
        ))
        .id();

    // The name row: the selected setting's name beside the copy button.
    let name_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..row(Val::Px(8.0))
            },
            Name::new("debug-settings:name-row"),
            ChildOf(right),
        ))
        .id();
    let name_text = commands
        .spawn((
            Text::default(),
            UiFont::Mono.at(FONT),
            TextColor(CELL_COLOR),
            Name::new("debug-settings:name"),
            ChildOf(name_row),
        ))
        .id();
    let copy_button = crate::preferences::spawn_footer_button(
        &mut commands,
        name_row,
        "debug-settings-copy-name",
        0,
    );
    commands.entity(copy_button).observe(on_copy_setting_name);

    // The comment and type read-outs.
    let comment_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT),
            TextColor(MUTED_COLOR),
            Name::new("debug-settings:comment"),
            ChildOf(right),
        ))
        .id();
    let type_text = spawn_detail_value_row(&mut commands, right, "debug-settings-type");
    let default_value = spawn_detail_value_row(&mut commands, right, "debug-settings-default");
    let global_value = spawn_detail_value_row(&mut commands, right, "debug-settings-global");
    let account_value = spawn_detail_value_row(&mut commands, right, "debug-settings-account");
    let effective_value = spawn_detail_value_row(&mut commands, right, "debug-settings-effective");

    // The scope selector: which layer edits and resets write to.
    let scope_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(8.0))
            },
            Name::new("debug-settings:scope-row"),
            ChildOf(right),
        ))
        .id();
    let scope_label = spawn_detail_label_slot(&mut commands, scope_row);
    commands.spawn((
        Text::default(),
        Translated::new("debug-settings-scope"),
        UiFont::Sans.at(FONT),
        TextColor(MUTED_COLOR),
        Pickable::IGNORE,
        ChildOf(scope_label),
    ));
    let scope_labels = [
        "debug-settings-scope-global".to_owned(),
        "debug-settings-scope-account".to_owned(),
    ];
    let scope_combo = spawn_combo(
        &mut commands,
        scope_row,
        &ComboSpec {
            element: "debug-settings-scope",
            labels: &scope_labels,
            active: 0,
            tab_index: 0,
            font_size: FONT,
            translate_labels: true,
        },
    );

    // The placeholder shown while nothing is selected.
    let placeholder_row = commands
        .spawn((
            Node::default(),
            Name::new("debug-settings:placeholder"),
            ChildOf(right),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("debug-settings-none-selected"),
        UiFont::Sans.at(FONT),
        TextColor(MUTED_COLOR),
        ChildOf(placeholder_row),
    ));

    // --- The per-kind editor stacks, built once and `Display`-toggled. ---

    // Bool: a checkbox (no `SettingBinding` — see `DebugBoolCheckbox`).
    let bool_row = spawn_editor_row(&mut commands, right, "debug-settings:edit:bool");
    let bool_checkbox = commands
        .spawn((
            Checkbox,
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
            DebugBoolCheckbox,
            ChildOf(bool_row),
        ))
        .id();

    // String: a line field.
    let string_row = spawn_editor_row(&mut commands, right, "debug-settings:edit:string");
    let string_field = spawn_editor_field(
        &mut commands,
        string_row,
        "debug-settings-string",
        TextInputKind::Line,
        24.0,
    );

    // The three scalar numerics, one pre-spawned field per kind (a numeric
    // field's kind is fixed at spawn).
    let f32_row = spawn_editor_row(&mut commands, right, "debug-settings:edit:f32");
    let f32_field = spawn_editor_field(
        &mut commands,
        f32_row,
        "debug-settings-f32",
        TextInputKind::Float,
        12.0,
    );
    let i32_row = spawn_editor_row(&mut commands, right, "debug-settings:edit:i32");
    let i32_field = spawn_editor_field(
        &mut commands,
        i32_row,
        "debug-settings-i32",
        TextInputKind::Integer,
        12.0,
    );
    let u32_row = spawn_editor_row(&mut commands, right, "debug-settings:edit:u32");
    let u32_field = spawn_editor_field(
        &mut commands,
        u32_row,
        "debug-settings-u32",
        TextInputKind::NonNegativeInteger,
        12.0,
    );

    // The 3-vector (Vec3 / Vec3d) and rectangle component fields, labelled
    // with their conventional single-letter axis names.
    let vec_row = spawn_editor_row(&mut commands, right, "debug-settings:edit:vec");
    let vec_fields = ["X", "Y", "Z"].map(|axis| {
        spawn_component_label(&mut commands, vec_row, axis);
        spawn_editor_field(
            &mut commands,
            vec_row,
            "debug-settings-vec",
            TextInputKind::Float,
            8.0,
        )
    });
    let rect_row = spawn_editor_row(&mut commands, right, "debug-settings:edit:rect");
    let rect_fields = ["L", "T", "R", "B"].map(|edge| {
        spawn_component_label(&mut commands, rect_row, edge);
        spawn_editor_field(
            &mut commands,
            rect_row,
            "debug-settings-rect",
            TextInputKind::Integer,
            7.0,
        )
    });

    // The colour swatch (Color3 / Color4) and the Color4 alpha field.
    let color_row = spawn_editor_row(&mut commands, right, "debug-settings:edit:color");
    let color_swatch =
        spawn_color_swatch(&mut commands, color_row, "debug-settings", 0, Color::BLACK);
    let alpha_row = spawn_editor_row(&mut commands, right, "debug-settings:edit:alpha");
    spawn_component_label(&mut commands, alpha_row, "A");
    let alpha_field = spawn_editor_field(
        &mut commands,
        alpha_row,
        "debug-settings-alpha",
        TextInputKind::Float,
        8.0,
    );

    // The reset button, trailing.
    let footer = commands
        .spawn((
            Node {
                justify_content: JustifyContent::FlexEnd,
                ..row(Val::Px(8.0))
            },
            Name::new("debug-settings:footer"),
            ChildOf(right),
        ))
        .id();
    let reset_button =
        crate::preferences::spawn_footer_button(&mut commands, footer, "debug-settings-reset", 0);
    commands.entity(reset_button).observe(on_reset_setting);

    commands.insert_resource(DebugSettingsUi {
        search_field: search.field,
        table: table.root,
        viewport: table.viewport,
        name_text,
        comment_text,
        type_text,
        default_value,
        global_value,
        account_value,
        effective_value,
        scope_combo,
        placeholder_row,
        bool_row,
        bool_checkbox,
        string_row,
        string_field,
        f32_row,
        f32_field,
        i32_row,
        i32_field,
        u32_row,
        u32_field,
        vec_row,
        vec_fields,
        rect_row,
        rect_fields,
        color_row,
        color_swatch,
        alpha_row,
        alpha_field,
    });
}

/// Spawn one detail read-out row — a fixed-width translated label beside a
/// value text — returning the value text node.
fn spawn_detail_value_row(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
) -> Entity {
    let row_node = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(8.0))
            },
            Name::new(format!("debug-settings:row:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    let label_slot = spawn_detail_label_slot(commands, row_node);
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT),
        TextColor(MUTED_COLOR),
        Pickable::IGNORE,
        ChildOf(label_slot),
    ));
    commands
        .spawn((
            Text::default(),
            UiFont::Mono.at(FONT),
            TextColor(CELL_COLOR),
            Name::new(format!("debug-settings:value:{label_key}")),
            ChildOf(row_node),
        ))
        .id()
}

/// Spawn a detail row's label **slot**: a plain wrapper carrying the aligned
/// least-width, so the label text inside stays unconstrained (a width bound
/// on a text node itself makes bevy_ui's measure wrap it into a stale box —
/// the "text measure loses width" gotcha).
fn spawn_detail_label_slot(commands: &mut Commands, parent: Entity) -> Entity {
    commands
        .spawn((
            Node {
                min_width: Val::Px(DETAIL_LABEL_WIDTH),
                flex_shrink: 0.0,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id()
}

/// Spawn one editor stack's row container, hidden until its kind is selected.
fn spawn_editor_row(commands: &mut Commands, parent: Entity, name: &'static str) -> Entity {
    commands
        .spawn((
            Node {
                display: Display::None,
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new(name),
            ChildOf(parent),
        ))
        .id()
}

/// Spawn one commit-tracked editor text field.
fn spawn_editor_field(
    commands: &mut Commands,
    parent: Entity,
    element: &'static str,
    kind: TextInputKind,
    width_glyphs: f32,
) -> Entity {
    let field = spawn_text_input(
        commands,
        parent,
        &TextInputSpec {
            font_size: FONT,
            width_glyphs,
            ..TextInputSpec::new(element, kind)
        },
    );
    commands.entity(field).insert(DebugEditField);
    field
}

/// Spawn a component field's single-letter lead-in label (an axis / edge
/// name, deliberately untranslated — a technical symbol, as in the
/// reference's spinner labels).
fn spawn_component_label(commands: &mut Commands, parent: Entity, letter: &'static str) {
    commands.spawn((
        Text::new(letter),
        UiFont::Sans.at(FONT),
        TextColor(MUTED_COLOR),
        Pickable::IGNORE,
        ChildOf(parent),
    ));
}

// ---------------------------------------------------------------------------
// The list model.
// ---------------------------------------------------------------------------

/// Every setting the editor lists, in the store's sorted-name order —
/// skipping declarations marked [`SettingDecl::editor_hidden`] (mechanical UI
/// state: window geometry, tab splits, table sort orders), exactly as the
/// reference enumeration skips `isHiddenFromSettingsEditor` controls.
fn build_entries(store: &sl_settings::SettingsStore) -> Vec<DebugEntry> {
    store
        .names()
        .filter_map(|name| {
            let decl = store.declaration(name)?;
            if decl.editor_hidden() {
                return None;
            }
            Some(DebugEntry {
                name_lower: name.to_lowercase(),
                comment_lower: decl.comment().to_lowercase(),
                name: name.to_owned(),
            })
        })
        .collect()
}

/// The view over `entries` for a lowercased filter `term` and the changed-only
/// toggle: the indices of the matching entries, in the entries' (sorted-name)
/// order. Pure, so the filter behaviour is unit-testable.
fn build_view(
    entries: &[DebugEntry],
    term: &str,
    hide_default: bool,
    overridden: impl Fn(&str) -> bool,
) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            term.is_empty() || entry.name_lower.contains(term) || entry.comment_lower.contains(term)
        })
        .filter(|(_, entry)| !hide_default || overridden(&entry.name))
        .map(|(index, _)| index)
        .collect()
}

/// Rebuild [`DebugSettingsModel`] when its inputs move: enumerate the store
/// once (the declarations are fixed at startup), then re-derive the view
/// whenever the search term changes or the store moves (an override appearing
/// or vanishing feeds both the changed-only filter and the `*` markers).
/// Keeps the viewport's [`VirtualList::item_count`] current and drops a
/// selection the filter removed.
fn refresh_debug_view(
    ui: Option<Res<DebugSettingsUi>>,
    settings: Option<Res<ViewerSettings>>,
    mut model: ResMut<DebugSettingsModel>,
    mut state: ResMut<DebugEditorState>,
    fields: Query<&EditableText>,
    mut lists: Query<&mut VirtualList>,
    mut tables: Query<&mut TableState>,
) {
    let (Some(ui), Some(settings)) = (ui, settings) else {
        return;
    };
    let term = fields
        .get(ui.search_field)
        .map(|field| field.value().to_string().trim().to_lowercase())
        .unwrap_or_default();
    let term_changed = model.filter != term;
    let rebuild_entries = model.entries.is_empty();
    if !term_changed && !rebuild_entries && !settings.is_changed() {
        return;
    }
    if rebuild_entries {
        model.entries = build_entries(settings.store());
    }
    if term_changed {
        model.filter.clone_from(&term);
    }
    let hide_default = settings
        .store()
        .get_bool(SETTING_HIDE_DEFAULT)
        .unwrap_or(false);
    let store = settings.store();
    let view = build_view(&model.entries, &term, hide_default, |name| {
        store.is_overridden(name)
    });
    if let Ok(mut list) = lists.get_mut(ui.viewport) {
        if list.item_count != view.len() {
            list.item_count = view.len();
        }
        if term_changed {
            list.scroll_to_top();
        }
    }
    // A selection the filter removed is dropped, so the detail pane never
    // edits a setting the list no longer shows.
    if let Some(selected) = state.selected.clone() {
        let still_visible = view.iter().any(|index| {
            model
                .entries
                .get(*index)
                .is_some_and(|e| e.name == selected)
        });
        if !still_visible {
            if let Ok(mut table) = tables.get_mut(ui.table) {
                table.clear_selection();
            }
            state.selected = None;
        }
    }
    if model.view != view {
        model.view = view;
    }
}

/// Give each freshly-pooled viewport row its table cells and the
/// [`DebugRowParts`] wiring.
fn populate_debug_rows(
    mut commands: Commands,
    ui: Option<Res<DebugSettingsUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        let cells = spawn_table_row(&mut commands, row_entity, ui.table, &DEBUG_TABLE);
        let (Some(changed_cell), Some(name_cell)) = (cells.cell(0), cells.cell(1)) else {
            continue;
        };
        commands.entity(row_entity).insert(DebugRowParts {
            changed_cell,
            name_cell,
        });
    }
}

/// Project the view into the pooled rows: the `*` changed marker and the
/// setting name. Re-runs per row on a window move, and for every row on a
/// model rebuild or a store change (an override toggling moves the marker
/// without changing the view).
fn bind_debug_rows(
    model: Res<DebugSettingsModel>,
    ui: Option<Res<DebugSettingsUi>>,
    settings: Option<Res<ViewerSettings>>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &DebugRowParts)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let (Some(ui), Some(settings)) = (ui, settings) else {
        return;
    };
    let refresh_all = model.is_changed() || settings.is_changed();
    for (row, child_of, parts) in &rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let Some(index) = row.index else {
            continue;
        };
        let Some(entry) = model
            .view
            .get(index)
            .and_then(|entry_index| model.entries.get(*entry_index))
        else {
            continue;
        };
        let marker = if settings.store().is_overridden(&entry.name) {
            CHANGED_MARK
        } else {
            ""
        };
        set_table_cell(&mut texts, parts.changed_cell, marker, CELL_COLOR);
        set_table_cell(&mut texts, parts.name_cell, &entry.name, CELL_COLOR);
    }
}

/// Follow the table's selection into [`DebugEditorState::selected`], mapping
/// the view index back to the setting name.
fn track_debug_selection(
    ui: Option<Res<DebugSettingsUi>>,
    tables: Query<&TableState>,
    model: Res<DebugSettingsModel>,
    mut state: ResMut<DebugEditorState>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Ok(table) = tables.get(ui.table) else {
        return;
    };
    if table.selection_revision() == state.last_selection_revision {
        return;
    }
    let selected = table
        .primary_selected()
        .and_then(|view_index| model.view.get(view_index))
        .and_then(|entry_index| model.entries.get(*entry_index))
        .map(|entry| entry.name.clone());
    state.last_selection_revision = table.selection_revision();
    if state.selected != selected {
        state.selected = selected;
    }
}

// ---------------------------------------------------------------------------
// The scope selector and its account guard.
// ---------------------------------------------------------------------------

/// A user pick on the scope combo moves the edit target between the Global
/// and Account layers. (Programmatic [`ComboSelection`] writes emit no
/// [`ComboChanged`], so the guard's snap-back never loops through here.)
fn apply_debug_scope_picks(
    mut changes: MessageReader<ComboChanged>,
    ui: Option<Res<DebugSettingsUi>>,
    mut state: ResMut<DebugEditorState>,
) {
    let Some(ui) = ui else {
        return;
    };
    for change in changes.read() {
        if change.combo != ui.scope_combo {
            continue;
        }
        let scope = if change.active == 1 {
            Scope::Account
        } else {
            Scope::Global
        };
        if state.scope != scope {
            state.scope = scope;
        }
    }
}

/// Disable the scope selector until the account scope loads at login (an
/// account edit before that could not be persisted), snapping the edit target
/// back to Global meanwhile — the preferences floater's account-guard idiom.
fn guard_debug_account_scope(
    ui: Option<Res<DebugSettingsUi>>,
    settings: Option<Res<ViewerSettings>>,
    mut state: ResMut<DebugEditorState>,
    mut combos: Query<(Has<InteractionDisabled>, &mut ComboSelection)>,
    mut commands: Commands,
) {
    let (Some(ui), Some(settings)) = (ui, settings) else {
        return;
    };
    let Ok((disabled, mut selection)) = combos.get_mut(ui.scope_combo) else {
        return;
    };
    let want_disabled = !settings.account_loaded();
    if want_disabled && !disabled {
        commands.entity(ui.scope_combo).insert(InteractionDisabled);
    } else if !want_disabled && disabled {
        commands
            .entity(ui.scope_combo)
            .remove::<InteractionDisabled>();
    }
    if want_disabled {
        if state.scope != Scope::Global {
            state.scope = Scope::Global;
        }
        if selection.active != 0 {
            selection.active = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// The detail pane: read-outs, editor-stack switching, and seeding.
// ---------------------------------------------------------------------------

/// A [`SettingKind`]'s display name — a technical type label, shown verbatim.
const fn kind_label(kind: SettingKind) -> &'static str {
    match kind {
        SettingKind::Bool => "Bool",
        SettingKind::I32 => "I32",
        SettingKind::U32 => "U32",
        SettingKind::F32 => "F32",
        SettingKind::String => "String",
        SettingKind::Color3 => "Color3",
        SettingKind::Color4 => "Color4",
        SettingKind::Vec3 => "Vec3",
        SettingKind::Vec3d => "Vec3d",
        SettingKind::Rect => "Rect",
    }
}

/// A setting value's display form for the detail read-outs.
fn format_setting_value(value: &SettingValue) -> String {
    match value {
        SettingValue::Bool(v) => v.to_string(),
        SettingValue::I32(v) => v.to_string(),
        SettingValue::U32(v) => v.to_string(),
        SettingValue::F32(v) => v.to_string(),
        SettingValue::String(v) => v.clone(),
        SettingValue::Color3([r, g, b]) | SettingValue::Vec3([r, g, b]) => {
            format!("({r}, {g}, {b})")
        }
        SettingValue::Color4([r, g, b, a]) => format!("({r}, {g}, {b}, {a})"),
        SettingValue::Vec3d([x, y, z]) => format!("({x}, {y}, {z})"),
        SettingValue::Rect([l, t, r, b]) => format!("({l}, {t}, {r}, {b})"),
    }
}

/// Set a read-out label's text in place, change-guarded.
fn set_label(texts: &mut Query<&mut Text>, entity: Entity, value: &str) {
    if let Ok(mut text) = texts.get_mut(entity)
        && text.0 != value
    {
        value.clone_into(&mut text.0);
    }
}

/// Seed an editor text field from the store, skipping the focused or
/// IME-composing field so an active edit is never clobbered (the
/// [`crate::settings_binding`] text-sync guard).
#[expect(
    clippy::cmp_owned,
    reason = "the editor's SplitString has no borrow-free comparison against &str; the guard \
              keeps the pass write-free when nothing changed"
)]
fn seed_field(
    editables: &mut Query<&mut EditableText>,
    focused: Option<Entity>,
    entity: Entity,
    want: &str,
) {
    let Ok(mut editable) = editables.get_mut(entity) else {
        return;
    };
    if focused == Some(entity) || editable.is_composing() {
        return;
    }
    if editable.value().to_string() != want {
        editable.editor_mut().set_text(want);
    }
}

/// Seed the colour swatch from the store, within the picker's quantisation
/// epsilon so a byte round-trip does not thrash the fill repaint.
fn seed_swatch(swatches: &mut Query<&mut ColorSwatchValue>, entity: Entity, rgb: [f32; 3]) {
    let Ok(mut value) = swatches.get_mut(entity) else {
        return;
    };
    let current = value.0.to_srgba();
    let [red, green, blue] = rgb;
    let agrees = (current.red - red).abs() <= COLOR_SEED_EPSILON
        && (current.green - green).abs() <= COLOR_SEED_EPSILON
        && (current.blue - blue).abs() <= COLOR_SEED_EPSILON;
    if !agrees {
        value.0 = Color::srgb(red, green, blue);
    }
}

/// Keep the detail pane following the selection and the store: the name /
/// comment / type read-outs, the four per-layer value read-outs, which editor
/// stack is visible, and the visible editors' seeded values. Change-gated on
/// the editor state and the store, so a quiet frame writes nothing.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the retained \
              entities, the editor state, the store, the focus guard, and one query per \
              widget family the pane seeds"
)]
fn sync_debug_detail(
    ui: Option<Res<DebugSettingsUi>>,
    state: Res<DebugEditorState>,
    settings: Option<Res<ViewerSettings>>,
    focus: Option<Res<InputFocus>>,
    checkboxes: Query<Has<Checked>>,
    mut texts: Query<&mut Text>,
    mut nodes: Query<&mut Node>,
    mut editables: Query<&mut EditableText>,
    mut swatches: Query<&mut ColorSwatchValue>,
    mut commands: Commands,
) {
    let (Some(ui), Some(settings)) = (ui, settings) else {
        return;
    };
    if !state.is_changed() && !settings.is_changed() {
        return;
    }
    let store = settings.store();
    let selected = state
        .selected
        .as_deref()
        .and_then(|name| store.declaration(name).map(|decl| (name, decl)));

    // The read-outs.
    let override_line = |scope: Scope, name: &str| {
        store
            .get_override(scope, name)
            .map_or_else(|| NO_OVERRIDE.to_owned(), format_setting_value)
    };
    match selected {
        Some((name, decl)) => {
            let type_line = if decl.persist() {
                kind_label(decl.kind()).to_owned()
            } else {
                format!("{} (transient)", kind_label(decl.kind()))
            };
            set_label(&mut texts, ui.name_text, name);
            set_label(&mut texts, ui.comment_text, decl.comment());
            set_label(&mut texts, ui.type_text, &type_line);
            set_label(
                &mut texts,
                ui.default_value,
                &format_setting_value(decl.default()),
            );
            set_label(
                &mut texts,
                ui.global_value,
                &override_line(Scope::Global, name),
            );
            set_label(
                &mut texts,
                ui.account_value,
                &override_line(Scope::Account, name),
            );
            set_label(
                &mut texts,
                ui.effective_value,
                &store
                    .get(name)
                    .map_or_else(String::new, format_setting_value),
            );
        }
        None => {
            for entity in [
                ui.name_text,
                ui.comment_text,
                ui.type_text,
                ui.default_value,
                ui.global_value,
                ui.account_value,
                ui.effective_value,
            ] {
                set_label(&mut texts, entity, "");
            }
        }
    }

    // The editor-stack switch: exactly the selected kind's rows are visible.
    let kind = selected.map(|(_, decl)| decl.kind());
    let stacks = [
        (ui.placeholder_row, kind.is_none()),
        (ui.bool_row, kind == Some(SettingKind::Bool)),
        (ui.string_row, kind == Some(SettingKind::String)),
        (ui.f32_row, kind == Some(SettingKind::F32)),
        (ui.i32_row, kind == Some(SettingKind::I32)),
        (ui.u32_row, kind == Some(SettingKind::U32)),
        (
            ui.vec_row,
            matches!(kind, Some(SettingKind::Vec3 | SettingKind::Vec3d)),
        ),
        (ui.rect_row, kind == Some(SettingKind::Rect)),
        (
            ui.color_row,
            matches!(kind, Some(SettingKind::Color3 | SettingKind::Color4)),
        ),
        (ui.alpha_row, kind == Some(SettingKind::Color4)),
    ];
    for (row_entity, wanted) in stacks {
        if let Ok(mut node) = nodes.get_mut(row_entity) {
            let display = if wanted { Display::Flex } else { Display::None };
            if node.display != display {
                node.display = display;
            }
        }
    }

    // Seed the visible editors from the **effective** value (whichever layer
    // it resolves through); the scope selector only chooses the write target.
    let focused = focus.as_ref().and_then(|focus| focus.get());
    let Some((name, _decl)) = selected else {
        return;
    };
    let Some(value) = store.get(name) else {
        return;
    };
    match value.clone() {
        SettingValue::Bool(want) => {
            if let Ok(checked) = checkboxes.get(ui.bool_checkbox)
                && checked != want
            {
                if want {
                    commands.entity(ui.bool_checkbox).insert(Checked);
                } else {
                    commands.entity(ui.bool_checkbox).remove::<Checked>();
                }
            }
        }
        SettingValue::String(want) => seed_field(&mut editables, focused, ui.string_field, &want),
        SettingValue::F32(v) => seed_field(&mut editables, focused, ui.f32_field, &v.to_string()),
        SettingValue::I32(v) => seed_field(&mut editables, focused, ui.i32_field, &v.to_string()),
        SettingValue::U32(v) => seed_field(&mut editables, focused, ui.u32_field, &v.to_string()),
        SettingValue::Vec3(components) => {
            for (entity, component) in ui.vec_fields.iter().zip(components) {
                seed_field(&mut editables, focused, *entity, &component.to_string());
            }
        }
        SettingValue::Vec3d(components) => {
            for (entity, component) in ui.vec_fields.iter().zip(components) {
                seed_field(&mut editables, focused, *entity, &component.to_string());
            }
        }
        SettingValue::Rect(edges) => {
            for (entity, edge) in ui.rect_fields.iter().zip(edges) {
                seed_field(&mut editables, focused, *entity, &edge.to_string());
            }
        }
        SettingValue::Color3(rgb) => seed_swatch(&mut swatches, ui.color_swatch, rgb),
        SettingValue::Color4([red, green, blue, alpha]) => {
            seed_swatch(&mut swatches, ui.color_swatch, [red, green, blue]);
            seed_field(&mut editables, focused, ui.alpha_field, &alpha.to_string());
        }
    }
}

// ---------------------------------------------------------------------------
// The commit paths: widget → store, at the selected scope.
// ---------------------------------------------------------------------------

/// Observer: the Bool editor's checkbox was toggled — reflect its `Checked`
/// state at once (the binding layer's reconcile idiom) and write the bool to
/// the selected setting at the selected scope.
fn on_debug_bool_toggle(
    change: On<ValueChange<bool>>,
    boxes: Query<(), With<DebugBoolCheckbox>>,
    state: Res<DebugEditorState>,
    settings: Option<ResMut<ViewerSettings>>,
    mut commands: Commands,
) {
    if boxes.get(change.source).is_err() {
        return;
    }
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    let (Some(name), Some(mut settings)) = (state.selected.as_deref(), settings) else {
        return;
    };
    // Only write while a Bool setting is actually selected — a stale toggle
    // against a re-selected setting of another kind must not type-error.
    if settings
        .store()
        .declaration(name)
        .is_some_and(|decl| decl.kind() == SettingKind::Bool)
    {
        settings.set(state.scope, name, SettingValue::Bool(change.value));
    }
}

/// Narrow a committed field's `f64` to the `f32` an `F32` setting stores.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "f64 → f32 at the field boundary: the committed number is the user's typed value \
              for an f32-typed setting, so the narrowing is the intended precision"
)]
const fn f64_to_f32(value: f64) -> f32 {
    value as f32
}

/// One field's committed float, or `None` while its text is not a complete
/// number.
fn parsed_f64(
    fields: &Query<&EditableText, With<DebugEditField>>,
    entity: Entity,
    kind: TextInputKind,
) -> Option<f64> {
    let editor = fields.get(entity).ok()?;
    match kind.parse(&editor.value().to_string())? {
        TextInputValue::Float(value) => Some(value),
        TextInputValue::Integer(_) | TextInputValue::Unsigned(_) => None,
    }
}

/// One field's committed signed integer, or `None` while incomplete.
fn parsed_i64(fields: &Query<&EditableText, With<DebugEditField>>, entity: Entity) -> Option<i64> {
    let editor = fields.get(entity).ok()?;
    match TextInputKind::Integer.parse(&editor.value().to_string())? {
        TextInputValue::Integer(value) => Some(value),
        TextInputValue::Float(_) | TextInputValue::Unsigned(_) => None,
    }
}

/// One field's committed unsigned integer, or `None` while incomplete.
fn parsed_u64(fields: &Query<&EditableText, With<DebugEditField>>, entity: Entity) -> Option<u64> {
    let editor = fields.get(entity).ok()?;
    match TextInputKind::NonNegativeInteger.parse(&editor.value().to_string())? {
        TextInputValue::Unsigned(value) => Some(value),
        TextInputValue::Float(_) | TextInputValue::Integer(_) => None,
    }
}

/// Assemble the selected kind's [`SettingValue`] from the visible editor
/// fields, or `None` when any field is incomplete / out of range (the commit
/// is then abandoned; the next seed pass restores the display) — or when the
/// kind has no text-committed editor (Bool and Color3 commit through their
/// own observers).
fn assemble_field_value(
    kind: SettingKind,
    ui: &DebugSettingsUi,
    fields: &Query<&EditableText, With<DebugEditField>>,
    swatches: &Query<&ColorSwatchValue>,
) -> Option<SettingValue> {
    match kind {
        SettingKind::Bool | SettingKind::Color3 => None,
        SettingKind::F32 => Some(SettingValue::F32(f64_to_f32(parsed_f64(
            fields,
            ui.f32_field,
            TextInputKind::Float,
        )?))),
        SettingKind::I32 => Some(SettingValue::I32(
            i32::try_from(parsed_i64(fields, ui.i32_field)?).ok()?,
        )),
        SettingKind::U32 => Some(SettingValue::U32(
            u32::try_from(parsed_u64(fields, ui.u32_field)?).ok()?,
        )),
        SettingKind::String => {
            let editor = fields.get(ui.string_field).ok()?;
            Some(SettingValue::String(editor.value().to_string()))
        }
        SettingKind::Vec3 => {
            let [x, y, z] = ui.vec_fields;
            Some(SettingValue::Vec3([
                f64_to_f32(parsed_f64(fields, x, TextInputKind::Float)?),
                f64_to_f32(parsed_f64(fields, y, TextInputKind::Float)?),
                f64_to_f32(parsed_f64(fields, z, TextInputKind::Float)?),
            ]))
        }
        SettingKind::Vec3d => {
            let [x, y, z] = ui.vec_fields;
            Some(SettingValue::Vec3d([
                parsed_f64(fields, x, TextInputKind::Float)?,
                parsed_f64(fields, y, TextInputKind::Float)?,
                parsed_f64(fields, z, TextInputKind::Float)?,
            ]))
        }
        SettingKind::Rect => {
            let [left, top, right, bottom] = ui.rect_fields;
            Some(SettingValue::Rect([
                i32::try_from(parsed_i64(fields, left)?).ok()?,
                i32::try_from(parsed_i64(fields, top)?).ok()?,
                i32::try_from(parsed_i64(fields, right)?).ok()?,
                i32::try_from(parsed_i64(fields, bottom)?).ok()?,
            ]))
        }
        SettingKind::Color4 => {
            let srgba = swatches.get(ui.color_swatch).ok()?.0.to_srgba();
            let alpha = f64_to_f32(parsed_f64(fields, ui.alpha_field, TextInputKind::Float)?)
                .clamp(0.0, 1.0);
            Some(SettingValue::Color4([
                srgba.red,
                srgba.green,
                srgba.blue,
                alpha,
            ]))
        }
    }
}

/// Commit the visible editor fields on `Enter` or focus loss: parse every
/// field of the selected kind's stack, assemble the [`SettingValue`], and
/// write it to the selected scope. Any incomplete field abandons the commit.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the retained \
              entities, the editor state, the focus / keyboard commit triggers, the field and \
              swatch reads, and the store"
)]
fn commit_debug_text_fields(
    ui: Option<Res<DebugSettingsUi>>,
    state: Res<DebugEditorState>,
    focus: Option<Res<InputFocus>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut focus_track: ResMut<DebugFieldFocus>,
    fields: Query<&EditableText, With<DebugEditField>>,
    swatches: Query<&ColorSwatchValue>,
    settings: Option<ResMut<ViewerSettings>>,
) {
    let Some(ui) = ui else {
        return;
    };
    let focused_field = focus
        .as_ref()
        .and_then(|focus| focus.get())
        .filter(|entity| fields.contains(*entity));
    let enter =
        keyboard.just_pressed(KeyCode::Enter) || keyboard.just_pressed(KeyCode::NumpadEnter);
    let commit = if enter {
        focused_field
    } else if focus_track.last != focused_field {
        focus_track.last.filter(|entity| fields.contains(*entity))
    } else {
        None
    };
    focus_track.last = focused_field;
    if commit.is_none() {
        return;
    }
    let Some(name) = state.selected.as_deref() else {
        return;
    };
    let Some(mut settings) = settings else {
        return;
    };
    let Some(kind) = settings.store().declaration(name).map(SettingDecl::kind) else {
        return;
    };
    let Some(value) = assemble_field_value(kind, &ui, &fields, &swatches) else {
        return;
    };
    settings.set(state.scope, name, value);
}

/// Every [`ColorPicked`] reply to the editor's swatch writes the colour to
/// the selected setting at the selected scope — the live drag, OK, and the
/// picker Cancel's original-colour re-emit alike (a self-reverting write, the
/// binding layer's model). A `Color4` merges the alpha field's committed
/// value, falling back to the current effective alpha while it is incomplete.
fn handle_debug_color_picks(
    mut picks: MessageReader<ColorPicked>,
    ui: Option<Res<DebugSettingsUi>>,
    state: Res<DebugEditorState>,
    fields: Query<&EditableText, With<DebugEditField>>,
    settings: Option<ResMut<ViewerSettings>>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Some(mut settings) = settings else {
        return;
    };
    for pick in picks.read() {
        if pick.requester != ui.color_swatch {
            continue;
        }
        let Some(name) = state.selected.as_deref() else {
            continue;
        };
        let Some(kind) = settings.store().declaration(name).map(SettingDecl::kind) else {
            continue;
        };
        let srgba = pick.color.to_srgba();
        let value = match kind {
            SettingKind::Color3 => SettingValue::Color3([srgba.red, srgba.green, srgba.blue]),
            SettingKind::Color4 => {
                let alpha = parsed_f64(&fields, ui.alpha_field, TextInputKind::Float)
                    .map(f64_to_f32)
                    .or_else(|| {
                        settings
                            .store()
                            .get(name)
                            .and_then(SettingValue::as_color4)
                            .map(|[_, _, _, alpha]| alpha)
                    })
                    .unwrap_or(1.0)
                    .clamp(0.0, 1.0);
                SettingValue::Color4([srgba.red, srgba.green, srgba.blue, alpha])
            }
            _other => continue,
        };
        settings.set(state.scope, name, value);
    }
}

/// Observer: the reset button — drop the selected setting's override at the
/// selected scope, reverting it to the layer below.
fn on_reset_setting(
    _activate: On<Activate>,
    state: Res<DebugEditorState>,
    settings: Option<ResMut<ViewerSettings>>,
) {
    let (Some(name), Some(mut settings)) = (state.selected.as_deref(), settings) else {
        return;
    };
    settings.reset(state.scope, name);
}

/// Observer: the copy button — put the selected setting's name on the OS
/// clipboard (the reference's Firestorm-added copy affordance).
fn on_copy_setting_name(
    _activate: On<Activate>,
    state: Res<DebugEditorState>,
    clipboard: Option<Res<ViewerClipboard>>,
) {
    if let (Some(name), Some(clipboard)) = (state.selected.as_deref(), clipboard) {
        copy_to_clipboard(&clipboard, name);
    }
}

// ---------------------------------------------------------------------------
// The gallery specimen.
// ---------------------------------------------------------------------------

/// The static debug-settings-editor specimen for the gallery / headless
/// harness: the search box, a short stand-in settings list with a changed
/// marker, and the detail column with a scope combo, a numeric field and the
/// two buttons — the layout, with none of the live behaviour (per the element
/// registry's rule: no plugin, no store, no observers).
pub fn spawn_debug_settings_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    // The card is content-sized with a floor (the quick-preferences specimen
    // idiom): it grows to its widest row, so no text is ever squeezed into
    // wrapping inside a stale measured box.
    let card = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(10.0)),
                min_width: Val::Px(520.0),
                ..row(Val::Px(12.0))
            },
            Name::new("debug-settings-specimen"),
            ChildOf(parent),
        ))
        .id();

    // The left pane: search over a static three-row list.
    let left = commands
        .spawn((
            Node {
                ..column(Val::Px(6.0))
            },
            ChildOf(card),
        ))
        .id();
    spawn_search_field(
        commands,
        left,
        &SearchFieldSpec {
            font_size: cx.font_size,
            search_glyph: true,
            ..SearchFieldSpec::new("debug-settings-specimen")
        },
    );
    let list_rows: [(&str, bool); 3] = [
        ("AudioMasterVolume", true),
        ("MiniMapRotate", false),
        ("ShowPropertyLines", false),
    ];
    for (name, changed) in list_rows {
        let list_row = commands
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    ..row(Val::Px(6.0))
                },
                ChildOf(left),
            ))
            .id();
        // The marker is a glyph, not prose — deliberately untranslated, so the
        // fixed marker column never has to fit a long translation.
        commands.spawn((
            Text::new(if changed { CHANGED_MARK } else { "" }),
            cx.font(UiFont::Mono),
            TextColor(CELL_COLOR),
            Node {
                width: Val::Px(CHANGED_COL_WIDTH),
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(list_row),
        ));
        commands.spawn((
            Text::new(cx.text(name)),
            cx.font(UiFont::Mono),
            TextColor(CELL_COLOR),
            ChildOf(list_row),
        ));
    }

    // The right pane: the read-outs, scope combo, a numeric editor stand-in
    // and the buttons.
    let right = commands
        .spawn((
            Node {
                ..column(Val::Px(6.0))
            },
            ChildOf(card),
        ))
        .id();
    commands.spawn((
        Text::new(cx.text("AudioMasterVolume")),
        cx.font(UiFont::Mono),
        TextColor(CELL_COLOR),
        ChildOf(right),
    ));
    commands.spawn((
        Text::new(cx.text("Master audio level")),
        cx.font(UiFont::Sans),
        TextColor(MUTED_COLOR),
        ChildOf(right),
    ));
    let detail_rows: [(&str, &str); 4] = [
        ("Type", "F32"),
        ("Default", "1"),
        ("Global", "0.5"),
        ("Effective", "0.5"),
    ];
    for (label, value) in detail_rows {
        let detail_row = commands
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    ..row(Val::Px(8.0))
                },
                ChildOf(right),
            ))
            .id();
        let label_slot = commands
            .spawn((
                Node {
                    min_width: Val::Px(DETAIL_LABEL_WIDTH),
                    flex_shrink: 0.0,
                    ..default()
                },
                ChildOf(detail_row),
            ))
            .id();
        commands.spawn((
            Text::new(cx.text(label)),
            cx.font(UiFont::Sans),
            TextColor(MUTED_COLOR),
            ChildOf(label_slot),
        ));
        commands.spawn((
            Text::new(cx.text(value)),
            cx.font(UiFont::Mono),
            TextColor(CELL_COLOR),
            ChildOf(detail_row),
        ));
    }
    let scope_labels = [cx.text("Global"), cx.text("Account")];
    spawn_combo(
        commands,
        right,
        &ComboSpec {
            element: "debug-settings-specimen-scope",
            labels: &scope_labels,
            active: 0,
            tab_index: 0,
            font_size: cx.font_size,
            translate_labels: false,
        },
    );
    spawn_text_input(
        commands,
        right,
        &TextInputSpec {
            initial: "0.5".to_owned(),
            font_size: cx.font_size,
            width_glyphs: 12.0,
            ..TextInputSpec::new("debug-settings-specimen-value", TextInputKind::Float)
        },
    );
    let buttons = commands
        .spawn((
            Node {
                justify_content: JustifyContent::FlexEnd,
                ..row(Val::Px(8.0))
            },
            ChildOf(right),
        ))
        .id();
    for label in ["Copy name", "Reset to default"] {
        let button = commands
            .spawn((
                Node {
                    padding: UiRect::axes(Val::Px(14.0), Val::Px(5.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BorderColor::all(CONTROL_BORDER),
                BackgroundColor(Color::srgb(0.16, 0.19, 0.25)),
                ChildOf(buttons),
            ))
            .id();
        commands.spawn((
            Text::new(cx.text(label)),
            cx.font(UiFont::Sans),
            TextColor(CELL_COLOR),
            ChildOf(button),
        ));
    }

    card
}

#[cfg(test)]
mod tests {
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::prelude::*;
    use bevy::text::EditableText;
    use bevy::ui::InteractionDisabled;
    use bevy::ui_widgets::{Activate, Checkbox, ValueChange};
    use pretty_assertions::assert_eq;
    use sl_settings::{Scope, SettingValue, SettingsStore};

    use super::{
        DebugBoolCheckbox, DebugEditField, DebugEditorState, DebugEntry, DebugFieldFocus,
        DebugSettingsUi, NO_OVERRIDE, build_entries, build_view, commit_debug_text_fields,
        format_setting_value, guard_debug_account_scope, on_debug_bool_toggle, on_reset_setting,
        sync_debug_detail,
    };
    use crate::settings::ViewerSettings;
    use crate::ui_combo::ComboSelection;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A headless app over a store populated by `register`; each test adds
    /// the systems / observers it exercises.
    fn app(register: impl FnOnce(&mut SettingsStore)) -> App {
        let mut store = SettingsStore::new();
        register(&mut store);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(ViewerSettings::from_store_for_test(store))
            .init_resource::<DebugEditorState>()
            .init_resource::<DebugFieldFocus>();
        app
    }

    /// A [`DebugSettingsUi`] whose every entity is the placeholder — a test
    /// overrides just the entities its scenario spawns; the systems skip the
    /// rest (a placeholder never resolves in a query).
    fn placeholder_ui() -> DebugSettingsUi {
        let entity = Entity::PLACEHOLDER;
        DebugSettingsUi {
            search_field: entity,
            table: entity,
            viewport: entity,
            name_text: entity,
            comment_text: entity,
            type_text: entity,
            default_value: entity,
            global_value: entity,
            account_value: entity,
            effective_value: entity,
            scope_combo: entity,
            placeholder_row: entity,
            bool_row: entity,
            bool_checkbox: entity,
            string_row: entity,
            string_field: entity,
            f32_row: entity,
            f32_field: entity,
            i32_row: entity,
            i32_field: entity,
            u32_row: entity,
            u32_field: entity,
            vec_row: entity,
            vec_fields: [entity; 3],
            rect_row: entity,
            rect_fields: [entity; 4],
            color_row: entity,
            color_swatch: entity,
            alpha_row: entity,
            alpha_field: entity,
        }
    }

    /// A read-only view of the store under test.
    fn store(app: &App) -> &SettingsStore {
        app.world().resource::<ViewerSettings>().store()
    }

    /// An entry whose lowercased match targets are derived, as the refresh
    /// builds them.
    fn entry(name: &str, comment: &str) -> DebugEntry {
        DebugEntry {
            name: name.to_owned(),
            name_lower: name.to_lowercase(),
            comment_lower: comment.to_lowercase(),
        }
    }

    /// Every [`SettingValue`] variant has a display form, and the interesting
    /// ones read as expected.
    #[test]
    fn format_setting_value_covers_every_kind() {
        assert_eq!(format_setting_value(&SettingValue::Bool(true)), "true");
        assert_eq!(format_setting_value(&SettingValue::I32(-7)), "-7");
        assert_eq!(format_setting_value(&SettingValue::U32(42)), "42");
        assert_eq!(format_setting_value(&SettingValue::F32(2.5)), "2.5");
        assert_eq!(
            format_setting_value(&SettingValue::String("hi".to_owned())),
            "hi"
        );
        assert_eq!(
            format_setting_value(&SettingValue::Color3([0.0, 0.5, 1.0])),
            "(0, 0.5, 1)"
        );
        assert_eq!(
            format_setting_value(&SettingValue::Color4([0.0, 0.5, 1.0, 0.25])),
            "(0, 0.5, 1, 0.25)"
        );
        assert_eq!(
            format_setting_value(&SettingValue::Vec3([1.0, 2.0, 3.0])),
            "(1, 2, 3)"
        );
        assert_eq!(
            format_setting_value(&SettingValue::Vec3d([1.5, 2.5, 3.5])),
            "(1.5, 2.5, 3.5)"
        );
        assert_eq!(
            format_setting_value(&SettingValue::Rect([1, 2, 3, 4])),
            "(1, 2, 3, 4)"
        );
    }

    /// The list enumerates every registered setting in sorted order, but
    /// skips editor-hidden declarations — the mechanical UI-state keys
    /// (floater geometry, table sort orders) the persistence layers register.
    #[test]
    fn entries_skip_editor_hidden_settings() {
        let mut store = SettingsStore::new();
        store
            .register("AudioMasterVolume", SettingValue::F32(1.0), "master level")
            .ok();
        store
            .register_hidden_in(
                &["floater"],
                "inventory_rect",
                SettingValue::Rect([0, 0, 0, 0]),
                "Window rectangle",
            )
            .ok();
        let entries = build_entries(&store);
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, vec!["AudioMasterVolume"]);
    }

    /// The view matches the term against the name *and* the comment,
    /// case-insensitively, preserving the entries' order; an empty term
    /// presents everything.
    #[test]
    fn view_filters_by_name_and_comment() {
        let entries = vec![
            entry("AudioMasterVolume", "Master audio level"),
            entry("MiniMapRotate", "Rotate the mini-map with the camera"),
            entry("ShowPropertyLines", "Draw parcel property lines"),
        ];
        assert_eq!(build_view(&entries, "", false, |_| false), vec![0, 1, 2]);
        assert_eq!(build_view(&entries, "minimap", false, |_| false), vec![1]);
        // A comment-only match: "parcel" appears in no name.
        assert_eq!(build_view(&entries, "parcel", false, |_| false), vec![2]);
        assert_eq!(
            build_view(&entries, "no such term", false, |_| false),
            Vec::<usize>::new()
        );
    }

    /// The changed-only toggle keeps exactly the overridden settings.
    #[test]
    fn changed_only_view_keeps_only_overridden() {
        let entries = vec![
            entry("AudioMasterVolume", "Master audio level"),
            entry("MiniMapRotate", "Rotate the mini-map"),
            entry("ShowPropertyLines", "Draw property lines"),
        ];
        let overridden = |name: &str| name == "MiniMapRotate";
        assert_eq!(build_view(&entries, "", true, overridden), vec![1]);
        // The term and the toggle compose.
        assert_eq!(
            build_view(&entries, "audio", true, overridden),
            Vec::<usize>::new()
        );
    }

    /// A toggle of the Bool editor writes the selected setting at the
    /// selected scope — and switching the scope moves the write target
    /// without touching the other layer.
    #[test]
    fn bool_toggle_writes_selected_scope() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Flag", SettingValue::Bool(false), "a toggle")
                .ok();
        });
        app.add_observer(on_debug_bool_toggle);
        let checkbox = app.world_mut().spawn((Checkbox, DebugBoolCheckbox)).id();
        app.world_mut().resource_mut::<DebugEditorState>().selected = Some("Flag".to_owned());
        app.update();
        app.world_mut().trigger(ValueChange {
            source: checkbox,
            value: true,
            is_final: true,
        });
        app.update();
        assert_eq!(
            store(&app).get_override(Scope::Global, "Flag"),
            Some(&SettingValue::Bool(true)),
            "the Global layer takes the write"
        );

        app.world_mut()
            .resource_mut::<ViewerSettings>()
            .mark_account_loaded_for_test();
        app.world_mut().resource_mut::<DebugEditorState>().scope = Scope::Account;
        app.world_mut().trigger(ValueChange {
            source: checkbox,
            value: false,
            is_final: true,
        });
        app.update();
        assert_eq!(
            store(&app).get_override(Scope::Account, "Flag"),
            Some(&SettingValue::Bool(false)),
            "the Account layer takes the write after the scope switch"
        );
        assert_eq!(
            store(&app).get_override(Scope::Global, "Flag"),
            Some(&SettingValue::Bool(true)),
            "the Global layer is untouched by the Account write"
        );
        Ok(())
    }

    /// `Enter` commits the focused numeric field; a blur (focus moving away)
    /// commits too; an incomplete field commits nothing.
    #[test]
    fn numeric_commit_on_enter_and_blur() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Level", SettingValue::F32(1.0), "a level")
                .ok();
        });
        app.insert_resource(InputFocus::default());
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.add_systems(Update, commit_debug_text_fields);
        let field = app
            .world_mut()
            .spawn((EditableText::new("2.5"), DebugEditField))
            .id();
        app.insert_resource(DebugSettingsUi {
            f32_field: field,
            ..placeholder_ui()
        });
        app.world_mut().resource_mut::<DebugEditorState>().selected = Some("Level".to_owned());
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Navigated);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert!((store(&app).get_f32("Level")? - 2.5).abs() < 1.0e-6);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();

        // The blur commit: edit the text, then move focus away.
        if let Some(mut editable) = app.world_mut().entity_mut(field).get_mut::<EditableText>() {
            editable.editor_mut().set_text("7.25");
        }
        app.update();
        app.insert_resource(InputFocus::default());
        app.update();
        assert!((store(&app).get_f32("Level")? - 7.25).abs() < 1.0e-6);

        // An incomplete field abandons the commit.
        if let Some(mut editable) = app.world_mut().entity_mut(field).get_mut::<EditableText>() {
            editable.editor_mut().set_text("-");
        }
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Navigated);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert!(
            (store(&app).get_f32("Level")? - 7.25).abs() < 1.0e-6,
            "an unparsable field commits nothing"
        );
        Ok(())
    }

    /// A vector commit assembles all three component fields — into `f32`s
    /// for a `Vec3` setting and `f64`s for a `Vec3d` one.
    #[test]
    fn vector_commit_assembles_three_fields() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Offset", SettingValue::Vec3([0.0; 3]), "an offset")
                .ok();
            store
                .register(
                    "GlobalPos",
                    SettingValue::Vec3d([0.0; 3]),
                    "a global position",
                )
                .ok();
        });
        app.insert_resource(InputFocus::default());
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.add_systems(Update, commit_debug_text_fields);
        let fields = ["1", "2.5", "-3"].map(|text| {
            app.world_mut()
                .spawn((EditableText::new(text), DebugEditField))
                .id()
        });
        app.insert_resource(DebugSettingsUi {
            vec_fields: fields,
            ..placeholder_ui()
        });
        app.world_mut().resource_mut::<DebugEditorState>().selected = Some("Offset".to_owned());
        let [first, _second, _third] = fields;
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(first, FocusCause::Navigated);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert_eq!(
            store(&app).get_override(Scope::Global, "Offset"),
            Some(&SettingValue::Vec3([1.0, 2.5, -3.0]))
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();

        // The same fields serve a Vec3d setting, kept at f64.
        app.world_mut().resource_mut::<DebugEditorState>().selected = Some("GlobalPos".to_owned());
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert_eq!(
            store(&app).get_override(Scope::Global, "GlobalPos"),
            Some(&SettingValue::Vec3d([1.0, 2.5, -3.0]))
        );
        Ok(())
    }

    /// Reset drops only the selected scope's override; the other layer (and
    /// so the effective value's fallback) survives.
    #[test]
    fn reset_clears_only_selected_scope() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Level", SettingValue::F32(10.0), "a level")
                .ok();
        });
        {
            let mut settings = app.world_mut().resource_mut::<ViewerSettings>();
            settings.mark_account_loaded_for_test();
            settings.set(Scope::Global, "Level", SettingValue::F32(20.0));
            settings.set(Scope::Account, "Level", SettingValue::F32(30.0));
        }
        let button = app.world_mut().spawn_empty().id();
        app.world_mut().entity_mut(button).observe(on_reset_setting);
        {
            let mut state = app.world_mut().resource_mut::<DebugEditorState>();
            state.selected = Some("Level".to_owned());
            state.scope = Scope::Account;
        }
        app.world_mut().trigger(Activate {
            entity: button,
            button: None,
        });
        app.update();
        assert_eq!(
            store(&app).get_override(Scope::Account, "Level"),
            None,
            "the Account override is dropped"
        );
        assert_eq!(
            store(&app).get_override(Scope::Global, "Level"),
            Some(&SettingValue::F32(20.0)),
            "the Global override survives"
        );
        assert!((store(&app).get_f32("Level")? - 20.0).abs() < 1.0e-6);
        Ok(())
    }

    /// Until the account scope loads the scope combo is disabled and a forced
    /// Account edit target snaps back to Global; login lifts the guard.
    #[test]
    fn account_scope_locked_until_login() -> Result<(), TestError> {
        let mut app = app(|_store| {});
        app.add_systems(Update, guard_debug_account_scope);
        let combo = app
            .world_mut()
            .spawn(ComboSelection {
                element: "debug-settings-scope",
                active: 1,
            })
            .id();
        app.insert_resource(DebugSettingsUi {
            scope_combo: combo,
            ..placeholder_ui()
        });
        app.world_mut().resource_mut::<DebugEditorState>().scope = Scope::Account;
        app.update();
        assert!(
            app.world().entity(combo).contains::<InteractionDisabled>(),
            "the combo is guarded pre-login"
        );
        assert_eq!(
            app.world().resource::<DebugEditorState>().scope,
            Scope::Global,
            "the edit target snaps back to Global"
        );
        let selection = app
            .world()
            .entity(combo)
            .get::<ComboSelection>()
            .ok_or("the combo lost its selection")?;
        assert_eq!(selection.active, 0, "the combo shows Global");

        app.world_mut()
            .resource_mut::<ViewerSettings>()
            .mark_account_loaded_for_test();
        app.update();
        assert!(
            !app.world().entity(combo).contains::<InteractionDisabled>(),
            "login lifts the guard"
        );
        Ok(())
    }

    /// The detail read-outs follow an external store change while a setting
    /// is selected — the override lines and the effective line move.
    #[test]
    fn detail_labels_follow_external_change() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Level", SettingValue::F32(10.0), "a level")
                .ok();
        });
        app.add_systems(Update, sync_debug_detail);
        let labels: [Entity; 7] =
            core::array::from_fn(|_| app.world_mut().spawn(Text::new(String::new())).id());
        let [name, comment, kind, default, global, account, effective] = labels;
        app.insert_resource(DebugSettingsUi {
            name_text: name,
            comment_text: comment,
            type_text: kind,
            default_value: default,
            global_value: global,
            account_value: account,
            effective_value: effective,
            ..placeholder_ui()
        });
        app.world_mut().resource_mut::<DebugEditorState>().selected = Some("Level".to_owned());
        app.update();
        let text = |app: &App, entity: Entity| -> String {
            app.world()
                .entity(entity)
                .get::<Text>()
                .map(|text| text.0.clone())
                .unwrap_or_default()
        };
        assert_eq!(text(&app, name), "Level");
        assert_eq!(text(&app, comment), "a level");
        assert_eq!(text(&app, kind), "F32");
        assert_eq!(text(&app, default), "10");
        assert_eq!(text(&app, global), NO_OVERRIDE);
        assert_eq!(text(&app, effective), "10");

        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            "Level",
            SettingValue::F32(25.5),
        );
        app.update();
        assert_eq!(text(&app, global), "25.5", "the Global line follows");
        assert_eq!(text(&app, effective), "25.5", "the effective line follows");
        assert_eq!(text(&app, default), "10", "the default line stays");
        Ok(())
    }
}
