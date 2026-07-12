---
id: test-p2-01
title: Runtime refactor (do before the remaining cases, not a test case):** s
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 2 — Local chat `[both]`
---

Context: [context/test.md](../context/test.md).

**Runtime refactor (do before the remaining cases, not a test case):**
surface session/region state in the **bevy** runtime the idiomatic ECS way so
later cases (and features) can query it at any tick instead of catching a
one-shot event or threading flat accessors. Move the globally-unique login
facts now carried by the fire-once `SlIdentity` event (agent id, session id,
circuit code, seed capability — plus the region handle the
`chat-whisper-shout-range` work added to the tokio `Client`) into a Bevy
**`Resource`**, and put per-region state (region handle, sim address, region
info, neighbours, parcels, …) on **`Component`s of region entities**. The
motivation: `chat-whisper-shout-range` had to bolt one more flat accessor
(`region_handle`) onto `Session`/`Client`; doing the structured split early
means future tests extend a model that already knows where new global vs
per-region facts belong, rather than accreting ad-hoc `SlIdentity` fields and
`Client` accessors. Keep tokio-side parity in mind (the
`agent_id`/`session_id`/`circuit_code`/`seed_capability`/`region_handle`
accessors are the flat precursor this supersedes on the bevy side). Done in
`sl-client-bevy/src/world.rs`: `SlIdentity` is now a `Resource` (agent/
session/circuit/seed + current `region_handle`, read with `Res<SlIdentity>`
and `is_changed`-gated), no longer an event. Per-region state lives on region
entities — `SlRegion { handle, sim }` for the login region and every
`EnableSimulator` neighbour, marked `SlCurrentRegion` / `SlNeighbor`, with
`SlRegionIdentity` / `SlRegionLimits` components and child `SlParcel`
entities. A `maintain_world` system (chained after `drive`) folds the
`SlEvent` stream into this model: spawning the current region on
`CircuitEstablished`, neighbours on `NeighborDiscovered`, moving the current
marker and updating the global handle on `RegionChanged`, and clearing it on
logout/disconnect. `sl-repl-bevy` now reads the resource; covered by unit
tests in `world.rs`. Tokio keeps its flat accessors for parity.
