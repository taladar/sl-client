---
id: viewer-left-click-click-actions
title: Carry out a prim's click action on a left click
topic: viewer
status: done
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

## Done (2026-11-03)

`sl_viewer_world_api::prim_clicks` holds the decision — `left_click` (the
reference's `useClickAction` + the `handleLeftClickPick` switch + the
touchable / physical grab gate) and `final_click_action`, which the double-click
teleport now shares. `hud_pick::touch_hit` asks it after the media-face check:
a click action goes out as a `ClickActionRequest`, *disabled* and a prim that
takes no click send nothing (closing
[[viewer-left-click-touches-a-prim-that-takes-none]]), and only a touch sends
the grab. `sl_viewer_ui_context_menus::left_click_actions` answers the request
with the object pie's own machinery:

- **sit** — `AgentRequestSit` at the Sit Here offset; skipped when already
  sitting or when `FSBlockClickSit` is on ([[viewer-sit-stand-actions]]), in
  which case the click falls through to pay, then touch;
- **open** — the root's contents (`OpenObjectContents`), when the root holds
  any and the agent owns or may modify it (`allowOpen`), otherwise nothing;
- **zoom** — `FrameObject` on the picked prim's bounding box;
- **open media** — the agent parcel's media URL in the web browser;
- **pay** / **buy** / **play** — a notification each (`ClickAction*Unavailable`)
  until [[viewer-money-economy-ui]] and parcel media in
  [[viewer-video-playback]] exist; pay only where the prim or its root takes
  money, as in the reference.

Acceptance in `world_test` (`a_left_click_on_a_prim_does_what_the_prim_says`):
one click per action, asserting the commands on the wire and the window /
camera / notification requests. Not covered by a fixture: open media (no
`ParcelInfo` fixture in the world fold); its decision is unit-tested.
RLVa's `@buy` / `@pay` / interact gates on the click remain with
[[viewer-rlv-send-side-consumers]].
