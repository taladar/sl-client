---
id: idiomatic-p3-05
title: magic ints → enums:
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 3 — Intent enums replacing bool / magic-int params (low-medium)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

magic ints → enums: map-layer constant (`session.rs:43`); consolidate the
`TELEPORT_FLAGS_*` constants into the existing `TeleportFlags` newtype
(`types/editing.rs:490`). Two changes. **Map-layer flag:** the bare
`const MAP_LAYER_FLAG: u32 = 2` (and its sibling `MapBlockRequest` `0`) became
a new *public* `MapRequestFlags(pub u32)` newtype in `types/map.rs`, modelled
on `TeleportFlags` (named consts `LAYER` = `2` and `RETURN_NULL_SIMS` =
`0x0001_0000`, both matching the reference viewer's
`llworldmapmessage.cpp` `LAYER_FLAG`/`MAP_SIM_RETURN_NULL_SIMS`, plus a
`contains`). The internal `MapBlockRequest`/`MapNameRequest`/`MapItemRequest`/
`MapLayerRequest` senders now write `MapRequestFlags::LAYER` (the
`MapBlockRequest` keeps its `flags: 0` unchanged); the **server-side** surface
is now typed end-to-end: the four `ServerEvent::Map*Requested { flags }`
fields, the `SimSession::send_map_{block,item,layer}_reply` params, and the
`build_map_{block,item,layer}_reply` conversions all take `MapRequestFlags`,
wrapping at the codec boundary (decode `MapRequestFlags(raw)`, encode
`flags.0`) so the agent-block `Flags` word is byte-identical. Re-exported
through `sl-proto` (`types.rs` + `lib.rs`); +3 unit tests (constant values,
raw round-trip, `contains`). **Teleport flags:** the standalone
`const TELEPORT_FLAGS_VIA_LURE: u32 = 4` in `session.rs` was deleted and
folded into the existing `TeleportFlags` newtype — `accept_teleport_lure`
passes `TeleportFlags(TeleportFlags::VIA_LURE)` and
`Circuit::send_teleport_lure_request` now takes a `TeleportFlags` (unwrapping
`.0` at the `TeleportLureRequest` boundary), so the `1 << 2` wire value is
unchanged (lifecycle test still asserts
`teleport_flags == TeleportFlags::VIA_LURE`, i.e. `4`). NO sl-types touched
(both are client wire-protocol concepts). The
REPL/runtimes are unaffected (the map-reply helpers are server-only and
`accept_teleport_lure`'s public signature did not change).
