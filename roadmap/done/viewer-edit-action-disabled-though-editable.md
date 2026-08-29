---
id: viewer-edit-action-disabled-though-editable
title: The Edit action is greyed out on objects that build mode edits fine
topic: viewer
status: done
origin: user report while verifying viewer-underwater-name-tags-not-drawn (2026-08-29)
refs: [viewer-object-pie-enable-fidelity]
---

Context: [context/viewer.md](../context/viewer.md).

The **Edit** affordance read disabled on objects that could be selected and
edited perfectly well by entering build mode the ordinary way (Ctrl+B / the
toolbar). Both candidates named when the ticket was written turned out to be
real, and both are fixed.

## Cause 1: the attachment pie's Edit slice was a permanent placeholder

There is exactly one greyable Edit affordance in the viewer, and it is the
**attachment-self pie's**: the in-world object pie's Edit is unconditional
(`object_menu.rs`, `when: None`), and entering build mode from the Build menu
carries no `enabled_when` at all. `ATTACHMENT_SELF_PIE` declared its Edit slice
gated on `UNIMPLEMENTED` — the sentinel documented as never being pushed into
the live condition set — so the slot could never light up, and there was no
`"edit"` arm in the attachment dispatcher either. Any pick whose
`summary.attachment` is true routes to that pie (`avatar_menu.rs`), so every
worn attachment, HUD included, showed a dead Edit.

The reference's `EnableEdit` (`enable_object_edit`, `llviewermenu.cpp`) is
satisfied by any valid selection — it even special-cases an attachment as
editable inside a prelude sandbox — and `Object.Edit` on an attachment
(`handle_attachment_edit`) deselects, selects the worn object, and opens the
build floater.

**Fix.** The slice is `when: None`, like the object pie's, and the dispatcher
opens Build Tools on the worn object. The body both pies share is now
`object_menu::edit_picked_object`, which shows the floater by its stable id and
rewrites the selection to the picked prim or its root according to "Edit linked
parts". New test `edit_is_live_on_an_own_attachment` pins the slice live for a
standing/droppable/touchable attachment and for a seated one; the address table
already pinned its south-east position.

## Cause 2: the Object / Features tab's own modify predicate

The canonical predicate is `ObjectState::agent_can_modify`, which ORs the
object's agent-relative flags **up to the linkset root** (that is where the
simulator puts the modify / copy / transfer / move grants) and is optimistic for
an untracked object. The gizmos, the transform fields, the Texture tab, the
Material tab and prim contents all use it. `edit_params.rs` did not — it read
`data.update_flags & FLAGS_OBJECT_MODIFY` off the object's **own** flags, so a
child prim selected with "Edit linked parts" greyed the whole Object / Features
tab even though the modify bit rode its root.

**Fix.** `SnapshotData` carries `can_modify` (from `agent_can_modify`) and
`agent_flags` (from the newly-public `ObjectState::agent_flags`); `has_modify`
reads the former. Putting them in the snapshot also means a root whose
permission bits change re-gates a selected child prim, which comparing the
child's own fields would have missed.

The same divergence made the read-only **"You can:"** line report nothing for a
selected child prim; it now reads `agent_flags` too.

New test `agent_flags_fold_in_the_linkset_root`
(`sl-viewer-world-objects/src/objects.rs`) pins the walk: a child prim with no
flags of its own under a modify-bit root reads modifiable, and a linkset with
the bit nowhere stays no-modify.

## Not verified

Interactively. The Edit slice's live/disabled state and the address are unit
tested, and the modify walk is unit tested, but neither "right-click a worn
attachment ▸ Edit opens Build Tools on it" nor "the Object tab is editable on a
child prim with Edit linked parts" has been driven by hand on a grid.
