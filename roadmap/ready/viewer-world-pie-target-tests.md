---
id: viewer-world-pie-target-tests
title: Right-clicking each world target opens exactly its pie
topic: viewer
status: ready
origin: test-harness plan (2026-08-30)
points: 5
refs: [viewer-ui-radial-menu, viewer-ui-interaction-harness]
blocked_by: [viewer-world-test-harness]
---

Context: [context/testing.md](../context/testing.md).

All six targets landed (2026-08-31): prim → `OpenObjectMenu` (and none
over empty sky), another avatar and the own avatar → `OpenAvatarMenu`
naming the right agent, a worn attachment → `OpenAttachmentMenu` with
`hud: false`, bare land → `OpenLandMenu`, and — with the vendored
character assets satisfying `setup_hud_screen` — a HUD-Center
attachment → `OpenAttachmentMenu` with `hud: true` through the
orthographic HUD pick. Each is a real right click through the synthetic
pointer and the CPU resolver ([[viewer-cpu-pick-resolver]], done).
Remaining: the through-a-floater and after-a-right-drag negatives, the
seated stand-up condition, and the `OpenPieMenu`-level assertions
(expected element at the cursor, layout, compass click → action).

The four live pie address tables are pinned; what nobody tests is
*target classification under a real right click*. In the fixture world
with the CPU pick resolver: right-click on a prim, another avatar, the own
avatar, a worn attachment, bare land and a HUD attachment each drain
exactly one `OpenPieMenu` with the expected element at the cursor (seated
adds the stand-up condition); a right-click through a floater or after a
right-drag opens nothing. Then the spawned pie lays out clean
(`layout_violations` empty) and a compass click drains the declared action
— the end-to-end "right-click prim → Edit → `EditToolState.active`" check.
Pin the menu-bar action table the way the pies are, if it is not already.
