---
id: viewer-left-click-click-actions
title: Carry out a prim's click action on a left click
topic: viewer
status: ready
origin: gridspec-touch-grab (2026-10-12)
refs: [gridspec-touch-grab, viewer-object-pie-enable-fidelity,
  viewer-build-general-sale-clickaction, viewer-touch-cursor,
  viewer-left-click-touches-a-prim-that-takes-none]
---

Context: [context/viewer.md](../context/viewer.md).

A prim's click action is the viewer's to carry out: neither grid withholds
a touch for one, and a script in a prim set to *sit* or to *disabled* is
handed its touch events all the same when a viewer sends the grab
(`book/src/gridspec/building.md` § Touch and grab, 2026-10-12). Our left
click always sends the grab.

The reference (`LLToolPie::handleLeftClickPick`, `useClickAction`,
`final_click_action`): for a prim in the world that is not an attachment,
take the prim's own action when it has one or its root disables its own,
the root's otherwise, and

- **sit** — sit on it, unless already sitting;
- **pay** — open the pay dialog, where the prim or its root takes money;
- **buy**, **open**, **play**, **open media**, **zoom** — each its own
  action, and no touch;
- **disabled** — nothing, and no touch;
- **touch** (the default) — the touch.

`double_click_teleport.rs` already ports `final_click_action`
(`ObjectClick::final_action`); move it where the left click can share it.
Each action that has a viewer feature behind it (sit, zoom, open, buy,
pay) is wired to it; one that has none yet says so in a notification
rather than touching.

Acceptance: a fixture test per action that the left click sends the
action's command and no `GrabObject`, and one that *disabled* sends
nothing.
