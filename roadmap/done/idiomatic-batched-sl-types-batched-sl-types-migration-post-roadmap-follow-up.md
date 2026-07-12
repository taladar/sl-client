---
id: idiomatic-batched-sl-types
title: Batched sl-types migration (post-roadmap follow-up)
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Batched `sl-types` migration (post-roadmap follow-up)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

The value types created **client-local in `sl-proto`** during this pass (under
the standing "new types go local first, batch-migrate later to avoid version
churn" rule) were moved into the shared `sl-types` crate in one release
(`sl-types 0.5.0`), and `sl-proto` now consumes them:

- Moved to `sl-types`: `LindenBalance`/`NegativeBalanceError` (→ `money.rs`),
  `LandArea` (→ `map.rs`), `EventId` + `ClassifiedCategory` (→ `search.rs`),
  `GroupRoleKey`, `MeshKey`, and the three union keys `AgentOrObjectKey`/
  `InventoryItemOrFolderKey`/`SculptOrMeshKey` (→ `key.rs`). Each gained the
  conventions of its sibling (chumsky parsers; serde on `LindenBalance`; the
  single-UUID keys reshaped to wrap `Key`).
- `sl_types::map::Distance` gained a public `new(meters)`/`meters()`
  constructor, and `draw_distance` adopted it (the wire `Far` `f32` conversion
  lives at the single `AgentUpdate` encode site in `sl-proto`).
- Removed the misfit `sl_types::key::EventKey` (the SL event id is a numeric
  `U32`, not a UUID — verified against the viewer); `ViewerUri::EventAbout` now
  carries `EventId`.
- `sl-proto` keeps only the `sl_wire::WireError` codec boundary helpers
  (`land_area_*`/`linden_*`); it re-exports the moved types via
  `pub use sl_types::…` so the flat `sl_proto::…` surface and downstream crates
  are unchanged. `sl-proto`'s `serde` dependency was dropped (its only user,
  `LandArea`, left).

## Second batch + `GridRectangle` widening (`sl-types 0.6.0`)

The remaining general value/geometry types created client-local in the Phase 7
passes were migrated in a second release (`sl-types 0.6.0`), alongside the
deferred `GridRectangle` widening. `sl-wire`/`sl-proto` now consume them and
re-export through the flat surface + runtimes, so downstream paths are
unchanged.

- **`GridRectangle` widened to `u32`** (the Phase 7D "STILL NOT converted"
  TODO): `GridCoordinates`/`GridCoordinateOffset`/`GridRectangle` and the
  `GridRectangleLike` trait now work in `u32`, so the SL whole-grid map layer's
  bounds (which can exceed `u16::MAX`) no longer truncate. `MapLayer`
  (`sl-proto`) dropped its `left`/`right`/`top`/`bottom: u32` for one
  `rect: GridRectangle` (codec byte-identical: wire `(left, bottom)` =
  lower-left, `(right, top)` = upper-right). The widening rippled through the
  `sl-map-tools` consumers (sl-glw, sl-map-apis, sl-map-cli, sl-map-web); the
  redb region caches moved to `*_v2` tables (old `u16` tables dropped on open).
  `RegionHandle → GridCoordinates` became infallible (`TryFrom` → `From`;
  `RegionHandleError` removed). `MapBlockReply` region indices stay `u16` on the
  wire (narrowed at that boundary).
- **`pps_hud_config`** (`sl-map-tools` `GridRectangleLike`) is now expressed
  through `GlobalCoordinates::from_grid_corner` (the migrated global-metre
  type).
- Moved to `sl-types`: `Direction`, `GlobalCoordinates`, `Scale`,
  `TeleportFlags` (→ `map.rs`); `PickKey`, `GroupNoticeKey` (→ `key.rs`,
  reshaped to wrap `Key`); `ScriptPermissions` (→ `lsl.rs`);
  `Color`/`ColorAlpha`/`Glow`/`CloudPosDensity` (→ new `environment.rs`);
  `FriendRights` (→ new `friend.rs`); `ExperienceProperties` + its `PROPERTY_*`
  consts (→ new `experience.rs`); `ParcelAccessFlags`/`ParcelReturnType` (→ new
  `parcel.rs`). The float value types gained `serde` (matching the `map.rs`
  coordinate siblings).
- Kept client-local (codec/wire/correlation, not general concepts): `narrow`
  (sl-wire LLSD helper), `SEARCH_PAGE_SIZE`, the LLSD `{color,scale,…}_*_llsd`
  codec helpers, and — by explicit decision — `ProposalVoteId`/
  `ProposalCandidateId` (the defunct group-voting feature), `SoundFlags`,
  `DirFindFlags`, `LandSearchType`, `MapRequestFlags`, plus the
  bookkeeping/correlation/region-local ids.
