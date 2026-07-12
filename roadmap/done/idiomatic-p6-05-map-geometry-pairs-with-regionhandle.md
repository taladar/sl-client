---
id: idiomatic-p6-05
title: map geometry — pairs with RegionHandle
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 6 — Adopt `sl-types` non-key value types (low-medium)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`map` geometry — pairs with `RegionHandle`. Consume-only (no `sl-types`
    change). **`GridCoordinates`:** the redundant `grid_x: u32` /
    `grid_y: u32` pair on `RegionIdentity`, `NeighborInfo`, and
    `MapRegionInfo` collapses to one typed `grid_coordinates: GridCoordinates`
    field. For the two handle-derived carriers a new private
    `grid_coordinates_from_handle` decodes the handle via the Phase-4
    `TryFrom<RegionHandle>` (falling back to the `(0,0)` unknown sentinel);
    for `MapRegionInfo` the wire `u16` pair is primary and the region handle
    is the typed `RegionHandle::from(grid_coordinates)` inverse. Codec wraps
    at the boundary (`map_region_info` decode /
    `map_region_info_to_data_block` encode use `.x()`/`.y()` directly — no
    more `u16::try_from` narrowing) so the `MapBlockReply` `Data` block is
    byte-identical. **`RegionCoordinates`:** the region-local teleport
    *position* (`Command::Teleport.position`, `Session::teleport_to`, and
    `ScriptTeleportRequest.position` — the look-at stays a direction
    `Vector`/tuple) is now `RegionCoordinates`; `teleport_to` unwraps it to
    the wire `Vector` at the `TeleportLocationRequest` boundary, the
    `ScriptTeleportRequest` decode wraps the wire vector, so wire bytes are
    unchanged. Both types re-exported through `sl-proto`/`sl-client-tokio`/
    `sl-client-bevy` (parity); REPL `teleport` wraps the parsed position
    (`RegionCoordinates::from`), `sl-survey` typed its `ARRIVAL_POSITION`
    const + `grid_coordinates.x()`/`.y()` reads (widened to its `u32` bounds).
    +2 focused unit tests (grid/handle consistency,
    `RegionCoordinates`⇄`Vector` round-trip); lifecycle + `sim_session` suites
    updated; `book/src/content/region.md` updated.
    **`map::Distance` (`draw_distance`/`far`): DONE** in the later batched
    `sl-types` migration (see "Batched `sl-types` migration" below) —
    `sl_types::map::Distance` gained a public `new`/`meters` constructor in
    `sl-types 0.5.0`, and `draw_distance` (`Session`/`Circuit` state,
    `set_draw_distance`, `Command::SetDrawDistance`) now carries it, converted
    to the wire `Far` `f32` at the single `AgentUpdate` encode site.
    **`map::Location` and `map::ZoomLevel`: NOT ADOPTED** (user decision, see
    "considered, not adopted") — no matching LLUDP wire field (no map-zoom
    field exists; `Location`'s integer-coord + mandatory-name shape matches
    neither the float region-local teleport positions nor the grid-coord map
    blocks).
