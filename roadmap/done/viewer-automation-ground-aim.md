---
id: viewer-automation-ground-aim
title: Double-click teleport on every surface, and the world actions it needs
topic: viewer
status: done
origin: test-e2e-sweep-single-viewer-ui (2026-10-01)
points: 5
refs: [viewer-automation-world-aim, viewer-camera-reset-on-distant-teleport,
  test-e2e-sweep-single-viewer-ui]
---

## Done (2026-10-01)

**The viewer's rule** (`double_click_teleport.rs`, `lands`): the reference's
`teleportToClickedLocation` with Firestorm's FIRE-1765. Terrain and water
teleport — water to the ground under it, by marching the pick ray down to the
terrain (`ground_under_water`), since the reference's pick looks through
water. Another avatar teleports, onto the picked point; one's own avatar and
anything it wears never do, and a HUD never does. An in-world object
teleports when it takes no click of its own: no click action and no touch
script on the prim or its root (`final_click_action`'s rule: an attachment
has none, *disabled* masks the root's). With
`FSAllowDoubleClickOnScriptedObjects` (new setting, on by default as in
Firestorm, a checkbox under the Move tab's double-click row) it also lands on
any object whose own click action is not *sit*. The arrival height is the
point plus the own avatar's pelvis-to-foot height, as the reference adds
`getPelvisToFoot`. The click action was not tracked at all before:
`TrackedObject::click_action` and the pick summary's
`ObjectPickSummary::{picked,root}_click_action` now carry it.

**Not done here: RLVa's `@tplocal` / `@sittp` veto.** No send path in the
viewer consults `RlvActions` yet, and the `RlvActionSource` over the viewer's
world it needs is [[viewer-rlv-send-side-consumers]]'s job; the double-click
is listed there.

**Automation**: `WorldAction::DoubleClick` (`WorldHandle::double_click`, ctl
`world double-click`), and the ground as a target: `GroundPoint { region, x,
y }` in a separate `GroundAction` request (not a `WorldLocator` kind — a
locator is a query that finds things, and ground is not found but placed),
answered with `GroundDone { hit_point }` in that region's metres. `GroundAim`
finds the region by name (ASCII case-insensitive), reads the terrain height,
projects the point, refuses one under a UI node, and probes it: only ground
within the footprint of a few pixels around the point counts.
`GroundNotActionable` names what a click there hits instead, `GroundTimedOut`
reports `attached` while the region or its ground is unknown. Select and
shift-select are refused; click, double-click, right-click, hover, place and
drop-from (rez from inventory onto the ground) are taken. ctl: `world
ground-double-click <region> <X,Y>`.

**Two gaps the e2e checks found and fixed**: the fake grid's local teleport
sent `TeleportLocal` but never the moved avatar's update (OpenSim's
`ScenePresence.Teleport` sends one), so the agent stood where it was for
everyone; and the viewer's `AgentRegionPosition` waited for that update,
where the reference takes the `TeleportLocal`'s position at once.

**Tests**: unit — the rule per surface and setting, the root's click
action, attachments, the underwater march, the ground-point bounds, the
protocol's JSON. Fixture (`automation_ground_aim.rs`, CPU pick, through the
executor and the real gesture): a ground double-click teleports onto the spot
with the pelvis-to-foot lift; a spot under a prim fails naming the prim, an
unknown region times out on `attached`, a select and a point past the edge
are refused; a plain prim and another avatar are landed on, a sit prim and
one's own avatar never, a touch-scripted prim only with the setting on.
End to end (`tests/e2e_double_click.rs`, fake grid, both backends, the
gesture switched on by its Ctrl+Shift+D chord, a parked flycam): the ground
within the region, the border's marker pillar and the neighbour's ground
across the border, and an NPC avatar — each landing where the double-click
did, with the camera kept exactly where it was.

Context: [context/automation.md](../context/automation.md).

[[viewer-camera-reset-on-distant-teleport]]'s one gesture check — double-click
to teleport within the region, and the camera keeps the framing that aimed
it — is covered only by its arrival (a grid-side `TeleportLocal`), not by the
double-click: a world action aims at an object, an avatar or an attachment
(`WorldLocator`), and nothing names a piece of ground, nor double-clicks.

Double-click teleport has to be tested on the **surfaces it is used on**, not
only the terrain (user, 2026-10-01):

- **terrain** — addressed by a **region** (by name) and a point in it (x, y
  in region metres; the ground height is the terrain's), the one place a
  world locator states a position, since bare ground has no other name. The
  region is part of the address because a double-click teleport reaches a
  neighbouring region's ground across the border
  (`double_click_teleport.rs`'s `teleport_destination`). The aim projects
  the point, and the pick resolver must confirm `PickResolution::Terrain`
  there, as an object aim's probe confirms its object;
- **an object** — addressed by its ordinary `WorldLocator` and aimed through
  the pick resolver like a touch, so the teleport lands where the pick hits
  its surface (the top of a floor prim, a ramp);
- **an avatar** (another one; never one's own).

So: a `WorldAction::DoubleClick`, a ground target for it (a `WorldLocator`
kind or a separate request), and the e2e checks — the agent arrives where
the double-click landed: within the region (a parked camera keeps its pose),
and on a neighbour's ground or an object across the border (the teleport is
a near one, so the camera is kept there too).

## The viewer side diverges first

`double_click_teleport.rs` teleports **only** when the pick resolves to
terrain; objects, avatars and water are ignored. The reference
(`LLToolPie::teleportToClickedLocation`, `lltoolpie.cpp`) teleports to the
picked point on land **or** on an in-world object that has no click action —
and, with Firestorm's `FSAllowDoubleClickOnScriptedObjects`, on any object
whose click action is not sit — and to another avatar, refusing only one's
own avatar and a HUD; RLVa's `@tplocal` restriction can veto it. Bring the
viewer to that rule (with tests below the e2e tier: a pick on each surface,
the refusals) before the e2e checks above.
