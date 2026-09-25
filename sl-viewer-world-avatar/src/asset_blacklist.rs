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
//! Every [`DerenderKind`] is listed here, asset
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
use bevy::ui_widgets::Activate;
use bevy_flair::style::components::{ClassList, PseudoElementsSupport};
use sl_client_bevy::Uuid;
use sl_viewer_ui_core::glyph;
use sl_viewer_ui_core::skin::{SELECTED_CLASS, set_state_class, text_role};
use sl_viewer_ui_core::skin_palette::SkinPalette;

use crate::derender::UnDerender;
use sl_viewer_platform::local_time::LocalTimeZone;
use sl_viewer_settings::ViewerSettings;
use sl_viewer_ui_core::i18n::{TransArgs, Translated, Translator};
use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::ui_spawn::{self, ButtonKind, ButtonSpec, UiLabel};
use sl_viewer_ui_core::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists};
use sl_viewer_ui_widgets::floater::{
    DeferredFloaterContent, FloaterCaps, FloaterHandle, FloaterSpec, floater_shown, spawn_floater,
};
use sl_viewer_ui_widgets::ui_search::{SearchFieldSpec, spawn_search_field};
use sl_viewer_ui_widgets::ui_table::{
    SpecimenTable, TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableRowCells,
    TableSelectionMode, TableSortDefault, TableSpec, TableState, order_by_sort_keys,
    register_table_settings, set_table_cell, spawn_specimen_table_rows, spawn_table,
    spawn_table_row,
};
use sl_viewer_world_api::{DerenderEntry, DerenderKind, DerenderList};

/// The floater's stable id (persistence, `SL_VIEWER_OPEN_FLOATER`).
pub const BLACKLIST_FLOATER_ID: &str = "asset-blacklist";

/// The persisted-settings section the table's sort / widths live under.
const BLACKLIST_SECTION: &[&str] = &["blacklist"];

// --- Palette / geometry (the sibling list floaters' values) ---------------

/// Header / cell font size, logical px.
const FONT_SIZE: f32 = 13.0;

/// Table row height, logical px.
const ROW_HEIGHT: f32 = 20.0;

/// The default cell / label colour.
const LABEL_COLOR: Color = SkinPalette::FALLBACK.text_primary;

/// The dimmed header / secondary colour.
const DIM_LABEL_COLOR: Color = SkinPalette::FALLBACK.text_muted;

/// An action button's background.
const ACTION_BACKGROUND: Color = Color::srgb(0.24, 0.29, 0.38);

/// The trailing action column's width, logical px.
const ACTION_COL_WIDTH: f32 = 140.0;

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
#[must_use]
pub fn matches_filter(entry: &DerenderEntry, filter: &str) -> bool {
    let filter = filter.trim().to_lowercase();
    filter.is_empty()
        || entry.name.to_lowercase().contains(&filter)
        || entry.region.to_lowercase().contains(&filter)
}

/// Order `rows` by the table's sort keys (most significant first), falling back
/// to a case-insensitive name compare so the order is total.
pub fn sort_rows(rows: &mut [DerenderEntry], keys: &[(&str, bool)]) {
    order_by_sort_keys(
        rows,
        keys,
        |token, left, right| match *token {
            "region" => left.region.to_lowercase().cmp(&right.region.to_lowercase()),
            "type" => left.kind.rank().cmp(&right.kind.rank()),
            "date" => left.added_epoch_secs.cmp(&right.added_epoch_secs),
            "permanent" => left.permanent.cmp(&right.permanent),
            _name => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
        },
        |left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()),
    );
}

/// The local-time `YYYY-MM-DD hh:mm` stamp of an entry's epoch seconds, or an
/// empty string when the value is out of the representable range.
#[must_use]
pub fn format_date(epoch_secs: i64, zone: Option<&LocalTimeZone>) -> String {
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
pub struct AssetBlacklistPlugin;

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

/// The blacklist floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn blacklist_floater_spec() -> FloaterSpec {
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
    }
}

/// Startup: spawn the floater chrome; the content builds on first open.
fn spawn_blacklist_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, blacklist_floater_spec());
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
    let ui = spawn_blacklist_content(&mut commands, handle.content, FONT_SIZE);
    commands.insert_resource(ui);
}

/// Build the floater's content into `parent` at `font_size`: the filter row,
/// the table with its count line, and the trailing action buttons. Shared by
/// the live floater's first-open build and its specimen.
fn spawn_blacklist_content(commands: &mut Commands, parent: Entity, font_size: f32) -> BlacklistUi {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(4.0))
            },
            Name::new("blacklist-content"),
            ChildOf(parent),
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
        commands,
        controls,
        &SearchFieldSpec {
            tab_index: 0,
            font_size,
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
    let table = spawn_table(commands, table_column, &BLACKLIST_TABLE);
    commands.entity(table.viewport).insert(TabIndex(1));

    let count_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(font_size),
            text_role(DIM_LABEL_COLOR),
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
        spawn_blacklist_action(commands, actions, button, font_size);
    }

    BlacklistUi {
        table: table.root,
        viewport: table.viewport,
        filter_field: search.field,
        count_text,
    }
}

/// Spawn one trailing action button — the button widget in the flat
/// action-column shape — and its `Activate` observer, which a primary click and
/// `Enter` / `Space` on the focused button raise alike.
///
/// The selection and the request queue are optional so an activation in a host that
/// has neither (the gallery's specimen) is a no-op rather than a failed
/// observer.
fn spawn_blacklist_action(
    commands: &mut Commands,
    parent: Entity,
    button: BlacklistButton,
    font_size: f32,
) {
    let entity = ui_spawn::spawn_button(
        commands,
        parent,
        ButtonSpec::flat(UiLabel::key(button.label_key()), "blacklist-action")
            .kind(ButtonKind::Headless)
            // Behind the filter (0) and the table (1) in the tab cycle.
            .tab_index(2)
            .colors(ACTION_BACKGROUND, ACTION_BACKGROUND)
            .label_color(LABEL_COLOR)
            .font_size(font_size),
    )
    .button;
    commands.entity(entity).insert(button).observe(
        move |_activate: On<Activate>,
              selected: Option<ResMut<SelectedBlacklistEntry>>,
              requests: Option<MessageWriter<UnDerender>>| {
            let (Some(mut selected), Some(mut requests)) = (selected, requests) else {
                return;
            };
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
    let (sort_revision, keys) = tables
        .get(ui.table)
        .map(TableState::sort_stamp)
        .unwrap_or_default();
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
        let cells = spawn_table_row(&mut commands, row_entity, ui.table, &BLACKLIST_TABLE);
        // The Permanent column holds no text: a permanent entry's cell is the
        // skin's `glyph::YES` mark (the reference's ✔), a temporary one's is
        // empty. `bind_blacklist_rows` says which.
        if let Some(cell) = cells.cell(COL_PERMANENT) {
            commands.entity(cell).insert(PseudoElementsSupport);
        }
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

/// The cells one table row is painted into, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the rows themselves, their
/// backgrounds and the texts inside them.
#[derive(Debug, bevy::ecs::system::SystemParam)]
pub struct RowCells<'w, 's> {
    /// The virtual rows and what each is currently bound to.
    rows: Query<
        'w,
        's,
        (
            Entity,
            Ref<'static, VirtualRow>,
            &'static ChildOf,
            &'static TableRowCells,
            &'static mut BoundBlacklist,
        ),
    >,
    /// Each row's selection class. `Without<Text>` because the cell query
    /// below also reaches a `ClassList` — the cell text's — and two unfiltered
    /// `&mut ClassList` in one bundle are Bevy's B0001, a panic on the first
    /// run. A row carries no `Text`, so the two are provably disjoint.
    classes: Query<'w, 's, &'static mut ClassList, Without<Text>>,
    /// The cell texts, their colours, and the role class the colour names.
    texts: Query<
        'w,
        's,
        (
            &'static mut Text,
            &'static mut TextColor,
            Option<&'static mut ClassList>,
        ),
    >,
}

/// Bind each pooled row to the entry it now presents.
fn bind_blacklist_rows(
    view: Res<BlacklistView>,
    selected: Res<SelectedBlacklistEntry>,
    ui: Option<Res<BlacklistUi>>,
    zone: Option<Res<LocalTimeZone>>,
    translator: Translator,
    mut table: RowCells,
) {
    let Some(ui) = ui else {
        return;
    };
    let refresh_all = view.is_changed() || selected.is_changed();
    let unnamed = translator.get("blacklist-unnamed");
    let unknown_region = translator.get("blacklist-unknown-region");
    for (row_entity, row, child_of, cells, mut bound) in &mut table.rows {
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
                    set_table_cell(&mut table.texts, cell, "", LABEL_COLOR);
                }
            }
            set_permanent_mark(&mut table.texts, cells, false);
            continue;
        };
        let cell_values = blacklist_row_values(
            data,
            &unnamed,
            &unknown_region,
            translator.get(data.kind.label_key()),
            zone.as_deref(),
        );
        for (column, value, color) in cell_values {
            if let Some(cell) = cells.cell(column) {
                set_table_cell(&mut table.texts, cell, &value, color);
            }
        }
        set_permanent_mark(&mut table.texts, cells, data.permanent);
        if let Ok(mut classes) = table.classes.get_mut(row_entity) {
            set_state_class(&mut classes, SELECTED_CLASS, selected.0 == Some(data.id));
        }
    }
}

/// Show or clear the Permanent column's [`glyph::YES`] mark on a row. The
/// cell is always a glyph host (so the mark takes the cell's role colour);
/// only the slot comes and goes, and without it the host matches the empty
/// baseline.
fn set_permanent_mark(
    texts: &mut Query<(&mut Text, &mut TextColor, Option<&mut ClassList>)>,
    cells: &TableRowCells,
    permanent: bool,
) {
    let Some(cell) = cells.cell(COL_PERMANENT) else {
        return;
    };
    if let Ok((_, _, Some(mut classes))) = texts.get_mut(cell) {
        permanent_mark_classes(&mut classes, permanent);
    }
}

/// The Permanent cell's classes: always a glyph host, with the
/// [`glyph::YES`] slot only while the entry is permanent.
fn permanent_mark_classes(classes: &mut Mut<'_, ClassList>, permanent: bool) {
    set_state_class(classes, glyph::GLYPH_CLASS, true);
    set_state_class(classes, glyph::YES, permanent);
}

/// The `(column, value, colour)` cells of one blacklist row. A blank name or
/// region shows `unnamed` / `unknown_region`; `type_label` is the entry kind's
/// translated name. The Permanent cell holds no text — its mark is a class
/// ([`permanent_mark_classes`]).
fn blacklist_row_values(
    data: &DerenderEntry,
    unnamed: &str,
    unknown_region: &str,
    type_label: String,
    zone: Option<&LocalTimeZone>,
) -> [(usize, String, Color); 5] {
    let name = if data.name.trim().is_empty() {
        unnamed.to_owned()
    } else {
        data.name.clone()
    };
    let region = if data.region.trim().is_empty() {
        unknown_region.to_owned()
    } else {
        data.region.clone()
    };
    [
        (COL_NAME, name, LABEL_COLOR),
        (COL_REGION, region, DIM_LABEL_COLOR),
        (COL_TYPE, type_label, LABEL_COLOR),
        (
            COL_DATE,
            format_date(data.added_epoch_secs, zone),
            DIM_LABEL_COLOR,
        ),
        (COL_PERMANENT, String::new(), LABEL_COLOR),
    ]
}

// --- Gallery specimen -----------------------------------------------------

/// The Asset Blacklist floater's gallery / `ui_test` specimen: the live
/// content, built by the same `spawn_blacklist_content` the floater is, with
/// the table filled from sample entries through the live filter-free
/// projection — [`sort_rows`] in the table's default order, then the live
/// cell mapping (`blacklist_row_values`) and Permanent mark
/// (`permanent_mark_classes`). The first row is shown selected.
///
/// The Type cell is the one the live bind translates; with no translator in a
/// specimen host it is bound to its key as a [`Translated`] label instead,
/// which resolves to the same string. The rows carry no press observer — the
/// selection it would write lives in resources a specimen host has none of.
pub fn spawn_blacklist_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: sl_viewer_ui_core::ui_element::ElementCx,
) -> Entity {
    let ui = spawn_blacklist_content(commands, parent, cx.font_size);
    let table = SpecimenTable {
        root: ui.table,
        viewport: ui.viewport,
    };
    let mut rows = specimen_entries(cx);
    let total = rows.len();
    let keys: Vec<(&str, bool)> = BLACKLIST_TABLE
        .default_sort
        .iter()
        .filter_map(|key| {
            BLACKLIST_TABLE
                .columns
                .get(key.column)
                .map(|column| (column.token, key.ascending))
        })
        .collect();
    sort_rows(&mut rows, &keys);
    let values: Vec<Vec<(String, Color)>> = rows
        .iter()
        .map(|data| {
            let mut cells = vec![(String::new(), LABEL_COLOR); BLACKLIST_TABLE.columns.len()];
            for (column, value, color) in blacklist_row_values(data, "", "", String::new(), None) {
                if let Some(cell) = cells.get_mut(column) {
                    *cell = (value, color);
                }
            }
            cells
        })
        .collect();
    let bound = spawn_specimen_table_rows(commands, table, &BLACKLIST_TABLE, &values);
    for (index, (data, (row, cells))) in rows.iter().zip(&bound).enumerate() {
        if let Some(cell) = cells.cell(COL_TYPE) {
            commands
                .entity(cell)
                .insert(Translated::new(data.kind.label_key()));
        }
        if let Some(cell) = cells.cell(COL_PERMANENT) {
            let permanent = data.permanent;
            commands
                .entity(cell)
                .insert(PseudoElementsSupport)
                .entry::<ClassList>()
                .and_modify(move |mut classes| permanent_mark_classes(&mut classes, permanent));
        }
        commands
            .entity(*row)
            .insert(BoundBlacklist(Some(data.id)))
            .entry::<ClassList>()
            .and_modify(move |mut classes| {
                set_state_class(&mut classes, SELECTED_CLASS, index == 0);
            });
    }
    // The live line is `blacklist-count` formatted with the two counts; the
    // specimen has no translator to format with, so it writes the English
    // sentence that key produces.
    commands.entity(ui.count_text).insert(Text::new(
        cx.text(&format!("{} of {total} blacklisted", rows.len())),
    ));
    parent
}

/// A fixed sample of blacklist entries, one of each common kind, temporary and
/// permanent — no real residents' or regions' names.
fn specimen_entries(cx: sl_viewer_ui_core::ui_element::ElementCx) -> Vec<DerenderEntry> {
    [
        (
            1_u128,
            "Flashing Sign",
            "Sample Region",
            DerenderKind::Object,
            true,
            1_750_000_000,
        ),
        (
            2,
            "Sample Resident",
            "Test Parcel Region",
            DerenderKind::Resident,
            false,
            1_750_086_400,
        ),
        (
            3,
            "Particle Fountain",
            "Sample Region",
            DerenderKind::Object,
            false,
            1_750_172_800,
        ),
        (
            4,
            "Loud Door Chime",
            "Example Island",
            DerenderKind::Sound,
            true,
            1_750_259_200,
        ),
    ]
    .into_iter()
    .map(
        |(id, name, region, kind, permanent, added_epoch_secs)| DerenderEntry {
            id: Uuid::from_u128(0x6b1e_0000_0000_4000_8000_0000_0000_0000 | id),
            name: cx.text(name),
            region: cx.text(region),
            kind,
            permanent,
            added_epoch_secs,
        },
    )
    .collect()
}

#[cfg(test)]
mod tests {
    use super::{format_date, matches_filter, sort_rows};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::Uuid;
    use sl_viewer_world_api::DerenderEntry;
    use sl_viewer_world_api::DerenderKind;

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

    /// **A blacklist action button acts from the keyboard as from the mouse.**
    ///
    /// The action column used to be hand-rolled boxes observing
    /// `Pointer<Press>`: no tab stop, and nothing for `Enter` / `Space` to
    /// reach. On the button widget each gesture, in a fresh app driven through
    /// the real input and focus stack, must raise the same one request.
    #[test]
    fn a_blacklist_action_acts_on_enter_space_and_a_click() -> Result<(), String> {
        use super::{BlacklistButton, SelectedBlacklistEntry, spawn_blacklist_action};
        use crate::derender::UnDerender;
        use bevy::input::keyboard::Key;
        use bevy::prelude::*;
        use sl_viewer_testkit::interact::{self, InteractionTest};
        use sl_viewer_testkit::{drain, find_by_name, record, settle};
        use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems};

        /// The Clear-temporary button's node name, per `spawn_blacklist_action`.
        const CLEAR: &str = "blacklist-action";

        /// The ids one gesture on the Clear-temporary button asked to drop.
        fn requests_after(
            gesture: fn(&mut App, Entity) -> Result<(), String>,
        ) -> Result<Vec<Uuid>, String> {
            let mut app = InteractionTest::new().build();
            record::<UnDerender>(&mut app);
            app.init_resource::<SelectedBlacklistEntry>();
            app.add_systems(
                Startup,
                (|mut commands: Commands, root: Res<UiRoot>| {
                    spawn_blacklist_action(
                        &mut commands,
                        root.0,
                        BlacklistButton::ClearTemporary,
                        13.0,
                    );
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
            settle(&mut app);
            let button = find_by_name(&mut app, CLEAR).ok_or("no Clear-temporary button")?;
            let _spawned = drain::<UnDerender>(&mut app);
            gesture(&mut app, button)?;
            settle(&mut app);
            Ok(drain::<UnDerender>(&mut app)
                .into_iter()
                .map(|request| request.id)
                .collect())
        }

        // A nil id is the model's "every temporary entry".
        let want = vec![Uuid::nil()];
        assert_eq!(
            requests_after(|app, _button| interact::click_node(app, CLEAR))?,
            want,
            "a primary click"
        );
        assert_eq!(
            requests_after(|app, button| {
                interact::focus(app, button);
                interact::tap(app, KeyCode::Enter, Key::Enter);
                Ok(())
            })?,
            want,
            "Enter on the focused button"
        );
        assert_eq!(
            requests_after(|app, button| {
                interact::focus(app, button);
                interact::tap(app, KeyCode::Space, Key::Space);
                Ok(())
            })?,
            want,
            "Space on the focused button"
        );
        Ok(())
    }
}
