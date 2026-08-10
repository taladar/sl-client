---
id: viewer-perf-main-world-material-specialization-check
title: Main-world per-material specialization checks cost ~8.4 ms/frame
topic: viewer
status: ideas
origin: Tracy full-session aditi capture (2026-08-10)
refs:
  - viewer-perf-steady-state-46fps-ceiling
  - viewer-perf-probe-material-respecialize
  - viewer-perf-pipeline-specialization-stalls
---

Context: [context/viewer.md](../context/viewer.md).

## ⚠️ Correction (2026-08-10): real wall-clock is ~2 ms, not 8.44 ms

The 8.44 ms below is the **sum** of the 15 systems' per-call times — but a
`-u` unwrap over a steady window shows they all **start within ~0.4 ms of
each other and finish inside a ~2 ms span**, running **concurrently** across
worker threads (each does a `par_iter`). Their real contribution to
PostUpdate's wall-clock is therefore **~2 ms**, not 8.44 ms — the classic
summed-self-time-of-parallel-work over-count (see the profiling notes). The
system is also **already change-filtered** (`Query<Entity, Or<(Changed<Mesh3d>,
AssetChanged<Mesh3d>, Changed<MeshMaterial3d<M>>, AssetChanged<..>)>>`), so a
static material's per-frame cost is mostly `par_iter` **dispatch overhead**,
not an entity scan. A run-condition gate would still only reclaim ~2 ms of
*summed* worker time (little wall-clock), so **this is now a low-priority
lever.** The real single-threaded PostUpdate cost is
`check_dir_light_mesh_visibility` (~5–6 ms serial) — see
[[viewer-perf-pbr-shadow-cluster-rez]]. Kept below for the record.

Distinct from the **render-world** `specialize_material_meshes` /
`queue_material_meshes` cost (that is
[[viewer-perf-probe-material-respecialize]]): this is the **main-world**
`bevy_pbr::material::check_entities_needing_specialization<M>` system that
Bevy 0.19 registers **once per material type** in `PostUpdate`. The aditi
steady-state capture (visible phase, `t ≥ 140 s`) measures the family at
**8.44 ms/frame summed across 15 material types**, the single largest
PostUpdate cluster:

| Material `M` | ms/frame |
| --- | --- |
| `stars::StarMaterial` | 1.38 |
| `water::WaterMaterial` | 1.20 |
| `terrain::TerrainMaterial` | 1.19 |
| `ExtendedMaterial<StandardMaterial, SlFaceExt>` | 1.07 |
| `clouds::CloudMaterial` | 0.92 |
| `name_tag_billboard::NameTagMaterial` | 0.76 |
| `sun_disc::SunDiscMaterial` | 0.68 |
| `parcel_borders::ParcelBorderMaterial` | 0.66 |
| `sky::SkyMaterial` | 0.55 |
| (6 more, small) | ~1.3 total |

They run on main-app worker threads, so they parallelise across the
executor — but there are **15 of them**, each takes a world query pass
every frame, and the ones that dominate are attached to **singletons or
slow-changing content**: sky, stars, sun disc, clouds are one entity each;
water, terrain, parcel borders change rarely. A per-frame full scan to
discover "does any entity of this material need specialization" is almost
pure waste for those.

## Why the cost is suspicious for singletons

`StarMaterial` (one star-dome entity) costing 1.38 ms/frame is not a query
over many entities — it is dominated by fixed per-system overhead
(scheduling, the `EntitiesNeedingSpecialization<M>` resource churn, the
change-tick scan) that scales with the **number of material types
registered**, not the number of entities. We have registered a lot of
bespoke sky/environment materials, so we pay this 15×.

## Feasibility (checked 2026-08-10) — gating is NOT easy

The obvious fix (a run-condition that skips the check for static materials)
is **not readily available**: the system is registered by Bevy's
`MaterialPlugin<M>` (`bevy_pbr/src/material.rs:384`, added directly with
`.after(AssetEventSystems)`, not in a named set), and Bevy 0.19 offers no
clean way to bolt a `run_if` onto another plugin's already-scheduled system.
So the realistic levers are both **bigger than a one-liner**:

- **Reduce the number of material *types* (most promising).** We register
  15; the cheap-but-numerous ones are our env singletons (`SkyMaterial`,
  `StarMaterial`, `SunDiscMaterial`, `CloudMaterial`). Collapsing those into
  one `EnvironmentMaterial` (mode via a uniform; the sky-family shaders
  already share a gbuffer-clamp path) removes ~3 systems' worth from the
  chain. A rendering refactor — verify it does not regress the render-world
  specialization or the individual shaders.
- **Fork/patch Bevy** to gate or serialise the check for change-free
  materials (the `[patch.crates-io]` path) — heavier, upstreamable.

## A/B first — is it even on the critical path?

Before either refactor, **confirm the ~2 ms is reclaimable**: the material
systems sit in the PostUpdate sequential chain (before
`check_visibility` → `check_dir_light_mesh_visibility`), so they are
*plausibly* on the critical path, but they might overlap otherwise-idle
workers, in which case removing them saves ~0 wall-clock. Cheap experiment:
temporarily stop registering the 4 env-material types (render them stubbed)
and measure whether **PostUpdate median actually drops ~2 ms**. Only invest
in the type-collapse refactor if it does.

Priority: **secondary** to [[viewer-perf-pbr-shadow-cluster-rez]] (the
`check_dir_light_mesh_visibility` ~5–6 ms serial cost, unambiguously on the
critical path). Revisit after the [[viewer-r26]] mesh errors are fixed.
