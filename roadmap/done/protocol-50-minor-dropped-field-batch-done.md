---
id: protocol-50
title: Minor dropped-field batch. Done
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**50. Minor dropped-field batch. ✅ Done.** Small, individually-low-value drops,
all now surfaced:

- `ScriptTeleportRequest.Options.Flags` — added as
  `ScriptTeleportRequest::flags` (the first option block's `Flags`).
- `TeleportFailed.AlertInfo` — a new `AlertInfo { message, extra_params }`
  (the localizable message *key* + its substitution params) is attached to
  `Event::TeleportFailed` as `alert_info: Option<AlertInfo>` (`None` for the
  timeout path); the plain `Reason` string is still surfaced as `reason`.
- `MapBlockReply` per-block `WaterHeight` — added as
  `MapRegionInfo::water_height`.
- `TimeDilation` (the U16 in the `RegionData` of each object-update
  message, affecting motion dead-reckoning) — tracked per sim, surfaced anew
  `Event::TimeDilation { region_handle, dilation }` (the `0.0`..=`1.0`
  fraction), emitted only when the value *changes* for a region (de-duped on
  the raw `u16` so a steady sim does not re-emit on every update); cleared with
  the rest of a sim's state on `DisableSimulator`/handover/relogin.
- The avatar collision plane (the `LLVector4` read-and-discarded in
  `full_object_motion_inner` and `terse_update`) — added as
  `ObjectMotion::collision_plane` (`Option<[f32; 4]>`; `Some` for avatar
  updates, `None` for ordinary objects).
- The deprecated `JointType`/`JointPivot`/`JointAxisOrAnchor` trio dropped
  by `object_from_full_update` — added as `Object::joint_type` / `joint_pivot` /
  `joint_axis_or_anchor` (zeroed for compressed updates, which do not carry it).
- Convenience accessors for the packed bytes (no prior loss; every bit was
  retained, but the caller had to mask them): `TextureFace::bumpmap` /
  `fullbright` / `shininess` / `media_enabled` / `tex_gen` (mirroring the
  viewer's `getBumpmap()`/`getFullbright()`/`getShiny()`/…), and
  `Object::name_values` / `name_value_data` which parse the packed
  newline-separated `name_value` string into structured `NameValue` entries
  (faithful to the viewer's `LLNameValue` parser: optional `class`/`sendto`
  keywords, defaulting to `RW`/`S`).

Covered by three new `types.rs` unit tests (the bump/shiny + media-flag
unpacking and the `name_value` parser incl. defaults + blank-line skipping) and
four new `lifecycle.rs` tests (the `ScriptTeleport` option flags, the
`TeleportFailed` `AlertInfo`, the `TimeDilation` emit-on-change + de-dup, the
joint trio, and the terse avatar collision-plane vs. plain-prim `None`), plus a
`MapBlockReply` water-height assertion folded into the existing map-block test.
*Live-verified against the local OpenSim: one login showed the region's
`TimeDilation` (1.0, emitted once thanks to the de-dup), the test avatar's
collision plane (`[0, 0, 1, 30.249]` — a +Z foot plane at the standing height),
and its `name_value` pairs parsed into `FirstName`/`LastName`/`Title` entries.*
*Test: local OpenSim.*
