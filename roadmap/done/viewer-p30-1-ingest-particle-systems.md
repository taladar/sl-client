---
id: viewer-p30-1
title: Ingest particle systems
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 30 — Particles
---

Context: [context/viewer.md](../context/viewer.md).

**P30.1. Ingest particle systems.** Parse a prim's `LLPartSysData` (the
particle-system block on ObjectUpdate / generic data): flags, pattern,
burst / age params, per-particle colour / scale / velocity ranges, target.
Keep it Bevy-free where practical. Reference: `LLPartSysData` / `LLPartData`.
The Bevy-free wire decode already existed in `sl-proto`
(`decode_particle_system` → `ParticleSystem`, both the legacy 86-byte and the
modern size-prefixed glow/blend-extended forms, on `Object::particles`), so
the net-new work was the **viewer-side ingest**, mirroring the P25.1 light
ingest exactly: a new `sl-client-bevy-viewer::particles` module with an
`ObjectParticleSystem` component carrying the decoded system, a
`particles_from_object` lift, and an `apply_particles` reconcile that
`apply_object` calls on both the spawn and update paths (beside `apply_light`)
so a source toggled on/off/retuned between updates is tracked. The lift
honours the reference viewer's `LLPartSysData::isNullPS` semantics — an empty
`PSBlock` (sl-proto already yields `None`) **and** a zero-CRC "null" system
(the `llParticleSystem([])` stop sentinel) both clear the component rather
than attach a dead emitter, matching `LLViewerPartSourceScript::unpackPSS`
returning `NULL`. The component rides the **object entity** (its world
transform), the way `LLViewerPartSourceScript` tracks its source object — so
the emitter follows the prim, ready for the P30.2
simulation + billboard render. `ParticleSystem` / `particle_pattern` were
already re-exported from `sl-client-bevy`, so there was no re-export gap.
**Live-verified on aditi:** 9 in-view particle sources ingested with varied
patterns (`0x01` DROP / `0x02` EXPLODE / `0x08` ANGLE_CONE), flags, burst
rates and real texture ids, over 2134 tracked objects, no null-system false
positives; clean build/clippy/tests and no OpenSim login regression (OpenSim's
Default Region carries no particle content, so the source ingest is exercised
on real SL).
