//! The **My Environments** library (`viewer-environment-my-environments`):
//! every settings asset in inventory, filterable, with the actions that apply
//! one, edit it, rename it or throw it away.
//!
//! The reference's `LLFloaterMyEnvironment`: a filter row of three kind
//! checkboxes over a name filter, the list, and a bottom row with the creators
//! and the trash. Its per-row actions live in `menu_settings_gear.xml`, which is
//! the context menu here.
//!
//! # The list is flat, and that is the collector's shape
//!
//! The reference embeds an `asset_filtered_inv_panel` — the inventory *tree*,
//! filtered to settings items — so its rows sit under their folders and it has a
//! **Show All Folders** toggle for the empty ones. This window lists
//! [`SettingsIndex`] instead, which is the reference's *other* settings
//! collector (`FSSettingsCollector`, the one Quick Preferences' three combos are
//! filled from): every settings item in the whole inventory, agent tree and
//! Library both, outside the Trash and Marketplace Listings, de-duplicated by
//! **asset** id and kept in name order.
//!
//! That is a deliberate divergence, and it costs one thing and buys another. It
//! costs the folder hierarchy — so each row carries a **Where** cell naming the
//! folder it came from, prefixed *Library* for a Library item, which is the
//! context the tree would have given by position. It buys the de-duplication:
//! two items of one asset are one row, which is what a library of environments
//! is actually for, and it buys one collector for the whole viewer rather than a
//! second walk that could disagree with the combos about what exists.
//!
//! # What a row's actions do
//!
//! - **Edit** opens the settings editor for the row's kind — the same
//!   [`OpenSettingsEditor`] the inventory's Open sends, so there is one route
//!   into those windows. A day cycle has no editor yet, so the entry is greyed
//!   rather than absent ([[viewer-environment-day-cycle-editor]]).
//! - **Apply Only To Myself** is the reference's `PARAMETER_LOCAL`: a
//!   [`LocalEnvironmentPick`], which fetches the asset and installs it in the
//!   **local** environment layer once it decodes. The window never touches the
//!   asset store itself, and two surfaces asking for the same asset is one fetch.
//!   This is the only entry an object holding `@setenv` takes away, which is why
//!   the restriction is on the *action* rather than on the window: renaming a
//!   sky you own is not something a collar holding the sky has a say in.
//! - **Rename** and **Delete** are the inventory operations, spelled the way the
//!   inventory floater spells them: a rename is a `MoveInventoryItem` into the
//!   same folder under a new name, and a delete is a `MoveInventoryItem` into the
//!   Trash — after the reference's `DeleteItems` confirmation.
//! - **New Sky** / **New Water** mint an item through [`new_settings_item`],
//!   the same function the inventory's create menus call.
//!
//! # Not here
//!
//! - **Apply To Parcel / Apply To Region.** Publishing an environment to land is
//!   the region / parcel environment panel's job
//!   ([[viewer-region-environment-panel]]), which owns the permission tests the
//!   reference guards those two entries with (`canAgentUpdateRegionEnvironment`
//!   / `canAgentUpdateParcelEnvironment`) and the altitude-track scoping that
//!   goes with them. Offering the verb here without those tests would be an
//!   entry that fails on most land.
//! - **Copy / Paste.** The reference's gear menu forwards them to the inventory
//!   panel's own clipboard, which this window is not hosting one of.
//! - **New Day Cycle.** Greyed, as it is in the inventory's create menu and for
//!   the same reason: an item nothing can open is worse than an entry that says
//!   so.
//!
//! Reference (Firestorm, read-only): `llfloatermyenvironment.cpp`,
//! `floater_my_environments.xml`, `menu_settings_gear.xml`,
//! `menu_settings_add.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::Checked;
use bevy::ui_widgets::{Checkbox, ValueChange};
use sl_client_bevy::{
    AssetKey, AssetType, Command, FolderType, InventoryKey, Permissions, SettingsKind, SlCommand,
    SlEvent, SlSessionEvent,
};
use sl_viewer_inventory::inventory::{InventoryModel, query_folder_page};
use sl_viewer_inventory::inventory_actions::new_settings_item;
use sl_viewer_inventory::settings_index::SettingsIndex;
use sl_viewer_notifications::{NotificationResponse, ShowNotification};
use sl_viewer_settings::ViewerSettings;
use sl_viewer_ui_core::i18n::{TransArgs, Translated, Translator};
use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_element::UiAction;
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists};
use sl_viewer_ui_widgets::floater::{
    DeferredFloaterContent, FloaterCaps, FloaterHandle, FloaterSpec, floater_shown, spawn_floater,
};
use sl_viewer_ui_widgets::menu::{MenuCommand, MenuDef, MenuItemDef, OpenContextMenu};
use sl_viewer_ui_widgets::ui_search::{SearchFieldSpec, spawn_search_field};
use sl_viewer_ui_widgets::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableRowCells, TableSelectionMode,
    TableSortDefault, TableSpec, TableState, register_table_settings, set_table_cell, spawn_table,
    spawn_table_row,
};
use sl_viewer_ui_widgets::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use sl_viewer_world_api::OpenSettingsEditor;
use sl_viewer_world_api::rlv::{RlvSession, can_change_environment};
use sl_viewer_world_scene::environment::LocalEnvironmentPick;
use std::collections::VecDeque;

use crate::settings_list::{
    FILTER_KINDS, SettingsListFilters, SettingsListRow, kind_key, kind_slug, location_text,
    project, sort_rows, total,
};
use crate::style::{
    ACTION_BACKGROUND, CONTROL_BORDER, DIM_LABEL_COLOR, FONT_SIZE, LABEL_COLOR, LIST_BACKGROUND,
    ROW_HEIGHT, SELECTED_BACKGROUND,
};

/// The floater's stable id.
pub const MY_ENVIRONMENTS_FLOATER_ID: &str = "my-environments";

/// The `element` this window's menu actions are attributed to.
const MY_ENVIRONMENTS_ELEMENT: &str = "my-environments";

/// The settings section the table's sort and column widths persist under.
const MY_ENVIRONMENTS_SECTION: &[&str] = &["ui", "my-environments"];

/// Column index of the settings kind.
const COL_KIND: usize = 0;

/// Column index of the item's name.
const COL_NAME: usize = 1;

/// Column index of where the item lives.
const COL_WHERE: usize = 2;

/// A checkbox's side, logical px.
const CHECK_SIZE: f32 = 13.0;

/// A ticked checkbox's fill.
const CHECK_ON: Color = Color::srgb(0.45, 0.62, 0.90);

/// An unticked checkbox's fill.
const CHECK_OFF: Color = Color::srgba(0.10, 0.11, 0.14, 1.0);

/// A disabled action button's background — dimmed, so an entry that is present
/// but not usable reads as such. Bevy's `InteractionDisabled` is advisory: it
/// stops this window's own observer from acting, and nothing paints it.
const DISABLED_BACKGROUND: Color = Color::srgb(0.17, 0.19, 0.23);

/// A disabled action button's label.
const DISABLED_LABEL: Color = Color::srgb(0.48, 0.51, 0.57);

/// The two kinds this viewer can mint from nothing, in the add row's order.
const CREATABLE_KINDS: [SettingsKind; 2] = [SettingsKind::Sky, SettingsKind::Water];

// --- Table ----------------------------------------------------------------

/// The library table: the kind beside the name and where it came from, sorted
/// by name ascending — which is the order [`SettingsIndex`] already keeps, and
/// the reference's own default.
static MY_ENVIRONMENTS_TABLE: TableSpec = TableSpec {
    element: MY_ENVIRONMENTS_ELEMENT,
    // Module-owned: the selection keys on the *item*, so it survives a re-sort
    // and the virtualized row recycling.
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "my-environments-col-kind",
            token: "kind",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 84.0 },
            align: TableAlign::Start,
            sortable: true,
        },
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
    sort_setting: Some("MyEnvironmentsSortOrder"),
    widths_setting: Some("MyEnvironmentsColumnWidths"),
};

// --- Context menu ---------------------------------------------------------

/// Condition: the pressed row has an editor to open (a sky or a water frame).
const COND_HAS_EDITOR: &str = "my-environments-has-editor";

/// Condition: the pressed row's item may be written to — the owner modify bit,
/// and not the read-only Library.
const COND_MODIFIABLE: &str = "my-environments-modifiable";

/// The per-row menu — the reference's `menu_settings_gear`.
static MY_ENVIRONMENTS_MENU: MenuDef = MenuDef {
    label: "Environment",
    items: &[
        MenuItemDef::Command(MenuCommand::new("Edit", "edit").enabled_when(COND_HAS_EDITOR)),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new("Apply Only To Myself", "apply-local")),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new("Copy UUID", "copy-uuid")),
        MenuItemDef::Command(MenuCommand::new("Delete", "delete").enabled_when(COND_MODIFIABLE)),
    ],
};

// --- Resources ------------------------------------------------------------

/// The list's live view state.
#[derive(Resource, Debug, Default)]
struct MyEnvironmentsView {
    /// The rows, in table order.
    rows: Vec<SettingsListRow>,
    /// How many settings assets exist at all, for the count line.
    total: usize,
    /// The filters the rows were built with.
    built_filters: Option<SettingsListFilters>,
    /// The index the rows were built from.
    built_index: Option<SettingsIndex>,
    /// The table sort revision they were ordered at.
    built_sort_revision: u64,
    /// Whether anything has been projected yet.
    built: bool,
}

/// The selected row, keyed by item so it survives a re-sort and the row pool.
#[derive(Resource, Debug, Default)]
struct SelectedEnvironment(Option<InventoryKey>);

/// The row the open context menu targets.
#[derive(Resource, Debug, Default)]
struct MyEnvironmentsMenuTarget(Option<InventoryKey>);

/// The creations this window has asked for and not yet seen land, oldest first.
///
/// The reply to a `CreateInventoryItem` names the item, but not who asked — and
/// the inventory's own create menu mints the same items. So this holds *our*
/// outstanding asks, and a created settings item is claimed (and selected) only
/// while one of them is ours.
///
/// It carries the **kind** rather than a count because the reveal below needs
/// it, and the kind we asked for is a better answer than the one read back off
/// the reply: it is what the user clicked, whatever the simulator stamped.
#[derive(Resource, Debug, Default)]
struct PendingEnvironmentCreations(VecDeque<SettingsKind>);

/// Set when the selection was moved by something other than a click, so the
/// list has to be scrolled to it once a row for it exists.
///
/// A separate flag rather than scrolling where the selection is set, because at
/// that moment there is no row: the item was created a frame ago and the index
/// has not been rebuilt around it yet.
#[derive(Resource, Debug, Default)]
struct ScrollToSelection(bool);

/// The delete waiting on the reference's `DeleteItems` confirmation.
///
/// Only one at a time, and the response is only acted on while one is
/// outstanding — the notification reply carries the template name rather than
/// the raise's own id, so this is what tells our confirmation from anybody
/// else's.
#[derive(Resource, Debug, Default)]
struct PendingEnvironmentDelete(Option<InventoryKey>);

/// The window's retained entities.
#[derive(Resource, Debug)]
struct MyEnvironmentsUi {
    /// The table root, carrying its [`TableState`].
    table: Entity,
    /// The virtualized viewport.
    viewport: Entity,
    /// The name-filter field's [`EditableText`].
    filter_field: Entity,
    /// The rename field's [`EditableText`].
    rename_field: Entity,
    /// The count / status line.
    status_text: Entity,
}

/// The row a pooled list row currently presents.
#[derive(Component, Debug, Clone, Copy, Default)]
struct BoundEnvironment(Option<InventoryKey>);

/// A kind checkbox, naming which of [`FILTER_KINDS`] it toggles.
#[derive(Component, Debug, Clone, Copy)]
struct KindFilterCheckbox(usize);

/// What a bottom-row button does.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum MyEnvironmentsButton {
    /// Mint a fresh settings item of this kind.
    New(SettingsKind),
    /// Rename the selected item to whatever the rename field holds.
    Rename,
    /// Throw the selected item away, after the confirmation.
    Delete,
}

impl MyEnvironmentsButton {
    /// The Fluent key for this button's label.
    const fn label_key(self) -> &'static str {
        match self {
            Self::New(SettingsKind::Sky) => "my-environments-new-sky",
            Self::New(SettingsKind::Water) => "my-environments-new-water",
            Self::New(SettingsKind::DayCycle) => "my-environments-new-day-cycle",
            Self::Rename => "my-environments-rename",
            Self::Delete => "my-environments-delete",
        }
    }

    /// The button's element-id tail.
    const fn slug(self) -> &'static str {
        match self {
            Self::New(SettingsKind::Sky) => "new-sky",
            Self::New(SettingsKind::Water) => "new-water",
            Self::New(SettingsKind::DayCycle) => "new-day-cycle",
            Self::Rename => "rename",
            Self::Delete => "delete",
        }
    }
}

// --- Plugin ---------------------------------------------------------------

/// The My Environments window: its view state, its deferred content and the
/// action wiring.
#[derive(Debug, Clone, Copy, Default)]
pub struct MyEnvironmentsPlugin;

impl Plugin for MyEnvironmentsPlugin {
    fn build(&self, app: &mut App) {
        // The shared filter type is *this* window's resource: it is the only
        // surface whose filters the user drives. The picker fixes its own kind
        // and keeps its filters inside its per-window state, so the two cannot
        // collide over it.
        app.init_resource::<SettingsListFilters>()
            .init_resource::<MyEnvironmentsView>()
            .init_resource::<SelectedEnvironment>()
            .init_resource::<MyEnvironmentsMenuTarget>()
            .init_resource::<PendingEnvironmentDelete>()
            .init_resource::<PendingEnvironmentCreations>()
            .init_resource::<ScrollToSelection>()
            // Idempotent: the viewer's own plugins init both, but this window
            // must still stand up in a host that has neither (the gallery).
            .init_resource::<LocalEnvironmentPick>()
            .init_resource::<SettingsIndex>()
            .add_message::<CreateSettingsItem>()
            // The channels this window speaks over. Every one of them is
            // registered by whichever plugin *owns* it as well; registering them
            // here too (all idempotent) is what lets the window stand up in a
            // host that brought only some of them — and is the difference
            // between a dead button and a working one, since a `MessageWriter`
            // for an unregistered message is a system that never runs.
            .add_message::<UiAction>()
            .add_message::<OpenContextMenu>()
            .add_message::<OpenSettingsEditor>()
            .add_message::<ShowNotification>()
            .add_message::<NotificationResponse>()
            .add_systems(
                Startup,
                (
                    register_my_environments_settings,
                    spawn_my_environments_floater.after(UiScaffoldSystems::SpawnRoot),
                ),
            )
            .add_systems(
                Update,
                (
                    mirror_my_environments_filter,
                    rebuild_my_environments_view,
                    scroll_to_selected_environment,
                    seed_rename_field,
                )
                    .chain()
                    .before(layout_virtual_lists)
                    .run_if(floater_shown(MY_ENVIRONMENTS_FLOATER_ID)),
            )
            .add_systems(
                Update,
                (populate_environment_rows, bind_environment_rows)
                    .chain()
                    .after(layout_virtual_lists)
                    .run_if(floater_shown(MY_ENVIRONMENTS_FLOATER_ID)),
            )
            // The two dispatchers run whether or not the window is on screen: a
            // delete confirmation can be answered after it has been closed.
            .add_systems(
                Update,
                (
                    handle_my_environments_actions,
                    confirm_environment_delete,
                    select_created_environment,
                    sync_kind_checkboxes,
                )
                    .chain(),
            );
    }
}

/// Register the table's sort / width persistence.
fn register_my_environments_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    register_table_settings(
        &mut settings,
        MY_ENVIRONMENTS_SECTION,
        &MY_ENVIRONMENTS_TABLE,
    );
}

// --- Floater --------------------------------------------------------------

/// The My Environments floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn my_environments_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: MY_ENVIRONMENTS_FLOATER_ID,
        title: "My Environments".to_owned(),
        position: Vec2::new(220.0, 160.0),
        default_size: Some(Vec2::new(520.0, 420.0)),
        min_size: Some(Vec2::new(340.0, 240.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: false,
        },
    }
}

/// Startup: the chrome only; the content is built on the first open.
fn spawn_my_environments_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, my_environments_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("my-environments-title"));
    let builder = commands.register_system(build_my_environments_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build: the filter row, the list, the rename row and the
/// bottom actions.
fn build_my_environments_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                padding: UiRect::all(Val::Px(6.0)),
                ..column(Val::Px(4.0))
            },
            Name::new("my-environments:content"),
            ChildOf(handle.content),
        ))
        .id();

    let filter_field = spawn_filter_row(&mut commands, content);

    let table = spawn_table(&mut commands, content, &MY_ENVIRONMENTS_TABLE);
    commands
        .entity(table.viewport)
        .insert((BackgroundColor(LIST_BACKGROUND), TabIndex(4)));

    let status_text = commands
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
            Name::new("my-environments-status"),
            ChildOf(content),
        ))
        .id();

    let rename_field = spawn_rename_row(&mut commands, content);
    spawn_action_row(&mut commands, content);

    commands.insert_resource(MyEnvironmentsUi {
        table: table.root,
        viewport: table.viewport,
        filter_field,
        rename_field,
        status_text,
    });
}

/// The filter row: the three kind checkboxes and the name filter under them.
/// Returns the filter field's [`EditableText`] entity.
fn spawn_filter_row(commands: &mut Commands, parent: Entity) -> Entity {
    let checks = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..row(Val::Px(10.0))
            },
            Name::new("my-environments-filters:row"),
            ChildOf(parent),
        ))
        .id();
    for (index, kind) in FILTER_KINDS.into_iter().enumerate() {
        spawn_kind_checkbox(commands, checks, index, kind);
    }

    let search = spawn_search_field(
        commands,
        parent,
        &SearchFieldSpec {
            tab_index: 3,
            font_size: FONT_SIZE,
            min_width: 160.0,
            placeholder: "Filter Environments".to_owned(),
            search_glyph: true,
            ..SearchFieldSpec::new("my-environments-filter")
        },
    );
    if let Some(placeholder) = search.placeholder {
        commands
            .entity(placeholder)
            .insert(Translated::new("my-environments-filter-placeholder"));
    }
    search.field
}

/// One kind checkbox with its label, ticked to start (all three kinds show).
fn spawn_kind_checkbox(commands: &mut Commands, parent: Entity, index: usize, kind: SettingsKind) {
    let holder = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(4.0))
            },
            ChildOf(parent),
        ))
        .id();
    commands
        .spawn((
            Checkbox,
            Checked,
            Node {
                width: Val::Px(CHECK_SIZE),
                height: Val::Px(CHECK_SIZE),
                border: UiRect::all(Val::Px(2.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(CHECK_ON),
            TabIndex(i32::try_from(index).unwrap_or(0)),
            KindFilterCheckbox(index),
            Name::new(format!("my-environments-filter-{}:check", kind_slug(kind))),
            ChildOf(holder),
        ))
        .observe(on_kind_filter_toggle);
    commands.spawn((
        Text::default(),
        Translated::new(kind_key(kind)),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(holder),
    ));
}

/// The rename row: a label, the field seeded from the selection, and the button
/// that commits it. Returns the field's [`EditableText`] entity.
fn spawn_rename_row(commands: &mut Commands, parent: Entity) -> Entity {
    let holder = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new("my-environments-rename:row"),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("my-environments-name"),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(holder),
    ));
    let field = spawn_text_input(
        commands,
        holder,
        &TextInputSpec {
            tab_index: 5,
            font_size: FONT_SIZE,
            fill: true,
            max_characters: Some(63),
            ..TextInputSpec::new("my-environments-rename", TextInputKind::Line)
        },
    );
    spawn_action_button(commands, holder, MyEnvironmentsButton::Rename, 6, false);
    field
}

/// The bottom action row: the creators, then the trash at the trailing edge —
/// the reference's `pnl_bottom` (its add menu flattened to its three entries,
/// and its gear menu moved onto the rows).
fn spawn_action_row(commands: &mut Commands, parent: Entity) {
    let holder = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new("my-environments-actions:row"),
            ChildOf(parent),
        ))
        .id();
    let mut tab = 7;
    for kind in CREATABLE_KINDS {
        spawn_action_button(
            commands,
            holder,
            MyEnvironmentsButton::New(kind),
            tab,
            false,
        );
        tab = tab.saturating_add(1);
    }
    // The third creator is in the row and disabled, as it is in the inventory's
    // create menu: the entry is where it will be, and says why it is not usable
    // yet by being greyed rather than missing.
    let day_cycle = spawn_action_button(
        commands,
        holder,
        MyEnvironmentsButton::New(SettingsKind::DayCycle),
        tab,
        true,
    );
    commands
        .entity(day_cycle)
        .insert(bevy::ui::InteractionDisabled);
    tab = tab.saturating_add(1);

    commands.spawn((
        Node {
            flex_grow: 1.0,
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(holder),
    ));
    spawn_action_button(commands, holder, MyEnvironmentsButton::Delete, tab, false);
}

/// One bottom-row button.
fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    button: MyEnvironmentsButton,
    tab: i32,
    disabled: bool,
) -> Entity {
    commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(if disabled {
                DISABLED_BACKGROUND
            } else {
                ACTION_BACKGROUND
            }),
            TabIndex(tab),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            button,
            Name::new(format!("my-environments-{}:button", button.slug())),
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
            TextColor(if disabled {
                DISABLED_LABEL
            } else {
                LABEL_COLOR
            }),
            Pickable::IGNORE,
        ))
        .observe(on_my_environments_button)
        .id()
}

// --- View systems ---------------------------------------------------------

/// Keep the filter term in step with the search field.
fn mirror_my_environments_filter(
    ui: Option<Res<MyEnvironmentsUi>>,
    fields: Query<&EditableText>,
    mut filters: ResMut<SettingsListFilters>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Ok(field) = fields.get(ui.filter_field) else {
        return;
    };
    let term = field.value().to_string();
    if filters.search != term {
        filters.search = term;
    }
}

/// Reproject when the index, the filters or the sort moved, and refresh the
/// count line.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the two models and the \
              filters it projects from, the window's entities, the translator, and the view \
              and widgets it writes"
)]
fn rebuild_my_environments_view(
    index: Res<SettingsIndex>,
    model: Option<Res<InventoryModel>>,
    filters: Res<SettingsListFilters>,
    ui: Option<Res<MyEnvironmentsUi>>,
    translator: Translator,
    mut view: ResMut<MyEnvironmentsView>,
    tables: Query<&TableState>,
    mut lists: Query<&mut VirtualList>,
    mut texts: Query<&mut Text>,
) {
    let (Some(ui), Some(model)) = (ui, model) else {
        return;
    };
    let sort = tables
        .get(ui.table)
        .ok()
        .map(|table| (table.sort_revision(), table.sort().keys().to_vec()));
    let sort_revision = sort.as_ref().map_or(0, |(revision, _keys)| *revision);
    if view.built
        && view.built_sort_revision == sort_revision
        && view.built_filters.as_ref() == Some(&*filters)
        && view.built_index.as_ref() == Some(&*index)
        && !model.is_changed()
    {
        return;
    }
    view.built = true;
    view.built_sort_revision = sort_revision;
    view.built_filters = Some(filters.clone());
    view.built_index = Some(index.clone());

    let mut rows = project(&index, &model, &filters);
    let keys: Vec<(&str, bool)> = sort
        .map(|(_revision, keys)| keys)
        .unwrap_or_default()
        .iter()
        .filter_map(|key| {
            MY_ENVIRONMENTS_TABLE
                .columns
                .get(key.column)
                .map(|column| (column.token, key.ascending))
        })
        .collect();
    sort_rows(&mut rows, &keys);
    view.rows = rows;
    view.total = total(&index);

    if let Ok(mut list) = lists.get_mut(ui.viewport) {
        list.item_count = view.rows.len();
    }
    let label = translator.format(
        "my-environments-count",
        &TransArgs::new()
            .int("shown", i64::try_from(view.rows.len()).unwrap_or(i64::MAX))
            .int("total", i64::try_from(view.total).unwrap_or(i64::MAX)),
    );
    set_status(&mut texts, Some(ui.status_text), &label);
}

/// Keep the rename field showing the selected item's name.
///
/// Seeded from the *selection* rather than typed into by the list: the field is
/// the new name, so it starts as the old one and a rename with nothing changed
/// is a no-op rather than a blank.
fn seed_rename_field(
    ui: Option<Res<MyEnvironmentsUi>>,
    selected: Res<SelectedEnvironment>,
    view: Res<MyEnvironmentsView>,
    mut fields: Query<&mut EditableText>,
) {
    let Some(ui) = ui else {
        return;
    };
    // The view matters as much as the selection: an item created a moment ago is
    // selected before the rebuilt index has a row for it, and the name arrives
    // with the row rather than with the selection.
    if !selected.is_changed() && !view.is_changed() {
        return;
    }
    let wanted = match selected.0 {
        Some(item) => match view.rows.iter().find(|row| row.item == item) {
            Some(row) => row.name.clone(),
            // Selected but not on screen yet — leave the field alone rather than
            // blanking it and then filling it in a frame later.
            None => return,
        },
        None => String::new(),
    };
    if let Ok(mut field) = fields.get_mut(ui.rename_field)
        && field.value().to_string() != wanted
    {
        field.editor.set_text(&wanted);
    }
}

/// Build the cells of each freshly-pooled row and attach its press observer.
fn populate_environment_rows(
    mut commands: Commands,
    ui: Option<Res<MyEnvironmentsUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        spawn_table_row(&mut commands, row_entity, ui.table, &MY_ENVIRONMENTS_TABLE);
        commands
            .entity(row_entity)
            .insert(BoundEnvironment(None))
            .observe(on_environment_row_press);
    }
}

/// Bind each pooled row to the entry it now presents.
fn bind_environment_rows(
    view: Res<MyEnvironmentsView>,
    selected: Res<SelectedEnvironment>,
    ui: Option<Res<MyEnvironmentsUi>>,
    translator: Translator,
    mut rows: Query<(
        Entity,
        Ref<VirtualRow>,
        &ChildOf,
        &TableRowCells,
        &mut BoundEnvironment,
    )>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    let refresh_all = view.is_changed() || selected.is_changed();
    let library_label = translator.get("my-environments-library");
    for (row_entity, row, child_of, cells, mut bound) in &mut rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let data = row.index.and_then(|index| view.rows.get(index));
        bound.0 = data.map(|entry| entry.item);
        let (kind, name, location) = data.map_or_else(
            || (String::new(), String::new(), String::new()),
            |entry| {
                (
                    translator.get(kind_key(entry.kind)),
                    entry.name.clone(),
                    location_text(&library_label, entry.library, &entry.folder),
                )
            },
        );
        for (column, value, color) in [
            (COL_KIND, kind, DIM_LABEL_COLOR),
            (COL_NAME, name, LABEL_COLOR),
            (COL_WHERE, location, DIM_LABEL_COLOR),
        ] {
            if let Some(cell) = cells.cell(column) {
                set_table_cell(&mut texts, cell, &value, color);
            }
        }
        if let Ok(mut background) = backgrounds.get_mut(row_entity) {
            let wanted = if data.is_some() && selected.0 == bound.0 {
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

// --- Interaction ----------------------------------------------------------

/// A kind checkbox was toggled: reflect its `Checked` state and re-filter.
fn on_kind_filter_toggle(
    change: On<ValueChange<bool>>,
    boxes: Query<&KindFilterCheckbox>,
    mut filters: ResMut<SettingsListFilters>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut commands: Commands,
) {
    let Ok(check) = boxes.get(change.source) else {
        return;
    };
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    if let Ok(mut background) = backgrounds.get_mut(change.source) {
        let wanted = if change.value { CHECK_ON } else { CHECK_OFF };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
    if let Some(slot) = filters.kinds.get_mut(check.0) {
        *slot = change.value;
    }
}

/// A press on a pooled row: primary selects; secondary selects and opens the
/// per-row menu with the open-time condition snapshot.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the row pool, the view \
              and inventory the conditions are read from, and the focus / selection / \
              menu-target stashes the press writes"
)]
fn on_environment_row_press(
    mut press: On<Pointer<Press>>,
    rows: Query<&BoundEnvironment>,
    view: Res<MyEnvironmentsView>,
    model: Option<Res<InventoryModel>>,
    ui: Res<MyEnvironmentsUi>,
    mut focus: ResMut<InputFocus>,
    mut selected: ResMut<SelectedEnvironment>,
    mut target: ResMut<MyEnvironmentsMenuTarget>,
    mut menus: MessageWriter<OpenContextMenu>,
) {
    let Ok(BoundEnvironment(Some(item))) = rows.get(press.entity).copied() else {
        return;
    };
    press.propagate(false);
    focus.set(ui.viewport, FocusCause::Navigated);
    selected.0 = Some(item);
    if press.button != PointerButton::Secondary {
        return;
    }
    let Some(entry) = view.rows.iter().find(|row| row.item == item) else {
        return;
    };
    let mut conditions: Vec<&'static str> = Vec::new();
    // A day cycle has no editor yet; the entry stays, greyed.
    if entry.kind != SettingsKind::DayCycle {
        conditions.push(COND_HAS_EDITOR);
    }
    if model
        .as_deref()
        .is_some_and(|model| is_modifiable(model, entry))
    {
        conditions.push(COND_MODIFIABLE);
    }
    target.0 = Some(item);
    menus.write(OpenContextMenu {
        menu: &MY_ENVIRONMENTS_MENU,
        at: press.pointer_location.position,
        element: MY_ENVIRONMENTS_ELEMENT,
        conditions,
    });
}

/// Whether `entry`'s item may be written to: the owner modify bit, and not one
/// of the shared read-only Library's.
fn is_modifiable(model: &InventoryModel, entry: &SettingsListRow) -> bool {
    !entry.library
        && model
            .find_item(entry.item)
            .is_some_and(|item| item.permissions.owner.contains(Permissions::MODIFY))
}

/// A press on one of the bottom-row buttons.
fn on_my_environments_button(
    mut press: On<Pointer<Press>>,
    buttons: Query<&MyEnvironmentsButton>,
    disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    selected: Res<SelectedEnvironment>,
    mut actions: MessageWriter<UiAction>,
    mut creates: MessageWriter<CreateSettingsItem>,
) {
    if press.button != PointerButton::Primary || disabled.contains(press.entity) {
        return;
    }
    let Ok(button) = buttons.get(press.entity).copied() else {
        return;
    };
    press.propagate(false);
    debug!("my-environments: {} pressed", button.slug());
    match button {
        MyEnvironmentsButton::New(kind) => {
            creates.write(CreateSettingsItem { kind });
        }
        // Routed through the same action channel the menu writes, so the
        // rename and the delete have one implementation each whichever
        // affordance asked for them. They act on the *selection*, which a
        // menu open has just set as well.
        MyEnvironmentsButton::Rename | MyEnvironmentsButton::Delete => {
            if selected.0.is_none() {
                return;
            }
            actions.write(UiAction {
                element: MY_ENVIRONMENTS_ELEMENT,
                action: if button == MyEnvironmentsButton::Rename {
                    "rename"
                } else {
                    "delete"
                },
            });
        }
    }
}

/// A request to mint a fresh settings item of one kind.
///
/// A message rather than a call, because the button's observer cannot hold the
/// inventory model and the command channel at once without pulling half the
/// window's state into it.
#[derive(Message, Debug, Clone, Copy)]
struct CreateSettingsItem {
    /// Which kind to mint.
    kind: SettingsKind,
}

/// Dispatch the window's actions — the per-row menu's picks, the bottom row's
/// rename and delete, and the creators.
#[expect(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "the window's one dispatcher takes every input its actions need — grouped into \
              tuples by role (the action channels and targets, the mutated stashes, the \
              message outputs) to fit the SystemParam arity"
)]
fn handle_my_environments_actions(
    inputs: (
        MessageReader<UiAction>,
        MessageReader<CreateSettingsItem>,
        Res<MyEnvironmentsMenuTarget>,
        Res<SelectedEnvironment>,
        Option<Res<RlvSession>>,
    ),
    view: Res<MyEnvironmentsView>,
    model: Option<Res<InventoryModel>>,
    ui: Option<Res<MyEnvironmentsUi>>,
    fields: Query<&EditableText>,
    translator: Translator,
    mut stashes: (
        ResMut<PendingEnvironmentDelete>,
        ResMut<PendingEnvironmentCreations>,
        Option<ResMut<LocalEnvironmentPick>>,
        Option<ResMut<bevy::clipboard::Clipboard>>,
    ),
    mut outputs: (
        MessageWriter<SlCommand>,
        MessageWriter<OpenSettingsEditor>,
        MessageWriter<ShowNotification>,
    ),
    mut texts: Query<&mut Text>,
) {
    let (mut actions, mut creates, target, selected, rlv) = inputs;
    let (ref mut pending_delete, ref mut pending_creations, ref mut pick, ref mut clipboard) =
        stashes;
    let (ref mut commands, ref mut editors, ref mut notify) = outputs;
    let Some(model) = model else {
        // Every action below reaches the inventory mirror; without one there is
        // nothing to act on. Drain so a click made before login is not replayed
        // against a later session.
        if !actions.is_empty() || !creates.is_empty() {
            warn!("my-environments: no inventory mirror yet; dropping the click");
        }
        actions.clear();
        creates.clear();
        return;
    };
    let status = ui.as_ref().map(|ui| ui.status_text);

    for create in creates.read() {
        let dest = model
            .folder_by_type(FolderType::Settings)
            .or_else(|| model.agent_root());
        let Some(dest) = dest else {
            warn!("my-environments: no Settings folder and no agent root to create in");
            set_message(&mut texts, status, &translator, "my-environments-no-folder");
            continue;
        };
        let name = translator.get(default_name_key(create.kind));
        info!(
            "my-environments: creating a new {:?} named {name:?} in folder {dest}",
            create.kind
        );
        // The simulator mints the item, authors its default asset and stamps its
        // subtype; there is nothing to upload and nothing to stamp afterwards.
        commands.write(SlCommand(new_settings_item(create.kind, &name, dest)));
        query_folder_page(dest, commands);
        pending_creations.0.push_back(create.kind);
    }

    for action in actions.read() {
        if action.element != MY_ENVIRONMENTS_ELEMENT {
            continue;
        }
        // A menu pick acts on the row it was opened over; a button acts on the
        // selection. A menu open sets both, so the target is preferred and the
        // selection is what a button falls back to.
        let Some(item) = target.0.or(selected.0) else {
            continue;
        };
        let Some(entry) = view.rows.iter().find(|row| row.item == item) else {
            warn!(
                "my-environments: '{}' has no row for item {item} any more",
                action.action
            );
            continue;
        };
        debug!("my-environments: '{}' on {:?}", action.action, entry.name);
        match action.action {
            "edit" => {
                let Some(info) = model.find_item(item) else {
                    continue;
                };
                if entry.kind == SettingsKind::DayCycle {
                    set_message(&mut texts, status, &translator, "my-environments-no-editor");
                    continue;
                }
                editors.write(OpenSettingsEditor {
                    name: entry.name.clone(),
                    asset_id: entry.asset_id,
                    item_id: item,
                    folder_id: info.folder_id,
                    kind: entry.kind,
                    editable: info.permissions.owner.contains(Permissions::MODIFY),
                });
            }
            "apply-local" => {
                // The one action here that changes what the user is standing
                // under, and so the one an object holding `@setenv` takes away.
                // The rest of the window is inventory work and stays usable.
                if rlv
                    .as_deref()
                    .is_some_and(|session| !can_change_environment(session.state()))
                {
                    set_message(&mut texts, status, &translator, "my-environments-setenv");
                    continue;
                }
                if let Some(pick) = pick.as_deref_mut() {
                    pick.request(AssetKey::from(entry.asset_id));
                }
                set_message(&mut texts, status, &translator, "my-environments-applied");
            }
            "copy-uuid" => {
                if let Some(clipboard) = clipboard.as_deref_mut() {
                    // A failed clipboard write (a headless run) is dropped.
                    let _set = clipboard.set_text(entry.asset_id.to_string());
                }
            }
            "rename" => {
                let Some(info) = model.find_item(item) else {
                    continue;
                };
                if !is_modifiable(&model, entry) {
                    set_message(
                        &mut texts,
                        status,
                        &translator,
                        "my-environments-not-modifiable",
                    );
                    continue;
                }
                let wanted = ui
                    .as_ref()
                    .and_then(|ui| fields.get(ui.rename_field).ok())
                    .map(|field| field.value().to_string().trim().to_owned())
                    .unwrap_or_default();
                if wanted.is_empty() || wanted == entry.name {
                    continue;
                }
                commands.write(SlCommand(Command::MoveInventoryItem {
                    item_id: item,
                    folder_id: info.folder_id,
                    new_name: wanted,
                }));
                query_folder_page(info.folder_id, commands);
            }
            "delete" => {
                if !is_modifiable(&model, entry) {
                    set_message(
                        &mut texts,
                        status,
                        &translator,
                        "my-environments-not-modifiable",
                    );
                    continue;
                }
                pending_delete.0 = Some(item);
                notify.write(ShowNotification::new("DeleteItems").arg(
                    "QUESTION",
                    translator.get("my-environments-delete-question"),
                ));
            }
            _other => {}
        }
    }
}

/// The Fluent key of a fresh item's name, per kind.
const fn default_name_key(kind: SettingsKind) -> &'static str {
    match kind {
        SettingsKind::Sky => "my-environments-new-sky",
        SettingsKind::Water => "my-environments-new-water",
        SettingsKind::DayCycle => "my-environments-new-day-cycle",
    }
}

/// Carry out a delete the user confirmed: move the item into the Trash, as the
/// reference's `onItemsRemovalConfirmation` does.
fn confirm_environment_delete(
    mut responses: MessageReader<NotificationResponse>,
    mut pending: ResMut<PendingEnvironmentDelete>,
    model: Option<Res<InventoryModel>>,
    mut commands: MessageWriter<SlCommand>,
) {
    for response in responses.read() {
        if response.template != "DeleteItems" {
            continue;
        }
        // Only ours while one is outstanding; any other answer clears it, so a
        // stale confirmation cannot delete a later selection.
        let Some(item) = pending.0.take() else {
            continue;
        };
        if response.button != Some("Yes") {
            continue;
        }
        let Some(model) = model.as_deref() else {
            continue;
        };
        let (Some(trash), Some(info)) = (
            model.folder_by_type(FolderType::Trash),
            model.find_item(item),
        ) else {
            continue;
        };
        let from = info.folder_id;
        commands.write(SlCommand(Command::MoveInventoryItem {
            item_id: item,
            folder_id: trash,
            new_name: String::new(),
        }));
        query_folder_page(from, &mut commands);
        query_folder_page(trash, &mut commands);
    }
}

/// Keep each kind checkbox showing what the filters actually hold.
///
/// The checkbox observer used to be the only writer, so the widget *was* the
/// state. A creation that reveals a hidden kind is a second writer, and without
/// this the box would still read unticked while its kind was on screen.
fn sync_kind_checkboxes(
    filters: Res<SettingsListFilters>,
    boxes: Query<(Entity, &KindFilterCheckbox, Has<Checked>)>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut commands: Commands,
) {
    if !filters.is_changed() {
        return;
    }
    for (entity, check, checked) in &boxes {
        let wanted = filters.kinds.get(check.0).copied().unwrap_or(false);
        if wanted != checked {
            if wanted {
                commands.entity(entity).insert(Checked);
            } else {
                commands.entity(entity).remove::<Checked>();
            }
        }
        if let Ok(mut background) = backgrounds.get_mut(entity) {
            let fill = if wanted { CHECK_ON } else { CHECK_OFF };
            if background.0 != fill {
                background.0 = fill;
            }
        }
    }
}

/// Select a settings item this window asked the simulator to create, the moment
/// its `UpdateCreateInventoryItem` reply names it.
///
/// The reference selects a freshly created item too (its inventory panel puts it
/// straight into inline rename). Selecting it here is what makes the add row
/// usable: the list is name-ordered over the whole inventory, so a new item can
/// land anywhere in it, and hunting for the row you just made is not a thing a
/// person should have to do.
fn select_created_environment(
    mut events: MessageReader<SlEvent>,
    ui: Option<Res<MyEnvironmentsUi>>,
    mut pending: ResMut<PendingEnvironmentCreations>,
    mut selected: ResMut<SelectedEnvironment>,
    mut filters: ResMut<SettingsListFilters>,
    mut scroll: ResMut<ScrollToSelection>,
    mut fields: Query<&mut EditableText>,
) {
    for event in events.read() {
        let SlSessionEvent::InventoryItemCreated { item, .. } = &event.0 else {
            continue;
        };
        // The wire item carries raw type codes, not the typed enums.
        if i32::from(item.item_type) != AssetType::Settings.to_code() {
            continue;
        }
        let Some(kind) = pending.0.pop_front() else {
            // Somebody else's creation — the inventory's own create menu, or an
            // item the simulator materialised.
            continue;
        };
        // Widen whatever would have hidden it, or the selection below points at
        // a row nothing draws.
        if filters.reveal(kind, &item.name)
            && let Some(ui) = ui.as_ref()
            && let Ok(mut field) = fields.get_mut(ui.filter_field)
        {
            // The mirror reads the widget every frame, so the *widget* is what
            // has to be cleared — clearing only the resource would be undone on
            // the next pass.
            field.editor.set_text("");
        }
        selected.0 = Some(item.item_id);
        scroll.0 = true;
    }
}

/// Scroll the list to the selected row once one exists for it — the other half
/// of selecting a freshly created item, since the list is name-ordered over the
/// whole inventory and a new item can land anywhere in it.
fn scroll_to_selected_environment(
    ui: Option<Res<MyEnvironmentsUi>>,
    view: Res<MyEnvironmentsView>,
    selected: Res<SelectedEnvironment>,
    mut scroll: ResMut<ScrollToSelection>,
    mut lists: Query<&mut VirtualList>,
) {
    if !scroll.0 {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    let Some(item) = selected.0 else {
        scroll.0 = false;
        return;
    };
    // Held, not spent, until the rebuilt view has a row: the create lands a
    // frame or more before the index is walked again.
    let Some(index) = view.rows.iter().position(|row| row.item == item) else {
        return;
    };
    scroll.0 = false;
    if let Ok(mut list) = lists.get_mut(ui.viewport) {
        list.scroll_to_index(index);
    }
}

/// Write a translated one-line message into the status readout.
fn set_message(
    texts: &mut Query<&mut Text>,
    status: Option<Entity>,
    translator: &Translator,
    key: &str,
) {
    let message = translator.get(key);
    set_status(texts, status, &message);
}

/// Write a one-line message into the status readout, only when it would change.
fn set_status<F: bevy::ecs::query::QueryFilter>(
    texts: &mut Query<&mut Text, F>,
    status: Option<Entity>,
    message: &str,
) {
    if let Some(status) = status
        && let Ok(mut text) = texts.get_mut(status)
        && text.0 != message
    {
        message.clone_into(&mut text.0);
    }
}
