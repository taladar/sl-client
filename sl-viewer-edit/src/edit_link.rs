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
//! primary first, then back through the earlier picks. `link_order` does
//! exactly that — it must never re-sort the set (e.g. into id order), or the
//! wrong prim becomes root.
//!
//! # Unlink names every prim
//!
//! The reference sends an `ObjectDelink` with **every** prim of the selected
//! linksets (`SEND_INDIVIDUALS`), not just the roots — a root-only delink would
//! leave the simulator re-linking the orphaned children into a fresh set rather
//! than breaking the linkset fully apart. [`ObjectState::linkset_members`]
//! gathers them.
//!
//! # Unlink keeps every prim selected
//!
//! In the reference, selecting a linkset selects every prim of it, so after a
//! delink every former member is still selected, now as a linkset of its own —
//! a wrongly ordered link is immediately re-linkable the other way around. Our
//! whole-linkset selection holds only roots (and folds a selected child into its
//! root whenever the tool state changes), so the delinked children would drop
//! out. [`PendingDelink`] remembers them: as each lands from the grid as a root
//! of its own, `reselect_delinked` adds it to the selection, provided the
//! linkset it left is still selected.
//!
//! # Enablement
//!
//! [`can_link`] / [`can_unlink`] mirror the reference's `enableLinkObjects` /
//! `enableUnlinkObjects`: link needs whole-linkset (not edit-linked-parts) mode,
//! at least two selected roots, and at least one modifiable object; unlink needs
//! at least one modifiable object (attachments are already kept out of the set
//! by the selection core). The Build-menu entries grey out when these fail
//! (`crate::menu_bar`), and the shortcut path re-checks before sending. The
//! per-linkset prim **limit** is not part of the enable gate (the reference
//! checks it only at link time, in `linkObjects`); `link_selection` enforces
//! it before sending.
//!
//! Reference (Firestorm, read-only): `llselectmgr` `linkObjects` / `sendLink`,
//! `unlinkObjects` / `sendDelink`, `enableLinkObjects`, `enableUnlinkObjects`.

use bevy::prelude::*;
use sl_client_bevy::{Command, Permissions, ScopedObjectId, SlCommand};

use crate::menu::TOP_MENU_ELEMENT;
use crate::ui_element::UiAction;
use crate::world_api::EditToolState;
use crate::world_api::ObjectState;
use crate::world_api::{SelectedNode, SelectionSet};

/// The Build-menu action string the Link entry emits.
pub const LINK_ACTION: &str = "link-objects";

/// The Build-menu action string the Unlink entry emits.
pub const UNLINK_ACTION: &str = "unlink-objects";

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

/// Whether `node` is a linkset root — the only kind of node a whole-linkset
/// link counts and names, the reference's `root_iterator`. A selected prim
/// becomes a child when a link it was in lands from the grid, and stays in the
/// selection as one (unlink leaves the selection in place); one the viewer
/// does not track counts as a root, optimistically, like a missing
/// permissions reply.
fn is_linkset_root(node: &SelectedNode, objects: &ObjectState) -> bool {
    objects
        .linkset_root_of(&node.scoped())
        .is_none_or(|root| root == node.scoped())
}

/// The link order for the current selection: its linkset roots **reversed**,
/// so the primary (last-selected) object leads and becomes the linkset root —
/// the reference's `SEND_ONLY_ROOTS`. See the [module documentation](self) —
/// this must preserve the set's insertion order, never re-sort it.
#[must_use]
pub fn link_order(selection: &SelectionSet, objects: &ObjectState) -> Vec<ScopedObjectId> {
    let mut local_ids: Vec<ScopedObjectId> = selection
        .iter()
        .filter(|node| is_linkset_root(node, objects))
        .map(SelectedNode::scoped)
        .collect();
    // Insertion order keeps the primary (last-selected) last; reverse so it
    // leads and becomes the linkset root.
    local_ids.reverse();
    local_ids
}

/// Whether the current selection can be **linked** — the reference's
/// `enableLinkObjects`: whole-linkset (not edit-linked-parts) mode, at least two
/// selected **roots** (`getRootObjectCount`, so the two halves of a link that
/// just landed are one linkset, not two), and at least one modifiable root.
#[must_use]
pub fn can_link(selection: &SelectionSet, tool: &EditToolState, objects: &ObjectState) -> bool {
    let roots: Vec<&SelectedNode> = selection
        .iter()
        .filter(|node| is_linkset_root(node, objects))
        .collect();
    !tool.edit_linked && roots.len() >= 2 && roots.into_iter().any(node_modifiable)
}

/// Whether the current selection can be **unlinked** — the reference's
/// `enableUnlinkObjects`: at least one modifiable selected object. Attachments
/// (which the reference also excludes) never enter the selection set, so no
/// extra guard is needed here.
pub fn can_unlink(selection: &SelectionSet) -> bool {
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
    if !can_link(selection, tool, objects) {
        return false;
    }
    let local_ids = link_order(selection, objects);
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

/// The prims to name in the `ObjectDelink` for the current selection.
///
/// Mode-aware, matching the reference's `SEND_INDIVIDUALS` (which sends exactly
/// the selected nodes):
///
/// - **Whole-linkset mode** (the default): the selection tracks only linkset
///   roots, so each is expanded to its full membership — the whole set breaks
///   apart, as selecting a whole linkset in the reference selects all its prims.
/// - **Edit-linked-parts mode**: the selection is already individual prims, so
///   they are sent verbatim — unlinking a **subset** pulls exactly those prims
///   out (a lone root pops off and the sim re-links the remainder; a lone child
///   detaches), rather than shattering the whole linkset.
fn delink_ids(
    selection: &SelectionSet,
    tool: &EditToolState,
    objects: &ObjectState,
) -> Vec<ScopedObjectId> {
    let mut local_ids: Vec<ScopedObjectId> = Vec::new();
    for node in selection.iter() {
        if tool.edit_linked {
            if !local_ids.contains(&node.scoped()) {
                local_ids.push(node.scoped());
            }
        } else {
            for member in objects.linkset_members(&node.scoped()) {
                if !local_ids.contains(&member) {
                    local_ids.push(member);
                }
            }
        }
    }
    local_ids
}

/// The prims a whole-linkset delink is taking out of a selected linkset, each
/// with the root it had: added to the selection as they land from the grid as
/// roots of their own ([module documentation](self)).
#[derive(Resource, Debug, Default)]
pub struct PendingDelink {
    /// `(prim, the root it was linked under)`, in link order.
    members: Vec<(ScopedObjectId, ScopedObjectId)>,
}

/// Send the `ObjectDelink` for the current selection, if it can be unlinked,
/// and remember the children leaving a selected linkset in `pending`. Leaves
/// the selection in place. Returns whether a delink was sent.
fn unlink_selection(
    selection: &SelectionSet,
    tool: &EditToolState,
    objects: &ObjectState,
    pending: &mut PendingDelink,
    commands: &mut MessageWriter<SlCommand>,
) -> bool {
    if !can_unlink(selection) {
        return false;
    }
    let local_ids = delink_ids(selection, tool, objects);
    if local_ids.is_empty() {
        return false;
    }
    debug!("build-tools: unlink {} prims", local_ids.len());
    // In edit-linked-parts mode the selection already names every prim it
    // unlinks, so there is nothing to add back.
    pending.members = if tool.edit_linked {
        Vec::new()
    } else {
        local_ids
            .iter()
            .filter_map(|member| {
                let root = objects.linkset_root_of(member)?;
                (root != *member && !selection.is_selected(*member)).then_some((*member, root))
            })
            .collect()
    };
    commands.write(SlCommand(Command::DelinkObjects { local_ids }));
    true
}

/// Add each prim a delink took out of a selected linkset to the selection once
/// the grid says it is a root of its own, ahead of the nodes already there so
/// the primary stays primary. A prim whose former root is no longer selected
/// (the user moved on), or that is gone, is dropped.
fn reselect_delinked(
    objects: Res<ObjectState>,
    mut pending: ResMut<PendingDelink>,
    mut selection: ResMut<SelectionSet>,
) {
    if pending.members.is_empty() || !objects.is_changed() {
        return;
    }
    let mut landed: Vec<SelectedNode> = Vec::new();
    let members = core::mem::take(&mut pending.members);
    for (member, root) in members {
        if !selection.is_selected(root) {
            continue;
        }
        match objects.linkset_root_of(&member) {
            Some(now) if now == member => {
                if let (Some(full), Some(entity)) =
                    (objects.full_key(&member), objects.entity_by_scoped(&member))
                    && !selection.is_selected(member)
                {
                    landed.push(SelectedNode {
                        scoped: member,
                        full,
                        entity,
                        properties: None,
                        faces: None,
                        last_face: crate::world_api::FIRST_FACE,
                    });
                }
            }
            // Still linked: the delink has not landed yet.
            Some(_linked) => pending.members.push((member, root)),
            None => {}
        }
    }
    if !landed.is_empty() {
        landed.extend(selection.nodes().iter().cloned());
        selection.replace_nodes(landed);
    }
}

/// The plugin wiring linking / unlinking into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub struct EditLinkPlugin;

impl Plugin for EditLinkPlugin {
    /// Register the link / unlink driver.
    fn build(&self, app: &mut App) {
        // Link / unlink only fires while the build tool is active (it already
        // bailed otherwise), so gate it out of the scheduler outside build mode.
        app.init_resource::<PendingDelink>().add_systems(
            Update,
            (drive_link_unlink, reselect_delinked)
                .run_if(crate::edit_tool::edit_tool_active_or_settling),
        );
    }
}

/// Drive Link / Unlink from the Build-menu entries — picked, or reached by the
/// `Ctrl+L` / `Ctrl+Shift+L` accelerators drawn against them.
///
/// There is no keyboard system here: the chords are the entries' accelerators,
/// dispatched to them by `sl_viewer_ui_widgets::menu_accel`, which honours the
/// same `CAN_LINK` / `CAN_UNLINK` enable gates that grey the lines and stands
/// down while a text field holds focus (so `Ctrl+L` typed into a field never
/// links). The exact-modifier match is what keeps `Ctrl+Shift+L` off the Link
/// entry and `Ctrl+L` off Unlink's.
fn drive_link_unlink(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    mut pending: ResMut<PendingDelink>,
    mut actions: MessageReader<UiAction>,
    mut commands: MessageWriter<SlCommand>,
) {
    let mut do_link = false;
    let mut do_unlink = false;

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
        unlink_selection(&selection, &tool, &objects, &mut pending, &mut commands);
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_LINKSET_PRIMS, can_link, can_unlink, delink_ids, link_order};
    use crate::world_api::EditToolState;
    use crate::world_api::ObjectState;
    use crate::world_api::SelectionSet;
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
        assert_eq!(
            link_order(&set, &ObjectState::default()),
            vec![scoped(12), scoped(11), scoped(10)]
        );
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
        assert_eq!(
            link_order(&set, &ObjectState::default()).first(),
            Some(&scoped(1))
        );
    }

    /// Link needs at least two roots and whole-linkset mode; unlink needs a
    /// non-empty selection. Properties-less nodes count as modifiable
    /// (optimistic).
    #[test]
    fn enable_gates_follow_the_reference() {
        let tool = EditToolState::default();
        let objects = ObjectState::default();
        let mut set = SelectionSet::default();
        assert!(!can_link(&set, &tool, &objects), "no selection → no link");
        assert!(!can_unlink(&set), "no selection → no unlink");

        set.insert(scoped(1), full(1), Entity::PLACEHOLDER);
        assert!(
            !can_link(&set, &tool, &objects),
            "one root is not enough to link"
        );
        assert!(can_unlink(&set), "a lone selection can still be unlinked");

        set.insert(scoped(2), full(2), Entity::PLACEHOLDER);
        assert!(can_link(&set, &tool, &objects), "two roots → link");

        // Edit-linked-parts (component) mode disables link.
        let edit_linked = EditToolState {
            edit_linked: true,
            ..EditToolState::default()
        };
        assert!(
            !can_link(&set, &edit_linked, &objects),
            "component mode → no link"
        );
    }

    /// The prim-limit constant is the reference's root + 255 children.
    #[test]
    fn linkset_limit_is_reference_faithful() {
        assert_eq!(MAX_LINKSET_PRIMS, 256);
    }

    /// In **edit-linked-parts** mode the delink names exactly the selected
    /// prims (the subset case), verbatim and de-duplicated — it does not expand
    /// to the whole linkset, so unlinking a subset pulls out only those prims.
    #[test]
    fn edit_linked_delink_sends_the_selected_subset() {
        let objects = ObjectState::default();
        let tool = EditToolState {
            edit_linked: true,
            ..EditToolState::default()
        };
        let mut set = SelectionSet::default();
        set.insert(scoped(5), full(5), Entity::PLACEHOLDER);
        set.insert(scoped(7), full(7), Entity::PLACEHOLDER);
        // Exactly the two selected prims, in selection order (no whole-linkset
        // expansion — the objects table is untouched in this mode).
        assert_eq!(
            delink_ids(&set, &tool, &objects),
            vec![scoped(5), scoped(7)]
        );
    }
}
