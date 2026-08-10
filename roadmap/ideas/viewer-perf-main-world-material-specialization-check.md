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

## Levers (measure first, then pick)

- **Gate the check for static-material types.** For materials whose
  entity set and content are effectively fixed after setup (sky, stars,
  sun disc, clouds — the `sky.rs`/`water.rs`-style world-root builders),
  the specialization set only changes at (re)build. A run
  condition that skips `check_entities_needing_specialization<M>` unless a
  material/entity of that type actually changed would drop most of the
  8.4 ms. Confirm Bevy 0.19 lets us add a run condition to the
  bevy-registered system (or replace the registration).
- **Collapse material types.** Several of these could share one material +
  a mode uniform (the sky-family shaders already share a gbuffer clamp
  path). Fewer registered `M` types = fewer per-frame check systems.
  Larger change, verify it does not regress the render-world
  specialization.
- **Upstream:** if the per-type fixed overhead is inherent to Bevy 0.19's
  design, a cheaper "any entity of this material changed" gate is a
  `bevy_pbr` improvement (the fork-and-upstream path).

## Measure

A/B on the same aditi region (or headlessly in the gallery, which
instantiates the sky/star/sun materials): steady-state PostUpdate median
and the summed `check_entities_needing_specialization<*>` self-time before
and after, window visible. Target: recover most of the 8.4 ms from
PostUpdate's 22 ms — the biggest single average-frame lever found in the
2026-08-10 capture.

Confidence: high on the measurement (15 systems, 8.44 ms summed, isolated
zones); medium on the fix (needs the Bevy 0.19 run-condition check).
