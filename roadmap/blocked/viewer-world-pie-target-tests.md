---
id: viewer-world-pie-target-tests
title: Right-clicking each world target opens exactly its pie
topic: viewer
status: blocked
origin: test-harness plan (2026-08-30)
points: 5
refs: [viewer-ui-radial-menu, viewer-ui-interaction-harness]
blocked_by: [viewer-world-test-harness]
---

Context: [context/testing.md](../context/testing.md).

First slice landed (2026-08-31): the prim target —
`a_right_click_on_a_prim_asks_for_the_object_pie` drives a real right
click through the synthetic pointer and the CPU resolver and drains
exactly one `OpenObjectMenu` (and none over empty sky). The remaining
targets wait on the harness's avatar / attachment / terrain / HUD
fixtures; [[viewer-cpu-pick-resolver]] itself is done.

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
