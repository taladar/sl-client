---
id: viewer-create-shift-drag-duplicate
title: Create tool — Shift-drag to duplicate an object into a new one
topic: viewer
status: ready
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
