//! The **Avatar Render Settings** floater
//! (`viewer-avatar-render-settings-manager`): every standing per-avatar
//! exception to the automatic complexity limit, and the place to add, change or
//! drop one.
//!
//! The model — what each exception means, how it is applied and how it persists
//! — is [`crate::avatar_render_settings`]; this is its surface, laid out like
//! the Asset Blacklist beside it: a filter box over a sortable, virtualized
//! table (Name / Setting / Date) with the actions at its trailing edge.
//!
//! - **Render Fully / Never Render** re-decide the selected person, exactly as
//!   the avatar pie's `More ▸ Render ▸` does.
//! - **Remove** drops the exception, handing that person back to the automatic
//!   rules (the reference's "Remove From Exceptions").
//! - **Add Fully… / Add Never…** open the shared avatar picker
//!   ([`crate::avatar_picker`]), so someone who is nowhere near you — the usual
//!   case for a decision made after an event — can be added by name. This is the
//!   reference's `+` menu, whose two entries are the same two settings.
//!
//! # Deliberate divergences from the reference
//!
//! - **Single-select.** As in the block list and the blacklist, the table
//!   selects one row; the reference's multi-select is a Firestorm addition.
//! - **A Date column.** The reference lists name and setting alone. Ours stamps
//!   when the decision was made, because the list is long-lived by design and
//!   "who did I mute the year before last" is the question you ask of it.
//!
//! Reference (Firestorm, read-only): `fsfloateravatarrendersettings`,
//! `floater_fs_avatar_render_settings.xml`, `menu_fs_avatar_render_setting.xml`,
//! `menu_avatar_rendering_settings_add.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::AgentKey;

use crate::avatar_complexity::RenderOverride;
use crate::avatar_picker::{AvatarPicked, OpenAvatarPicker};
use crate::avatar_render_settings::{
    AvatarRenderSettings, RenderException, RequestRenderException,
};
use crate::floater::{
    DeferredFloaterContent, FloaterCaps, FloaterHandle, FloaterSpec, floater_shown, spawn_floater,
};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::settings::ViewerSettings;
use crate::snapshot_floater::LocalTimeZone;
use crate::ui::{UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;
use crate::ui_search::{SearchFieldSpec, spawn_search_field};
use crate::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableRowCells, TableSelectionMode,
    TableSortDefault, TableSpec, TableState, register_table_settings, set_table_cell, spawn_table,
    spawn_table_row,
};
use crate::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists, spawn_virtual_scrollbar};

/// The floater's stable id (persistence, `SL_VIEWER_OPEN_FLOATER`).
pub(crate) const RENDER_SETTINGS_FLOATER_ID: &str = "avatar-render-settings";

/// The persisted-settings section the table's sort / widths live under.
const RENDER_SETTINGS_SECTION: &[&str] = &["avatarrender"];

/// The picker tag for an **Add ▸ Render Fully** pick.
const PICKER_ADD_FULLY: &str = "avatar-render-add-fully";

/// The picker tag for an **Add ▸ Never Render** pick.
const PICKER_ADD_NEVER: &str = "avatar-render-add-never";

// --- Palette / geometry (the sibling list floaters' values) ---------------

/// Header / cell font size, logical px.
const FONT_SIZE: f32 = 13.0;

/// Table row height, logical px.
const ROW_HEIGHT: f32 = 20.0;

/// The default cell / label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// The dimmed header / secondary colour.
const DIM_LABEL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// The list viewport backdrop.
const LIST_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);

/// A selected row's background highlight.
const SELECTED_BACKGROUND: Color = Color::srgba(0.24, 0.34, 0.52, 0.55);

/// An action button's background.
const ACTION_BACKGROUND: Color = Color::srgb(0.24, 0.29, 0.38);

/// The trailing action column's width, logical px.
const ACTION_COL_WIDTH: f32 = 140.0;

// --- Table ----------------------------------------------------------------

/// Column index of the avatar's name.
const COL_NAME: usize = 0;
/// Column index of the exception itself.
const COL_SETTING: usize = 1;
/// Column index of the date the decision was made.
const COL_DATE: usize = 2;

/// The exception table: a flexible name beside the fixed setting / date cells,
/// sorted by name ascending by default (the reference's `sort_column="0"`).
static RENDER_SETTINGS_TABLE: TableSpec = TableSpec {
    element: "avatar-render-settings",
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "avatar-render-col-name",
            token: "name",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "avatar-render-col-setting",
            token: "setting",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 110.0 },
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "avatar-render-col-date",
            token: "date",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 128.0 },
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
    sort_setting: Some("AvatarRenderSortOrder"),
    widths_setting: Some("AvatarRenderColumnWidths"),
};

// --- Pure view model ------------------------------------------------------

/// One row of the list as it is shown: the stored decision, plus the name the
/// row actually reads by. Both the filter and the sort work on that label, so
/// they agree with what is on screen even when the live name cache has moved on
/// from the name stored with the decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExceptionRow {
    /// The stored decision this row presents.
    pub(crate) entry: RenderException,
    /// What the Name column reads.
    pub(crate) label: String,
}

/// Whether `row` survives the list's filter — a case-insensitive substring of
/// the shown name, or of the id, so an entry whose name has not resolved is
/// still findable by what the list *does* show.
pub(crate) fn matches_filter(row: &ExceptionRow, filter: &str) -> bool {
    let filter = filter.trim().to_lowercase();
    filter.is_empty()
        || row.label.to_lowercase().contains(&filter)
        || row.entry.agent.to_string().to_lowercase().contains(&filter)
}

/// Order `rows` by the table's sort keys (most significant first), falling back
/// to a case-insensitive name compare so the order is total.
pub(crate) fn sort_rows(rows: &mut [ExceptionRow], keys: &[(&str, bool)]) {
    rows.sort_by(|left, right| {
        for (token, ascending) in keys {
            let ordering = match *token {
                "setting" => left.entry.setting.rank().cmp(&right.entry.setting.rank()),
                "date" => left
                    .entry
                    .added_epoch_secs
                    .cmp(&right.entry.added_epoch_secs),
                _name => left.label.to_lowercase().cmp(&right.label.to_lowercase()),
            };
            let ordering = if *ascending {
                ordering
            } else {
                ordering.reverse()
            };
            if ordering != core::cmp::Ordering::Equal {
                return ordering;
            }
        }
        left.label.to_lowercase().cmp(&right.label.to_lowercase())
    });
}

// --- Resources ------------------------------------------------------------

/// The floater's live view state: the filter and the ordered rows the virtual
/// list binds, plus the stamps they were built against.
#[derive(Resource, Debug, Default)]
struct RenderSettingsView {
    /// The live filter text.
    filter: String,
    /// The display rows, in table order.
    rows: Vec<ExceptionRow>,
    /// The exception-store revision the rows were built at.
    built_revision: u64,
    /// The table sort revision the rows were ordered at.
    built_sort_revision: u64,
    /// The filter the rows were filtered by.
    built_filter: String,
}

/// The selected avatar, which the action buttons act on. Keyed by id (not row
/// index) so it survives the re-sort and the virtualized row recycling.
#[derive(Resource, Debug, Default)]
struct SelectedRenderException(Option<AgentKey>);

/// The floater's retained entities (inserted by the deferred content build;
/// consumers take `Option<Res<RenderSettingsUi>>` until then).
#[derive(Resource, Debug)]
struct RenderSettingsUi {
    /// The table root (carries [`TableState`]).
    table: Entity,
    /// The virtualized viewport (carries [`VirtualList`]).
    viewport: Entity,
    /// The filter box's [`EditableText`] entity.
    filter_field: Entity,
    /// The entry-count line.
    count_text: Entity,
}

/// The avatar a pooled row currently presents.
#[derive(Component, Debug, Clone, Copy, Default)]
struct BoundRenderException(Option<AgentKey>);

/// What a trailing action button does.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum RenderSettingsButton {
    /// Re-decide the selected person to "always draw them in full".
    RenderFully,
    /// Re-decide them to "never draw them in full".
    NeverRender,
    /// Drop their exception, handing them back to the automatic rules.
    Remove,
    /// Pick someone and add them as a Render Fully exception.
    AddFully,
    /// Pick someone and add them as a Never Render exception.
    AddNever,
}

impl RenderSettingsButton {
    /// The Fluent key for this button's label.
    const fn label_key(self) -> &'static str {
        match self {
            Self::RenderFully => "avatar-render-action-fully",
            Self::NeverRender => "avatar-render-action-never",
            Self::Remove => "avatar-render-action-remove",
            Self::AddFully => "avatar-render-action-add-fully",
            Self::AddNever => "avatar-render-action-add-never",
        }
    }

    /// The exception this button decides on the **selected** row, if it is one
    /// of the three that act on the selection.
    const fn setting(self) -> Option<RenderOverride> {
        match self {
            Self::RenderFully => Some(RenderOverride::AlwaysFull),
            Self::NeverRender => Some(RenderOverride::Never),
            Self::Remove => Some(RenderOverride::Normal),
            Self::AddFully | Self::AddNever => None,
        }
    }

    /// The picker tag this button opens the avatar picker with, if it is one of
    /// the two that add someone not in the list.
    const fn picker_tag(self) -> Option<&'static str> {
        match self {
            Self::AddFully => Some(PICKER_ADD_FULLY),
            Self::AddNever => Some(PICKER_ADD_NEVER),
            Self::RenderFully | Self::NeverRender | Self::Remove => None,
        }
    }
}

/// The exception a finished pick records, by the tag the picker was opened
/// with.
const fn picked_setting(tag: &str) -> Option<RenderOverride> {
    match tag.as_bytes() {
        b"avatar-render-add-fully" => Some(RenderOverride::AlwaysFull),
        b"avatar-render-add-never" => Some(RenderOverride::Never),
        _other => None,
    }
}

// --- Plugin ---------------------------------------------------------------

/// Registers the Avatar Render Settings floater, its view state and its
/// actions.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AvatarRenderFloaterPlugin;

impl Plugin for AvatarRenderFloaterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderSettingsView>()
            .init_resource::<SelectedRenderException>()
            .add_systems(
                Startup,
                (
                    register_render_settings_table,
                    spawn_render_settings_floater.after(UiScaffoldSystems::SpawnRoot),
                ),
            )
            .add_systems(Update, handle_render_settings_picks)
            .add_systems(
                Update,
                (mirror_render_settings_filter, rebuild_render_settings_view)
                    .chain()
                    .before(layout_virtual_lists)
                    .run_if(floater_shown(RENDER_SETTINGS_FLOATER_ID)),
            )
            .add_systems(
                Update,
                (populate_render_settings_rows, bind_render_settings_rows)
                    .chain()
                    .after(layout_virtual_lists)
                    .run_if(floater_shown(RENDER_SETTINGS_FLOATER_ID)),
            );
    }
}

/// Register the exception table's sort / width persistence.
fn register_render_settings_table(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    register_table_settings(
        &mut settings,
        RENDER_SETTINGS_SECTION,
        &RENDER_SETTINGS_TABLE,
    );
}

// --- Floater --------------------------------------------------------------

/// Startup: spawn the floater chrome; the content builds on first open.
fn spawn_render_settings_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: RENDER_SETTINGS_FLOATER_ID,
            title: "Avatar Render Settings".to_owned(),
            position: Vec2::new(330.0, 180.0),
            default_size: Some(Vec2::new(620.0, 320.0)),
            min_size: Some(Vec2::new(430.0, 200.0)),
            dock_host: None,
            caps: FloaterCaps {
                resizable: true,
                minimizable: false,
                closable: true,
                dockable: false,
            },
        },
    );
    commands
        .entity(handle.title_text)
        .insert(Translated::new("avatar-render-title"));
    let builder = commands.register_system(build_render_settings_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build: the filter row, the table with its count line, and
/// the trailing action buttons.
fn build_render_settings_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(4.0))
            },
            Name::new("avatar-render-content"),
            ChildOf(handle.content),
        ))
        .id();

    let controls = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new("avatar-render-controls"),
            ChildOf(content),
        ))
        .id();
    let search = spawn_search_field(
        &mut commands,
        controls,
        &SearchFieldSpec {
            tab_index: 0,
            font_size: FONT_SIZE,
            min_width: 160.0,
            placeholder: "Filter the exceptions".to_owned(),
            search_glyph: true,
            ..SearchFieldSpec::new("avatar-render-filter")
        },
    );
    if let Some(placeholder) = search.placeholder {
        commands
            .entity(placeholder)
            .insert(Translated::new("avatar-render-filter-placeholder"));
    }

    let body = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..row(Val::Px(6.0))
            },
            Name::new("avatar-render-body"),
            ChildOf(content),
        ))
        .id();
    let table_column = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(2.0))
            },
            Name::new("avatar-render-list-column"),
            ChildOf(body),
        ))
        .id();
    let table = spawn_table(&mut commands, table_column, &RENDER_SETTINGS_TABLE);
    commands
        .entity(table.viewport)
        .insert((BackgroundColor(LIST_BACKGROUND), TabIndex(1)));
    spawn_virtual_scrollbar(&mut commands, table.viewport);

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
            Name::new("avatar-render-count"),
            ChildOf(table_column),
        ))
        .id();

    let actions = commands
        .spawn((
            Node {
                width: Val::Px(ACTION_COL_WIDTH),
                flex_shrink: 0.0,
                align_items: AlignItems::Stretch,
                ..column(Val::Px(4.0))
            },
            Name::new("avatar-render-actions"),
            ChildOf(body),
        ))
        .id();
    for button in [
        RenderSettingsButton::RenderFully,
        RenderSettingsButton::NeverRender,
        RenderSettingsButton::Remove,
        RenderSettingsButton::AddFully,
        RenderSettingsButton::AddNever,
    ] {
        spawn_render_settings_action(&mut commands, actions, button);
    }

    commands.insert_resource(RenderSettingsUi {
        table: table.root,
        viewport: table.viewport,
        filter_field: search.field,
        count_text,
    });
}

/// Spawn one trailing action button and its press observer.
fn spawn_render_settings_action(
    commands: &mut Commands,
    parent: Entity,
    button: RenderSettingsButton,
) {
    commands
        .spawn((
            button,
            Node {
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(ACTION_BACKGROUND),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("avatar-render-action"),
            ChildOf(parent),
        ))
        .with_child((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Translated::new(button.label_key()),
            Pickable::IGNORE,
        ))
        .observe(
            move |mut press: On<Pointer<Press>>,
                  selected: Res<SelectedRenderException>,
                  store: Res<AvatarRenderSettings>,
                  mut requests: MessageWriter<RequestRenderException>,
                  mut pickers: MessageWriter<OpenAvatarPicker>| {
                press.propagate(false);
                if press.button != PointerButton::Primary {
                    return;
                }
                if let Some(requester) = button.picker_tag() {
                    pickers.write(OpenAvatarPicker { requester });
                    return;
                }
                let (Some(setting), Some(agent)) = (button.setting(), selected.0) else {
                    return;
                };
                // The name the entry already carries rides along, so a
                // re-decision never blanks a resolved name back to an id.
                let name = store
                    .entries()
                    .iter()
                    .find(|entry| entry.agent == agent.uuid())
                    .map(|entry| entry.name.clone())
                    .unwrap_or_default();
                requests.write(RequestRenderException {
                    agent,
                    name,
                    setting,
                });
            },
        );
}

/// Record an exception for someone chosen in the shared avatar picker — the
/// setting is the one whose Add button opened it.
fn handle_render_settings_picks(
    mut picks: MessageReader<AvatarPicked>,
    mut requests: MessageWriter<RequestRenderException>,
) {
    for pick in picks.read() {
        let Some(setting) = picked_setting(pick.requester) else {
            continue;
        };
        requests.write(RequestRenderException {
            agent: pick.agent,
            name: pick.name.clone(),
            setting,
        });
    }
}

// --- View systems (floater open) ------------------------------------------

/// Mirror the filter field's live text into the view state.
fn mirror_render_settings_filter(
    ui: Option<Res<RenderSettingsUi>>,
    fields: Query<&EditableText>,
    mut view: ResMut<RenderSettingsView>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Ok(field) = fields.get(ui.filter_field) else {
        return;
    };
    let term = field.value().to_string();
    if view.filter != term {
        view.filter = term;
    }
}

/// Rebuild the row projection when the list, the sort or the filter moved, and
/// keep the count line and the virtual list's item count in step.
fn rebuild_render_settings_view(
    store: Res<AvatarRenderSettings>,
    mut view: ResMut<RenderSettingsView>,
    ui: Option<Res<RenderSettingsUi>>,
    translator: Translator,
    tables: Query<&TableState>,
    mut lists: Query<&mut VirtualList>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    let sort = tables
        .get(ui.table)
        .ok()
        .map(|table| (table.sort_revision(), table.sort().keys().to_vec()));
    let sort_revision = sort.as_ref().map_or(0, |(revision, _keys)| *revision);
    if view.built_revision == store.revision()
        && view.built_sort_revision == sort_revision
        && view.built_filter == view.filter
    {
        return;
    }
    // Reborrowed so the two filter fields are borrowed disjointly (a `ResMut`
    // deref would borrow the whole resource).
    let view = &mut *view;
    view.built_revision = store.revision();
    view.built_sort_revision = sort_revision;
    view.built_filter.clone_from(&view.filter);

    let total = store.entries().len();
    let mut rows: Vec<ExceptionRow> = store
        .entries()
        .iter()
        .map(|entry| ExceptionRow {
            label: name_label(entry, store.live_name(AgentKey::from(entry.agent))),
            entry: entry.clone(),
        })
        .filter(|row| matches_filter(row, &view.filter))
        .collect();
    let keys: Vec<(&str, bool)> = sort
        .map(|(_revision, keys)| keys)
        .unwrap_or_default()
        .iter()
        .filter_map(|key| {
            RENDER_SETTINGS_TABLE
                .columns
                .get(key.column)
                .map(|column| (column.token, key.ascending))
        })
        .collect();
    sort_rows(&mut rows, &keys);
    view.rows = rows;

    if let Ok(mut list_state) = lists.get_mut(ui.viewport) {
        list_state.item_count = view.rows.len();
    }
    let label = translator.format(
        "avatar-render-count",
        &TransArgs::new()
            .int("shown", i64::try_from(view.rows.len()).unwrap_or(i64::MAX))
            .int("total", i64::try_from(total).unwrap_or(i64::MAX)),
    );
    if let Ok(mut text) = texts.get_mut(ui.count_text)
        && text.0 != label
    {
        text.0 = label;
    }
}

/// Build the cells of each freshly-pooled row and attach the press observer.
fn populate_render_settings_rows(
    mut commands: Commands,
    ui: Option<Res<RenderSettingsUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        spawn_table_row(&mut commands, row_entity, ui.table, &RENDER_SETTINGS_TABLE);
        commands
            .entity(row_entity)
            .insert(BoundRenderException(None))
            .observe(on_render_settings_row_press);
    }
}

/// Select the pressed row (the actions act on the selection).
fn on_render_settings_row_press(
    mut press: On<Pointer<Press>>,
    rows: Query<&BoundRenderException>,
    ui: Res<RenderSettingsUi>,
    mut focus: ResMut<InputFocus>,
    mut selected: ResMut<SelectedRenderException>,
) {
    let Ok(BoundRenderException(Some(agent))) = rows.get(press.entity).copied() else {
        return;
    };
    press.propagate(false);
    if press.button != PointerButton::Primary {
        return;
    }
    focus.set(ui.viewport, FocusCause::Navigated);
    selected.0 = Some(agent);
}

/// Bind each pooled row to the exception it now presents.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the view, the \
              selection, the UI handles, the local time zone and translator the cells render \
              through, and the row / background / text queries they are written into"
)]
fn bind_render_settings_rows(
    view: Res<RenderSettingsView>,
    selected: Res<SelectedRenderException>,
    ui: Option<Res<RenderSettingsUi>>,
    zone: Option<Res<LocalTimeZone>>,
    translator: Translator,
    mut rows: Query<(
        Entity,
        Ref<VirtualRow>,
        &ChildOf,
        &TableRowCells,
        &mut BoundRenderException,
    )>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    let refresh_all = view.is_changed() || selected.is_changed();
    for (row_entity, row, child_of, cells, mut bound) in &mut rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let data = row.index.and_then(|index| view.rows.get(index));
        bound.0 = data.map(|data| AgentKey::from(data.entry.agent));
        let Some(data) = data else {
            for column in 0..RENDER_SETTINGS_TABLE.columns.len() {
                if let Some(cell) = cells.cell(column) {
                    set_table_cell(&mut texts, cell, "", LABEL_COLOR);
                }
            }
            continue;
        };
        let cell_values: [(usize, String, Color); 3] = [
            (COL_NAME, data.label.clone(), LABEL_COLOR),
            (
                COL_SETTING,
                translator.get(data.entry.setting.label_key()),
                LABEL_COLOR,
            ),
            (
                COL_DATE,
                crate::asset_blacklist::format_date(data.entry.added_epoch_secs, zone.as_deref()),
                DIM_LABEL_COLOR,
            ),
        ];
        for (column, value, color) in cell_values {
            if let Some(cell) = cells.cell(column) {
                set_table_cell(&mut texts, cell, &value, color);
            }
        }
        if let Ok(mut background) = backgrounds.get_mut(row_entity) {
            let wanted = if selected.0 == Some(AgentKey::from(data.entry.agent)) {
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

/// How a row names its avatar: the **live** name if the name cache has one (so
/// a resident who has since changed their display name reads as they are now),
/// else the name stored with the decision, else their id in parentheses — the
/// same fallback the other people-listing surfaces use, and the reason the id is
/// filterable.
pub(crate) fn name_label(entry: &RenderException, live: Option<&str>) -> String {
    if let Some(live) = live.map(str::trim).filter(|name| !name.is_empty()) {
        return live.to_owned();
    }
    if entry.name.trim().is_empty() {
        format!("({})", entry.agent)
    } else {
        entry.name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{ExceptionRow, matches_filter, name_label, picked_setting, sort_rows};
    use crate::avatar_complexity::RenderOverride;
    use crate::avatar_render_settings::RenderException;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::Uuid;

    /// An exception entry, for the projection tests.
    fn entry(id: u128, name: &str, setting: RenderOverride, at: i64) -> RenderException {
        RenderException {
            agent: Uuid::from_u128(id),
            name: name.to_owned(),
            setting,
            added_epoch_secs: at,
        }
    }

    /// A display row for `entry`, labelled the way the floater would with no
    /// live name resolved.
    fn row(entry: RenderException) -> ExceptionRow {
        ExceptionRow {
            label: name_label(&entry, None),
            entry,
        }
    }

    /// The filter matches the shown name or the id, case-insensitively; a blank
    /// filter keeps everything.
    #[test]
    fn filter_matches_name_or_id() {
        let listed = row(entry(0x1234, "Alpha Resident", RenderOverride::Never, 0));
        assert!(matches_filter(&listed, ""));
        assert!(matches_filter(&listed, "  "));
        assert!(matches_filter(&listed, "alpha"));
        assert!(matches_filter(&listed, "RESIDENT"));
        assert!(matches_filter(&listed, "1234"));
        assert!(!matches_filter(&listed, "beta"));
    }

    /// Each sort key orders on its own column, and the shown name is the
    /// tie-break.
    #[test]
    fn sort_keys_order_their_column() {
        let mut rows = vec![
            row(entry(1, "Beta", RenderOverride::Never, 200)),
            row(entry(2, "Alpha", RenderOverride::AlwaysFull, 100)),
        ];
        sort_rows(&mut rows, &[("name", true)]);
        assert_eq!(
            rows.iter().map(|row| row.label.clone()).collect::<Vec<_>>(),
            vec!["Alpha", "Beta"]
        );
        sort_rows(&mut rows, &[("date", false)]);
        assert_eq!(
            rows.iter()
                .map(|row| row.entry.added_epoch_secs)
                .collect::<Vec<_>>(),
            vec![200, 100]
        );
        sort_rows(&mut rows, &[("setting", true)]);
        assert_eq!(
            rows.iter().map(|row| row.entry.setting).collect::<Vec<_>>(),
            vec![RenderOverride::AlwaysFull, RenderOverride::Never]
        );
    }

    /// The row reads by the live name when the cache has one — a resident who
    /// renamed since the decision reads as they are now — then by the name
    /// stored with the decision, and only then by the raw id, so a row is never
    /// blank. A live name a grid answered with nothing is not one.
    #[test]
    fn the_row_reads_live_name_then_stored_then_id() {
        let named = entry(1, "Alpha Resident", RenderOverride::Never, 0);
        assert_eq!(name_label(&named, Some("Alpha Renamed")), "Alpha Renamed");
        assert_eq!(name_label(&named, Some("  ")), "Alpha Resident");
        assert_eq!(name_label(&named, None), "Alpha Resident");
        let bare = entry(1, "  ", RenderOverride::Never, 0);
        let label = name_label(&bare, None);
        assert!(
            bare.agent
                .to_string()
                .contains(label.trim_matches(|character| character == '(' || character == ')')),
            "expected the id in parentheses, got {label}"
        );
    }

    /// Each Add button's picker tag records its own setting, and a pick from any
    /// other feature's picker is ignored.
    #[test]
    fn picks_record_the_button_that_asked() {
        assert_eq!(
            picked_setting(super::PICKER_ADD_FULLY),
            Some(RenderOverride::AlwaysFull)
        );
        assert_eq!(
            picked_setting(super::PICKER_ADD_NEVER),
            Some(RenderOverride::Never)
        );
        assert_eq!(picked_setting("inventory-share"), None);
    }
}
