---
id: viewer-world-map-markers
title: World-map floater — MapItem marker layers
topic: viewer
status: done
origin: user request (2026-07); split from viewer-world-map
blocked_by: [viewer-world-map-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The `MapItem` marker layers over the map surface: avatar dots, telehubs,
land-for-sale, and the event classes, with their per-layer filters. The protocol
side is done — `request_map_items` returns `MapItem` (with `region_handle()` /
`region_position()`) and `MapItemType` already distinguishes telehubs, agent
locations, land-for-sale and the event classes.

Done (2026-07-23), inside `world_map.rs`: in the detail regime the floater
requests each visible, known region's enabled item layers (send-rate capped,
per-region+type freshness 30 s) and composites them onto the surface — agent
dots (green, radius grows with the count in `extra`), telehub / infohub
rings, land-for-sale squares (yellow; adult variant orange), and the three
event classes as colour-coded discs. Per-layer filters live in the
right-click context menu (People / Telehubs / Land for Sale / Events /
Moderate / Adult Events) **and** as visible side-panel checkboxes,
persisted under `[worldmap]` (`WorldMapShow*`; Moderate/Adult events
default off). OpenSim's count-0 agent-locations sentinel (sent for a
region without other avatars, at the region corner) is filtered out.
Hovering a marker shows a tooltip line by kind (avatar count, telehub vs.
infohub name, for-sale name + L$ price + m² area from `extra`/`extra2`,
event name); markers are hit-test data like the minimap's dots. A
committed test pins that every menu action string has a handler arm.

Reference (Firestorm, read-only): `llfloaterworldmap`, `llworldmap`,
`llworldmapmessage`.

Builds on: `protocol-12` map items and the map floater surface
([[viewer-world-map-floater]]).

Deps: [[viewer-world-map-floater]].
