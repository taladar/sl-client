//! Prim **linking & unlinking** (`viewer-prim-linking`): the wire half of the
//! build tool's Link / Unlink commands, driven from `Ctrl+L` / `Ctrl+Shift+L`
//! and the Build menu.
//!
//! # Selection order is the link order
//!
//! The reference viewer packs a link's selected roots **most-recently-selected
//! first** — `LLObjectSelection::addNode` prepends on select, and
//! `LLSelectMgr::sendListToRegions` iterates the list front-to-back — and both
//! the Second Life simulator and OpenSim
//! ([`SceneGraph::DelinkObjects`](https://opensimulator.org)/`HandleObjectLink`
//! → `parentprimid = ObjectData[0]`) make the **first** `ObjectLink` block the
//! new linkset **root**. So the last-selected object becomes the root, which is
//! the muscle-memory builders rely on ("select the parts, select the intended
//! root last, link"), and the link numbers scripts read (`llGetLinkNumber`) are
//! assigned from that same order.
//!
//! Our [`SelectionSet`] keeps the primary (last-selected) node **last** in
//! insertion order, so the link order is simply the selection **reversed**:
//! primary first, then back through the earlier picks. [`link_order`] does
//! exactly that — it must never re-sort the set (e.g. into id order), or the
//! wrong prim becomes root.
//!
//! # Unlink names every prim
//!
//! The reference sends an `ObjectDelink` with **every** prim of the selected
//! linksets (`SEND_INDIVIDUALS`), not just the roots — a root-only delink would
//! leave the simulator re-linking the orphaned children into a fresh set rather
//! than breaking the linkset fully apart. [`ObjectState::linkset_members`]
//! gathers them. Unlink leaves the selection in place, so a wrongly ordered
//! link is immediately re-linkable the other way around.
//!
//! # Enablement
//!
//! [`can_link`] / [`can_unlink`] mirror the reference's `enableLinkObjects` /
//! `enableUnlinkObjects`: link needs whole-linkset (not edit-linked-parts) mode,
//! at least two selected roots, and at least one modifiable object; unlink needs
//! at least one modifiable object (attachments are already kept out of the set
//! by the selection core). The Build-menu entries grey out when these fail
//! ([`crate::menu_bar`]), and the shortcut path re-checks before sending. The
//! per-linkset prim **limit** is not part of the enable gate (the reference
//! checks it only at link time, in `linkObjects`); [`link_selection`] enforces
//! it before sending.
//!
//! Reference (Firestorm, read-only): `llselectmgr` `linkObjects` / `sendLink`,
//! `unlinkObjects` / `sendDelink`, `enableLinkObjects`, `enableUnlinkObjects`.

use bevy::prelude::*;
use sl_client_bevy::{Command, Permissions, ScopedObjectId, SlCommand};

use crate::edit_selection::{SelectedNode, SelectionSet};
use crate::edit_tool::EditToolState;
use crate::input_context::InputContext;
use crate::menu_bar::TOP_MENU_ELEMENT;
use crate::objects::ObjectState;
use crate::ui_element::UiAction;

/// The Build-menu action string the Link entry emits.
pub(crate) const LINK_ACTION: &str = "link-objects";

/// The Build-menu action string the Unlink entry emits.
pub(crate) const UNLINK_ACTION: &str = "unlink-objects";

/// The most prims one linkset may hold — a root plus the reference's
/// `MAX_CHILDREN_PER_TASK` (255) children. A link whose combined prim count
/// would exceed this is refused, matching `LLSelectMgr::linkObjects`'
/// `object_count > object_max + 1` guard.
pub(crate) const MAX_LINKSET_PRIMS: usize = 256;

/// Whether the agent may modify this selected object — the reference's
/// `permModify`. An object whose `ObjectProperties` reply has not yet arrived
/// counts as modifiable (optimistic): the reply lands within a frame or two of
/// selection, and the simulator is the final arbiter of a link either way.
fn node_modifiable(node: &SelectedNode) -> bool {
    node.properties()
        .is_none_or(|properties| properties.permissions.owner.contains(Permissions::MODIFY))
}

/// The link order for the current selection: the selection **reversed**, so the
/// primary (last-selected) object leads and becomes the linkset root. See the
/// [module documentation](self) — this must preserve the set's insertion order,
/// never re-sort it.
pub(crate) fn link_order(selection: &SelectionSet) -> Vec<ScopedObjectId> {
    let mut local_ids: Vec<ScopedObjectId> = selection.iter().map(SelectedNode::scoped).collect();
    // Insertion order keeps the primary (last-selected) last; reverse so it
    // leads and becomes the linkset root.
    local_ids.reverse();
    local_ids
}

/// Whether the current selection can be **linked** — the reference's
/// `enableLinkObjects`: whole-linkset (not edit-linked-parts) mode, at least two
/// selected roots, and at least one modifiable object.
pub(crate) fn can_link(selection: &SelectionSet, tool: &EditToolState) -> bool {
    !tool.edit_linked && selection.len() >= 2 && selection.iter().any(node_modifiable)
}

/// Whether the current selection can be **unlinked** — the reference's
/// `enableUnlinkObjects`: at least one modifiable selected object. Attachments
/// (which the reference also excludes) never enter the selection set, so no
/// extra guard is needed here.
pub(crate) fn can_unlink(selection: &SelectionSet) -> bool {
    !selection.is_empty() && selection.iter().any(node_modifiable)
}

/// Send the `ObjectLink` for the current selection, if it can be linked and is
/// within the linkset prim limit. Returns whether a link was sent.
fn link_selection(
    selection: &SelectionSet,
    tool: &EditToolState,
    objects: &ObjectState,
    commands: &mut MessageWriter<SlCommand>,
) -> bool {
    if !can_link(selection, tool) {
        return false;
    }
    let local_ids = link_order(selection);
    // The reference refuses a link whose combined prim count would overflow one
    // linkset (`linkObjects`' `UnableToLinkObjects`). Each selected root brings
    // its whole family.
    let total: usize = local_ids
        .iter()
        .map(|scoped| objects.linkset_prim_count(scoped).max(1))
        .sum();
    if total > MAX_LINKSET_PRIMS {
        info!("build-tools: refusing link of {total} prims (limit {MAX_LINKSET_PRIMS})");
        return false;
    }
    debug!(
        "build-tools: link {} objects, root {:?}",
        local_ids.len(),
        local_ids.first()
    );
    commands.write(SlCommand(Command::LinkObjects { local_ids }));
    true
}

/// Send the `ObjectDelink` for the current selection, if it can be unlinked.
/// Names every prim of each selected linkset so the sets break fully apart, and
/// leaves the selection in place. Returns whether a delink was sent.
fn unlink_selection(
    selection: &SelectionSet,
    objects: &ObjectState,
    commands: &mut MessageWriter<SlCommand>,
) -> bool {
    if !can_unlink(selection) {
        return false;
    }
    let mut local_ids: Vec<ScopedObjectId> = Vec::new();
    for node in selection.iter() {
        for member in objects.linkset_members(&node.scoped()) {
            if !local_ids.contains(&member) {
                local_ids.push(member);
            }
        }
    }
    if local_ids.is_empty() {
        return false;
    }
    debug!("build-tools: unlink {} prims", local_ids.len());
    commands.write(SlCommand(Command::DelinkObjects { local_ids }));
    true
}

/// The plugin wiring linking / unlinking into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EditLinkPlugin;

impl Plugin for EditLinkPlugin {
    /// Register the link / unlink driver.
    fn build(&self, app: &mut App) {
        app.add_systems(Update, drive_link_unlink);
    }
}

/// Drive Link / Unlink from either the keyboard chords (`Ctrl+L` /
/// `Ctrl+Shift+L`, while the build tool is active and the world owns the
/// keyboard) or the Build-menu entries.
fn drive_link_unlink(
    keyboard: Res<ButtonInput<KeyCode>>,
    context: Res<InputContext>,
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    mut actions: MessageReader<UiAction>,
    mut commands: MessageWriter<SlCommand>,
) {
    let mut do_link = false;
    let mut do_unlink = false;

    // The keyboard chords: only while editing and only when the world (not a
    // text field) owns the keyboard, so `Ctrl+L` typed into a field never
    // links. `Ctrl+Shift+L` unlinks; `Ctrl+L` links.
    if tool.active
        && *context != InputContext::TextEntry
        && (keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight))
        && keyboard.just_pressed(KeyCode::KeyL)
    {
        if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight) {
            do_unlink = true;
        } else {
            do_link = true;
        }
    }

    // The Build-menu picks (the entries are greyed out when the operation is
    // unavailable, but re-check below regardless).
    for action in actions.read() {
        if action.element != TOP_MENU_ELEMENT {
            continue;
        }
        match action.action {
            LINK_ACTION => do_link = true,
            UNLINK_ACTION => do_unlink = true,
            _other => {}
        }
    }

    if do_link {
        link_selection(&selection, &tool, &objects, &mut commands);
    }
    if do_unlink {
        unlink_selection(&selection, &objects, &mut commands);
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_LINKSET_PRIMS, can_link, can_unlink, link_order};
    use crate::edit_selection::SelectionSet;
    use crate::edit_tool::EditToolState;
    use bevy::prelude::Entity;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{CircuitId, ObjectKey, RegionLocalObjectId, ScopedObjectId, Uuid};

    /// A scoped id for tests.
    fn scoped(id: u32) -> ScopedObjectId {
        ScopedObjectId {
            circuit: CircuitId::new(1),
            id: RegionLocalObjectId(id),
        }
    }

    /// A full key for tests.
    fn full(id: u128) -> ObjectKey {
        ObjectKey::from(Uuid::from_u128(id))
    }

    /// The link order is the selection reversed: the primary (last-selected)
    /// leads and becomes the linkset root, the earlier picks follow in reverse
    /// pick order — the reference's most-recently-selected-first packing.
    #[test]
    fn link_order_puts_primary_first() {
        let mut set = SelectionSet::default();
        set.insert(scoped(10), full(10), Entity::PLACEHOLDER);
        set.insert(scoped(11), full(11), Entity::PLACEHOLDER);
        set.insert(scoped(12), full(12), Entity::PLACEHOLDER);
        // Selected 10, 11, 12 (12 last / primary); root must be 12.
        assert_eq!(link_order(&set), vec![scoped(12), scoped(11), scoped(10)]);
    }

    /// Re-selecting an object promotes it to primary, so it leads the next
    /// link — the "select the intended root last" workflow.
    #[test]
    fn reselecting_a_root_makes_it_lead() {
        let mut set = SelectionSet::default();
        set.insert(scoped(1), full(1), Entity::PLACEHOLDER);
        set.insert(scoped(2), full(2), Entity::PLACEHOLDER);
        set.insert(scoped(3), full(3), Entity::PLACEHOLDER);
        // Click 1 again to make it the intended root.
        set.insert(scoped(1), full(1), Entity::PLACEHOLDER);
        assert_eq!(link_order(&set).first(), Some(&scoped(1)));
    }

    /// Link needs at least two roots and whole-linkset mode; unlink needs a
    /// non-empty selection. Properties-less nodes count as modifiable
    /// (optimistic).
    #[test]
    fn enable_gates_follow_the_reference() {
        let tool = EditToolState::default();
        let mut set = SelectionSet::default();
        assert!(!can_link(&set, &tool), "no selection → no link");
        assert!(!can_unlink(&set), "no selection → no unlink");

        set.insert(scoped(1), full(1), Entity::PLACEHOLDER);
        assert!(!can_link(&set, &tool), "one root is not enough to link");
        assert!(can_unlink(&set), "a lone selection can still be unlinked");

        set.insert(scoped(2), full(2), Entity::PLACEHOLDER);
        assert!(can_link(&set, &tool), "two roots → link");

        // Edit-linked-parts (component) mode disables link.
        let edit_linked = EditToolState {
            edit_linked: true,
            ..EditToolState::default()
        };
        assert!(!can_link(&set, &edit_linked), "component mode → no link");
    }

    /// The prim-limit constant is the reference's root + 255 children.
    #[test]
    fn linkset_limit_is_reference_faithful() {
        assert_eq!(MAX_LINKSET_PRIMS, 256);
    }
}
