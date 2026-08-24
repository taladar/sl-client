//! The **Asset Blacklist** floater (`viewer-derender-blacklist`): the list of
//! everything this avatar has derendered, and the only way back.
//!
//! The model — what is suppressed, how, and its persistence — is
//! [`crate::derender`]; this is its surface, laid out like the radar and the
//! block list beside it: a filter box over a sortable, virtualized table
//! (Name / Region / Type / Date / Permanent) with the actions at its trailing
//! edge.
//!
//! - **Re-render** drops the selected entry: the suppression stops *and* the
//!   objects it was hiding are re-fetched from the simulator, so they come back
//!   within the round trip rather than at the next region stream (see
//!   [`crate::derender`] — the reference only forgets the entry and leaves you
//!   to teleport away and back).
//! - **Clear temporary** drops every session-only entry at once (the
//!   reference's "Clear temporary" button).
//!
//! Every [`DerenderKind`](crate::world_api::DerenderKind) is listed here, asset
//! entries included: the model honours a blacklisted sound / animation / texture
//! at its own point of use, so this is where one is seen and removed even though
//! no surface produces one yet (the explorer floaters will).
//!
//! # Deliberate divergences from the reference
//!
//! - **No Play / Stop Sound buttons.** They preview a blacklisted *sound*
//!   asset; the sound explorer that produces such entries is a separate task,
//!   so there is nothing to preview yet.
//! - **No Flags column.** The reference's flags (silence an avatar's worn /
//!   rezzed / gesture sounds) are part of that same sound work.
//! - **Single-select.** As in the block list, the table selects one row; the
//!   reference's multi-select removal is a Firestorm addition.
//!
//! Reference (Firestorm, read-only): `fsfloaterassetblacklist`,
//! `floater_fs_asset_blacklist.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::Uuid;

use crate::derender::{DerenderEntry, DerenderList, UnDerender};
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
pub(crate) const BLACKLIST_FLOATER_ID: &str = "asset-blacklist";

/// The persisted-settings section the table's sort / widths live under.
const BLACKLIST_SECTION: &[&str] = &["blacklist"];

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

/// The glyph marking a permanent (blacklisted) entry — the reference's ✔.
const PERMANENT_GLYPH: &str = "\u{2714}";

// --- Table ----------------------------------------------------------------

/// Column index of the entry's name.
const COL_NAME: usize = 0;
/// Column index of the region it was derendered in.
const COL_REGION: usize = 1;
/// Column index of the entry kind.
const COL_TYPE: usize = 2;
/// Column index of the date it was added.
const COL_DATE: usize = 3;
/// Column index of the permanent marker.
const COL_PERMANENT: usize = 4;

/// The blacklist table: a flexible name beside the fixed region / type / date /
/// permanent cells, sorted by name ascending by default (the reference's
/// `sort_column="0"`).
static BLACKLIST_TABLE: TableSpec = TableSpec {
    element: "asset-blacklist",
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "blacklist-col-name",
            token: "name",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "blacklist-col-region",
            token: "region",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 110.0 },
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "blacklist-col-type",
            token: "type",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 74.0 },
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "blacklist-col-date",
            token: "date",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 128.0 },
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "blacklist-col-permanent",
            token: "permanent",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 76.0 },
            align: TableAlign::Center,
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
    sort_setting: Some("BlacklistSortOrder"),
    widths_setting: Some("BlacklistColumnWidths"),
};

// --- Pure view model ------------------------------------------------------

/// Whether `entry` survives the list's filter — a case-insensitive substring of
/// the name *or* the region, so "that thing in Sandbox" is findable either way.
pub(crate) fn matches_filter(entry: &DerenderEntry, filter: &str) -> bool {
    let filter = filter.trim().to_lowercase();
    filter.is_empty()
        || entry.name.to_lowercase().contains(&filter)
        || entry.region.to_lowercase().contains(&filter)
}

/// Order `rows` by the table's sort keys (most significant first), falling back
/// to a case-insensitive name compare so the order is total.
pub(crate) fn sort_rows(rows: &mut [DerenderEntry], keys: &[(&str, bool)]) {
    rows.sort_by(|left, right| {
        for (token, ascending) in keys {
            let ordering = match *token {
                "region" => left.region.to_lowercase().cmp(&right.region.to_lowercase()),
                "type" => left.kind.rank().cmp(&right.kind.rank()),
                "date" => left.added_epoch_secs.cmp(&right.added_epoch_secs),
                "permanent" => left.permanent.cmp(&right.permanent),
                _name => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
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
        left.name.to_lowercase().cmp(&right.name.to_lowercase())
    });
}

/// The local-time `YYYY-MM-DD hh:mm` stamp of an entry's epoch seconds, or an
/// empty string when the value is out of the representable range.
pub(crate) fn format_date(epoch_secs: i64, zone: Option<&LocalTimeZone>) -> String {
    let Ok(stamp) = jiff::Timestamp::from_second(epoch_secs) else {
        return String::new();
    };
    let zone = zone.map_or_else(jiff::tz::TimeZone::system, |zone| zone.zone().clone());
    stamp.to_zoned(zone).strftime("%Y-%m-%d %H:%M").to_string()
}

// --- Resources ------------------------------------------------------------

/// The floater's live view state: the filter and the ordered rows the virtual
/// list binds, plus the stamps they were built against.
#[derive(Resource, Debug, Default)]
struct BlacklistView {
    /// The live filter text.
    filter: String,
    /// The display rows, in table order.
    rows: Vec<DerenderEntry>,
    /// The derender-list revision the rows were built at.
    built_revision: u64,
    /// The table sort revision the rows were ordered at.
    built_sort_revision: u64,
    /// The filter the rows were filtered by.
    built_filter: String,
}

/// The selected entry's id, which the action buttons act on. Keyed by id (not
/// row index) so it survives the re-sort and the virtualized row recycling.
#[derive(Resource, Debug, Default)]
struct SelectedBlacklistEntry(Option<Uuid>);

/// The floater's retained entities (inserted by the deferred content build;
/// consumers take `Option<Res<BlacklistUi>>` until then).
#[derive(Resource, Debug)]
struct BlacklistUi {
    /// The table root (carries [`TableState`]).
    table: Entity,
    /// The virtualized viewport (carries [`VirtualList`]).
    viewport: Entity,
    /// The filter box's [`EditableText`] entity.
    filter_field: Entity,
    /// The entry-count line.
    count_text: Entity,
}

/// The entry a pooled row currently presents.
#[derive(Component, Debug, Clone, Copy, Default)]
struct BoundBlacklist(Option<Uuid>);

/// What a trailing action button does.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum BlacklistButton {
    /// Drop the selected entry from the blacklist.
    ReRender,
    /// Drop every temporary entry.
    ClearTemporary,
}

impl BlacklistButton {
    /// The Fluent key for this button's label.
    const fn label_key(self) -> &'static str {
        match self {
            Self::ReRender => "blacklist-action-rerender",
            Self::ClearTemporary => "blacklist-action-clear-temporary",
        }
    }
}

// --- Plugin ---------------------------------------------------------------

/// Registers the Asset Blacklist floater, its view state and its actions.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AssetBlacklistPlugin;

impl Plugin for AssetBlacklistPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlacklistView>()
            .init_resource::<SelectedBlacklistEntry>()
            .add_systems(
                Startup,
                (
                    register_blacklist_settings,
                    spawn_blacklist_floater.after(UiScaffoldSystems::SpawnRoot),
                ),
            )
            .add_systems(
                Update,
                (mirror_blacklist_filter, rebuild_blacklist_view)
                    .chain()
                    .before(layout_virtual_lists)
                    .run_if(floater_shown(BLACKLIST_FLOATER_ID)),
            )
            .add_systems(
                Update,
                (populate_blacklist_rows, bind_blacklist_rows)
                    .chain()
                    .after(layout_virtual_lists)
                    .run_if(floater_shown(BLACKLIST_FLOATER_ID)),
            );
    }
}

/// Register the blacklist table's sort / width persistence.
fn register_blacklist_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    register_table_settings(&mut settings, BLACKLIST_SECTION, &BLACKLIST_TABLE);
}

// --- Floater --------------------------------------------------------------

/// Startup: spawn the floater chrome; the content builds on first open.
fn spawn_blacklist_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: BLACKLIST_FLOATER_ID,
            title: "Asset Blacklist".to_owned(),
            position: Vec2::new(300.0, 160.0),
            default_size: Some(Vec2::new(660.0, 340.0)),
            min_size: Some(Vec2::new(440.0, 200.0)),
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
        .insert(Translated::new("blacklist-title"));
    let builder = commands.register_system(build_blacklist_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build: the filter row, the table with its count line, and
/// the trailing action buttons.
fn build_blacklist_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(4.0))
            },
            Name::new("blacklist-content"),
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
            Name::new("blacklist-controls"),
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
            placeholder: "Filter the blacklist".to_owned(),
            search_glyph: true,
            ..SearchFieldSpec::new("blacklist-filter")
        },
    );
    if let Some(placeholder) = search.placeholder {
        commands
            .entity(placeholder)
            .insert(Translated::new("blacklist-filter-placeholder"));
    }

    let body = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..row(Val::Px(6.0))
            },
            Name::new("blacklist-body"),
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
            Name::new("blacklist-list-column"),
            ChildOf(body),
        ))
        .id();
    let table = spawn_table(&mut commands, table_column, &BLACKLIST_TABLE);
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
            Name::new("blacklist-count"),
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
            Name::new("blacklist-actions"),
            ChildOf(body),
        ))
        .id();
    for button in [BlacklistButton::ReRender, BlacklistButton::ClearTemporary] {
        spawn_blacklist_action(&mut commands, actions, button);
    }

    commands.insert_resource(BlacklistUi {
        table: table.root,
        viewport: table.viewport,
        filter_field: search.field,
        count_text,
    });
}

/// Spawn one trailing action button and its press observer.
fn spawn_blacklist_action(commands: &mut Commands, parent: Entity, button: BlacklistButton) {
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
            Name::new("blacklist-action"),
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
                  mut selected: ResMut<SelectedBlacklistEntry>,
                  mut requests: MessageWriter<UnDerender>| {
                press.propagate(false);
                if press.button != PointerButton::Primary {
                    return;
                }
                match button {
                    BlacklistButton::ReRender => {
                        if let Some(id) = selected.0.take() {
                            requests.write(UnDerender { id });
                        }
                    }
                    // A nil id is the model's "every temporary entry" request.
                    BlacklistButton::ClearTemporary => {
                        requests.write(UnDerender { id: Uuid::nil() });
                    }
                }
            },
        );
}

// --- View systems (floater open) ------------------------------------------

/// Mirror the filter field's live text into the view state.
fn mirror_blacklist_filter(
    ui: Option<Res<BlacklistUi>>,
    fields: Query<&EditableText>,
    mut view: ResMut<BlacklistView>,
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
fn rebuild_blacklist_view(
    list: Res<DerenderList>,
    mut view: ResMut<BlacklistView>,
    ui: Option<Res<BlacklistUi>>,
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
    if view.built_revision == list.revision()
        && view.built_sort_revision == sort_revision
        && view.built_filter == view.filter
    {
        return;
    }
    // Reborrowed so the two filter fields are borrowed disjointly (a `ResMut`
    // deref would borrow the whole resource).
    let view = &mut *view;
    view.built_revision = list.revision();
    view.built_sort_revision = sort_revision;
    view.built_filter.clone_from(&view.filter);

    let total = list.entries().len();
    let mut rows: Vec<DerenderEntry> = list
        .entries()
        .iter()
        .filter(|entry| matches_filter(entry, &view.filter))
        .cloned()
        .collect();
    let keys: Vec<(&str, bool)> = sort
        .map(|(_revision, keys)| keys)
        .unwrap_or_default()
        .iter()
        .filter_map(|key| {
            BLACKLIST_TABLE
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
        "blacklist-count",
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
fn populate_blacklist_rows(
    mut commands: Commands,
    ui: Option<Res<BlacklistUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        spawn_table_row(&mut commands, row_entity, ui.table, &BLACKLIST_TABLE);
        commands
            .entity(row_entity)
            .insert(BoundBlacklist(None))
            .observe(on_blacklist_row_press);
    }
}

/// Select the pressed row (the actions act on the selection).
fn on_blacklist_row_press(
    mut press: On<Pointer<Press>>,
    rows: Query<&BoundBlacklist>,
    ui: Res<BlacklistUi>,
    mut focus: ResMut<InputFocus>,
    mut selected: ResMut<SelectedBlacklistEntry>,
) {
    let Ok(BoundBlacklist(Some(id))) = rows.get(press.entity).copied() else {
        return;
    };
    press.propagate(false);
    if press.button != PointerButton::Primary {
        return;
    }
    focus.set(ui.viewport, FocusCause::Navigated);
    selected.0 = Some(id);
}

/// Bind each pooled row to the entry it now presents.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the view, the \
              selection, the UI handles, the local time zone and translator the cells render \
              through, and the row / background / text queries they are written into"
)]
fn bind_blacklist_rows(
    view: Res<BlacklistView>,
    selected: Res<SelectedBlacklistEntry>,
    ui: Option<Res<BlacklistUi>>,
    zone: Option<Res<LocalTimeZone>>,
    translator: Translator,
    mut rows: Query<(
        Entity,
        Ref<VirtualRow>,
        &ChildOf,
        &TableRowCells,
        &mut BoundBlacklist,
    )>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    let refresh_all = view.is_changed() || selected.is_changed();
    let unnamed = translator.get("blacklist-unnamed");
    let unknown_region = translator.get("blacklist-unknown-region");
    for (row_entity, row, child_of, cells, mut bound) in &mut rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let data = row.index.and_then(|index| view.rows.get(index));
        bound.0 = data.map(|data| data.id);
        let Some(data) = data else {
            for column in 0..BLACKLIST_TABLE.columns.len() {
                if let Some(cell) = cells.cell(column) {
                    set_table_cell(&mut texts, cell, "", LABEL_COLOR);
                }
            }
            continue;
        };
        let name = if data.name.trim().is_empty() {
            unnamed.clone()
        } else {
            data.name.clone()
        };
        let region = if data.region.trim().is_empty() {
            unknown_region.clone()
        } else {
            data.region.clone()
        };
        let cell_values: [(usize, String, Color); 5] = [
            (COL_NAME, name, LABEL_COLOR),
            (COL_REGION, region, DIM_LABEL_COLOR),
            (COL_TYPE, translator.get(data.kind.label_key()), LABEL_COLOR),
            (
                COL_DATE,
                format_date(data.added_epoch_secs, zone.as_deref()),
                DIM_LABEL_COLOR,
            ),
            (
                COL_PERMANENT,
                if data.permanent {
                    PERMANENT_GLYPH.to_owned()
                } else {
                    String::new()
                },
                LABEL_COLOR,
            ),
        ];
        for (column, value, color) in cell_values {
            if let Some(cell) = cells.cell(column) {
                set_table_cell(&mut texts, cell, &value, color);
            }
        }
        if let Ok(mut background) = backgrounds.get_mut(row_entity) {
            let wanted = if selected.0 == Some(data.id) {
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

#[cfg(test)]
mod tests {
    use super::{format_date, matches_filter, sort_rows};
    use crate::derender::DerenderEntry;
    use crate::world_api::DerenderKind;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::Uuid;

    /// An entry, for the projection tests.
    fn entry(id: u128, name: &str, region: &str, permanent: bool, at: i64) -> DerenderEntry {
        DerenderEntry {
            id: Uuid::from_u128(id),
            name: name.to_owned(),
            region: region.to_owned(),
            kind: DerenderKind::Object,
            permanent,
            added_epoch_secs: at,
        }
    }

    /// The filter matches the name or the region, case-insensitively; a blank
    /// filter keeps everything.
    #[test]
    fn filter_matches_name_or_region() {
        let row = entry(1, "Spinning Cube", "Sandbox Island", true, 0);
        assert!(matches_filter(&row, ""));
        assert!(matches_filter(&row, "  "));
        assert!(matches_filter(&row, "spinning"));
        assert!(matches_filter(&row, "ISLAND"));
        assert!(!matches_filter(&row, "teapot"));
    }

    /// Each sort key orders on its own column, and the name is the tie-break.
    #[test]
    fn sort_keys_order_their_column() {
        let mut rows = vec![
            entry(1, "Beta", "Zeta", false, 200),
            entry(2, "Alpha", "Yankee", true, 100),
        ];
        sort_rows(&mut rows, &[("name", true)]);
        assert_eq!(
            rows.iter().map(|row| row.name.clone()).collect::<Vec<_>>(),
            vec!["Alpha", "Beta"]
        );
        sort_rows(&mut rows, &[("date", false)]);
        assert_eq!(
            rows.iter()
                .map(|row| row.added_epoch_secs)
                .collect::<Vec<_>>(),
            vec![200, 100]
        );
        sort_rows(&mut rows, &[("permanent", true)]);
        assert_eq!(
            rows.iter().map(|row| row.permanent).collect::<Vec<_>>(),
            vec![false, true]
        );
        sort_rows(&mut rows, &[("region", true)]);
        assert_eq!(
            rows.iter()
                .map(|row| row.region.clone())
                .collect::<Vec<_>>(),
            vec!["Yankee", "Zeta"]
        );
    }

    /// A stamp renders as a local `YYYY-MM-DD hh:mm`; an out-of-range value
    /// renders empty rather than panicking.
    #[test]
    fn dates_render_or_fall_back() {
        let rendered = format_date(1_700_000_000, None);
        assert_eq!(
            rendered.len(),
            16,
            "expected YYYY-MM-DD hh:mm, got {rendered}"
        );
        assert!(format_date(i64::MAX, None).is_empty());
    }
}
