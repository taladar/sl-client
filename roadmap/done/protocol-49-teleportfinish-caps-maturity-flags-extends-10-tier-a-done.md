---
id: protocol-49
title: TeleportFinish (CAPS) maturity & flags (extends #10, Tier A). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**49. `TeleportFinish` (CAPS) maturity & flags (extends #10, Tier A). ✅ Done.**
Both teleport-finish decode paths (the UDP `TeleportFinish` handler and the CAPS
`teleport_finish_from_llsd`) previously read only `SimIP`/`SimPort`/
`SeedCapability` and dropped `SimAccess` (destination region maturity —
PG/Mature/Adult) and `TeleportFlags` (how/why the teleport happened — lure,
landmark, login, telehub, home, …). Both are now surfaced as a new
`Event::TeleportFinished { region_handle, sim, maturity, flags }`, emitted right
when the teleport finish is decoded (before the circuit handover, which still
proceeds to its eventual `RegionChanged`). The maturity is the typed
`Maturity::from_sim_access`; the flags are a new `TeleportFlags(u32)` bitfield
value type mirroring the reference viewer's `TELEPORT_FLAGS_*`
(`llteleportflags.h`) with named constants and a `contains` helper. The CAPS
decoder reads `SimAccess`/`TeleportFlags` tolerantly (integer or binary LLSD).
Re-exported through both runtimes (`SlSessionEvent` is `sl_proto::Event`, so the
new variant flows automatically) and added to the runtimes'/survey's exhaustive
event matches. Covered by two lifecycle tests (the UDP path asserting
Mature + `VIA_LURE | IS_FLYING`, and the CAPS path extended to assert
Mature + `VIA_LURE | VIA_LANDMARK`). *Note: OpenSim collapses the flags it
sends to `VIA_LOCATION` (+`IS_FLYING`), so the full `VIA_*` set is only
observable on the SL grid; the decode is unit-tested. Test: SL grid (the CAPS
teleport path).*
