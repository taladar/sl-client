---
id: viewer-movement-controls-floater
title: Movement controls floater + stand / stop-flying buttons
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-sit-stand-actions, viewer-camera-controls-window, viewer-qol-toggles]
---

Context: [context/viewer.md](../context/viewer.md).

The on-screen movement controls (`llfloatermove`): walk / run / fly mode
buttons and directional arrows (click-and-hold moves, feeding the same input
actions the keyboard uses), plus the always-visible **Stand** /
**Stop Flying** buttons that appear while sitting / flying.

Placement decision (user, 2026-07-22): the stand / stop-flying buttons are
**not** docked above the chat bar as the Vintage skin does — that wastes a
row between the conversations floater and the bottom bars. This task must
propose an alternative (candidates: attached to the bottom toolbar's right
edge, a corner overlay near the avatar, or inside the movement floater
itself which auto-opens while sitting) and record the choice in this file
when fleshed.

Also owns the movement-mode preference (walk/run default, the Always Run
toggle surface pairs with [[viewer-qol-toggles]]).

Reference (Firestorm, read-only): `llfloatermove` / `floater_moveview.xml`,
`panel_stand_stop_flying.xml`.

Builds on: the movement input actions (`movement.rs`, `input_action.rs`)
and the sit/stand command surface ([[viewer-sit-stand-actions]] wires the
sit protocol; the buttons here reuse its commands).
