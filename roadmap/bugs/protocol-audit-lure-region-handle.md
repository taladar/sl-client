---
id: protocol-audit-lure-region-handle
title: parse_lure_region_handle reads 8 bytes of any UUID and calls it a region handle
topic: protocol
status: bugs
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
