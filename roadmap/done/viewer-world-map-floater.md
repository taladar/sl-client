---
id: viewer-world-map-floater
title: World-map floater — surface, zoom & region tiles
topic: viewer
status: done
origin: user request (2026-07); split from viewer-world-map
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The world-map floater surface: pan and zoom across the grid with its two zoom
regimes (grid-wide vs. region-detail), region tiles as the backdrop. This is the
root of the world-map cluster — search, markers and tracking / teleport all
extend the surface it introduces.

Done (2026-07-23): `world_map.rs` + pure `world_map_math.rs` (unit-tested) +
`world_map_tiles.rs` in the viewer. A resizable floater (World ▸ World Map,
`Ctrl+M`, the bottom-toolbar Map button now landed) with a single
CPU-composited `ImageNode` surface, mirroring the minimap architecture:
stamp-gated recomposite, plain-drag pan, wheel zoom to the cursor
(`2^0.25`/notch), zoom presets in a right-click context menu, scale persisted
(`[worldmap] WorldMapScale`, debounced), floater geometry persisted by the
scaffold. The reference's two regimes are kept: tile level =
`scaleToLevel(scale)` (1–8), per-region info (map blocks, names, markers)
only in the detail regime (level ≤ 3); region-name labels are pooled `Text`
overlays from scale ≥ 96, region borders draw as grid lines, and coarser
resident tiles back-fill while the right level loads.

**Region tile imagery** reuses the sibling `sl-map-tools` fetch / cache as
planned: `sl-map-apis`' `MapTileCache` (memory + disk,
`http-cache-semantics`) gained a `new_with_base_url` constructor (the CDN was
hardcoded) and runs on a dedicated worker thread owning a current-thread
tokio runtime, talking to the ECS over crossbeam channels; the per-grid disk
cache lives under the viewer cache root (`maptiles/<grid>`). The **base URL
pairs per grid**: a new `map-server-url` login-response field (plumbed
sl-wire → sl-proto `Session::map_server_url()` → `SlIdentity`) — OpenSim
standalone announces it out of the box — overridden by a region's
`SimulatorFeatures` OpenSimExtras `map-server-url`, falling back to the
Second Life CDN on agni. The minimap's `DoubleClickAction::WorldMap` now
really opens the floater centred on the clicked point (`OpenWorldMap`
message).

The reference floater's side-panel controls are in too (user-requested
during the live check): clicking the map selects a location (red target
marker), with a readout line, region-local X/Y/Z fields, a **Teleport**
button (direct `Command::Teleport`, as the minimap teleports today — the
progress-screen flow stays with its own task), and **Copy SLURL** (the
shared `sl-types` `Location` maps-URL, via the OS clipboard / `arboard`),
plus visible layer-filter checkboxes mirroring the context-menu toggles.

Deviations, owned by their own tasks: map-side tracking hand-off to the
in-world beacon and double-click teleport
([[viewer-world-map-tracking-teleport]]); the legacy `MapLayerReply`
image path stays unused (tile-URL grids and per-grid fallback cover the
supported grids).

Reference (Firestorm, read-only): `llfloaterworldmap`, `llworldmap`,
`llworldmapmessage`, `llworldmipmap`.

Builds on: `protocol-12` map data, `protocol-65` map-service pairing, and
`sl-map-tools` for tiles.

Deps: [[viewer-ui-widget-scaffold]] (the floater).
