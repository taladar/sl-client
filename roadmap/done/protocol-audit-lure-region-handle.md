---
id: protocol-audit-lure-region-handle
title: parse_lure_region_handle reads 8 bytes of any UUID and calls it a region handle
topic: protocol
status: done
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/protocol.md](../context/protocol.md).

`sl-proto/src/session/conversions.rs:165` is
`RegionHandle(sl_wire::Reader::new(lure_id.as_bytes()).u64().unwrap_or(0))` —
it reads the first eight bytes of **any** UUID. Its own doc claims it "returns
`0` for an id that is not a fake parcel id (e.g. a Second Life lure id)", which
it never does.

The garbage handle becomes `TeleportPhase::Requested { target }`
(`methods.rs:7333`), which `handle_caps_event` uses as the `TeleportFinish`
fallback, feeding `begin_handover`'s `source.is_adjacent_to(region_handle)`
adjacency test (`methods.rs:1227`). So **`world_reset` can be decided from
noise** — and `world_reset` is what purges the scene.

Fix: distinguish a real fake-parcel lure id from an opaque one (the reference
recognises the encoding), and return `None` rather than a fabricated handle.
`parse_lure_region_handle` is pure and has no test — see
[[protocol-audit-conversions-test-coverage]].

## Fixed (2026-08-27)

`parse_lure_region_handle` now returns `Option<RegionHandle>` and delegates to
`sl_wire::FakeParcelId::parse` — the codec that already knows the layout and
already applies OpenSim's own plausibility checks (the handle's low bytes zero,
the coordinates below 16 km, a zero tail), so an opaque Second Life lure id
yields `None` instead of eight bytes of UUID.

`TeleportPhase::Requested::target` became `Option<RegionHandle>` in the same
pass, because `None` had nowhere to go otherwise. Two of its three other
writers — `enter_remote_teleport` and `teleport_via_landmark` — were already
spelling "unknown" as `RegionHandle(0)`, which the CAPS `TeleportFinish` arm
could not tell from a genuine handle for the region at the grid origin. Only
`teleport_to`, which is given a handle, now carries `Some`. The
`RegionHandle(0)` sentinel survives exactly once, at the end of the
`TeleportFinish` fallback chain where an `Event::TeleportFinished` has to name
something; the destination's own `AgentMovementComplete` overrides it when the
handover commits.

The consequence the task cared about follows: `begin_handover`'s adjacency test
is now fed either a real handle or `RegionHandle(0)`, which is adjacent to
nothing, so a lure to an unknown region resets the world unless the destination
is already a child circuit. That is the conservative answer — keeping a scene
whose local-id space belongs to another region is the worse failure — and it is
now a decision rather than a coin flip on eight random bytes.

Two tests pin the parse: an OpenSim fake parcel id round-trips to its region
handle, a random UUID and the nil UUID yield `None`.
