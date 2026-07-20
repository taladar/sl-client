---
id: protocol-16
title: Object/scene graph (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**16. Object/scene graph (done) ✅ — `ObjectUpdate`, `ObjectUpdateCompressed`,
`ObjectUpdateCached`, `ImprovedTerseObjectUpdate`, `KillObject`,
`ObjectProperties`, `RequestMultipleObjects` · 13 pts.** The largest single
piece — "seeing the world." Implemented an object cache keyed by source
simulator then region-local id (local ids are only unique within a sim), so the
current region *and* every neighbouring region streamed over a child circuit are
cached side by side; a sim's objects are dropped when its circuit goes away
(`DisableSimulator`, teleport handover, relogin, inactivity). All four update
decoders and an `ObjectAdded`/`Updated`/`Removed`/`Properties` event stream. New
value types `Object` (identity, parent, pcode, scale, `ObjectMotion`, owner,
sound, floating text, name-values, media URL, raw texture-entry/extra-params,
optional merged `ObjectProperties`), `ObjectMotion`, `ObjectProperties`, and a
`pcode` constants module. The three packed blobs are decoded by hand from the
generated `Vec<u8>` fields: **full** `ObjectUpdate` (60/76-byte motion blob:
pos/vel/acc full-f32 + packed-quat rotation + angvel, with the avatar
collision-plane prefix), **terse** `ImprovedTerseObjectUpdate` (local id + state

- full-f32 position + 16-bit quantized velocity ±128 / acceleration ±64 /
rotation ±1 (4 explicit comps) / angular velocity ±64, with LL's `U16_to_F32`
snap-to-zero), and **compressed** `ObjectUpdateCompressed` (the
`CompressedFlags` bitfield gating angvel / parent / tree / scratchpad /
floating-text / media-url; the reliable fixed prefix + text/media-url are
decoded, the trailing length-prefix-less particle/extra-param/ shape/texture
fields are left raw). Cache-miss handling: `ObjectUpdateCached` entries and
terse updates for unknown ids trigger a `RequestMultipleObjects` (full) fetch;
`KillObject` removes and emits `ObjectRemoved`; `ObjectProperties` (from
selecting via `ObjectSelect`) surfaces and merges into the cached object.
**Neighbour-region streaming:** object messages are handled on the child
circuits too (not just the root), keyed per sim; child circuits are driven with
the bandwidth throttle *and* periodic `AgentUpdate`s (camera/interest). The key
piece is the **per-neighbour seed-capability POST**: OpenSim gates a region's
entire initial scene push (`ScenePresence.SendInitialData`, which sends objects
to child agents too) on `Caps.CapsFlags.SentSeeds` — i.e. the viewer must POST
that region's seed cap. So `EstablishAgentCommunication` now surfaces an
`Event::NeighborSeed { sim, seed_capability }` and both runtimes POST it (the
same seed request the root does), which unlocks the neighbour's object stream
onto the child circuit. A sim's objects are dropped (with `ObjectRemoved`) when
its circuit goes away. Public API: `Session::objects()` (all regions) /
`objects_in_region(handle)` / `object(local_id)` (current region),
`request_objects`, `request_object_properties` (select) / `deselect_objects`,
wired as
`Command` /
`SlCommand::{RequestObjects, RequestObjectProperties, DeselectObjects}`
through both runtimes. Decoders + neighbour streaming covered by seven
`sl-proto` lifecycle tests (full/terse/cached/compressed/kill/properties + a
child-circuit neighbour test). *Live-verified against the local 2×2 OpenSim:
logged into Default with an OAR-loaded prim in the East neighbour, the client
received the avatar (pcode 47, collision-plane variant) in Default **and the
East prim (pcode 9) under East's region handle** over the child circuit —
confirming end-to-end neighbour streaming. Also verified the in-region full
`ObjectUpdate` decode and an `ObjectSelect` → `ObjectProperties` round-trip
(name + owner, merged into the cache). Stock OpenSim sends full `ObjectUpdate`s,
so the compressed/cached/terse- miss decoders (heavier on the SL grid) are
unit-tested only.* Even before a renderer this enables a scene auditor or
proximity bot; its full payoff needs #18–#20. *Test: local OpenSim — rez prims
via console/viewer (or load an OAR) to populate the scene; load into a neighbour
to exercise child-circuit streaming.*
