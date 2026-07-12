---
id: missing-out-batch-5
title: land & parcel
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

**Out batch 5 — land & parcel.** `ModifyLand` (terraform), `UndoLand` (undo
terraform; `RedoLand` is absent from the template),
`ParcelPropertiesRequestByID` (fetch a parcel by local id),
`ParcelSetOtherCleanTime` (parcel object
auto-return time).

Implemented as `Session::modify_land` (taking a typed [`LandEdit`]),
`Session::undo_land`, `Session::request_parcel_properties_by_id`, and
`Session::set_parcel_other_clean_time`. `ModifyLand` uses a new typed
`land` module instead of raw wire fields: [`LandBrushAction`] (the `E_LAND_*`
action enum — level/raise/lower/smooth/noise/revert), [`LandBrushSize`]
(small/medium/large, carrying both the LL metre radius sent in
`ModifyBlockExtended` and the deprecated legacy index byte), and
[`TerraformArea`] (the region-local ground rectangle); the optional target
parcel is a [`RegionLocalParcelId`] (`-1` when absent, as the viewer sends for
free brushing). The two parcel ops key off a [`ScopedParcelId`] like the rest
of the parcel API, and `set_parcel_other_clean_time` takes a
[`std::time::Duration`] rounded down to whole minutes (the wire `S32`).
`request_parcel_properties_by_id` fetches by local id where the existing
`request_parcel_properties` fetches by metre rectangle; both surface the reply
as `Event::ParcelProperties`. Wired as `Command::{ModifyLand, UndoLand,
RequestParcelPropertiesById, SetParcelOtherCleanTime}` through the tokio and
bevy runtimes, the `command_name` formatter, and the matching REPL tokens
(`modify_land` / `undo_land` / `request_parcel_properties_by_id` /
`set_parcel_other_clean_time`, with new `parse_land_brush_action` /
`parse_land_brush_size` helpers). Covered by one pack-the-wire lifecycle test,
three `land`-module unit tests, and four REPL parse tests. Live-testable on
OpenSim (terraform/undo and parcel auto-return all work against the local
grid).
