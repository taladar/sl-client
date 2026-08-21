---
id: viewer-radar-multi-select
title: Radar multi-row selection and its multi-avatar menu
topic: viewer
status: ready
origin: user question (2026-08-21) while building viewer-minimap-menu-multi-avatar
blocked_by: []
refs:
  [
    viewer-avatar-radar,
    viewer-minimap-menu-multi-avatar,
    viewer-conference-start-ui,
    viewer-radar-avatar-marks,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

The radar is single-select today: `SelectedRadarAvatar(Option<AgentKey>)`, a
row press that ignores Ctrl / Shift, and row actions that each act on the one
avatar. Firestorm's radar is **multi-select** — it ships a whole second
context menu for it (`menu_fs_radar_multiselect.xml`): mark / unmark the
selection, IM them, start a conference with them, offer teleport, block, and
the estate / parcel moderation entries, each acting on every selected row.

Scope:

- Ctrl-toggle and Shift-range selection in the radar table, honouring the
  modifier keys, with the selection surviving a refresh of the rows (a radar
  re-sorts constantly, so the set is by `AgentKey`, not by row index).
- The multi-avatar context menu — the entries above, routed to the **same**
  shared avatar-action channels the single-avatar menu already writes, never a
  second implementation.
- The action bar / trailing row buttons stay single-avatar (they belong to a
  row); the menu is where the selection acts.

The widget half already exists: `ui_table.rs` has
`TableSelectionMode::{None, Single, Multi}` with a unit-tested
`apply_click(index, ctrl, shift)` and `selected()` — but **no consumer uses
`Multi`**, and the radar drives its own highlight with `SelectionMode::None`.
So this task is the radar's adoption of that widget mode plus the menu, not
new selection algebra.

The dynamic-label menu machinery landed with
[[viewer-minimap-menu-multi-avatar]] (`MenuItemDef::DynamicSubmenu`, a slot of
runtime labels, `MenuDynamicPick`) — the radar's per-avatar submenus can reuse
it rather than inventing a second mechanism.

Reference (Firestorm, read-only): `fsradar.cpp` / `fspanelradar.cpp`,
`menu_fs_radar.xml`, `menu_fs_radar_multiselect.xml`.
