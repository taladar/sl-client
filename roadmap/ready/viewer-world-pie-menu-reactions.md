---
id: viewer-world-pie-menu-reactions
title: Right-click reactions per world target class
topic: viewer
status: ready
origin: user request (2026-07) — right-click avatar must show the pie menu
points: 5
blocked_by: [viewer-world-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

For each fixture target class, right-click through the real classifier
(`avatar_menu.rs::request_avatar_menu_on_right_click` — the single system
that raycasts and dispatches) and pin the reaction:

- the correct `Open*Menu` message per class (avatar / object / attachment
  / land / HUD / self);
- the resulting `OpenPieMenu`'s `PieMenuDef` entry set — committed
  per-class tables in the compass-address-table shape `pie_menu.rs`
  already mandates;
- each pie action's downstream mapping (`handle_avatar_menu_actions` and
  its object/attachment/land counterparts) to `SlCommand` — touch, sit,
  stand, pay, profile-open, edit-enter — including reach/distance and
  permission gating where the handlers apply it.

Covers `object_menu.rs`, `avatar_menu.rs`, `attachment_menu.rs`,
`land_menu.rs`.
