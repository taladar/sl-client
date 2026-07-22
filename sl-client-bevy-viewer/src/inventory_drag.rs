//! **Inventory drag-and-drop** (`viewer-inventory-context-actions`): dragging a
//! row moves it between folders, and a drag that leaves the window's list gives
//! the item to an avatar (a people-list row, a name tag, or the avatar's body
//! in-world) or rezzes an object where it lands, matching the reference
//! viewer's drag behaviours (`llfolderview` / `llinventorybridge` /
//! `lltooldragdrop`).
//!
//! # The gesture
//!
//! A primary-button drag on a pooled row starts it (`bevy_picking`'s
//! [`Pointer<DragStart>`], which fires only past the drag threshold — a plain
//! click never becomes a drag). A **ghost** — the row's icon and name —
//! follows the pointer, [`Pickable::IGNORE`] so it never occludes the drop
//! target under it. `Escape` cancels. On [`Pointer<DragEnd>`] the drop resolves
//! by where the pointer is, in the reference's occlusion order:
//!
//! 1. **Over the inventory list** — the row under the pointer names the
//!    destination folder (an item row means its containing folder; the empty
//!    space below the rows means the agent root). A move within the own tree, a
//!    **copy** when the source is a Library row (that is how the Library is
//!    used), rejected into the Library or into a folder's own subtree.
//! 2. **Over an avatar in the UI** — a people-list row ([`AgentDropTarget`]) or
//!    a floating name tag ([`AvatarPickTarget`]): **give** the item / folder
//!    ([`Command::GiveInventory`]); onto **yourself**: wear it instead, exactly
//!    as the reference treats a self-drop.
//! 3. **Over other blocking UI** — the drag cancels (a floater is not a drop
//!    target).
//! 4. **Over the world** — the mesh-accurate avatar pick resolves a body under
//!    the pointer (give / self-wear); otherwise an **object** item rezzes at
//!    the world point under the pointer ([`Command::RezObjectFromInventory`]),
//!    the reference's drag-rez.
//!
//! # While dragging over the list
//!
//! The pointer near the viewport's top / bottom edge **auto-scrolls** the
//! virtualized list, and lingering over a collapsed folder **auto-expands** it
//! ([`crate::inventory::InventoryUiAction::ExpandFolder`]) — the two affordances
//! that make a deep tree reachable mid-drag (the reference's `llfolderview`
//! does both). The destination folder's row is highlighted while it is the
//! target.

use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use sl_client_bevy::{
    AgentKey, Command, InventoryFolderKey, InventoryType, ItemInfo, Permissions, RestoreItem,
    RezObjectParams, SaleType, SlCommand, SlIdentity, TransactionId, Uuid, Vector,
};

use crate::avatar_pick::AvatarPicker;
use crate::avatars::AvatarPickTarget;
use crate::camera::ViewerCamera;
use crate::coords::bevy_to_sl_vec;
use crate::hud_pick::pointer_over_blocking_ui;
use crate::inventory::{
    InventoryModel, InventorySelection, InventoryUi, InventoryUiAction, InventoryView, RowKey,
    query_folder_page,
};
use crate::inventory_actions::{MenuTarget, WornAttachments, wear_commands};
use crate::ui::UiRoot;
use crate::ui_font::UiFont;
use crate::virtual_list::{VirtualList, VirtualRow};

/// The ghost's offset from the pointer, in logical pixels — clear of the hot
/// pixel so the pointer, not the ghost, decides the drop target.
const GHOST_OFFSET: Vec2 = Vec2::new(14.0, 10.0);

/// The ghost label's font size, in logical pixels.
const GHOST_FONT_SIZE: f32 = 13.0;

/// The ghost's text colour.
const GHOST_COLOR: Color = Color::srgb(0.92, 0.94, 0.98);

/// The ghost's backdrop, so it reads over any scene.
const GHOST_BACKGROUND: Color = Color::srgba(0.10, 0.12, 0.16, 0.85);

/// The drop-target folder row's highlight.
const DROP_HIGHLIGHT: Color = Color::srgba(0.24, 0.34, 0.52, 0.75);

/// How close to the viewport's top / bottom edge the pointer auto-scrolls, in
/// logical pixels.
const AUTO_SCROLL_EDGE: f32 = 28.0;

/// The auto-scroll speed at the edge, in logical pixels per second.
const AUTO_SCROLL_SPEED: f32 = 420.0;

/// How long the pointer must linger over a collapsed folder before it
/// auto-expands, in seconds (the reference's hover-expand delay).
const AUTO_EXPAND_SECS: f64 = 0.7;

/// An avatar-keyed UI drop target: dropping a dragged inventory row on a node
/// carrying this gives the item to that agent. The people list stamps its rows
/// with it; the floating name tags already carry the equivalent
/// [`AvatarPickTarget`], which the drop resolution also accepts.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AgentDropTarget(pub(crate) AgentKey);

/// The live drag, while one is in progress.
#[derive(Debug, Clone)]
struct ActiveDrag {
    /// The dragged rows, snapshotted at drag start (the whole selection when
    /// the drag began inside it), each with whether it sits in the read-only
    /// Library (a Library drop becomes a copy).
    sources: Vec<(MenuTarget, bool)>,
    /// The ghost entity following the pointer.
    ghost: Entity,
    /// The destination folder currently hovered, if any.
    hover_folder: Option<InventoryFolderKey>,
    /// When [`hover_folder`](Self::hover_folder) last changed, in
    /// [`Time::elapsed_secs_f64`] terms — drives the auto-expand delay.
    hover_since: f64,
}

/// The drag state resource: `None` when no drag is in progress.
#[derive(Resource, Debug, Default)]
pub(crate) struct InventoryDragState {
    /// The in-progress drag.
    active: Option<ActiveDrag>,
}

impl InventoryDragState {
    /// Whether a drag is in progress — while it is, the drop-target highlight
    /// owns the row backgrounds and the selection painter stands down.
    pub(crate) const fn is_active(&self) -> bool {
        self.active.is_some()
    }
}

// ---------------------------------------------------------------------------
// Pure drop arithmetic, tested in isolation.
// ---------------------------------------------------------------------------

/// What a drop onto a destination folder should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FolderDrop {
    /// Move the item / folder into the destination.
    Move,
    /// Copy the item into the destination (a Library source).
    Copy,
    /// Nothing legal to do.
    Reject,
}

/// Classify a drop of a dragged row onto a destination folder.
///
/// - into the **Library** — rejected (it is read-only);
/// - a **Library** source — a copy (an item copies singly, a folder deep-
///   copies recursively);
/// - into the **same folder** it is already in — rejected (a no-op move);
/// - a folder into its **own subtree** — rejected (the server would orphan it);
/// - otherwise — a move.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "the five independent yes/no facts of a drop are exactly what the classification \
              consumes; a builder or enum wrapper around a pure five-flag truth table would \
              obscure the table the tests pin"
)]
pub(crate) const fn classify_folder_drop(
    source_is_folder: bool,
    source_from_library: bool,
    dest_in_library: bool,
    same_folder: bool,
    dest_in_source_subtree: bool,
) -> FolderDrop {
    if dest_in_library || same_folder {
        return FolderDrop::Reject;
    }
    if source_from_library {
        return FolderDrop::Copy;
    }
    if source_is_folder && dest_in_source_subtree {
        return FolderDrop::Reject;
    }
    FolderDrop::Move
}

/// The row index under a pointer `local_y` logical pixels below the viewport's
/// top, given the list scroll and row height. `None` for a degenerate row
/// height.
pub(crate) fn row_index_at(local_y: f32, scroll: f32, row_height: f32) -> Option<usize> {
    if row_height <= 0.0 {
        return None;
    }
    let offset = local_y + scroll;
    if !offset.is_finite() || offset < 0.0 {
        return None;
    }
    let index = (offset / row_height).floor().min(4_294_967_040.0);
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "guarded finite, non-negative and bounded, so the truncation is exact"
    )]
    let index = index as usize;
    Some(index)
}

/// The give command for a dropped row offered to `to`: an item or a whole
/// folder, `None` for a Library source (not ours to give).
pub(crate) fn give_command(
    source: &MenuTarget,
    from_library: bool,
    to: AgentKey,
) -> Option<Command> {
    if from_library {
        return None;
    }
    match source {
        MenuTarget::Item(item) => Some(Command::GiveInventory {
            to_agent_id: to,
            item_id: item.item_id,
            asset_type: item.asset_type,
            item_name: item.name.clone(),
            transaction_id: TransactionId::from(Uuid::new_v4()),
        }),
        MenuTarget::Folder(folder) => Some(Command::GiveInventoryFolder {
            to_agent_id: to,
            folder_id: folder.folder_id,
            folder_name: folder.name.clone(),
            transaction_id: TransactionId::from(Uuid::new_v4()),
        }),
    }
}

/// The rez command for an object item dropped at a world point: the reference's
/// drag-rez. `ray_start` is the camera; `ray_end` the struck point (region
/// coordinates). A no-copy item is **moved** to the world (`remove_item`), a
/// copyable one leaves the inventory copy behind — the reference's rule.
pub(crate) fn rez_object_command(item: &ItemInfo, ray_start: Vector, ray_end: Vector) -> Command {
    let copyable = item.permissions.owner.contains(Permissions::COPY);
    let (sale_type, sale_price) = match item.sale.clone() {
        Some((sale_type, price)) => (sale_type, Some(price)),
        None => (SaleType::NotForSale, None),
    };
    Command::RezObjectFromInventory {
        params: Box::new(RezObjectParams {
            group_id: None,
            from_task_id: None,
            bypass_raycast: true,
            ray_start,
            ray_end,
            ray_target_id: None,
            ray_end_is_intersection: true,
            rez_selected: false,
            remove_item: !copyable,
            item_flags: item.flags,
            group_mask: item.permissions.group.bits(),
            everyone_mask: item.permissions.everyone.bits(),
            next_owner_mask: item.permissions.next_owner.bits(),
            item: RestoreItem {
                item_id: item.item_id,
                folder_id: item.folder_id,
                creator_id: item.creator_id,
                owner: item.owner,
                group: item.group,
                permissions: item.permissions,
                transaction_id: Uuid::new_v4(),
                asset_type: i8::try_from(item.asset_type.to_code()).unwrap_or(-1),
                inv_type: i8::try_from(item.inv_type.to_code()).unwrap_or(-1),
                flags: item.flags,
                sale_type,
                sale_price,
                name: item.name.clone(),
                description: item.description.clone(),
                creation_date: item.creation_date,
                crc: 0,
            },
        }),
    }
}

// ---------------------------------------------------------------------------
// Observers: drag start / end on the pooled rows.
// ---------------------------------------------------------------------------

/// A primary-button drag began on a row: snapshot it, spawn the ghost, and —
/// for a folder row — revert the expand-toggle its press already fired (a drag
/// is not a click, but the press arrived first).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the row pool, the view / \
              model to resolve the row, the UI root for the ghost, the drag state, and the \
              toggle-revert channel"
)]
pub(crate) fn on_row_drag_start(
    drag: On<Pointer<DragStart>>,
    rows: Query<&VirtualRow>,
    view: Res<InventoryView>,
    model: Res<InventoryModel>,
    selection: Res<InventorySelection>,
    root: Res<UiRoot>,
    time: Res<Time>,
    mut state: ResMut<InventoryDragState>,
    mut actions: MessageWriter<InventoryUiAction>,
    mut commands: Commands,
) {
    if drag.button != PointerButton::Primary || state.active.is_some() {
        return;
    }
    let Ok(row) = rows.get(drag.entity) else {
        return;
    };
    let Some(display) = row.index.and_then(|index| view.rows().get(index)) else {
        return;
    };
    // Dragging a selected row drags the whole selection, in view order.
    let keys = if selection.contains(display.key()) && selection.count() > 1 {
        selection.keys_in_view_order(view.rows())
    } else {
        vec![display.key()]
    };
    let mut sources: Vec<(MenuTarget, bool)> = Vec::new();
    for key in keys {
        match key {
            RowKey::Folder(folder_key) => {
                if let Some(info) = model.folder_info(folder_key).cloned() {
                    sources.push((MenuTarget::Folder(info), model.is_library(folder_key)));
                }
            }
            RowKey::Item(item_key) => {
                if let Some(info) = model.find_item(item_key).cloned() {
                    let library = model.is_library(info.folder_id);
                    sources.push((MenuTarget::Item(info), library));
                }
            }
        }
    }
    if sources.is_empty() {
        return;
    }
    // The press that began this gesture already toggled the dragged folder
    // row; a drag is not a click, so put the expand state back.
    if let RowKey::Folder(key) = display.key() {
        actions.write(InventoryUiAction::ToggleFolder(key));
    }
    // Component-wise: whole-`Vec2` `+` is a `glam` operator the workspace
    // `arithmetic_side_effects` lint trips on.
    let pointer = drag.pointer_location.position;
    let at = Vec2::new(pointer.x + GHOST_OFFSET.x, pointer.y + GHOST_OFFSET.y);
    // A multi-row drag's ghost shows the count instead of one row's label.
    let (ghost_icon, ghost_label) = if sources.len() > 1 {
        (String::new(), format!("{} items", sources.len()))
    } else {
        (display.icon().to_owned(), display.name().to_owned())
    };
    let ghost = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(at.x),
                top: Val::Px(at.y),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                column_gap: Val::Px(4.0),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(GHOST_BACKGROUND),
            GlobalZIndex(i32::MAX - 8),
            Pickable::IGNORE,
            Name::new("inventory-drag-ghost"),
            ChildOf(root.0),
        ))
        .with_child((
            Text::new(ghost_icon),
            UiFont::Sans.at(GHOST_FONT_SIZE),
            TextColor(GHOST_COLOR),
            Pickable::IGNORE,
        ))
        .with_child((
            Text::new(ghost_label),
            UiFont::Sans.at(GHOST_FONT_SIZE),
            TextColor(GHOST_COLOR),
            Pickable::IGNORE,
        ))
        .id();
    state.active = Some(ActiveDrag {
        sources,
        ghost,
        hover_folder: None,
        hover_since: time.elapsed_secs_f64(),
    });
}

/// The logical-pixel rectangle of the inventory viewport: `(top_left, size)`.
/// Component-wise f32 maths, per the workspace `arithmetic_side_effects`
/// convention on `glam` operators.
fn viewport_rect(computed: &ComputedNode, transform: &UiGlobalTransform) -> (Vec2, Vec2) {
    let scale = computed.inverse_scale_factor();
    let physical = computed.size();
    let size = Vec2::new(physical.x * scale, physical.y * scale);
    let centre = transform.translation;
    (
        Vec2::new(
            centre.x * scale - size.x / 2.0,
            centre.y * scale - size.y / 2.0,
        ),
        size,
    )
}

/// The destination folder a pointer over the list names: the folder row under
/// it, the containing folder of an item row, or the agent root over the empty
/// tail. `None` when the pointer is outside the viewport.
fn drop_folder_at(
    cursor: Vec2,
    rect: (Vec2, Vec2),
    list: &VirtualList,
    view: &InventoryView,
    model: &InventoryModel,
) -> Option<InventoryFolderKey> {
    let (top_left, size) = rect;
    if cursor.x < top_left.x
        || cursor.y < top_left.y
        || cursor.x > top_left.x + size.x
        || cursor.y > top_left.y + size.y
    {
        return None;
    }
    let index = row_index_at(cursor.y - top_left.y, list.scroll_offset(), list.row_height)?;
    match view
        .rows()
        .get(index)
        .map(crate::inventory::DisplayRow::key)
    {
        Some(RowKey::Folder(folder)) => Some(folder),
        Some(RowKey::Item(item)) => model.find_item(item).map(|info| info.folder_id),
        // The empty space below the last row: the agent root.
        None => model.agent_root(),
    }
}

/// Drive the in-progress drag each frame: move the ghost with the pointer,
/// highlight the hovered destination folder, auto-scroll at the list's edges,
/// auto-expand a collapsed folder lingered over, and cancel on `Escape`.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the drag state, the window \
              pointer, the clock, the list / viewport geometry, the view / model for the hit \
              test, the row pool for the highlight, the ghost node, and the expand channel"
)]
pub(crate) fn drive_inventory_drag(
    mut state: ResMut<InventoryDragState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    ui: Option<Res<InventoryUi>>,
    view: Res<InventoryView>,
    model: Res<InventoryModel>,
    viewports: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut lists: Query<&mut VirtualList>,
    mut rows: Query<(&VirtualRow, &ChildOf, &mut BackgroundColor)>,
    mut nodes: Query<&mut Node>,
    mut actions: MessageWriter<InventoryUiAction>,
    mut commands: Commands,
) {
    let Some(ui) = ui else {
        return;
    };
    let Some(active) = state.active.as_mut() else {
        return;
    };
    if keyboard.just_pressed(KeyCode::Escape) {
        commands.entity(active.ghost).despawn();
        state.active = None;
        clear_row_highlights(ui.viewport(), &mut rows);
        return;
    }
    let Some(cursor) = windows.iter().next().and_then(Window::cursor_position) else {
        return;
    };
    // The ghost follows the pointer.
    if let Ok(mut node) = nodes.get_mut(active.ghost) {
        node.left = Val::Px(cursor.x + GHOST_OFFSET.x);
        node.top = Val::Px(cursor.y + GHOST_OFFSET.y);
    }
    // The hovered destination, viewport geometry permitting.
    let rect = viewports
        .get(ui.viewport())
        .ok()
        .map(|(computed, transform)| viewport_rect(computed, transform));
    let hovered = rect.and_then(|rect| {
        lists
            .get(ui.viewport())
            .ok()
            .and_then(|list| drop_folder_at(cursor, rect, list, &view, &model))
    });
    if hovered != active.hover_folder {
        active.hover_folder = hovered;
        active.hover_since = time.elapsed_secs_f64();
    } else if let Some(folder) = hovered
        && !model.is_expanded(folder)
        && time.elapsed_secs_f64() - active.hover_since >= AUTO_EXPAND_SECS
    {
        actions.write(InventoryUiAction::ExpandFolder(folder));
        // Restart the timer so a still-loading folder is not re-expanded every
        // frame.
        active.hover_since = time.elapsed_secs_f64();
    }
    // Auto-scroll at the edges while the pointer is over the list.
    if let Some((top_left, size)) = rect
        && cursor.x >= top_left.x
        && cursor.x <= top_left.x + size.x
        && let Ok(mut list) = lists.get_mut(ui.viewport())
    {
        let step = AUTO_SCROLL_SPEED * time.delta_secs();
        if cursor.y >= top_left.y && cursor.y <= top_left.y + AUTO_SCROLL_EDGE {
            list.scroll_by(-step);
        } else if cursor.y >= top_left.y + size.y - AUTO_SCROLL_EDGE
            && cursor.y <= top_left.y + size.y
        {
            list.scroll_by(step);
        }
    }
    // Paint the destination folder's row (if visible) and clear the rest.
    let target_index = active.hover_folder.and_then(|folder| {
        view.rows()
            .iter()
            .position(|row| row.key() == RowKey::Folder(folder))
    });
    for (row, child_of, mut background) in &mut rows {
        if child_of.parent() != ui.viewport() {
            continue;
        }
        let wanted = if row.index.is_some() && row.index == target_index {
            DROP_HIGHLIGHT
        } else {
            Color::NONE
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

/// Reset every pooled row's drop highlight.
fn clear_row_highlights(
    viewport: Entity,
    rows: &mut Query<(&VirtualRow, &ChildOf, &mut BackgroundColor)>,
) {
    for (_row, child_of, mut background) in rows {
        if child_of.parent() == viewport && background.0 != Color::NONE {
            background.0 = Color::NONE;
        }
    }
}

/// The drag ended: resolve the drop in the reference's occlusion order (the
/// list, an avatar in the UI, blocking UI, the world) and issue the commands.
#[expect(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "a Bevy observer's parameters are its injected resources — here every drop-target \
              source a drag can land on (the list geometry, the hover map and occlusion \
              queries, the avatar targets, the world camera / picker / ray caster) — grouped \
              into tuples by role to fit the SystemParam arity"
)]
pub(crate) fn on_row_drag_end(
    drag: On<Pointer<DragEnd>>,
    mut state: ResMut<InventoryDragState>,
    ui: Option<Res<InventoryUi>>,
    session: (
        Res<InventoryView>,
        Res<InventoryModel>,
        Res<SlIdentity>,
        ResMut<WornAttachments>,
    ),
    geometry: (
        Query<(&ComputedNode, &UiGlobalTransform)>,
        Query<&VirtualList>,
        Query<(&VirtualRow, &ChildOf, &mut BackgroundColor)>,
    ),
    occlusion: (
        Res<HoverMap>,
        Query<&Pickable>,
        Query<&ComputedNode>,
        Query<&ChildOf>,
    ),
    targets: (Query<&AgentDropTarget>, Query<&AvatarPickTarget>),
    world: (
        Query<(&Camera, &GlobalTransform), With<ViewerCamera>>,
        AvatarPicker,
        MeshRayCast,
    ),
    outputs: (
        MessageWriter<InventoryUiAction>,
        MessageWriter<SlCommand>,
        Commands,
    ),
) {
    let (view, model, identity, mut worn) = session;
    let (viewports, lists, mut rows) = geometry;
    let (hover_map, pickables, node_sizes, child_of) = occlusion;
    let (agent_targets, pick_targets) = targets;
    let (camera, picker, mut ray_cast) = world;
    let (mut actions, mut commands, mut commands_bevy) = outputs;
    let Some(ui) = ui else {
        return;
    };
    let Some(active) = state.active.take() else {
        return;
    };
    commands_bevy.entity(active.ghost).despawn();
    clear_row_highlights(ui.viewport(), &mut rows);
    let cursor = drag.pointer_location.position;

    // 1. Over the inventory list (and not occluded by another floater — the
    //    hover map only carries the viewport subtree when it is on top).
    let over_list = hover_map
        .values()
        .flat_map(|hits| hits.keys())
        .any(|hovered| {
            let mut node = *hovered;
            loop {
                if node == ui.viewport() {
                    return true;
                }
                match child_of.get(node) {
                    Ok(parent) => node = parent.parent(),
                    Err(_root) => return false,
                }
            }
        });
    if over_list {
        let dest = viewports
            .get(ui.viewport())
            .ok()
            .map(|(computed, transform)| viewport_rect(computed, transform))
            .and_then(|rect| {
                lists
                    .get(ui.viewport())
                    .ok()
                    .and_then(|list| drop_folder_at(cursor, rect, list, &view, &model))
            });
        if let Some(dest) = dest {
            drop_into_folder(
                &active,
                dest,
                &model,
                identity.agent_id,
                &mut actions,
                &mut commands,
            );
        }
        return;
    }

    // 2. An avatar-keyed UI node under the pointer: a people row or a name tag.
    let ui_agent = hover_map
        .values()
        .flat_map(|hits| hits.keys())
        .find_map(|hovered| {
            agent_targets
                .get(*hovered)
                .map(|target| target.0)
                .ok()
                .or_else(|| pick_targets.get(*hovered).ok().map(AvatarPickTarget::agent))
        });
    if let Some(agent) = ui_agent {
        drop_onto_agent(&active, agent, &identity, &model, &mut worn, &mut commands);
        return;
    }

    // 3. Any other blocking UI swallows the drop.
    if pointer_over_blocking_ui(&hover_map, &pickables, &node_sizes) {
        return;
    }

    // 4. The world: an avatar body first, then a rez point for an object item.
    let Ok((camera, camera_transform)) = camera.single() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    if let Some(hit) = picker.pick(ray) {
        drop_onto_agent(
            &active,
            hit.agent,
            &identity,
            &model,
            &mut worn,
            &mut commands,
        );
        return;
    }
    let settings = MeshRayCastSettings::default();
    if let Some((_entity, hit)) = ray_cast.cast_ray(ray, &settings).first() {
        let start = bevy_to_sl_vec(camera_transform.translation());
        let end = bevy_to_sl_vec(hit.point);
        for (source, from_library) in &active.sources {
            if let MenuTarget::Item(item) = source
                && matches!(
                    item.inv_type,
                    InventoryType::Object | InventoryType::Attachment
                )
                && !from_library
            {
                commands.write(SlCommand(rez_object_command(
                    item,
                    start.clone(),
                    end.clone(),
                )));
                query_folder_page(item.folder_id, &mut commands);
            }
        }
    }
}

/// Issue the commands for a drop onto a destination folder in the list —
/// every dragged row is applied independently (a rejected row is skipped,
/// the rest land).
fn drop_into_folder(
    active: &ActiveDrag,
    dest: InventoryFolderKey,
    model: &InventoryModel,
    own_agent: Option<AgentKey>,
    actions: &mut MessageWriter<InventoryUiAction>,
    commands: &mut MessageWriter<SlCommand>,
) {
    let mut any_landed = false;
    for (source, from_library) in &active.sources {
        let (source_is_folder, same_folder, dest_in_subtree) = match source {
            MenuTarget::Folder(folder) => (
                true,
                folder.parent_id == Some(dest) || folder.folder_id == dest,
                model.is_within(dest, folder.folder_id),
            ),
            MenuTarget::Item(item) => (false, item.folder_id == dest, false),
        };
        let decision = classify_folder_drop(
            source_is_folder,
            *from_library,
            model.is_library(dest),
            same_folder,
            dest_in_subtree,
        );
        match (decision, source) {
            (FolderDrop::Move, MenuTarget::Item(item)) => {
                commands.write(SlCommand(Command::MoveInventoryItem {
                    item_id: item.item_id,
                    folder_id: dest,
                    new_name: String::new(),
                }));
                query_folder_page(item.folder_id, commands);
                query_folder_page(dest, commands);
            }
            (FolderDrop::Move, MenuTarget::Folder(folder)) => {
                commands.write(SlCommand(Command::MoveInventoryFolder {
                    folder_id: folder.folder_id,
                    parent_id: dest,
                }));
                commands.write(SlCommand(Command::QueryInventoryFolders));
            }
            (FolderDrop::Copy, MenuTarget::Item(item)) => {
                let owner = match item.owner {
                    sl_client_bevy::OwnerKey::Agent(agent) => agent,
                    _other => own_agent.unwrap_or_else(|| AgentKey::from(Uuid::nil())),
                };
                commands.write(SlCommand(Command::CopyInventoryItem {
                    old_agent_id: owner,
                    old_item_id: item.item_id,
                    new_folder_id: dest,
                    new_name: String::new(),
                }));
            }
            (FolderDrop::Copy, MenuTarget::Folder(folder)) => {
                // A dragged Library folder deep-copies into the drop target.
                for command in crate::inventory_actions::deep_copy_commands(
                    model,
                    folder.folder_id,
                    &folder.name,
                    dest,
                    own_agent,
                ) {
                    commands.write(SlCommand(command));
                }
                commands.write(SlCommand(Command::QueryInventoryFolders));
                query_folder_page(dest, commands);
            }
            _rejected => continue,
        }
        any_landed = true;
    }
    // Show where it landed.
    if any_landed {
        actions.write(InventoryUiAction::ExpandFolder(dest));
    }
}

/// Issue the commands for a drop onto an avatar: wear it on **yourself**
/// (object → attach, wearable → wear), give it to anyone else.
fn drop_onto_agent(
    active: &ActiveDrag,
    agent: AgentKey,
    identity: &SlIdentity,
    model: &InventoryModel,
    worn: &mut WornAttachments,
    commands: &mut MessageWriter<SlCommand>,
) {
    for (source, from_library) in &active.sources {
        if identity.agent_id == Some(agent) {
            if let MenuTarget::Item(item) = source
                && !from_library
            {
                for command in wear_commands(item, identity.agent_id, model.worn_wearables(), false)
                {
                    commands.write(SlCommand(command));
                }
                if matches!(
                    item.inv_type,
                    InventoryType::Object | InventoryType::Attachment
                ) {
                    worn.items.insert(item.item_id);
                }
                // Keep the COF authoritative for a drag-wear too.
                let replaced =
                    crate::inventory_actions::replaced_by_wear(model.worn_wearables(), item);
                let batch = crate::inventory_actions::cof_wear_link_commands(
                    model.cof_key(),
                    &crate::inventory_actions::cof_links_with_slots(model),
                    item,
                    &replaced,
                );
                if !batch.is_empty() {
                    for command in batch {
                        commands.write(SlCommand(command));
                    }
                    if let Some(cof) = model.cof_key() {
                        query_folder_page(cof, commands);
                    }
                }
            }
            continue;
        }
        if let Some(give) = give_command(source, *from_library, agent) {
            commands.write(SlCommand(give));
        }
    }
}

/// The plugin wiring inventory drag-and-drop into the viewer. The row
/// observers themselves are installed by [`crate::inventory`]'s row pool.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct InventoryDragPlugin;

impl Plugin for InventoryDragPlugin {
    /// Register the drag state and the per-frame drive system.
    fn build(&self, app: &mut App) {
        app.init_resource::<InventoryDragState>()
            .add_systems(Update, drive_inventory_drag);
    }
}

#[cfg(test)]
mod tests {
    use super::{FolderDrop, classify_folder_drop, give_command, rez_object_command, row_index_at};
    use crate::inventory_actions::MenuTarget;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, AssetType, Command, FolderInfo, FolderState, FolderType, InventoryFolderKey,
        InventoryKey, InventoryType, ItemInfo, Permissions, Permissions5, Uuid, Vector,
    };

    /// A minimal item with the given owner permission mask.
    fn item(owner_mask: u32) -> ItemInfo {
        ItemInfo {
            item_id: InventoryKey::from(Uuid::from_u128(0x10)),
            folder_id: InventoryFolderKey::from(Uuid::from_u128(0xF0)),
            name: "Box".to_owned(),
            description: String::new(),
            asset_id: Uuid::from_u128(0xA0),
            asset_type: AssetType::Object,
            inv_type: InventoryType::Object,
            flags: 0,
            sale: None,
            creation_date: 0,
            owner: sl_client_bevy::OwnerKey::Agent(AgentKey::from(Uuid::from_u128(1))),
            last_owner_id: Uuid::nil(),
            creator_id: AgentKey::from(Uuid::from_u128(1)),
            group: None,
            permissions: Permissions5 {
                base: Permissions::from_bits(owner_mask),
                owner: Permissions::from_bits(owner_mask),
                group: Permissions::empty(),
                everyone: Permissions::empty(),
                next_owner: Permissions::empty(),
            },
        }
    }

    /// Own rows move; Library items copy; illegal destinations reject.
    #[test]
    fn drop_classification_covers_the_rules() {
        // An own item into another folder: move.
        assert_eq!(
            classify_folder_drop(false, false, false, false, false),
            FolderDrop::Move
        );
        // Into the Library: rejected.
        assert_eq!(
            classify_folder_drop(false, false, true, false, false),
            FolderDrop::Reject
        );
        // Into the folder it is already in: rejected (no-op).
        assert_eq!(
            classify_folder_drop(false, false, false, true, false),
            FolderDrop::Reject
        );
        // A Library item into the own tree: copy.
        assert_eq!(
            classify_folder_drop(false, true, false, false, false),
            FolderDrop::Copy
        );
        // A Library folder: a recursive deep copy.
        assert_eq!(
            classify_folder_drop(true, true, false, false, false),
            FolderDrop::Copy
        );
        // A folder into its own subtree: rejected.
        assert_eq!(
            classify_folder_drop(true, false, false, false, true),
            FolderDrop::Reject
        );
        // A folder elsewhere: move.
        assert_eq!(
            classify_folder_drop(true, false, false, false, false),
            FolderDrop::Move
        );
    }

    /// The pointer→row arithmetic honours the scroll offset and row height.
    #[test]
    fn row_hit_test_maps_scroll_and_height() {
        // 22 px rows, no scroll: y=0 is row 0, y=43.9 is row 1.
        assert_eq!(row_index_at(0.0, 0.0, 22.0), Some(0));
        assert_eq!(row_index_at(43.9, 0.0, 22.0), Some(1));
        // Scrolled 44 px: the top row on screen is row 2.
        assert_eq!(row_index_at(0.0, 44.0, 22.0), Some(2));
        // Degenerate height: no hit.
        assert_eq!(row_index_at(10.0, 0.0, 0.0), None);
    }

    /// A give carries the item's identity; a Library source gives nothing.
    #[test]
    fn give_wraps_the_item_and_refuses_library_sources() {
        let to = AgentKey::from(Uuid::from_u128(0x99));
        let source = MenuTarget::Item(item(0));
        assert!(matches!(
            give_command(&source, false, to),
            Some(Command::GiveInventory {
                to_agent_id,
                item_id,
                asset_type: AssetType::Object,
                ..
            }) if to_agent_id == to && item_id == InventoryKey::from(Uuid::from_u128(0x10))
        ));
        assert!(give_command(&source, true, to).is_none());

        let folder = MenuTarget::Folder(FolderInfo {
            folder_id: InventoryFolderKey::from(Uuid::from_u128(0x50)),
            parent_id: None,
            name: "Stuff".to_owned(),
            folder_type: FolderType::None,
            version: 1,
            state: FolderState::Unknown,
        });
        assert!(matches!(
            give_command(&folder, false, to),
            Some(Command::GiveInventoryFolder { folder_id, .. })
                if folder_id == InventoryFolderKey::from(Uuid::from_u128(0x50))
        ));
    }

    /// A vector literal for the ray fixtures.
    const fn vec3(x: f32, y: f32, z: f32) -> Vector {
        Vector { x, y, z }
    }

    /// The drag-rez keeps a copyable item and moves a no-copy one, and carries
    /// the ray placement.
    #[test]
    fn rez_respects_the_copy_permission() {
        let rezzed = rez_object_command(
            &item(Permissions::COPY.bits()),
            vec3(1.0, 2.0, 3.0),
            vec3(4.0, 5.0, 6.0),
        );
        assert!(matches!(
            &rezzed,
            Command::RezObjectFromInventory { params }
                if !params.remove_item
                    && params.bypass_raycast
                    && params.ray_end_is_intersection
                    && params.ray_end == vec3(4.0, 5.0, 6.0)
        ));
        let moved = rez_object_command(&item(0), vec3(1.0, 2.0, 3.0), vec3(4.0, 5.0, 6.0));
        assert!(matches!(
            &moved,
            Command::RezObjectFromInventory { params } if params.remove_item
        ));
    }
}
