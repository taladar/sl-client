---
id: viewer-create-shift-drag-duplicate
title: Create tool — Shift-drag to duplicate an object into a new one
topic: viewer
status: done
origin: user request (2026-07-26) while reviewing the Create tool
refs: [viewer-prim-creation, viewer-transform-gizmos,
  viewer-object-selection-core]
---

Context: [context/viewer.md](../context/viewer.md).

The reference viewer's build-tool **Shift-drag** gesture duplicates the
selected object(s): holding `Shift` while dragging a move handle leaves the
original in place and drags a fresh copy out of it (the reference's
`LLManip::handleMouseDownOnPart` "copy on shift-drag" path, driven by
`LLSelectMgr::selectDuplicate` /
`selectDuplicateOnRay`). It is the fast way to lay down a row of identical
prims once the first is built.

This is distinct from the Create tool's rez ([[viewer-prim-creation]]): it
copies an existing selection rather than rezzing a base type, and it is armed
from the **Move** manipulator, not the Create tool. It wants:

- detect a `Shift`-held drag-start on a move gizmo handle
  ([[viewer-transform-gizmos]]) and, on that first drag, send an
  `ObjectDuplicate` (`Command::DuplicateObjects`, already wired) for the
  current selection with the drag offset — then continue dragging the **copy**;
- the reference's `CreateToolCopyCenters` / `CreateToolCopyRotates` options
  (whether the copies snap to grid centres and inherit rotation);
- switch the selection to the new copy so a repeated Shift-drag chains copies.

The wire side (`ObjectDuplicate` / `DuplicateObjects`) already exists and is
used by the object pie's Duplicate; this task is the gizmo-drag gesture and the
copy-follows-the-drag interaction on top of it.

Reference (Firestorm, read-only): `llmanip.cpp` (the shift-drag copy branch),
`llselectmgr.cpp` (`selectDuplicate`, `selectDuplicateOnRay`), `lltoolplacer`
(`addDuplicate`).

## Done

`src/gizmos.rs`: holding **Shift** while starting a **move**-handle drag arms
the copy (`arms_duplicate`); on the drag's first movement
`drive_gizmo_interaction` queues the selected roots onto
`GizmoInteraction::pending_copy`, and a separate `dispatch_shift_drag_copy`
system sends one `ObjectDuplicate` (`Command::DuplicateObjects`) at **zero**
offset for those roots. The split keeps the permission/notice logic off the
already-16-parameter drag system.

**Reference mechanics — matched, not the task's wording.** The reference
(`LLManipTranslate` `MASK_COPY` → `selectDuplicate(LLVector3::zero, false)`,
`SEND_ONLY_ROOTS`) does **not** "drag a fresh copy out and select it"; it leaves
an *unselected* copy behind in place and keeps dragging the still-selected
**original** away — identical result (a row of prims), simpler mechanics, no
async copy-matching. Live-confirmed on OpenSim (the copy `ObjectAdded` arrives
at the original's start position while the original moves on). Chaining works
without switching selection.

Armed **only in whole-object mode** (the reference blocks copy-drag while
editing linked parts, `!selectGetNoIndividual()`). A selection the agent cannot
copy is not duplicated — a no-permission notice is posted to the on-screen
local-chat overlay instead (the reference's `NoCopyPermsNoObject`), via a new
client-side `chat::LocalChatNotice` message the overlay renders alongside
received chat. Copy permission = the owner mask's `COPY` bit for the agent's own
objects, else the everyone mask's (`can_copy`). Live-confirmed: shift-dragging a
non-owned prim logs `shift-drag copy blocked` and posts the notice; an owned
prim duplicates.

**Scope note:** the roadmap named `CreateToolCopyCenters` /
`CreateToolCopyRotates`, but those settings belong to the *separate*
`ObjectDuplicateOnRay` "copy selection" land tool (`LLToolPlacer`), which
`selectDuplicate` never consults — so no toggles were added. Their default
behaviours already hold: copies land on the drag's grid-snapped offset (existing
snap) and inherit the source rotation unchanged.
