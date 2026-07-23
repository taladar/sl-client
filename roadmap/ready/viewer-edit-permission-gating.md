---
id: viewer-edit-permission-gating
title: Permission-aware editing (grey out what perms forbid)
topic: viewer
status: ready
origin: user request during the edit-gizmo session (2026-07-23)
blocked_by: [viewer-object-selection-core]
refs: [viewer-transform-gizmos, viewer-object-edit-floater-shell]
---

Context: [context/viewer.md](../context/viewer.md).

The edit surfaces currently let the agent *attempt* any transform edit and
rely on the simulator to reject it; the selection set already tracks each
node's `ObjectProperties` permission masks, and the build floater only shows
a "no modify" note. Make editing **permission-aware** the way the reference
viewer is: read the selection's aggregated permissions (owner / group /
modify / move) and grey out — disable, not hide — whatever they forbid:

- no `MODIFY` → the size / rotation fields, the stretch and rotate gizmos,
  and the future parameter / texture tabs read disabled;
- no `MOVE` → the position fields and translate gizmo too;
- mixed multi-selections gate on the **intersection** of the nodes'
  permissions (one no-mod object locks the set, as the reference does);
- the disabled look reuses the skin's `--text-disabled` role and the
  floater's existing disabled-control conventions.

Reference (Firestorm, read-only): `LLSelectMgr::selectGetModify` /
`getFirstMoveable` and the per-control `getEnabled` gates in
`llfloatertools.cpp` / `llpanelobject.cpp`.

Builds on: the selection set's per-node `ObjectProperties`
([[viewer-object-selection-core]]) and the update-flags bits already used by
the pie menu's enable gates.
