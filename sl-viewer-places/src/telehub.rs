//! The **Telehub** floater (`viewer-region-telehub`): the estate owner's view of
//! the region's telehub — the object incoming teleports are routed to, and the
//! spawn points an arriving avatar is placed on.
//!
//! Reached from the Region / Estate floater's Region tab ("Manage Telehub…",
//! [`crate::about_region`]), which is the only way in — as in the reference,
//! where the button lives on `panel_region_general.xml`.
//!
//! # What it does
//!
//! Four commands, all `EstateOwnerMessage`/`telehub` and all already on the wire
//! ([`Command::ConnectTelehub`] and friends):
//!
//! - **Connect Telehub** makes the selected in-world object the region's telehub;
//! - **Disconnect** removes the telehub (and, on the simulator side, its spawn
//!   points with it);
//! - **Add Spawn** records the selected object's *position* as a spawn point.
//!   The object itself is not kept — the reference's help text says so out loud,
//!   and the simulator stores the position relative to the telehub — so the prim
//!   used to place one can be moved or deleted afterwards;
//! - **Remove Spawn** drops the spawn point selected in the list, by index.
//!
//! Every one of them is answered by a fresh `TelehubInfo`
//! ([`SlSessionEvent::TelehubInfo`]) stating the region's whole telehub
//! configuration, so this window never guesses what a command did: it asks once
//! on open and otherwise shows what the region last said. (The simulator replies
//! that way to *every* telehub method, including the `info ui` read — OpenSim's
//! `HandleOnEstateManageTelehub` ends in `SendTelehubInfo` whatever the command
//! was.)
//!
//! # The selection is the argument
//!
//! Connect and Add Spawn take no argument of their own: they act on whatever the
//! build tools have selected, exactly as the reference does
//! (`LLSelectMgr::sendGodlikeRequest`, which sends one message **per selected
//! root object** — `SEND_ONLY_ROOTS`). So selecting three prims and pressing Add
//! Spawn adds three spawn points, and the buttons are dead while nothing is
//! selected. Only ordinary prims count ([`pcode::PRIMITIVE`], the reference's
//! `selectionAllPCode(LL_PCODE_VOLUME)`): a telehub cannot be a tree.
//!
//! # Marking them in the world
//!
//! While the window is open it asks for two in-world markers
//! ([`DebugBeacons`], drawn by `sl-viewer-world-scene`'s `debug_beacons`): a
//! yellow one on the telehub itself and an orange one on the spawn point
//! selected in the list — the reference's two `addDebugBeacon` calls, colours
//! included. The telehub marker **follows the object** rather than the position
//! the reply carried, so moving the hub prim moves its marker; the spawn marker
//! rides the hub's frame, because that is where the simulator keeps it (a spawn
//! point is stored relative to the telehub and rotates with it).
//!
//! Closing the window takes both markers with it.
//!
//! # Divergences from the reference
//!
//! - The reference **hides** the Region / Estate floater as it opens this one
//!   (`LLPanelRegionInfo::onClickManageTelehub`), because both are wide windows
//!   over the build tools. Ours is one window per region and hiding an instance
//!   the person opened is a surprise, so both stay up.
//! - The reference switches the toolset to the translate tool on open, to make
//!   sure something *can* be selected. We leave the current tool alone: the
//!   viewer's selection is not owned by a floater.
//! - The reference lists spawn points as a plain scroll list of formatted
//!   positions; ours is the table widget with one column, which is the same
//!   thing plus the column sizing and keyboard behaviour every other list here
//!   already has.
//!
//! Reference (Firestorm, read-only): `llfloatertelehub.cpp`,
//! `floater_telehub.xml`, `panel_region_general.xml` (`manage_telehub_btn`),
//! `llselectmgr.cpp` (`sendGodlikeRequest`).

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use sl_client_bevy::{
    Command, ObjectKey, RegionHandle, ScopedObjectId, SlCommand, SlCurrentRegion, SlEvent,
    SlRegionIdentity, SlSessionEvent, TelehubInfo, Vector, pcode,
};

use crate::floater::{
    DeferredFloaterContent, Floater, FloaterCaps, FloaterHandle, FloaterSpec, spawn_floater,
};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;
use crate::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableSelectionMode, TableSpec,
    TableState, set_table_cell, spawn_table, spawn_table_row,
};
use crate::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists};
use crate::world_api::{DebugBeacon, DebugBeacons, ObjectState, SelectionSet};

/// The floater's stable [`Floater::id`].
pub const TELEHUB_FLOATER_ID: &str = "telehub";

/// The debug-beacon group this window owns.
const BEACON_OWNER: &str = "telehub";

/// The most spawn points a telehub may hold — the reference's
/// `MAX_SPAWNPOINTS_PER_TELEHUB`, which is also the width of the array its reply
/// is unpacked into.
const MAX_SPAWN_POINTS: usize = 16;

/// The floater's body font size, in logical pixels.
const FONT_SIZE: f32 = 13.0;

/// A value / label text colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A dim label / secondary text colour.
const DIM_LABEL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// A disabled control's text colour.
const DISABLED_COLOR: Color = Color::srgb(0.45, 0.47, 0.52);

/// An action button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

/// An action button's border.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// The spawn-point list's background.
const LIST_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);

/// The spawn-point list's bounded height, in logical pixels.
const LIST_HEIGHT: f32 = 150.0;

/// One list row's height, in logical pixels.
const ROW_HEIGHT: f32 = 22.0;

/// The telehub's own marker colour — the reference's `LLColor4::yellow`.
const HUB_MARKER_COLOR: Color = Color::srgb(1.0, 1.0, 0.0);

/// The selected spawn point's marker colour — the reference's
/// `LLColor4::orange`.
const SPAWN_MARKER_COLOR: Color = Color::srgb(1.0, 0.5, 0.0);

/// The spawn-point list: one column, the position relative to the telehub.
const SPAWN_TABLE: TableSpec = TableSpec {
    element: "telehub-spawn-points",
    selection: TableSelectionMode::Single,
    columns: &[TableColumn {
        header_key: "telehub-spawn-position",
        token: "position",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Flex(1.0),
        align: TableAlign::Start,
        sortable: false,
    }],
    default_sort: &[],
    builtin_sort: false,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_LABEL_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 6.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

// ---------------------------------------------------------------------------
// Messages.
// ---------------------------------------------------------------------------

/// Open (or raise) the Telehub window — written by the Region / Estate floater's
/// **Manage Telehub…** button.
#[derive(Message, Debug, Clone, Copy, Default)]
pub struct OpenTelehub;

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// A press-dispatch tag on the window's action buttons.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum TelehubAction {
    /// Make the selection the region's telehub.
    Connect,
    /// Remove the region's telehub.
    Disconnect,
    /// Record the selection's position as a spawn point.
    AddSpawn,
    /// Drop the spawn point selected in the list.
    RemoveSpawn,
}

/// The window's node handles, for the in-place updates.
#[derive(Resource, Debug)]
struct TelehubUi {
    /// The floater root (its `UiPanelShown` is the open/closed state).
    panel: Entity,
    /// The status line ("connected to X" / "no telehub").
    status: Entity,
    /// The help line under it.
    help: Entity,
    /// The spawn-point table's root (carries its [`TableState`]).
    table: Entity,
    /// The spawn-point table's scrolling viewport (the pooled rows' parent).
    viewport: Entity,
}

/// What the region last said about its telehub, and what the window has done
/// about it.
#[derive(Resource, Debug, Default)]
struct TelehubState {
    /// The last `TelehubInfo` the region sent, or `None` before the first reply.
    info: Option<TelehubInfo>,
    /// Whether the read request has gone out for this opening of the window.
    requested: bool,
    /// Whether the view needs a repaint (a reply landed, or the window opened).
    dirty: bool,
}

impl TelehubState {
    /// The spawn points the region last reported, relative to the telehub.
    fn spawn_points(&self) -> &[Vector] {
        self.info
            .as_ref()
            .map_or(&[], |info| info.spawn_points.as_slice())
    }

    /// The telehub object, or `None` while the region has none.
    fn hub(&self) -> Option<ObjectKey> {
        self.info.as_ref().and_then(|info| info.object_id)
    }
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin wiring the Telehub floater into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub struct TelehubPlugin;

impl Plugin for TelehubPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<OpenTelehub>()
            .init_resource::<TelehubState>()
            // The three shared models this window reads and writes. Their real
            // owners live above this crate — the selection in `sl-viewer-edit`,
            // the object table in the world layer, the beacons in the scene
            // renderer — and each declares them the same way, so whoever is
            // built first mints the one everybody shares. Declaring them here
            // too is what lets this plugin be scheduled on its own (the About
            // Region tests do exactly that) instead of panicking on frame one.
            .init_resource::<SelectionSet>()
            .init_resource::<ObjectState>()
            .init_resource::<DebugBeacons>()
            .add_systems(
                Startup,
                spawn_telehub_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    open_telehub,
                    request_on_open,
                    ingest_telehub_info,
                    sync_spawn_list,
                    populate_spawn_rows,
                    bind_spawn_rows,
                    update_status_lines,
                    update_button_enable,
                    publish_beacons,
                )
                    .chain()
                    .before(layout_virtual_lists),
            );
    }
}

/// The Telehub floater's [`FloaterSpec`] — shared with the `FLOATERS` registry,
/// so the swept window is the one the viewer spawns.
#[must_use]
pub fn telehub_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: TELEHUB_FLOATER_ID,
        title: "Telehub".to_owned(),
        // The reference notes an "explicit left edge to avoid overlapping build
        // tools", and the same reasoning applies here: the window is worked
        // alongside a selection, so it opens clear of the tools on the right.
        position: Vec2::new(160.0, 140.0),
        default_size: Some(Vec2::new(360.0, 380.0)),
        min_size: Some(Vec2::new(280.0, 260.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: false,
        },
    }
}

/// Startup: spawn the (hidden) floater chrome, its content deferred to first
/// open.
fn spawn_telehub_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, telehub_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("telehub-title"));
    let builder = commands.register_system(build_telehub_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

// ---------------------------------------------------------------------------
// Content.
// ---------------------------------------------------------------------------

/// First-open content build: the status lines, the connect row, the spawn-point
/// list and its two buttons, and the reference's explanatory footer.
fn build_telehub_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                padding: UiRect::all(Val::Px(8.0)),
                ..column(Val::Px(6.0))
            },
            Name::new("telehub:content"),
            ChildOf(handle.content),
        ))
        .id();

    let status = spawn_line(&mut commands, content, LABEL_COLOR);
    let help = spawn_line(&mut commands, content, DIM_LABEL_COLOR);

    let connect_row = spawn_row(&mut commands, content);
    spawn_action_button(
        &mut commands,
        connect_row,
        "telehub-connect",
        TelehubAction::Connect,
        0,
    );
    spawn_action_button(
        &mut commands,
        connect_row,
        "telehub-disconnect",
        TelehubAction::Disconnect,
        1,
    );

    spawn_label(&mut commands, content, "telehub-spawn-points");
    let wrapper = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(LIST_HEIGHT),
                ..default()
            },
            BackgroundColor(LIST_BACKGROUND),
            ChildOf(content),
        ))
        .id();
    let table = spawn_table(&mut commands, wrapper, &SPAWN_TABLE);

    let spawn_row_entity = spawn_row(&mut commands, content);
    spawn_action_button(
        &mut commands,
        spawn_row_entity,
        "telehub-add-spawn",
        TelehubAction::AddSpawn,
        2,
    );
    spawn_action_button(
        &mut commands,
        spawn_row_entity,
        "telehub-remove-spawn",
        TelehubAction::RemoveSpawn,
        3,
    );

    spawn_note(&mut commands, content, "telehub-spawn-help");

    commands.insert_resource(TelehubUi {
        panel: handle.root,
        status,
        help,
        table: table.root,
        viewport: table.viewport,
    });
}

/// A wrapping row of controls.
fn spawn_row(commands: &mut Commands, parent: Entity) -> Entity {
    commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                ..row(Val::Px(8.0))
            },
            ChildOf(parent),
        ))
        .id()
}

/// An empty text line the caller updates in place.
fn spawn_line(commands: &mut Commands, parent: Entity, color: Color) -> Entity {
    commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(color),
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id()
}

/// A translated label on its own line.
fn spawn_label(commands: &mut Commands, parent: Entity, key: &'static str) {
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(parent),
    ));
}

/// The wrapped explanatory footer.
fn spawn_note(commands: &mut Commands, parent: Entity, key: &'static str) {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                ..column(Val::Px(0.0))
            },
            ChildOf(parent),
        ))
        .with_child((
            Text::default(),
            Translated::new(key),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Pickable::IGNORE,
        ));
}

/// A translated action button dispatching `action`.
fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    action: TelehubAction,
    tab_index: i32,
) {
    let button = commands
        .spawn((
            Button,
            TabIndex(tab_index),
            action,
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            Name::new(format!("telehub-button:{label_key}")),
            ChildOf(parent),
        ))
        .observe(on_telehub_action)
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
}

// ---------------------------------------------------------------------------
// Open, read, ingest.
// ---------------------------------------------------------------------------

/// Show the window on an [`OpenTelehub`] — never toggle it: the Region floater's
/// button means "open this", and a second press while it is up should raise it,
/// not close it.
fn open_telehub(
    mut opens: MessageReader<OpenTelehub>,
    ui: Option<Res<TelehubUi>>,
    floaters: Query<(Entity, &Floater)>,
    mut panels: Query<&mut UiPanelShown>,
) {
    if opens.read().count() == 0 {
        return;
    }
    let panel = ui.as_deref().map(|ui| ui.panel).or_else(|| {
        floaters
            .iter()
            .find(|(_entity, floater)| floater.id == TELEHUB_FLOATER_ID)
            .map(|(entity, _floater)| entity)
    });
    if let Some(panel) = panel
        && let Ok(mut shown) = panels.get_mut(panel)
    {
        shown.0 = true;
    }
}

/// Ask the region for its telehub configuration each time the window opens, and
/// forget what it said when the window closes — the next opening asks again
/// rather than showing a configuration from before a teleport.
fn request_on_open(
    panels: Query<(Entity, &UiPanelShown), Changed<UiPanelShown>>,
    ui: Option<Res<TelehubUi>>,
    mut state: ResMut<TelehubState>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui.as_deref() else {
        return;
    };
    let Some((_entity, shown)) = panels.iter().find(|(entity, _shown)| *entity == ui.panel) else {
        return;
    };
    if shown.0 {
        if !state.requested {
            state.requested = true;
            commands.write(SlCommand(Command::RequestTelehubInfo));
        }
    } else {
        state.requested = false;
        state.info = None;
        state.dirty = true;
    }
}

/// Fold every `TelehubInfo` reply into the window's state. The reply states the
/// region's whole configuration, so it replaces what was there rather than
/// merging.
fn ingest_telehub_info(mut events: MessageReader<SlEvent>, mut state: ResMut<TelehubState>) {
    for event in events.read() {
        if let SlSessionEvent::TelehubInfo(info) = &event.0 {
            state.info = Some(info.clone());
            state.dirty = true;
        }
    }
}

// ---------------------------------------------------------------------------
// The spawn-point list.
// ---------------------------------------------------------------------------

/// Keep the list's item count in step with the reply, and put the selection on
/// the last spawn point when a reply arrives — the reference's
/// `selectNthItem(mNumSpawn - 1)`, which is what makes a just-added spawn point
/// the one the beacon marks.
fn sync_spawn_list(
    ui: Option<Res<TelehubUi>>,
    state: Res<TelehubState>,
    mut lists: Query<&mut VirtualList>,
    mut tables: Query<&mut TableState>,
) {
    let Some(ui) = ui.as_deref() else {
        return;
    };
    if !state.dirty {
        return;
    }
    let count = state.spawn_points().len();
    if let Ok(mut list) = lists.get_mut(ui.viewport)
        && list.item_count != count
    {
        list.item_count = count;
    }
    if let Ok(mut table) = tables.get_mut(ui.table) {
        match count.checked_sub(1) {
            Some(last) => table.set_selection(vec![last], Some(last)),
            None => table.clear_selection(),
        }
    }
}

/// Give each pooled row its cells the first time it is spawned.
fn populate_spawn_rows(
    mut commands: Commands,
    ui: Option<Res<TelehubUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui.as_deref() else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        spawn_table_row(&mut commands, row_entity, ui.table, &SPAWN_TABLE);
    }
}

/// Write each visible row's spawn-point position into its cell.
fn bind_spawn_rows(
    ui: Option<Res<TelehubUi>>,
    state: Res<TelehubState>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &crate::ui_table::TableRowCells)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui.as_deref() else {
        return;
    };
    let refresh = state.is_changed();
    for (row, child_of, cells) in &rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        if !refresh && !row.is_changed() {
            continue;
        }
        let Some(position) = row
            .index
            .and_then(|index| state.spawn_points().get(index))
            .map(format_position)
        else {
            continue;
        };
        if let Some(cell) = cells.cell(0) {
            set_table_cell(&mut texts, cell, &position, LABEL_COLOR);
        }
    }
}

/// One spawn point's position, as the reference formats it (`%.1f, %.1f, %.1f`).
fn format_position(position: &Vector) -> String {
    format!("{:.1}, {:.1}, {:.1}", position.x, position.y, position.z)
}

// ---------------------------------------------------------------------------
// The status lines and the button states.
// ---------------------------------------------------------------------------

/// Repaint the status and help lines from the last reply.
fn update_status_lines(
    ui: Option<Res<TelehubUi>>,
    mut state: ResMut<TelehubState>,
    translator: Translator,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui.as_deref() else {
        return;
    };
    if !state.dirty {
        return;
    }
    state.dirty = false;
    let (status, help) = match state.info.as_ref() {
        Some(info) if info.object_id.is_some() => (
            translator.format(
                "telehub-status-connected",
                &TransArgs::new().text("object", &info.object_name),
            ),
            translator.get("telehub-help-connected"),
        ),
        Some(_) => (
            translator.get("telehub-status-not-connected"),
            translator.get("telehub-help-not-connected"),
        ),
        None => (
            translator.get("telehub-status-loading"),
            translator.get("telehub-help-not-connected"),
        ),
    };
    if let Ok(mut text) = texts.get_mut(ui.status) {
        status.clone_into(&mut text.0);
    }
    if let Ok(mut text) = texts.get_mut(ui.help) {
        help.clone_into(&mut text.0);
    }
}

/// Whether the agent may manage this region's estate — the gate the reference
/// puts on the Region tab's button, re-checked here because the window outlives
/// the press that opened it (walk across a border and the rights can change).
fn can_manage(regions: &Query<&SlRegionIdentity, With<SlCurrentRegion>>) -> bool {
    regions
        .iter()
        .next()
        .is_some_and(|identity| identity.0.is_estate_manager)
}

/// The selected **root** prims, in selection order — the reference's
/// `SEND_ONLY_ROOTS` over `selectionAllPCode(LL_PCODE_VOLUME)`, which is what
/// Connect and Add Spawn act on.
fn selected_roots(selection: &SelectionSet, objects: &ObjectState) -> Vec<ScopedObjectId> {
    selection
        .iter()
        .filter(|node| {
            objects
                .objects
                .get(&node.scoped)
                .is_some_and(|tracked| tracked.is_root && tracked.shape.pcode() == pcode::PRIMITIVE)
        })
        .map(|node| node.scoped)
        .collect()
}

/// Whether each action can be taken right now — the reference's per-frame
/// `LLFloaterTelehub::refresh`, plus the estate gate.
fn action_enabled(
    action: TelehubAction,
    manage: bool,
    selected: usize,
    state: &TelehubState,
    spawn_selected: bool,
) -> bool {
    if !manage {
        return false;
    }
    match action {
        TelehubAction::Connect => selected > 0,
        TelehubAction::Disconnect => state.hub().is_some(),
        TelehubAction::AddSpawn => selected > 0 && state.spawn_points().len() < MAX_SPAWN_POINTS,
        TelehubAction::RemoveSpawn => spawn_selected,
    }
}

/// Grey out and refuse the buttons whose action cannot be taken.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the telehub \
              state and window handles, the region rights, the selection and object table the \
              preconditions read, the table's selection, the buttons and their disabled marker, \
              and the label / command outputs"
)]
fn update_button_enable(
    state: Res<TelehubState>,
    ui: Option<Res<TelehubUi>>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    tables: Query<&TableState>,
    buttons: Query<(Entity, &TelehubAction)>,
    disabled: Query<(), With<InteractionDisabled>>,
    children: Query<&Children>,
    mut texts: Query<&mut TextColor>,
    mut commands: Commands,
) {
    let Some(ui) = ui.as_deref() else {
        return;
    };
    let manage = can_manage(&regions);
    let selected = selected_roots(&selection, &objects).len();
    let spawn_selected = tables
        .get(ui.table)
        .is_ok_and(|table| table.primary_selected().is_some());
    for (entity, action) in &buttons {
        let enabled = action_enabled(*action, manage, selected, &state, spawn_selected);
        let is_disabled = disabled.contains(entity);
        if enabled && is_disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else if !enabled && !is_disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
        let want = if enabled { LABEL_COLOR } else { DISABLED_COLOR };
        if let Ok(label) = children.get(entity) {
            for child in label.iter() {
                if let Ok(mut color) = texts.get_mut(child)
                    && color.0 != want
                {
                    color.0 = want;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The in-world markers.
// ---------------------------------------------------------------------------

/// The beacons this window is asking for: the telehub in yellow, and the spawn
/// point selected in the list in orange, in the telehub's own frame.
fn telehub_beacons(
    info: &TelehubInfo,
    region: RegionHandle,
    selected_spawn: Option<usize>,
) -> Vec<DebugBeacon> {
    let Some(hub) = info.object_id else {
        return Vec::new();
    };
    let mut beacons = vec![DebugBeacon {
        region,
        anchor: Some(hub),
        position: info.position.clone(),
        rotation: info.rotation.clone(),
        offset: Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        color: HUB_MARKER_COLOR,
    }];
    if let Some(spawn) = selected_spawn.and_then(|index| info.spawn_points.get(index)) {
        beacons.push(DebugBeacon {
            region,
            anchor: Some(hub),
            position: info.position.clone(),
            rotation: info.rotation.clone(),
            offset: spawn.clone(),
            color: SPAWN_MARKER_COLOR,
        });
    }
    beacons
}

/// Publish (or drop) this window's markers. Dropped whenever the window is not
/// open, whatever took it down — a Close, a hide, or a `TelehubInfo` saying the
/// region has no telehub any more.
fn publish_beacons(
    state: Res<TelehubState>,
    ui: Option<Res<TelehubUi>>,
    panels: Query<&UiPanelShown>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    tables: Query<&TableState>,
    mut beacons: ResMut<DebugBeacons>,
) {
    let shown = ui
        .as_deref()
        .and_then(|ui| panels.get(ui.panel).ok())
        .is_some_and(|shown| shown.0);
    let wanted = match (
        shown,
        ui.as_deref(),
        state.info.as_ref(),
        regions.iter().next(),
    ) {
        (true, Some(ui), Some(info), Some(identity)) => {
            let selected = tables
                .get(ui.table)
                .ok()
                .and_then(TableState::primary_selected);
            telehub_beacons(info, identity.0.region_handle, selected)
        }
        _ => Vec::new(),
    };
    if wanted.is_empty() {
        if beacons.has(BEACON_OWNER) {
            beacons.clear(BEACON_OWNER);
        }
    } else {
        beacons.set(BEACON_OWNER, wanted);
    }
}

// ---------------------------------------------------------------------------
// The actions.
// ---------------------------------------------------------------------------

/// Send the pressed button's command.
#[expect(
    clippy::too_many_arguments,
    reason = "an observer's parameters are its injected resources / queries: the press, the \
              pressed button's action, the telehub state and window handles, the region rights, \
              the selection and object table, the table's selection, and the command sink"
)]
fn on_telehub_action(
    press: On<Pointer<Press>>,
    actions: Query<&TelehubAction>,
    state: Res<TelehubState>,
    ui: Option<Res<TelehubUi>>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    tables: Query<&TableState>,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(action) = actions.get(press.entity) else {
        return;
    };
    let Some(ui) = ui.as_deref() else {
        return;
    };
    let manage = can_manage(&regions);
    let roots = selected_roots(&selection, &objects);
    let spawn = tables
        .get(ui.table)
        .ok()
        .and_then(TableState::primary_selected);
    if !action_enabled(*action, manage, roots.len(), &state, spawn.is_some()) {
        return;
    }
    match action {
        // One message per selected root, as the reference's `SEND_ONLY_ROOTS`
        // does: connecting with several selected settles on the last, and adding
        // spawn points with several selected adds one each.
        TelehubAction::Connect => {
            for root in roots {
                commands.write(SlCommand(Command::ConnectTelehub {
                    object_local_id: root,
                }));
            }
        }
        TelehubAction::Disconnect => {
            commands.write(SlCommand(Command::DisconnectTelehub));
        }
        TelehubAction::AddSpawn => {
            for root in roots {
                commands.write(SlCommand(Command::AddTelehubSpawnPoint {
                    object_local_id: root,
                }));
            }
        }
        TelehubAction::RemoveSpawn => {
            if let Some(index) = spawn.and_then(|index| u32::try_from(index).ok()) {
                commands.write(SlCommand(Command::RemoveTelehubSpawnPoint {
                    spawn_index: index,
                }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_SPAWN_POINTS, TelehubAction, TelehubState, format_position, telehub_beacons};
    use sl_client_bevy::{ObjectKey, RegionHandle, Rotation, TelehubInfo, Uuid, Vector};

    /// A Second Life vector.
    const fn vector(x: f32, y: f32, z: f32) -> Vector {
        Vector { x, y, z }
    }

    /// The identity rotation.
    const fn identity() -> Rotation {
        Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        }
    }

    /// A telehub reply with `spawns` spawn points.
    fn info(connected: bool, spawns: usize) -> TelehubInfo {
        TelehubInfo {
            object_id: connected.then(|| ObjectKey::from(Uuid::from_u128(0x7e1e))),
            object_name: "Welcome Hub".to_owned(),
            position: vector(128.0, 128.0, 25.0),
            rotation: identity(),
            spawn_points: (0..spawns)
                .map(|index| {
                    #[expect(
                        clippy::as_conversions,
                        clippy::cast_precision_loss,
                        reason = "a small test index is exact as f32"
                    )]
                    vector(index as f32, 0.0, 0.0)
                })
                .collect(),
        }
    }

    /// A state holding `info`.
    fn state(info: Option<TelehubInfo>) -> TelehubState {
        TelehubState {
            info,
            ..TelehubState::default()
        }
    }

    /// The reference's `%.1f, %.1f, %.1f`.
    #[test]
    fn a_spawn_point_reads_as_three_one_decimal_numbers() {
        pretty_assertions::assert_eq!(
            format_position(&vector(1.25, -2.0, 30.0)),
            "1.2, -2.0, 30.0"
        );
    }

    /// Nothing can be done without estate rights, whatever is selected.
    #[test]
    fn without_estate_rights_nothing_is_enabled() {
        let state = state(Some(info(true, 2)));
        for action in [
            TelehubAction::Connect,
            TelehubAction::Disconnect,
            TelehubAction::AddSpawn,
            TelehubAction::RemoveSpawn,
        ] {
            assert!(!super::action_enabled(action, false, 1, &state, true));
        }
    }

    /// Connect and Add Spawn need a selection; Disconnect needs a telehub;
    /// Remove Spawn needs a selected row.
    #[test]
    fn each_action_has_its_own_precondition() {
        let none = state(Some(info(false, 0)));
        assert!(!super::action_enabled(
            TelehubAction::Connect,
            true,
            0,
            &none,
            false
        ));
        assert!(super::action_enabled(
            TelehubAction::Connect,
            true,
            1,
            &none,
            false
        ));
        assert!(!super::action_enabled(
            TelehubAction::Disconnect,
            true,
            1,
            &none,
            false
        ));
        let connected = state(Some(info(true, 1)));
        assert!(super::action_enabled(
            TelehubAction::Disconnect,
            true,
            0,
            &connected,
            false
        ));
        assert!(!super::action_enabled(
            TelehubAction::RemoveSpawn,
            true,
            0,
            &connected,
            false
        ));
        assert!(super::action_enabled(
            TelehubAction::RemoveSpawn,
            true,
            0,
            &connected,
            true
        ));
    }

    /// A full telehub takes no more spawn points, however much is selected.
    #[test]
    fn a_full_telehub_refuses_another_spawn_point() {
        let full = state(Some(info(true, MAX_SPAWN_POINTS)));
        assert!(!super::action_enabled(
            TelehubAction::AddSpawn,
            true,
            1,
            &full,
            false
        ));
        let room = state(Some(info(true, MAX_SPAWN_POINTS - 1)));
        assert!(super::action_enabled(
            TelehubAction::AddSpawn,
            true,
            1,
            &room,
            false
        ));
    }

    /// A region with no telehub is marked nowhere; one with a telehub is marked
    /// once, and twice while a spawn point is selected — the second riding the
    /// hub's frame at the spawn point's offset.
    #[test]
    fn the_markers_are_the_hub_and_the_selected_spawn() {
        let region = RegionHandle::new((1_000_000_u64 << 32) | 2_000_000_u64);
        assert!(telehub_beacons(&info(false, 0), region, None).is_empty());
        let connected = info(true, 3);
        let alone = telehub_beacons(&connected, region, None);
        pretty_assertions::assert_eq!(alone.len(), 1);
        pretty_assertions::assert_eq!(
            alone.first().and_then(|beacon| beacon.anchor),
            connected.object_id
        );
        let with_spawn = telehub_beacons(&connected, region, Some(2));
        pretty_assertions::assert_eq!(with_spawn.len(), 2);
        pretty_assertions::assert_eq!(
            with_spawn.get(1).map(|beacon| beacon.offset.clone()),
            Some(vector(2.0, 0.0, 0.0))
        );
        // A selection past the end of the list marks only the hub.
        pretty_assertions::assert_eq!(telehub_beacons(&connected, region, Some(9)).len(), 1);
    }
}
