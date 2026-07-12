---
id: idiomatic-p4-01
title: RegionHandle(u64) with a grid_coordinates() decode — public in types/o
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 4 — Domain ID newtypes (medium-high invasiveness)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

New newtypes in `sl-proto`/`sl-wire` (derive `Copy,Clone,Debug,Eq,Hash`; mirror
`sl-types::Key` ergonomics). Public/caller-facing first:

`RegionHandle(u64)` with a `grid_coordinates()` decode — public in
`types/object.rs:57`, `types/terrain.rs:93`, `types/map.rs:181`, plus the
teleport/`HandoverPending` paths. (Pairs with `map::GridCoordinates` in
Phase 6.) New public `RegionHandle(pub u64)` newtype in
`sl-wire/src/region_handle.rs` (`Copy`/`Eq`/`Hash`/`Ord`/`Default`, mirroring
the `sl-types` key ergonomics) carrying `new`/`get`, the
`from_global`/`global_coordinates` (metres) and `from_grid`/`grid_coordinates`
(region indices) packers/decoders, plus `Display`/`LowerHex`/`UpperHex`.
**Pulled the `map::GridCoordinates` pairing forward from Phase 6 at the user's
request:** `impl From<GridCoordinates> for RegionHandle` (total) and
`impl TryFrom<RegionHandle> for GridCoordinates` (fallible — a new public
`RegionHandleError::GridCoordinateOutOfRange` when a decoded index exceeds the
`u16` a `GridCoordinates` holds). Replaced the raw `u64` region handle on
**every** carrier: `Object`, `TerrainPatch`, `MapRegionInfo`, `NeighborInfo`,
`RegionIdentity`, the six `Event` variants (`TeleportFinished`/
`RegionChanged`/`TimeDilation`/`ObjectRemoved`/`GltfMaterialOverride`/
`SoundTrigger`), `RemoteParcelRequest` (sl-wire), the three `Command` variants
(`Teleport`/`RequestRemoteParcelId`/`RequestMapItems`), the
`ServerEvent::MapItemRequested` field, `SimSession` (+`new`), and the private
`Session` state
(`HandoverPending`, the `regions` map, `teleport_target`).
`MapItem::region_handle()` now returns `RegionHandle`; the public `Session`
methods `teleport_to`/`objects_in_region`/`terrain_patches_in_region`/
`request_map_items` take it. Codec wraps/unwraps at the boundary (decode
`RegionHandle(raw)`, encode `.0`) so the wire bytes are byte-identical. The
legacy free functions `handle_to_grid`/`grid_to_handle`/`handle_to_global`/
`global_to_handle` (public, still raw `u64`) now delegate to the newtype.
Re-exported through `sl-proto`/`sl-client-tokio`/`sl-client-bevy`; downstream
`sl-repl` (`SessionContext.region_handle`, the teleport/map/remote-parcel arg
parsers) and `sl-survey` (unwraps `.0` at the event boundary; its JSON
`RegionRecord` keeps a raw `u64`) updated. +7 unit tests on the newtype
(grid/global round-trips, raw packing, the unknown-`0` sentinel,
`GridCoordinates` round-trip, out-of-range rejection); lifecycle +
`sim_session` round-trip suites updated. NO sl-types touched (consumed
`GridCoordinates` only; `RegionHandle` is a client wire concept living in
`sl-wire`).
