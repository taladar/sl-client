---
id: viewer-object-inspect
title: Inspect objects floater (linkset breakdown)
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-object-selection-core]
refs: [viewer-area-search]
---

Context: [context/viewer.md](../context/viewer.md).

The Inspect floater: for the selected linkset, one row per prim — name,
**creator**, **owner**, creation date — with click-to-highlight of the
individual prim and jump-to-profile on creator/owner. The who-made-this
tool (and the polite way to find which linked prim is the full-perm
freebie). Data via per-prim `ObjectProperties` requests over the selection
(`protocol-36`).

Reference (Firestorm, read-only): `llfloaterinspect`,
`floater_inspect.xml`.

Deps: [[viewer-object-selection-core]] (selection + per-prim addressing).
