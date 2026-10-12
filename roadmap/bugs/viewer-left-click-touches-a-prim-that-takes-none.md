---
id: viewer-left-click-touches-a-prim-that-takes-none
title: A left click sends a grab and a degrab to a prim that takes no touch
topic: viewer
status: bugs
origin: gridspec-touch-grab (2026-10-12)
refs: [gridspec-touch-grab, viewer-left-click-click-actions,
  viewer-automation-world-aim]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

`sl-viewer-world-view/src/hud_pick.rs` begins a touch — an `ObjectGrab`,
and the `ObjectDeGrab` of the release — for every prim face a left click
lands on. The reference (`LLToolPie::handleLeftClickPick`) sends nothing
unless the prim or its root has `FLAGS_HANDLE_TOUCH`, or is physical, and
its click action is not *disabled*.

## Effect

None on the grid: a grab and a degrab with no update between them do
nothing to a prim with no touch script on Second Life or on OpenSim
(`book/src/gridspec/building.md` § Touch and grab). The viewer already
sends no `ObjectGrabUpdate` for such a prim, which is the message that
would move it. What is left is two reliable packets a click, and a click
that reads in a packet trace as a touch.

## Fix

Gate the grab on `ObjectState::pick_summary` — the touch flag of the prim
or its root, and the click action — as `double_click_teleport.rs` already
reads them (`ObjectClick::takes_a_click`). The fixture tests that count a
click by its `GrabObject` (`world_test.rs`, `automation_world_aim.rs`)
click prims with no touch flag and need one, and the automation's world
aim reports a click on such a prim as done: decide there whether "nothing
takes this click" is a refusal.
