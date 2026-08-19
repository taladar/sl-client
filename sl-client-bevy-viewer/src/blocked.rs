//! The **Blocked Residents & Objects** list (`viewer-block-list`) — the UI
//! over the fully-implemented mute protocol, hosted in the **Blocked** sub-tab
//! of the [People pane](crate::people) inside the Conversations floater.
//!
//! # Why it lives in the People pane's Blocked sub-tab
//!
//! The reference viewer's **Vintage** skin presents the block list as a
//! "Blocked Residents & Objects" tab inside the People floater
//! (`panel_people.xml`'s added Blocked tab over `llblocklist`), and
//! [`crate::people`] already owns that horizontal sub-tab strip plus an empty
//! content slot per tab. This module fills the Blocked slot, laid out like the
//! Friends and Groups lists beside it: a filter box over a sortable,
//! virtualized table with a trailing action column.
//!
//! # What it does
//!
//! - Lists every [`MuteEntry`] the [mute model](crate::mutes::MuteModel) holds,
//!   with the entry's **type** (Resident / Object / Group / By name /
//!   external) beside its name, sorted by name or type and filtered by a name
//!   fragment.
//! - **Unblock** removes the entry (`Command::Unmute`).
//! - The four **per-aspect toggles** — Block Text / Voice / Particles / Object
//!   Sounds — flip that aspect's *exception* bit in [`MuteFlags`] and re-send
//!   the entry (`Command::Mute` with the new flags), exactly as the reference's
//!   `LLBlockList::toggleMute` does. Turning every aspect off leaves nothing
//!   muted, so (again like the reference) the entry is removed instead. The
//!   toggles apply only to a **resident** mute, the one kind whose flags the
//!   grid honours.
//! - **Block Resident…** opens the shared [avatar picker](crate::avatar_picker)
//!   and blocks the pick; **Block object by name…** opens the small by-name
//!   floater (the reference's `floater_mute_object.xml`) and adds a
//!   [`MuteType::ByName`] entry — the way to silence a spammy object you cannot
//!   click.
//!
//! # Deliberate divergences from the reference
//!
//! The reference marks each row's kind with a small **type icon**; this list
//! spells the kind out in a sortable, localized **Type column** instead — the
//! same information, legible without a legend, and it is what the reference's
//! own "sort by type" view option orders on. Multi-select unblocking (a
//! Firestorm addition) is not built: the list is single-select, like Linden's.
//!
//! # Refusals
//!
//! Nothing here writes a `Command::Mute`: every block — the two add paths, and
//! the aspect toggles' re-send — goes out as a [`RequestBlock`], so the
//! reference's `LLMuteList::add` guards in [`crate::mutes`] decide. A full list
//! raises `MuteLimitReached`, a Linden raises `MuteLinden`, and a duplicate
//! by-name entry raises `MuteByNameFailed`, identically here and at every other
//! Block affordance in the viewer.
//!
//! Reference (Firestorm, read-only): `llblocklist`, `llpanelblockedlist`,
//! `llfloatergetblockedobjectname`, `menu_people_blocked_{gear,plus,view}.xml`,
//! `floater_mute_object.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{AgentKey, Command, MuteEntry, MuteFlags, MuteType, SlCommand, Uuid};

use crate::avatar_picker::{AvatarPicked, OpenAvatarPicker};
use crate::avatar_profile::OpenAvatarProfile;
use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::menu::{MenuCommand, MenuDef, MenuItemDef, OpenContextMenu};
use crate::mutes::{MuteModel, RequestBlock, flags_apply};
use crate::people::PeopleUi;
use crate::settings::ViewerSettings;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_element::UiAction;
use crate::ui_font::UiFont;
use crate::ui_search::{SearchFieldSpec, spawn_search_field};
use crate::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableRowCells, TableSelectionMode,
    TableSortDefault, TableSpec, TableState, register_table_settings, set_table_cell, spawn_table,
    spawn_table_row,
};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists, spawn_virtual_scrollbar};

/// The `element` the block list's menu / UI actions are attributed to.
const BLOCKED_ELEMENT: &str = "blocked";

/// The tag the block list opens the shared avatar picker under, so only its own
/// pick is consumed.
const PICKER_REQUESTER: &str = "blocked-list";

/// The by-name block floater's stable id (persistence, `SL_VIEWER_OPEN_FLOATER`).
const BLOCK_BY_NAME_FLOATER_ID: &str = "block-by-name";

/// The persisted-settings section the block list's table state lives under.
const BLOCKED_SECTION: &[&str] = &["blocked"];

// --- Palette / geometry (the sibling People panes' values) ----------------

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
const ACTION_COL_WIDTH: f32 = 150.0;

// --- Table ----------------------------------------------------------------

/// Column index of the blocked entity's name.
const COL_NAME: usize = 0;

/// Column index of the mute type.
const COL_TYPE: usize = 1;

/// The block-list table: a flexible name beside a fixed type, sorted by name
/// ascending by default (the reference's `E_SORT_BY_NAME`). Selection is
/// module-owned (keyed by entry, not row index) because the list re-sorts as
/// entries are added and removed.
static BLOCKED_TABLE: TableSpec = TableSpec {
    element: "blocked",
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "blocked-col-name",
            token: "name",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "blocked-col-type",
            token: "type",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 84.0 },
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
    sort_setting: Some("BlockedSortOrder"),
    widths_setting: Some("BlockedColumnWidths"),
};

// --- Context menu ---------------------------------------------------------

/// Condition: the pressed entry is a resident mute (its aspect flags apply, and
/// it has a profile to open).
const COND_AGENT: &str = "blocked-agent";

/// Condition: the pressed entry's text chat is muted.
const COND_TEXT: &str = "blocked-text-muted";

/// Condition: the pressed entry's voice chat is muted.
const COND_VOICE: &str = "blocked-voice-muted";

/// Condition: the pressed entry's particles are muted.
const COND_PARTICLES: &str = "blocked-particles-muted";

/// Condition: the pressed entry's object sounds are muted.
const COND_SOUNDS: &str = "blocked-sounds-muted";

/// The blocked-row context menu — the reference's `menu_people_blocked_gear`.
static BLOCKED_MENU: MenuDef = MenuDef {
    label: "Blocked",
    items: &[
        MenuItemDef::Command(MenuCommand::new("Unblock", "unblock")),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Block Text", "toggle-text")
                .visible_when(COND_AGENT)
                .checked_when(COND_TEXT),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Block Voice", "toggle-voice")
                .visible_when(COND_AGENT)
                .checked_when(COND_VOICE),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Block Particles", "toggle-particles")
                .visible_when(COND_AGENT)
                .checked_when(COND_PARTICLES),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Block Object Sounds", "toggle-sounds")
                .visible_when(COND_AGENT)
                .checked_when(COND_SOUNDS),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new("Profile...", "profile").visible_when(COND_AGENT)),
    ],
};

// --- Pure model -----------------------------------------------------------

/// Which entry a row / the selection refers to. Ids are unique for everything
/// but a [`MuteType::ByName`] mute (whose id is nil), so the name is carried
/// too — the same key [`MuteModel`] matches on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct BlockedKey {
    /// The blocked entity's id (nil for a by-name mute).
    id: Uuid,
    /// The blocked entity's name.
    name: String,
}

impl BlockedKey {
    /// The key naming `entry`.
    fn of(entry: &MuteEntry) -> Self {
        Self {
            id: entry.id,
            name: entry.name.clone(),
        }
    }
}

/// Whether `entry` survives the list's name filter (case-insensitive substring,
/// like the reference's `LLBlockList::findInsensitive`).
fn matches_filter(entry: &MuteEntry, filter: &str) -> bool {
    let filter = filter.trim();
    filter.is_empty() || entry.name.to_lowercase().contains(&filter.to_lowercase())
}

/// The sort rank of a mute type — the reference's type sort is by the
/// `LLMute::EType` order (by-name, resident, object, group, external).
const fn type_rank(mute_type: MuteType) -> i32 {
    mute_type.to_i32()
}

/// Order `rows` by the table's sort keys (most significant first), falling back
/// to a case-insensitive name compare so the order is total.
fn sort_rows(rows: &mut [MuteEntry], keys: &[(&str, bool)]) {
    rows.sort_by(|left, right| {
        for (token, ascending) in keys {
            let ordering = match *token {
                "type" => type_rank(left.mute_type).cmp(&type_rank(right.mute_type)),
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

/// The Fluent key naming `mute_type` in the Type column.
const fn type_key(mute_type: MuteType) -> &'static str {
    match mute_type {
        MuteType::ByName => "blocked-type-by-name",
        MuteType::Agent => "blocked-type-resident",
        MuteType::Object => "blocked-type-object",
        MuteType::Group => "blocked-type-group",
        MuteType::External => "blocked-type-external",
        MuteType::Unknown(_other) => "blocked-type-unknown",
        _other => "blocked-type-unknown",
    }
}

/// The `MuteFlags` exception bit each aspect toggle flips.
fn toggle_mask(action: &str) -> Option<u32> {
    match action {
        "toggle-text" => Some(MuteFlags::ALLOW_TEXT_CHAT),
        "toggle-voice" => Some(MuteFlags::ALLOW_VOICE_CHAT),
        "toggle-particles" => Some(MuteFlags::ALLOW_PARTICLES),
        "toggle-sounds" => Some(MuteFlags::ALLOW_OBJECT_SOUNDS),
        _other => None,
    }
}

/// Every exception bit set — no aspect of the entry is muted any more, which is
/// the reference's `LLMute::flagAll` condition for dropping the entry outright.
const ALL_EXCEPTIONS: u32 = MuteFlags::ALLOW_TEXT_CHAT
    | MuteFlags::ALLOW_VOICE_CHAT
    | MuteFlags::ALLOW_PARTICLES
    | MuteFlags::ALLOW_OBJECT_SOUNDS;

/// What an aspect toggle of `mask` on `entry` does: flip that exception bit and
/// re-send the entry, or — if that would except every aspect, leaving nothing
/// muted — remove the entry entirely.
#[derive(Debug, Clone)]
enum ToggleOutcome {
    /// Re-send the entry with the new exception flags (through the guarded
    /// [`RequestBlock`] channel, like every other block).
    Reblock(RequestBlock),
    /// Drop the entry: the reference's `LLMute::flagAll` case. The caller
    /// already holds the entry, so this carries nothing.
    Unblock,
}

/// The outcome an aspect toggle of `mask` on `entry` produces.
fn toggle_outcome(entry: &MuteEntry, mask: u32) -> ToggleOutcome {
    let flags = MuteFlags(entry.flags.0 ^ mask);
    if flags.0 == ALL_EXCEPTIONS {
        ToggleOutcome::Unblock
    } else {
        ToggleOutcome::Reblock(
            RequestBlock::new(entry.id, entry.name.clone(), entry.mute_type).with_flags(flags),
        )
    }
}

// --- Resources ------------------------------------------------------------

/// The block list's live view state: the name filter and the ordered rows the
/// virtual list binds, plus the stamps they were built against.
#[derive(Resource, Debug, Default)]
struct BlockedView {
    /// The live name filter.
    filter: String,
    /// The display rows, in table order.
    rows: Vec<MuteEntry>,
    /// The mute-model revision the rows were built at.
    built_revision: u64,
    /// The table sort revision the rows were ordered at.
    built_sort_revision: u64,
    /// The filter the rows were filtered by.
    built_filter: String,
}

/// The selected entry, which the action buttons act on. Keyed by entry (not row
/// index) so it survives the re-sort and the virtualized row recycling.
#[derive(Resource, Debug, Default)]
struct SelectedBlocked(Option<BlockedKey>);

/// The entry the open context menu targets.
#[derive(Resource, Debug, Default)]
struct BlockedMenuTarget(Option<BlockedKey>);

/// The block list's retained entities (inserted by the deferred panel build;
/// consumers take `Option<Res<BlockedUi>>` until then).
#[derive(Resource, Debug)]
struct BlockedUi {
    /// The table root (carries [`TableState`]).
    table: Entity,
    /// The virtualized viewport (carries [`VirtualList`]).
    viewport: Entity,
    /// The filter box's [`EditableText`] entity.
    filter_field: Entity,
    /// The entry-count line.
    count_text: Entity,
}

/// The by-name block floater's retained entities.
#[derive(Resource, Debug)]
struct BlockByNameUi {
    /// The floater root (carries [`UiPanelShown`]).
    panel: Entity,
    /// The object-name field's [`EditableText`] entity.
    field: Entity,
}

/// The entry a pooled row currently presents.
#[derive(Component, Debug, Clone, Default)]
struct BoundBlocked(Option<BlockedKey>);

/// What a trailing action button does.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum BlockedButton {
    /// Unblock the selected entry.
    Unblock,
    /// Open the avatar picker to block a resident.
    BlockResident,
    /// Open the by-name floater to block an object by name.
    BlockObject,
}

impl BlockedButton {
    /// The Fluent key for this button's label.
    const fn label_key(self) -> &'static str {
        match self {
            Self::Unblock => "blocked-action-unblock",
            Self::BlockResident => "blocked-action-block-resident",
            Self::BlockObject => "blocked-action-block-object",
        }
    }
}

// --- Plugin ---------------------------------------------------------------

/// Registers the block list's view state, the deferred panel build, the by-name
/// floater and the action wiring.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BlockedPlugin;

impl Plugin for BlockedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlockedView>()
            .init_resource::<SelectedBlocked>()
            .init_resource::<BlockedMenuTarget>()
            .add_systems(
                Startup,
                (
                    register_blocked_settings,
                    spawn_block_by_name_floater.after(UiScaffoldSystems::SpawnRoot),
                ),
            )
            .add_systems(
                Update,
                (
                    spawn_blocked_panel.after(UiScaffoldSystems::SpawnRoot),
                    mirror_blocked_filter,
                    rebuild_blocked_view,
                    handle_blocked_actions,
                    handle_blocked_picks,
                )
                    .chain()
                    .before(layout_virtual_lists),
            )
            .add_systems(
                Update,
                (populate_blocked_rows, bind_blocked_rows)
                    .chain()
                    .after(layout_virtual_lists),
            );
    }
}

/// Register the block table's sort / width persistence.
fn register_blocked_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    register_table_settings(&mut settings, BLOCKED_SECTION, &BLOCKED_TABLE);
}

// --- Spawn (deferred until the People pane exists) -------------------------

/// Spawn the block list into the People pane's Blocked content slot, once
/// ([`BlockedUi`] absent) and only after that pane exists ([`PeopleUi`]
/// present) — the same deferral the group list uses.
fn spawn_blocked_panel(
    mut commands: Commands,
    people: Option<Res<PeopleUi>>,
    blocked: Option<Res<BlockedUi>>,
) {
    if blocked.is_some() {
        return;
    }
    let Some(people) = people else {
        return;
    };
    let content = people.blocked_content();

    // The filter row.
    let controls = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                ..row(Val::Px(6.0))
            },
            Name::new("blocked-controls"),
            ChildOf(content),
        ))
        .id();
    let search = spawn_search_field(
        &mut commands,
        controls,
        &SearchFieldSpec {
            tab_index: 1,
            font_size: FONT_SIZE,
            min_width: 140.0,
            placeholder: "Filter by name".to_owned(),
            search_glyph: true,
            ..SearchFieldSpec::new("blocked-filter")
        },
    );
    if let Some(placeholder) = search.placeholder {
        commands
            .entity(placeholder)
            .insert(Translated::new("blocked-filter-placeholder"));
    }

    // The body row: the table takes the width, the actions sit at its trailing
    // edge (mirroring the Friends / Groups content layout).
    let body = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..row(Val::Px(6.0))
            },
            Name::new("blocked-body"),
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
            Name::new("blocked-list-column"),
            ChildOf(body),
        ))
        .id();
    let table = spawn_table(&mut commands, table_column, &BLOCKED_TABLE);
    commands
        .entity(table.viewport)
        .insert((BackgroundColor(LIST_BACKGROUND), TabIndex(2)));
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
            Name::new("blocked-count"),
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
            Name::new("blocked-actions"),
            ChildOf(body),
        ))
        .id();
    for button in [
        BlockedButton::Unblock,
        BlockedButton::BlockResident,
        BlockedButton::BlockObject,
    ] {
        spawn_blocked_button(&mut commands, actions, button);
    }

    commands.insert_resource(BlockedUi {
        table: table.root,
        viewport: table.viewport,
        filter_field: search.field,
        count_text,
    });
}

/// Spawn one trailing action button.
fn spawn_blocked_button(commands: &mut Commands, parent: Entity, button: BlockedButton) {
    commands
        .spawn((
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
            button,
            Name::new("blocked-action"),
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
        .observe(on_blocked_button_press);
}

/// Spawn the "Block Object by Name" floater (hidden): the prompt, the name
/// field, the reference's "only blocks object text" note, and OK / Cancel.
fn spawn_block_by_name_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: BLOCK_BY_NAME_FLOATER_ID,
            title: "Block Object by Name".to_owned(),
            position: Vec2::new(360.0, 200.0),
            default_size: None,
            min_size: None,
            dock_host: None,
            caps: FloaterCaps {
                resizable: false,
                minimizable: false,
                closable: true,
                dockable: false,
            },
        },
    );
    commands
        .entity(handle.title_text)
        .insert(Translated::new("block-by-name-title"));
    let content = handle.content;
    for key in ["block-by-name-prompt", "block-by-name-note"] {
        commands.spawn((
            Text::default(),
            Translated::new(key),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(if key == "block-by-name-note" {
                DIM_LABEL_COLOR
            } else {
                LABEL_COLOR
            }),
            Pickable::IGNORE,
            ChildOf(content),
        ));
    }
    let field = spawn_text_input(
        &mut commands,
        content,
        &TextInputSpec {
            font_size: FONT_SIZE,
            width_glyphs: 28.0,
            tab_index: 1,
            ..TextInputSpec::new("block-by-name-field", TextInputKind::Line)
        },
    );
    let buttons = commands
        .spawn((
            Node {
                ..row(Val::Px(8.0))
            },
            ChildOf(content),
        ))
        .id();
    for confirm in [true, false] {
        commands
            .spawn((
                Node {
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(ACTION_BACKGROUND),
                Pickable {
                    should_block_lower: true,
                    is_hoverable: true,
                },
                BlockByNameButton { confirm },
                ChildOf(buttons),
            ))
            .with_child((
                Text::default(),
                Translated::new(if confirm {
                    "block-by-name-ok"
                } else {
                    "block-by-name-cancel"
                }),
                UiFont::Sans.at(FONT_SIZE),
                TextColor(LABEL_COLOR),
                Pickable::IGNORE,
            ))
            .observe(on_block_by_name_press);
    }

    commands.insert_resource(BlockByNameUi {
        panel: handle.root,
        field,
    });
}

/// Which of the by-name floater's two buttons a node is.
#[derive(Component, Debug, Clone, Copy)]
struct BlockByNameButton {
    /// `true` for OK (add the block), `false` for Cancel.
    confirm: bool,
}

// --- View -----------------------------------------------------------------

/// Mirror the filter field's text into the view state.
fn mirror_blocked_filter(
    ui: Option<Res<BlockedUi>>,
    fields: Query<&EditableText>,
    mut view: ResMut<BlockedView>,
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

/// Rebuild the ordered, filtered rows when the mute list, the sort or the
/// filter moved, and refresh the count line.
fn rebuild_blocked_view(
    model: Res<MuteModel>,
    ui: Option<Res<BlockedUi>>,
    translator: Translator,
    mut view: ResMut<BlockedView>,
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
    if view.built_revision == model.revision()
        && view.built_sort_revision == sort_revision
        && view.built_filter == view.filter
    {
        return;
    }
    // Reborrow so the built-stamp write and the filter read are disjoint field
    // borrows rather than two whole-resource borrows through the `ResMut`.
    let view = &mut *view;
    view.built_revision = model.revision();
    view.built_sort_revision = sort_revision;
    view.built_filter.clone_from(&view.filter);

    let mut rows: Vec<MuteEntry> = model
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
            BLOCKED_TABLE
                .columns
                .get(key.column)
                .map(|column| (column.token, key.ascending))
        })
        .collect();
    sort_rows(&mut rows, &keys);
    view.rows = rows;

    if let Ok(mut list) = lists.get_mut(ui.viewport) {
        list.item_count = view.rows.len();
    }
    let label = translator.format(
        "blocked-count",
        &TransArgs::new()
            .int("shown", i64::try_from(view.rows.len()).unwrap_or(i64::MAX))
            .int(
                "total",
                i64::try_from(model.entries().len()).unwrap_or(i64::MAX),
            ),
    );
    if let Ok(mut text) = texts.get_mut(ui.count_text)
        && text.0 != label
    {
        text.0 = label;
    }
}

/// Build the widget cells of each freshly-pooled row and attach its press
/// observer.
fn populate_blocked_rows(
    mut commands: Commands,
    ui: Option<Res<BlockedUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        spawn_table_row(&mut commands, row_entity, ui.table, &BLOCKED_TABLE);
        commands
            .entity(row_entity)
            .insert(BoundBlocked(None))
            .observe(on_blocked_row_press);
    }
}

/// Bind each pooled row to the entry it now presents: the name and type cells
/// and the selection highlight.
fn bind_blocked_rows(
    view: Res<BlockedView>,
    selected: Res<SelectedBlocked>,
    ui: Option<Res<BlockedUi>>,
    translator: Translator,
    mut rows: Query<(
        Entity,
        Ref<VirtualRow>,
        &ChildOf,
        &TableRowCells,
        &mut BoundBlocked,
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
        bound.0 = data.map(BlockedKey::of);
        let (name, type_label) = data.map_or_else(
            || (String::new(), String::new()),
            |entry| {
                (
                    entry.name.clone(),
                    translator.get(type_key(entry.mute_type)),
                )
            },
        );
        if let Some(cell) = cells.cell(COL_NAME) {
            set_table_cell(&mut texts, cell, &name, LABEL_COLOR);
        }
        if let Some(cell) = cells.cell(COL_TYPE) {
            set_table_cell(&mut texts, cell, &type_label, DIM_LABEL_COLOR);
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

/// A press on a pooled row: primary selects; secondary selects and opens the
/// gear menu with the open-time condition snapshot.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the row pool, the \
              list UI, the mute model, and the focus / selection / menu-target stashes the \
              press writes"
)]
fn on_blocked_row_press(
    mut press: On<Pointer<Press>>,
    rows: Query<&BoundBlocked>,
    ui: Res<BlockedUi>,
    model: Res<MuteModel>,
    mut focus: ResMut<InputFocus>,
    mut selected: ResMut<SelectedBlocked>,
    mut target: ResMut<BlockedMenuTarget>,
    mut menus: MessageWriter<OpenContextMenu>,
) {
    let Ok(BoundBlocked(Some(key))) = rows.get(press.entity) else {
        return;
    };
    let key = key.clone();
    press.propagate(false);
    focus.set(ui.viewport, FocusCause::Navigated);
    selected.0 = Some(key.clone());
    if press.button != PointerButton::Secondary {
        return;
    }
    let Some(entry) = model.entry(key.id, &key.name) else {
        return;
    };
    let mut conditions: Vec<&'static str> = Vec::new();
    if flags_apply(entry.mute_type) {
        conditions.push(COND_AGENT);
    }
    for (mask, condition) in [
        (MuteFlags::ALLOW_TEXT_CHAT, COND_TEXT),
        (MuteFlags::ALLOW_VOICE_CHAT, COND_VOICE),
        (MuteFlags::ALLOW_PARTICLES, COND_PARTICLES),
        (MuteFlags::ALLOW_OBJECT_SOUNDS, COND_SOUNDS),
    ] {
        if !entry.flags.contains(mask) {
            conditions.push(condition);
        }
    }
    target.0 = Some(key);
    menus.write(OpenContextMenu {
        menu: &BLOCKED_MENU,
        at: press.pointer_location.position,
        element: BLOCKED_ELEMENT,
        conditions,
    });
}

/// A press on one of the trailing action buttons.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the button pool, the \
              mute model and selection the Unblock action reads, and the picker / floater / \
              command channels the three buttons write"
)]
fn on_blocked_button_press(
    mut press: On<Pointer<Press>>,
    buttons: Query<&BlockedButton>,
    model: Res<MuteModel>,
    selected: Res<SelectedBlocked>,
    mut panels: Query<&mut UiPanelShown>,
    by_name: Option<Res<BlockByNameUi>>,
    mut pickers: MessageWriter<OpenAvatarPicker>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(button) = buttons.get(press.entity).copied() else {
        return;
    };
    press.propagate(false);
    match button {
        BlockedButton::Unblock => {
            let Some(key) = selected.0.clone() else {
                return;
            };
            let Some(entry) = model.entry(key.id, &key.name) else {
                return;
            };
            sl_commands.write(SlCommand(Command::Unmute {
                id: entry.id,
                name: entry.name.clone(),
            }));
        }
        BlockedButton::BlockResident => {
            pickers.write(OpenAvatarPicker {
                requester: PICKER_REQUESTER,
            });
        }
        BlockedButton::BlockObject => {
            if let Some(by_name) = by_name
                && let Ok(mut shown) = panels.get_mut(by_name.panel)
            {
                shown.0 = true;
            }
        }
    }
}

/// A press on the by-name floater's OK / Cancel: OK asks for the by-name block
/// (the guards run in [`crate::mutes::apply_block_requests`], which raises the
/// refusal notification if one fires), Cancel just closes. Either way the
/// floater shuts, as the reference's does.
fn on_block_by_name_press(
    mut press: On<Pointer<Press>>,
    buttons: Query<&BlockByNameButton>,
    ui: Option<Res<BlockByNameUi>>,
    fields: Query<&EditableText>,
    mut panels: Query<&mut UiPanelShown>,
    mut blocks: MessageWriter<RequestBlock>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(button) = buttons.get(press.entity).copied() else {
        return;
    };
    let Some(ui) = ui else {
        return;
    };
    press.propagate(false);
    if button.confirm {
        let name = fields
            .get(ui.field)
            .map(|field| field.value().to_string().trim().to_owned())
            .unwrap_or_default();
        if name.is_empty() {
            return;
        }
        blocks.write(RequestBlock::new(Uuid::nil(), name, MuteType::ByName));
    }
    if let Ok(mut shown) = panels.get_mut(ui.panel) {
        shown.0 = false;
    }
}

/// Block a resident chosen in the shared avatar picker.
fn handle_blocked_picks(
    mut picks: MessageReader<AvatarPicked>,
    mut blocks: MessageWriter<RequestBlock>,
) {
    for pick in picks.read() {
        if pick.requester != PICKER_REQUESTER {
            continue;
        }
        blocks.write(RequestBlock::new(
            pick.agent.uuid(),
            pick.name.clone(),
            MuteType::Agent,
        ));
    }
}

/// Dispatch the gear menu's picks onto the mute commands and the profile
/// floater.
fn handle_blocked_actions(
    mut actions: MessageReader<UiAction>,
    target: Res<BlockedMenuTarget>,
    model: Res<MuteModel>,
    mut sl_commands: MessageWriter<SlCommand>,
    mut blocks: MessageWriter<RequestBlock>,
    mut profiles: MessageWriter<OpenAvatarProfile>,
) {
    for action in actions.read() {
        if action.element != BLOCKED_ELEMENT {
            continue;
        }
        let Some(key) = target.0.clone() else {
            continue;
        };
        let Some(entry) = model.entry(key.id, &key.name) else {
            continue;
        };
        match action.action {
            "unblock" => {
                sl_commands.write(SlCommand(Command::Unmute {
                    id: entry.id,
                    name: entry.name.clone(),
                }));
            }
            "profile" => {
                if flags_apply(entry.mute_type) {
                    profiles.write(OpenAvatarProfile {
                        agent: AgentKey::from(entry.id),
                    });
                }
            }
            other => {
                if let Some(mask) = toggle_mask(other)
                    && flags_apply(entry.mute_type)
                {
                    match toggle_outcome(entry, mask) {
                        ToggleOutcome::Reblock(request) => {
                            blocks.write(request);
                        }
                        ToggleOutcome::Unblock => {
                            sl_commands.write(SlCommand(Command::Unmute {
                                id: entry.id,
                                name: entry.name.clone(),
                            }));
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ALL_EXCEPTIONS, ToggleOutcome, matches_filter, sort_rows, toggle_mask, toggle_outcome,
        type_key,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{MuteEntry, MuteFlags, MuteType, Uuid};

    /// An entry of `mute_type` named `name`.
    fn entry(id: u128, name: &str, mute_type: MuteType, flags: u32) -> MuteEntry {
        MuteEntry {
            id: Uuid::from_u128(id),
            name: name.to_owned(),
            mute_type,
            flags: MuteFlags(flags),
        }
    }

    /// The filter is a case-insensitive substring, and an empty filter keeps
    /// everything.
    #[test]
    fn filter_is_case_insensitive() {
        let row = entry(1, "Spammy Vendor", MuteType::Object, 0);
        assert!(matches_filter(&row, ""));
        assert!(matches_filter(&row, "  "));
        assert!(matches_filter(&row, "vend"));
        assert!(matches_filter(&row, "SPAMMY"));
        assert!(!matches_filter(&row, "quiet"));
    }

    /// Name sorting is case-insensitive and honours the direction; the type
    /// sort falls back to the name for a tie.
    #[test]
    fn sorting_by_name_and_type() {
        let mut rows = vec![
            entry(1, "beta", MuteType::Object, 0),
            entry(2, "Alpha", MuteType::Agent, 0),
            entry(3, "gamma", MuteType::Agent, 0),
        ];
        sort_rows(&mut rows, &[("name", true)]);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "beta", "gamma"]);

        sort_rows(&mut rows, &[("name", false)]);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["gamma", "beta", "Alpha"]);

        // Agent (1) sorts before Object (2); the two agents tie and fall back
        // to the name.
        sort_rows(&mut rows, &[("type", true)]);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "gamma", "beta"]);
    }

    /// Every mute type maps to its own label key.
    #[test]
    fn type_keys_are_distinct() {
        let keys = [
            type_key(MuteType::ByName),
            type_key(MuteType::Agent),
            type_key(MuteType::Object),
            type_key(MuteType::Group),
            type_key(MuteType::External),
            type_key(MuteType::Unknown(9)),
        ];
        let mut sorted = keys;
        sorted.sort_unstable();
        let mut deduped = sorted.to_vec();
        deduped.dedup();
        assert_eq!(deduped.len(), keys.len());
    }

    /// Each toggle action maps to its own exception bit, and together they are
    /// exactly [`ALL_EXCEPTIONS`].
    #[test]
    fn toggle_masks_cover_every_aspect() {
        let mut union = 0;
        for action in [
            "toggle-text",
            "toggle-voice",
            "toggle-particles",
            "toggle-sounds",
        ] {
            let Some(mask) = toggle_mask(action) else {
                unreachable!("every toggle action has a mask")
            };
            assert_eq!(union & mask, 0, "{action} overlaps another aspect");
            union |= mask;
        }
        assert_eq!(union, ALL_EXCEPTIONS);
        assert_eq!(toggle_mask("unblock"), None);
    }

    /// Toggling an aspect off re-blocks the entry with that exception bit set;
    /// excepting the last aspect removes the entry instead.
    #[test]
    fn toggling_flags_and_the_last_aspect() {
        let row = entry(1, "Troll Resident", MuteType::Agent, 0);
        let ToggleOutcome::Reblock(request) = toggle_outcome(&row, MuteFlags::ALLOW_VOICE_CHAT)
        else {
            unreachable!("a partial exception re-blocks the entry")
        };
        assert_eq!(request.flags, MuteFlags(MuteFlags::ALLOW_VOICE_CHAT));
        assert_eq!(request.mute_type, MuteType::Agent);

        // Re-toggling the same aspect clears the bit again.
        let partial = entry(
            1,
            "Troll Resident",
            MuteType::Agent,
            MuteFlags::ALLOW_VOICE_CHAT,
        );
        let ToggleOutcome::Reblock(request) = toggle_outcome(&partial, MuteFlags::ALLOW_VOICE_CHAT)
        else {
            unreachable!("a partial exception re-blocks the entry")
        };
        assert_eq!(request.flags, MuteFlags(0));

        let nearly = entry(
            1,
            "Troll Resident",
            MuteType::Agent,
            ALL_EXCEPTIONS & !MuteFlags::ALLOW_TEXT_CHAT,
        );
        assert!(matches!(
            toggle_outcome(&nearly, MuteFlags::ALLOW_TEXT_CHAT),
            ToggleOutcome::Unblock
        ));
    }
}
