---
id: viewer-automation-ground-aim
title: Double-click teleport on every surface, and the world actions it needs
topic: viewer
status: ready
origin: test-e2e-sweep-single-viewer-ui (2026-10-01)
points: 5
refs: [viewer-automation-world-aim, viewer-camera-reset-on-distant-teleport,
  test-e2e-sweep-single-viewer-ui]
---

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
