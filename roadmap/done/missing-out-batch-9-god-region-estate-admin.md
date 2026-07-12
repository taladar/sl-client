---
id: missing-out-batch-9
title: god region/estate admin
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

**Out batch 9 — god region/estate admin.** `RequestGodlikePowers`,
`EjectUser`, `FreezeUser`, `GodUpdateRegionInfo`, `SimWideDeletes`. All
`NotTrusted` and viewer-sent with the god bit set; gated on the agent holding
god/estate powers.

Implemented as `Session::request_godlike_powers(godlike: bool)` (asks the
simulator to grant/drop god powers; the `Token` is packed nil as the
reference viewer does, and the grant arrives as the already-handled
[`Event::GodlikePowersGranted`]), `Session::eject_user(target: AgentKey,
action: EjectAction)` / `Session::freeze_user(target: AgentKey, action:
FreezeAction)` (remove/ban or freeze/unfreeze an avatar on the agent's land),
`Session::sim_wide_deletes(owner: AgentKey, flags: SimWideDeleteFlags)`
(delete/return an owner's objects region-wide), and
`Session::god_update_region_info(update: &GodRegionUpdate)` (push the
god-tools region parameters). New typed [`EjectAction`] (`Eject` /
`EjectAndBan`) and [`FreezeAction`] (`Freeze` / `Unfreeze`) enums replace the
raw `Flags` integers (each with a `to_wire`), a [`SimWideDeleteFlags`] struct
models the three `SWD_*` bits (`others_land_only` / `always_return_objects` /
`scripted_only`, all-`false` deletes everything), and a [`GodRegionUpdate`]
struct carries the region params with typed [`RegionName`] /
[`GridCoordinates`] (redirect grid) fields — its `region_flags` is the 64-bit
`RegionFlagsExtended`, truncated to the legacy 32-bit `RegionFlags` block on
the wire exactly as the reference viewer does. The remaining payloads reuse
typed `AgentKey` targets. Wired as
`Command::{RequestGodlikePowers, EjectUser, FreezeUser, SimWideDeletes,
GodUpdateRegionInfo}` through the tokio and bevy runtimes, the `command_name`
formatter, and the `request_godlike_powers` / `eject_user` / `freeze_user` /
`sim_wide_deletes` / `god_update_region_info` REPL tokens. Covered by two
pack-the-wire lifecycle tests and five REPL parse tests; all are SL-/god-only
so the round-trips exercise against aditi (OpenSim does not honour the god
bit without estate config).
