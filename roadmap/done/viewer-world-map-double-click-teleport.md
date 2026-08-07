---
id: viewer-world-map-double-click-teleport
title: World-map double-click teleport + tracking marker
topic: viewer
status: done
origin: split from viewer-world-map-tracking-teleport (2026-08-07) — the teleport half, done
refs: [viewer-world-map-tracking-teleport, viewer-world-map-floater, viewer-teleport-flow-progress]
---

Context: [context/viewer.md](../context/viewer.md).

The teleport half of [[viewer-world-map-tracking-teleport]], split out and
**done**. The in-world tracking **beam** hand-off stays in the parent task
(blocked on [[viewer-beacons-beam-render]]).

## Done (2026-08-07)

`world_map.rs`: **double-clicking a region** on the map surface teleports there
— it drops the shared `MapTracking` beacon at the clicked spot (unless already
tracking, matching the minimap / reference) and issues the teleport through the
shared `issue_teleport` backend (so it drives the same progress overlay). A
single click still only selects the region (feeding the X/Y/Z fields), and the
existing **Teleport** button now routes through the shared backend too. The
minimap double-click was likewise re-routed through `issue_teleport`, so the
map, minimap, and in-world double-click are **one teleport + progress path**,
not three. Verified live on OpenSim.

Deferred to the parent (blocked): the in-world tracking **beam** pointed at the
tracked point ([[viewer-beacons-beam-render]]), and object rebasing on the
cross-region arrival ([[viewer-seamless-region-handover-objects]]).
