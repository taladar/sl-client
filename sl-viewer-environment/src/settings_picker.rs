//! The **settings picker** (`LLFloaterSettingsPicker`): the chooser a panel
//! summons when a field holds a settings asset.
//!
//! A panel writes [`OpenSettingsPicker`] naming itself, the field and which
//! [`SettingsKind`] may be chosen; the window lists that kind of settings asset
//! (the same projection the library window draws — see
//! [`settings_list`](crate::settings_list)), narrowed by a name filter, and
//! answers with [`SettingsPicked`]. Selecting emits a **non-final** reply so the
//! panel can preview the choice, **OK** emits the committed one and **Cancel**
//! emits the asset the picker opened on — the protocol the texture and colour
//! pickers already follow, so a consumer written against one of them is written
//! against this.
//!
//! # The kind is the opener's, not the user's
//!
//! `setSettingsFilter` fixes the kind in the reference and it is fixed here: a
//! water field being handed a day cycle is not a choice worth offering. So this
//! window has no kind checkboxes — that is the library window's row, where the
//! user is browsing rather than answering.
//!
//! # One window per field, of the window that opened it
//!
//! This is a **keyed** floater, keyed by the opening window and the field
//! together (`picker_identity`), as the texture picker is. It used to be one
//! shared window that an open re-aimed, cancelling whatever pick was
//! outstanding — which reads fine for two fields of one panel, and not at all
//! for two *instances* of that panel: About Land is one window per parcel, and
//! picking an environment for the second parcel silently cancelled the first
//! parcel's pick.
//!
//! A keyed instance is transient, so it persists nothing and every piece of
//! per-window state is a component on the window root: closing it, or the
//! window that opened it (`FloaterOwner`), ends it outright.
//!
//! # Not here
//!
//! - **No track combo.** The reference's picker grows a *Select Track* combo
//!   when it is opened in `TRACK_WATER` / `TRACK_SKY` mode, to import one track
//!   out of the day cycle being chosen. Its only caller is the day-cycle
//!   editor's track import ([[viewer-environment-day-cycle-editor]]), and the
//!   combo needs the chosen asset **fetched and decoded** to know how many
//!   tracks it has — so it belongs with the window that has something to do with
//!   the answer.
//!
//! Reference (Firestorm, read-only): `llsettingspicker.cpp`,
//! `floater_settings_picker.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{InventoryKey, SettingsKind, Uuid};
use sl_viewer_inventory::inventory::InventoryModel;
use sl_viewer_inventory::settings_index::SettingsIndex;
use sl_viewer_ui_core::i18n::{TransArgs, Translated, Translator};
use sl_viewer_ui_core::ui::{UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists};
use sl_viewer_ui_widgets::floater::{
    Floater, FloaterCaps, FloaterCommand, FloaterHandle, FloaterOp, FloaterOwner, FloaterSpec,
    FloaterSystems, KeyedFloaterOpen, KeyedFloaters, host_floater, picker_identity,
};
use sl_viewer_ui_widgets::ui_search::{SearchFieldSpec, spawn_search_field};
use sl_viewer_ui_widgets::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableRowCells, TableSelectionMode,
    TableSortDefault, TableSpec, TableState, set_table_cell, spawn_table, spawn_table_row,
};
use sl_viewer_world_api::{OpenSettingsPicker, PickedSettings, SettingsPicked};

use crate::settings_list::{
    SettingsListFilters, SettingsListRow, location_text, project, sort_rows,
};
use crate::style::{
    ACTION_BACKGROUND, DIM_LABEL_COLOR, FONT_SIZE, LABEL_COLOR, LIST_BACKGROUND, ROW_HEIGHT,
    SELECTED_BACKGROUND,
};

/// The picker's stable floater id — and the element-id prefix of its controls.
pub const SETTINGS_PICKER_FLOATER_ID: &str = "settings-picker";

/// Column index of the item's name.
const COL_NAME: usize = 0;

/// Column index of where the item lives.
const COL_WHERE: usize = 1;

/// The picker's list: the name beside where it came from. No Kind column — the
/// opener fixed the kind, so every row is the same one and a column of it would
/// say nothing.
static SETTINGS_PICKER_TABLE: TableSpec = TableSpec {
    element: SETTINGS_PICKER_FLOATER_ID,
    // Module-owned: the selection keys on the item, so it survives a re-sort and
    // the virtualized row recycling.
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "my-environments-col-name",
            token: "name",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "my-environments-col-where",
            token: "where",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: true,
        },
    ],
    default_sort: &[TableSortDefault {
        column: COL_NAME,
        ascending: true,
    }],
    builtin_sort: true,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_LABEL_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    // The window is transient and aimed at a different field each time; a
    // persisted sort would be one field's order imposed on the next.
    sort_setting: None,
    widths_setting: None,
};

/// The Fluent key of the window's title for `kind` — the reference's
/// *Pick: …*, which is what tells two consecutive picks apart.
const fn title_key(kind: SettingsKind) -> &'static str {
    match kind {
        SettingsKind::Sky => "settings-picker-title-sky",
        SettingsKind::Water => "settings-picker-title-water",
        SettingsKind::DayCycle => "settings-picker-title-day-cycle",
    }
}

// --- State ----------------------------------------------------------------

/// What one picker window is answering, and what it has been told so far — a
/// component on the window root, so it dies with the instance.
#[derive(Component, Debug, Default)]
struct SettingsPickerState {
    /// The panel that opened it, and that its replies are tagged back to.
    /// `None` once OK / Cancel has answered, which is what tells a close it has
    /// nothing left to revert.
    requester: Option<Entity>,
    /// The field being picked for, shown under the title.
    field: String,
    /// Which kind may be chosen, and the name filter over it.
    filters: SettingsListFilters,
    /// The asset the picker opened on, restored by Cancel.
    original: Option<PickedSettings>,
    /// The row currently selected.
    selected: Option<InventoryKey>,
    /// The asset the opener named, until a row carrying it is found — the
    /// reference's `findItemID(asset_id, …)`, which cannot run before the
    /// inventory holding the item has loaded.
    wanted: Option<Uuid>,
    /// The rows on screen, in table order.
    rows: Vec<SettingsListRow>,
    /// The filters the rows were built with.
    built_filters: Option<SettingsListFilters>,
    /// The index they were built from.
    built_index: Option<SettingsIndex>,
    /// The table sort revision they were ordered at.
    built_sort_revision: u64,
    /// Whether anything has been projected yet.
    built: bool,
}

impl SettingsPickerState {
    /// The row `item` names, if it is still on screen.
    fn row(&self, item: InventoryKey) -> Option<&SettingsListRow> {
        self.rows.iter().find(|row| row.item == item)
    }

    /// The selection as an answer.
    fn chosen(&self) -> Option<PickedSettings> {
        self.selected
            .and_then(|item| self.row(item))
            .map(|row| PickedSettings {
                item: row.item,
                asset_id: row.asset_id,
                name: row.name.clone(),
            })
    }
}

/// One picker window's retained entities.
#[derive(Component, Debug)]
struct SettingsPickerUi {
    /// The floater's title text, retitled per kind on open.
    title_text: Entity,
    /// The line under the title naming the field being picked for.
    field_text: Entity,
    /// The table root, carrying its [`TableState`].
    table: Entity,
    /// The virtualized viewport.
    viewport: Entity,
    /// The name-filter field's [`EditableText`].
    filter_field: Entity,
    /// The count line.
    count_text: Entity,
}

/// The row a pooled list row currently presents.
#[derive(Component, Debug, Clone, Copy, Default)]
struct BoundPickerRow(Option<InventoryKey>);

/// Which of the two reply buttons this is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum PickerButton {
    /// Commit the selection.
    Ok,
    /// Put back what the picker opened on.
    Cancel,
}

impl PickerButton {
    /// The Fluent key for this button's label.
    const fn label_key(self) -> &'static str {
        match self {
            Self::Ok => "settings-picker-ok",
            Self::Cancel => "settings-picker-cancel",
        }
    }

    /// The button's element-id tail.
    const fn slug(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Cancel => "cancel",
        }
    }
}

// --- Plugin ---------------------------------------------------------------

/// The settings picker: its state, its deferred content and its reply protocol.
#[derive(Debug, Clone, Copy, Default)]
pub struct SettingsPickerPlugin;

impl Plugin for SettingsPickerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SettingsIndex>()
            .add_message::<OpenSettingsPicker>()
            .add_message::<SettingsPicked>()
            .add_systems(
                Update,
                // After the manager's command pass — see `FloaterSystems`: the
                // click on a "Use Inventory…" button also raises the window it
                // was clicked in, and the later raise wins the z-order.
                open_settings_picker
                    .after(FloaterSystems::Commands)
                    .after(UiScaffoldSystems::SpawnRoot)
                    .before(layout_virtual_lists),
            )
            .add_systems(
                Update,
                (
                    mirror_picker_filter,
                    rebuild_picker_rows,
                    resolve_wanted_row,
                )
                    .chain()
                    .after(open_settings_picker)
                    .before(layout_virtual_lists)
                    .run_if(any_with_component::<SettingsPickerState>),
            )
            .add_systems(
                Update,
                (populate_picker_rows, bind_picker_rows)
                    .chain()
                    .after(layout_virtual_lists)
                    .run_if(any_with_component::<SettingsPickerState>),
            )
            // Reads the close **command**, before the pass that carries it out
            // despawns the window: a keyed instance ends on close, so afterwards
            // there is no state left to answer from.
            .add_systems(
                Update,
                revert_picker_on_close.before(FloaterSystems::Commands),
            );
    }
}

// --- Floater --------------------------------------------------------------

/// The settings picker's [`FloaterSpec`] — shared with the `FLOATERS` registry,
/// so the swept window is the one the viewer spawns.
#[must_use]
pub fn settings_picker_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: SETTINGS_PICKER_FLOATER_ID,
        title: "Pick: Settings".to_owned(),
        position: Vec2::new(300.0, 180.0),
        default_size: Some(Vec2::new(380.0, 380.0)),
        min_size: Some(Vec2::new(260.0, 240.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            // The reference's picker cannot be minimized either: a chooser a
            // panel is waiting on must not be able to hide behind the window
            // that opened it.
            minimizable: false,
            closable: true,
            dockable: false,
        },
    }
}

/// One window's content: the field line, the filter, the list and the reply
/// row.
fn build_settings_picker_content(
    handle: &FloaterHandle,
    commands: &mut Commands,
) -> SettingsPickerUi {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                padding: UiRect::all(Val::Px(6.0)),
                ..column(Val::Px(4.0))
            },
            Name::new("settings-picker:content"),
            ChildOf(handle.content),
        ))
        .id();

    let field_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Pickable::IGNORE,
            Name::new("settings-picker-field"),
            ChildOf(content),
        ))
        .id();

    let search = spawn_search_field(
        commands,
        content,
        &SearchFieldSpec {
            tab_index: 0,
            font_size: FONT_SIZE,
            min_width: 160.0,
            placeholder: "Filter Settings".to_owned(),
            search_glyph: true,
            ..SearchFieldSpec::new("settings-picker-filter")
        },
    );
    if let Some(placeholder) = search.placeholder {
        commands
            .entity(placeholder)
            .insert(Translated::new("settings-picker-filter-placeholder"));
    }

    let table = spawn_table(commands, content, &SETTINGS_PICKER_TABLE);
    commands
        .entity(table.viewport)
        .insert((BackgroundColor(LIST_BACKGROUND), TabIndex(1)));

    let count_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Node {
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                ..default()
            },
            Pickable::IGNORE,
            Name::new("settings-picker-count"),
            ChildOf(content),
        ))
        .id();

    let buttons = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new("settings-picker-actions:row"),
            ChildOf(content),
        ))
        .id();
    for (index, button) in [PickerButton::Ok, PickerButton::Cancel]
        .into_iter()
        .enumerate()
    {
        spawn_picker_button(
            commands,
            buttons,
            button,
            i32::try_from(index).unwrap_or(0).saturating_add(2),
        );
    }

    SettingsPickerUi {
        title_text: handle.title_text,
        field_text,
        table: table.root,
        viewport: table.viewport,
        filter_field: search.field,
        count_text,
    }
}

/// One reply button.
fn spawn_picker_button(
    commands: &mut Commands,
    parent: Entity,
    button: PickerButton,
    tab: i32,
) -> Entity {
    commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(12.0), Val::Px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(ACTION_BACKGROUND),
            TabIndex(tab),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            button,
            Name::new(format!("settings-picker-{}:button", button.slug())),
            ChildOf(parent),
        ))
        .with_child((
            Text::default(),
            Translated::new(button.label_key()),
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..default()
            },
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ))
        .observe(on_picker_button)
        .id()
}

// --- Opening --------------------------------------------------------------

/// Open (or re-aim) the picker window for the field that asked.
///
/// Keyed by the **opening window and the field together**: the button that
/// asked gets its own window, a second press on it re-aims that one, and the
/// same field in a second instance of the opening window gets a window of its
/// own rather than cancelling the first's outstanding pick.
fn open_settings_picker(
    mut opens: MessageReader<OpenSettingsPicker>,
    mut floaters: KeyedFloaters,
    mut windows: Query<(&mut SettingsPickerState, &SettingsPickerUi)>,
    parents: Query<&ChildOf>,
    openers: Query<(Entity, &Floater)>,
    mut fields: Query<&mut EditableText>,
    mut commands: Commands,
) {
    let requests: Vec<OpenSettingsPicker> = opens.read().cloned().collect();
    for open in requests {
        let (owner, key) = picker_identity(open.requester, &open.field, &parents, &openers);
        let opened = floaters.open(settings_picker_floater_spec(), key);
        let window = opened.root();
        let title = Translated::new(title_key(open.kind));
        if let KeyedFloaterOpen::Spawned(handle) = opened {
            let ui = build_settings_picker_content(&handle, &mut commands);
            let title_text = ui.title_text;
            // Seeded here rather than after the insert: the components only
            // reach the world when this frame's commands flush, so a window
            // spawned now is not queryable yet.
            commands
                .entity(handle.root)
                .insert((aimed_state(&open), ui));
            commands.entity(title_text).insert(title);
            if let Some(owner) = owner {
                commands.entity(handle.root).insert(FloaterOwner(owner));
            }
            continue;
        }
        let Ok((mut state, ui)) = windows.get_mut(window) else {
            continue;
        };
        *state = aimed_state(&open);
        commands.entity(ui.title_text).insert(title);
        // A fresh aim starts with a fresh filter, or the last pick's term would
        // silently hide most of this one's choices.
        if let Ok(mut field) = fields.get_mut(ui.filter_field)
            && !field.value().to_string().is_empty()
        {
            field.editor.set_text("");
        }
    }
}

/// The state an open aims a window at — everything the request fixes, and
/// nothing carried over from a previous one.
fn aimed_state(open: &OpenSettingsPicker) -> SettingsPickerState {
    SettingsPickerState {
        requester: Some(open.requester),
        field: open.field.to_string(),
        filters: SettingsListFilters::only(open.kind),
        wanted: open.current,
        ..SettingsPickerState::default()
    }
}

// --- View systems ---------------------------------------------------------

/// Keep each window's filter term in step with its search field.
fn mirror_picker_filter(
    mut windows: Query<(&mut SettingsPickerState, &SettingsPickerUi)>,
    fields: Query<&EditableText>,
) {
    for (mut state, ui) in &mut windows {
        let Ok(field) = fields.get(ui.filter_field) else {
            continue;
        };
        let term = field.value().to_string();
        if state.filters.search != term {
            state.filters.search = term;
        }
    }
}

/// Reproject each window when the index, its filter or its sort moved.
fn rebuild_picker_rows(
    index: Res<SettingsIndex>,
    model: Option<Res<InventoryModel>>,
    translator: Translator,
    mut windows: Query<(&mut SettingsPickerState, &SettingsPickerUi)>,
    tables: Query<&TableState>,
    mut lists: Query<&mut VirtualList>,
    mut texts: Query<&mut Text>,
) {
    let Some(model) = model else {
        return;
    };
    for (mut state, ui) in &mut windows {
        let sort = tables
            .get(ui.table)
            .ok()
            .map(|table| (table.sort_revision(), table.sort().keys().to_vec()));
        let sort_revision = sort.as_ref().map_or(0, |(revision, _keys)| *revision);
        if state.built
            && state.built_sort_revision == sort_revision
            && state.built_filters.as_ref() == Some(&state.filters)
            && state.built_index.as_ref() == Some(&*index)
            && !model.is_changed()
        {
            continue;
        }
        state.built = true;
        state.built_sort_revision = sort_revision;
        state.built_filters = Some(state.filters.clone());
        state.built_index = Some(index.clone());

        let mut rows = project(&index, &model, &state.filters);
        let keys: Vec<(&str, bool)> = sort
            .map(|(_revision, keys)| keys)
            .unwrap_or_default()
            .iter()
            .filter_map(|key| {
                SETTINGS_PICKER_TABLE
                    .columns
                    .get(key.column)
                    .map(|column| (column.token, key.ascending))
            })
            .collect();
        sort_rows(&mut rows, &keys);
        state.rows = rows;

        if let Ok(mut list) = lists.get_mut(ui.viewport) {
            list.item_count = state.rows.len();
        }
        let label = translator.format(
            "settings-picker-count",
            &TransArgs::new().int("shown", i64::try_from(state.rows.len()).unwrap_or(i64::MAX)),
        );
        if let Ok(mut text) = texts.get_mut(ui.count_text)
            && text.0 != label
        {
            text.0 = label;
        }
    }
}

/// Select the row carrying the asset the opener named, once one is on screen.
///
/// The reference's `findItemID` runs against a loaded inventory; ours may not
/// have one yet when the panel opens the picker, so the wanted asset is held and
/// resolved on whichever projection first holds it — and dropped once resolved,
/// so a later filter that hides the row does not re-select it behind the user's
/// back.
fn resolve_wanted_row(mut windows: Query<&mut SettingsPickerState>) {
    for mut state in &mut windows {
        let Some(wanted) = state.wanted else {
            continue;
        };
        let Some(found) = state
            .rows
            .iter()
            .find(|row| row.asset_id == wanted)
            .map(|row| PickedSettings {
                item: row.item,
                asset_id: row.asset_id,
                name: row.name.clone(),
            })
        else {
            continue;
        };
        state.wanted = None;
        state.selected = Some(found.item);
        state.original = Some(found);
    }
}

/// Build the cells of each freshly-pooled row and attach its press observer.
fn populate_picker_rows(
    mut commands: Commands,
    windows: Query<&SettingsPickerUi>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    for (row_entity, child_of) in &new_rows {
        // The row's own window: with two pickers up, a row pooled into one
        // viewport must be built against that window's table.
        let Some(ui) = windows.iter().find(|ui| ui.viewport == child_of.parent()) else {
            continue;
        };
        spawn_table_row(&mut commands, row_entity, ui.table, &SETTINGS_PICKER_TABLE);
        commands
            .entity(row_entity)
            .insert(BoundPickerRow(None))
            .observe(on_picker_row_press);
    }
}

/// Bind each pooled row to the entry it now presents, and keep the field line in
/// step with what is being picked for.
fn bind_picker_rows(
    windows: Query<(Ref<SettingsPickerState>, &SettingsPickerUi)>,
    translator: Translator,
    mut rows: Query<(
        Entity,
        Ref<VirtualRow>,
        &ChildOf,
        &TableRowCells,
        &mut BoundPickerRow,
    )>,
    mut backgrounds: Query<&mut BackgroundColor>,
    // One `Text` query for the field line *and* the cells: two of them
    // (`Query<&mut Text>` beside `Query<(&mut Text, &mut TextColor)>`) is a
    // B0001 conflict, and the panic is on the system's first run — the whole
    // viewer, not this window. The field line carries a `TextColor` too, so the
    // wider query reaches it.
    mut cells_text: Query<(&mut Text, &mut TextColor)>,
) {
    let library_label = translator.get("my-environments-library");
    for (state, ui) in &windows {
        if state.is_changed()
            && let Ok((mut text, _color)) = cells_text.get_mut(ui.field_text)
        {
            let label = translator.format(
                "settings-picker-field",
                &TransArgs::new().text("field", &state.field),
            );
            if text.0 != label {
                text.0 = label;
            }
        }
        let refresh_all = state.is_changed();
        for (row_entity, row, child_of, cells, mut bound) in &mut rows {
            if child_of.parent() != ui.viewport {
                continue;
            }
            if !refresh_all && !row.is_changed() {
                continue;
            }
            let data = row.index.and_then(|index| state.rows.get(index));
            bound.0 = data.map(|entry| entry.item);
            let (name, location) = data.map_or_else(
                || (String::new(), String::new()),
                |entry| {
                    (
                        entry.name.clone(),
                        location_text(&library_label, entry.library, &entry.folder),
                    )
                },
            );
            for (column, value, color) in [
                (COL_NAME, name, LABEL_COLOR),
                (COL_WHERE, location, DIM_LABEL_COLOR),
            ] {
                if let Some(cell) = cells.cell(column) {
                    set_table_cell(&mut cells_text, cell, &value, color);
                }
            }
            if let Ok(mut background) = backgrounds.get_mut(row_entity) {
                let wanted = if data.is_some() && state.selected == bound.0 {
                    SELECTED_BACKGROUND
                } else {
                    Color::NONE
                };
                if background.0 != wanted {
                    background.0 = wanted;
                }
            }
        }
    }
}

// --- Interaction ----------------------------------------------------------

/// A press on a row selects it and previews the choice.
fn on_picker_row_press(
    mut press: On<Pointer<Press>>,
    rows: Query<&BoundPickerRow>,
    mut windows: Query<(&mut SettingsPickerState, &SettingsPickerUi)>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut focus: ResMut<InputFocus>,
    mut picked: MessageWriter<SettingsPicked>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(BoundPickerRow(Some(item))) = rows.get(press.entity).copied() else {
        return;
    };
    let Some(window) = host_floater(press.entity, &parents, &floaters) else {
        return;
    };
    let Ok((mut state, ui)) = windows.get_mut(window) else {
        return;
    };
    press.propagate(false);
    focus.set(ui.viewport, FocusCause::Navigated);
    state.selected = Some(item);
    // The reference's `mChangeIDSignal` on every selection change: the panel
    // sees the choice before it is committed, so it can show it.
    if let Some(requester) = state.requester {
        picked.write(SettingsPicked {
            requester,
            chosen: state.chosen(),
            final_pick: false,
        });
    }
}

/// A press on OK or Cancel.
fn on_picker_button(
    mut press: On<Pointer<Press>>,
    buttons: Query<&PickerButton>,
    mut windows: Query<&mut SettingsPickerState>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut picked: MessageWriter<SettingsPicked>,
    mut chrome: MessageWriter<FloaterCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(button) = buttons.get(press.entity).copied() else {
        return;
    };
    let Some(window) = host_floater(press.entity, &parents, &floaters) else {
        return;
    };
    let Ok(mut state) = windows.get_mut(window) else {
        return;
    };
    press.propagate(false);
    let Some(requester) = state.requester else {
        return;
    };
    let (chosen, final_pick) = match button {
        PickerButton::Ok => (state.chosen(), true),
        // Put the live preview back to what the picker opened on, as the
        // reference's `onButtonCancel` restores `mSettingItemID`.
        PickerButton::Cancel => (state.original.clone(), false),
    };
    picked.write(SettingsPicked {
        requester,
        chosen,
        final_pick,
    });
    // Cleared *before* the close, so `revert_picker_on_close` knows this window
    // has already answered and does not send a second reply.
    state.requester = None;
    chrome.write(FloaterCommand {
        floater: window,
        op: FloaterOp::Close,
    });
}

/// A close by the chrome's ✕ is a Cancel: the panel is still waiting, and a
/// window that vanished without answering would leave its preview in force.
fn revert_picker_on_close(
    mut closes: MessageReader<FloaterCommand>,
    mut windows: Query<&mut SettingsPickerState>,
    mut picked: MessageWriter<SettingsPicked>,
) {
    for command in closes.read() {
        if !matches!(command.op, FloaterOp::Close | FloaterOp::CloseNow) {
            continue;
        }
        let Ok(mut state) = windows.get_mut(command.floater) else {
            continue;
        };
        let Some(requester) = state.requester.take() else {
            continue;
        };
        picked.write(SettingsPicked {
            requester,
            chosen: state.original.clone(),
            final_pick: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        COL_NAME, COL_WHERE, PickerButton, SETTINGS_PICKER_TABLE, SettingsPickerState, title_key,
    };
    use crate::settings_list::{SettingsListFilters, SettingsListRow};
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{InventoryKey, SettingsKind, Uuid};

    /// A boxed error, so a test can `?` rather than reach for a `panic!` the
    /// workspace's lints forbid.
    type TestError = Box<dyn core::error::Error>;

    /// A row of the picker's list.
    fn row(id: u128, asset: u128, name: &str) -> SettingsListRow {
        SettingsListRow {
            item: InventoryKey::from(Uuid::from_u128(id)),
            asset_id: Uuid::from_u128(asset),
            name: name.to_owned(),
            kind: SettingsKind::Sky,
            library: false,
            folder: "Settings".to_owned(),
        }
    }

    /// A state holding `rows`, with `selected` selected.
    fn state(rows: Vec<SettingsListRow>, selected: Option<u128>) -> SettingsPickerState {
        SettingsPickerState {
            selected: selected.map(|id| InventoryKey::from(Uuid::from_u128(id))),
            rows,
            ..SettingsPickerState::default()
        }
    }

    /// **The answer carries the item *and* the asset.** They are two different
    /// ids — a link's own id, and the settings behind its target — and a panel
    /// publishing the item id to the grid would name something that is not a
    /// settings asset at all.
    #[test]
    fn the_answer_carries_both_ids_and_the_name() -> Result<(), TestError> {
        let picker = state(vec![row(1, 0xA1, "Clear sky")], Some(1));
        let chosen = picker.chosen().ok_or("a selected row answers")?;
        assert_eq!(chosen.item, InventoryKey::from(Uuid::from_u128(1)));
        assert_eq!(chosen.asset_id, Uuid::from_u128(0xA1));
        assert_eq!(chosen.name, "Clear sky");
        Ok(())
    }

    /// **A selection the filter has hidden is not an answer.** The rows are
    /// re-projected as the user types, and a selection kept across a projection
    /// that dropped it would commit a row nobody can see.
    #[test]
    fn a_selection_no_longer_on_screen_answers_with_nothing() {
        let picker = state(vec![row(2, 0xA2, "Other")], Some(1));
        assert!(picker.chosen().is_none());
        assert!(state(Vec::new(), None).chosen().is_none());
    }

    /// Each kind titles the window differently — the reference's *Pick: …*,
    /// which is what tells two consecutive picks apart when a panel opens one
    /// straight after another.
    #[test]
    fn every_kind_titles_the_window_apart() {
        let keys = [
            title_key(SettingsKind::Sky),
            title_key(SettingsKind::Water),
            title_key(SettingsKind::DayCycle),
        ];
        let mut unique = keys;
        unique.sort_unstable();
        let mut deduped = unique.to_vec();
        deduped.dedup();
        assert_eq!(deduped.len(), keys.len(), "{keys:?}");
    }

    /// The kind the opener fixes is the only one offered — the reference's
    /// `setSettingsFilter`, and the reason a water field cannot be handed a day
    /// cycle by mistake.
    #[test]
    fn the_openers_kind_is_the_only_one_offered() {
        let filters = SettingsListFilters::only(SettingsKind::Water);
        assert!(filters.shows(SettingsKind::Water));
        assert!(!filters.shows(SettingsKind::Sky));
        assert!(!filters.shows(SettingsKind::DayCycle));
    }

    /// The table's columns and the cell writers agree, and there is no Kind
    /// column: every row is the opener's kind, so a column of it would repeat
    /// one word down the list.
    #[test]
    fn the_column_indices_match_the_spec() {
        let tokens: Vec<&str> = SETTINGS_PICKER_TABLE
            .columns
            .iter()
            .map(|column| column.token)
            .collect();
        assert_eq!(tokens, ["name", "where"]);
        assert_eq!(COL_NAME, 0);
        assert_eq!(COL_WHERE, 1);
    }

    /// Both reply buttons have their own label key and element tail, so neither
    /// can be spawned wearing the other's.
    #[test]
    fn the_two_buttons_are_named_apart() {
        assert_ne!(
            PickerButton::Ok.label_key(),
            PickerButton::Cancel.label_key()
        );
        assert_ne!(PickerButton::Ok.slug(), PickerButton::Cancel.slug());
    }
}
