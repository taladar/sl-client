//! Object-edit **undo / redo** (`viewer-build-undo-redo`): the Build menu's
//! Undo (`Ctrl+Z`) / Redo (`Ctrl+Y`), driven from the keyboard and the menu.
//!
//! # There is no client-side undo ledger
//!
//! Second Life's object undo is **server-side**: the simulator keeps a bounded
//! per-object edit history and the reference viewer's `Edit.Undo` / `Edit.Redo`
//! (`LLSelectMgr::undo` / `redo`) simply send the `Undo` / `Redo` messages
//! carrying the selected objects' ids — `LLSelectMgr` is the active
//! `gEditMenuHandler` while building. The simulator (and OpenSim's
//! `SceneObjectPart.Undo()` / `Redo()`) do the reverting; there is no inverse
//! `ObjectUpdate` synthesised on the client, and no local action stack. This
//! module therefore records nothing — it maps the shortcut / menu pick to a
//! [`Command::UndoObjects`] / [`Command::RedoObjects`] over the current
//! selection, matching the reference exactly.
//!
//! # What gets sent
//!
//! The `Undo` / `Redo` wire messages address objects by full id, but our
//! [`Command::UndoObjects`] takes region-scoped ids (like every other object
//! command) and sl-proto resolves them to full ids from its object cache. The
//! send set is simply the current selection's nodes:
//!
//! - **Whole-linkset mode** (the default): the selection tracks only linkset
//!   roots, so the message names the roots — the reference's `SEND_ONLY_ROOTS`
//!   (used when *not* editing linked parts).
//! - **Edit-linked-parts mode**: the selection is individual prims, so those are
//!   sent — the reference's `SEND_CHILDREN_FIRST`.
//!
//! The order is immaterial: the simulator reverts each named object
//! independently, so — unlike linking, where the first id becomes the root — we
//! need not reproduce the reference's child-first ordering.
//!
//! # Enablement
//!
//! [`can_undo`] / [`can_redo`] mirror the reference's `canUndo` / `canRedo`
//! (`getFirstUndoEnabledObject` / `getFirstEditableObject`): undo needs a
//! selected object the agent can **modify or move**, redo one it can **modify**.
//! Both additionally require the build tool to be active (the reference gates on
//! `LLSelectMgr` being the edit-menu handler, i.e. while building). A node whose
//! `ObjectProperties` reply has not yet arrived counts as permitted
//! (optimistic), like [`crate::edit_link`]; the simulator is the final arbiter.
//! The Build-menu entries grey out when these fail ([`crate::menu_bar`]), and
//! the shortcut path re-checks before sending.
//!
//! Reference (Firestorm, read-only): `llselectmgr` `undo` / `redo`,
//! `canUndo` / `canRedo`, `getFirstUndoEnabledObject` / `getFirstEditableObject`,
//! `packObjectID`; messages `Undo` / `Redo`.

use bevy::prelude::*;
use sl_client_bevy::{Command, Permissions, ScopedObjectId, SlCommand};

use crate::input_context::InputContext;
use crate::menu::TOP_MENU_ELEMENT;
use crate::ui_element::UiAction;
use crate::world_api::EditToolState;
use crate::world_api::{SelectedNode, SelectionSet};

/// The Build-menu action string the Undo entry emits.
pub(crate) const UNDO_ACTION: &str = "undo-objects";

/// The Build-menu action string the Redo entry emits.
pub(crate) const REDO_ACTION: &str = "redo-objects";

/// Whether the agent may **modify** this selected object — the reference's
/// `permModify`. A node whose `ObjectProperties` reply has not yet arrived
/// counts as modifiable (optimistic); the reply lands within a frame or two of
/// selection and the simulator is the final arbiter of the undo either way.
fn node_modifiable(node: &SelectedNode) -> bool {
    node.properties()
        .is_none_or(|properties| properties.permissions.owner.contains(Permissions::MODIFY))
}

/// Whether the agent may **move** this selected object — the reference's
/// `permMove`. Optimistic for a not-yet-known node, like [`node_modifiable`].
fn node_movable(node: &SelectedNode) -> bool {
    node.properties()
        .is_none_or(|properties| properties.permissions.owner.contains(Permissions::MOVE))
}

/// Whether the current selection can be **undone** — the reference's `canUndo`
/// (`getFirstUndoEnabledObject`): the build tool is active and at least one
/// selected object is modifiable **or** movable. (The reference also excludes
/// `isPermanentEnforced` objects; the viewer does not track that flag, so a rare
/// permanent object would send an undo the simulator simply ignores.)
pub(crate) fn can_undo(selection: &SelectionSet, tool: &EditToolState) -> bool {
    tool.active
        && !selection.is_empty()
        && selection
            .iter()
            .any(|node| node_modifiable(node) || node_movable(node))
}

/// Whether the current selection can be **redone** — the reference's `canRedo`
/// (`getFirstEditableObject`): the build tool is active and at least one selected
/// object is modifiable.
pub(crate) fn can_redo(selection: &SelectionSet, tool: &EditToolState) -> bool {
    tool.active && !selection.is_empty() && selection.iter().any(node_modifiable)
}

/// The region-scoped ids to name in the `Undo` / `Redo` for the current
/// selection — every selected node (roots in whole-linkset mode, individual
/// prims in edit-linked-parts mode). See the [module documentation](self).
fn undo_ids(selection: &SelectionSet) -> Vec<ScopedObjectId> {
    selection.iter().map(SelectedNode::scoped).collect()
}

/// Send the `Undo` for the current selection, if it can be undone. Returns
/// whether an undo was sent.
fn undo_selection(
    selection: &SelectionSet,
    tool: &EditToolState,
    commands: &mut MessageWriter<SlCommand>,
) -> bool {
    if !can_undo(selection, tool) {
        return false;
    }
    let local_ids = undo_ids(selection);
    if local_ids.is_empty() {
        return false;
    }
    debug!("build-tools: undo {} object(s)", local_ids.len());
    commands.write(SlCommand(Command::UndoObjects { local_ids }));
    true
}

/// Send the `Redo` for the current selection, if it can be redone. Returns
/// whether a redo was sent.
fn redo_selection(
    selection: &SelectionSet,
    tool: &EditToolState,
    commands: &mut MessageWriter<SlCommand>,
) -> bool {
    if !can_redo(selection, tool) {
        return false;
    }
    let local_ids = undo_ids(selection);
    if local_ids.is_empty() {
        return false;
    }
    debug!("build-tools: redo {} object(s)", local_ids.len());
    commands.write(SlCommand(Command::RedoObjects { local_ids }));
    true
}

/// The plugin wiring object undo / redo into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EditUndoPlugin;

impl Plugin for EditUndoPlugin {
    /// Register the undo / redo driver.
    fn build(&self, app: &mut App) {
        // Undo / redo only fires while the build tool is active (it already
        // bailed otherwise), so gate it out of the scheduler outside build mode.
        app.add_systems(
            Update,
            drive_undo_redo.run_if(crate::edit_tool::edit_tool_active_or_settling),
        );
    }
}

/// Drive Undo / Redo from either the keyboard chords (`Ctrl+Z` / `Ctrl+Y`, while
/// the build tool is active and the world owns the keyboard) or the Build-menu
/// entries.
fn drive_undo_redo(
    keyboard: Res<ButtonInput<KeyCode>>,
    context: Res<InputContext>,
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    mut actions: MessageReader<UiAction>,
    mut commands: MessageWriter<SlCommand>,
) {
    let mut do_undo = false;
    let mut do_redo = false;

    // The keyboard chords: only while editing and only when the world (not a
    // text field) owns the keyboard, so `Ctrl+Z` typed into a field never undoes
    // an object edit. `Ctrl+Z` undoes; `Ctrl+Y` redoes — the reference
    // accelerators. Fired once per press (the reference's `allow_key_repeat`
    // OS-repeat is deliberately not reproduced — holding the chord would undo far
    // too fast at frame rate).
    if tool.active
        && *context != InputContext::TextEntry
        && (keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight))
    {
        if keyboard.just_pressed(KeyCode::KeyZ) {
            do_undo = true;
        }
        if keyboard.just_pressed(KeyCode::KeyY) {
            do_redo = true;
        }
    }

    // The Build-menu picks (the entries are greyed out when the operation is
    // unavailable, but re-check below regardless).
    for action in actions.read() {
        if action.element != TOP_MENU_ELEMENT {
            continue;
        }
        match action.action {
            UNDO_ACTION => do_undo = true,
            REDO_ACTION => do_redo = true,
            _other => {}
        }
    }

    if do_undo {
        undo_selection(&selection, &tool, &mut commands);
    }
    if do_redo {
        redo_selection(&selection, &tool, &mut commands);
    }
}

#[cfg(test)]
mod tests {
    use super::{can_redo, can_undo, undo_ids};
    use crate::world_api::EditToolState;
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

    /// An active build tool — undo / redo are gated on it.
    fn active_tool() -> EditToolState {
        EditToolState {
            active: true,
            ..EditToolState::default()
        }
    }

    /// Undo / redo need the build tool active: with it closed both are disabled
    /// even for a modifiable selection.
    #[test]
    fn gated_on_build_tool_active() {
        let mut set = SelectionSet::default();
        set.insert(scoped(1), full(1), Entity::PLACEHOLDER);
        let closed = EditToolState::default();
        assert!(!can_undo(&set, &closed), "closed build tool → no undo");
        assert!(!can_redo(&set, &closed), "closed build tool → no redo");
        assert!(can_undo(&set, &active_tool()), "active + selection → undo");
        assert!(can_redo(&set, &active_tool()), "active + selection → redo");
    }

    /// With nothing selected, neither undo nor redo is available (matching the
    /// reference's `getFirst*Object() != NULL`).
    #[test]
    fn empty_selection_disables_both() {
        let set = SelectionSet::default();
        assert!(!can_undo(&set, &active_tool()), "no selection → no undo");
        assert!(!can_redo(&set, &active_tool()), "no selection → no redo");
    }

    /// A properties-less node counts as permitted (optimistic), so both undo and
    /// redo enable the instant an object is selected — before its
    /// `ObjectProperties` reply lands.
    #[test]
    fn properties_less_node_is_optimistically_permitted() {
        let mut set = SelectionSet::default();
        set.insert(scoped(9), full(9), Entity::PLACEHOLDER);
        assert!(can_undo(&set, &active_tool()));
        assert!(can_redo(&set, &active_tool()));
    }

    /// The send set is every selected node, in selection order (order is
    /// immaterial: the simulator reverts each object independently).
    #[test]
    fn undo_ids_names_the_whole_selection() {
        let mut set = SelectionSet::default();
        set.insert(scoped(3), full(3), Entity::PLACEHOLDER);
        set.insert(scoped(4), full(4), Entity::PLACEHOLDER);
        set.insert(scoped(5), full(5), Entity::PLACEHOLDER);
        assert_eq!(undo_ids(&set), vec![scoped(3), scoped(4), scoped(5)]);
    }
}
