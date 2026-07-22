---
id: viewer-edit-face-selection
title: Select Face tool — per-face selection for texture edits
topic: viewer
status: blocked
origin: user request (2026-07-22) — was only a passing mention inside
  viewer-prim-texture-editing
blocked_by: [viewer-object-selection-core]
refs: [viewer-prim-texture-editing, viewer-object-edit-floater-shell]
---

Context: [context/viewer.md](../context/viewer.md).

The edit floater's **Select Face** mode (`LLToolFace`): clicking a prim
face selects that face instead of the object, `Shift`-click builds a
multi-face set (across prims of the selection), and the selected faces
draw the reference's **highlight overlay** so you can see what a
texture edit will hit. The per-face *pick* half already exists (the
`P` face-pick dump resolves a clicked face's `TextureFace`); this task
is the tool mode, the face-set state on the selection
([[viewer-object-selection-core]]), the overlay rendering, and
exposing the set so the texture / material tab
([[viewer-prim-texture-editing]]) applies edits to exactly the chosen
faces (`ObjectImage` takes a face index; "all faces" stays the
default).

Reference (Firestorm, read-only): `lltoolface.cpp`,
`llselectmgr.cpp` (face selection set + highlight),
`llpanelface.cpp` (per-face apply).
