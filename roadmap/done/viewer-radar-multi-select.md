---
id: viewer-radar-multi-select
title: Radar multi-row selection and its multi-avatar menu
topic: viewer
status: done
origin: user question (2026-08-21) while building viewer-minimap-menu-multi-avatar
blocked_by: []
refs:
  [
    viewer-avatar-radar,
    viewer-minimap-menu-multi-avatar,
    viewer-conference-start-ui,
    viewer-radar-avatar-marks,
    viewer-avatar-moderation-actions,
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

## Built (2026-08-21)

The widget already knew how to multi-select; nobody had ever asked it to. Two
small additions were all it needed: `TableState::anchor()` and
`set_selection(indices, anchor)` — the way back *in*, for a consumer whose rows
move under the selection. Everything else here is the radar using what was
there.

The interesting problem is that the two halves disagree about what a selection
*is*. The widget knows row indices, because that is what a click hands it; the
radar re-sorts every second, so an index means something for about one frame.
So the radar keeps the selection as `RadarSelection` — a list of `AgentKey`,
plus the range anchor as an agent too, because a `Shift`+click that ranges from
"whatever row index 4 is now" is worse than no anchor at all. The two are kept
in step in both directions: `mirror_radar_selection` reads the widget's
click through into agents (before the actions consume it and before the rebuild
re-sorts the indices it was phrased in), and `rebuild_radar_view` re-projects
the agents onto the new order at the end of every rebuild. Someone whose row
went away — they left, or the filter now excludes them — drops out: a hidden
member of a selection would act invisibly from the menu. The re-projection
deliberately does not read back as a selection *event*; it is the same people at
new indices.

Dropping the radar's hand-rolled row highlight for the widget's meant the widget
had to paint at the right moment: `apply_table_selection_highlight` now runs
after the virtual-list layout, or a recycled row is painted from the data index
it held last frame.

The menu is the reference's two files, `menu_fs_radar.xml` and
`menu_fs_radar_multiselect.xml`, as two `MenuDef`s over **one** set of action
arms. Which opens is how many rows are selected. A right-click inside the
selection keeps it (the widget ignores the secondary button, so the radar
applies that rule itself); outside it, the row becomes the selection first. The
multi shape leads with the **View Profiles** list
([[viewer-minimap-menu-multi-avatar]]'s `DynamicSubmenu` — one line per selected
avatar, `(loading)` until the name lands, re-labelled in place as it does) and
drops exactly the four entries that can only mean one avatar: View Profile, the
tracking pair, Teleport To. A unit test pins that difference, and another pins
that the multi menu adds no action the single menu cannot dispatch.

The arms take the whole snapshot, so "act on each of them" is not a second
implementation of anything: an offer teleport is one message (its `targets` was
always a list), a block loops over the ones not already muted, an unblock over
the ones that are — the conditions on a mixed selection are the **any** of it,
because hiding an entry unless the whole selection agrees leaves a menu with
nothing in it.

Three entries the single-avatar menu never had came with it, since the reference
has them on both and the model was already in the viewer: **Add to Set**
(`OpenAddToContactSet::many`, the same list the minimap's multi entry files),
**Mark…** (the five colours and the two clears, writing the same session
`MinimapMarks` the minimap's Mark submenu writes — the reference's radar menu
sets `LLNetMap`'s marks too), and **Render Settings** (the standing per-avatar
exception from [[viewer-avatar-render-settings-manager]]), plus **Remove
Friend**.

Two entries of the reference's multi menu are **not** here, both because the
feature behind them does not exist yet, and both now written down where they
will be picked up:

- *Start a conference* — [[viewer-conference-start-ui]], which has gained a note
  that the radar is its first waiting consumer. Until then *IM* on several rows
  opens several direct conversations rather than one conference; that arm
  becomes the conference verb when the task lands.
- *Freeze / Parcel Eject / Estate Kick / Teleport Home / Estate Ban* — no viewer
  surface offers these per avatar yet (the avatar pie holds them as disabled
  placeholders, and the estate tab acts on a typed name), so they are now
  [[viewer-avatar-moderation-actions]]: the shared visibility predicates and
  guarded request channels that let them light up in the pie, the radar, the
  minimap and the profile at once rather than only here. The reference's own
  `EstateBanUserMultiple` / `EstateTeleportHomeMultiple` notifications say those
  actions were always meant to take a list, which is exactly what this
  selection hands them.

Not done here: [[viewer-radar-avatar-marks]] keeps the *row tint* (the radar can
now set a mark, but only the minimap dot shows it — which is also what the
reference does), and the People panel's other lists remain single-select
([[viewer-people-lists-multi-select]]), now with a worked example to follow.
