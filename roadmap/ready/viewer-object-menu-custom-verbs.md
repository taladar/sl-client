---
id: viewer-object-menu-custom-verbs
title: Custom touch/sit text + click-action default verb
topic: viewer
status: ready
origin: script-interface survey (2026-07-23)
refs:
  [
    viewer-object-context-menu,
    viewer-object-pie-enable-fidelity,
    protocol-36,
    protocol-17,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

Scripts customise how an object presents its interaction verbs:
`llSetTouchText`/`llSetSitText` rename the pie/context-menu Touch and Sit
entries, and `llSetClickAction` picks the default left-click verb (touch,
sit, buy, pay, open, zoom). The data is already decoded — ObjectProperties
`TouchName`/`SitName` ([[protocol-36]]) and the per-object `ClickAction`
([[protocol-17]]) — but `object_menu.rs`/`pie_menu.rs` never read them.

Scope:

- Label the pie/context-menu Touch and Sit entries with the object's
  `TouchName`/`SitName` when non-empty (falling back to the defaults).
- Honour `ClickAction` as the default left-click behaviour on the object
  (cursor hint + dispatch to the matching action), as the reference does.
- Respect the pie-menu positional-stability convention: renamed entries
  keep their compass positions (update the committed position tests).

Reference (Firestorm, read-only): `llselectmgr.cpp` (touch/sit name
plumbing), `lltoolpie.cpp` (click-action dispatch, cursors).

Builds on: object context/pie menus (done) and the decoded
ObjectProperties field surface. Enable-fidelity polish is
[[viewer-object-pie-enable-fidelity]].
