---
id: viewer-perf-probe-material-respecialize
title: Reflection-probe capture pays a redundant per-view material re-specialize
topic: viewer
status: ready
origin: Follow-up to viewer-perf-pipeline-specialization-stalls (2026-07-31)
refs:
  - viewer-perf-pipeline-specialization-stalls
  - viewer-perf-slfaceext-material-reprep
  - viewer-perf-pbr-shadow-cluster-rez
---

Context: [context/viewer.md](../context/viewer.md).

After the shadow-view fix in [[viewer-perf-pipeline-specialization-stalls]]
removed the periodic `specialize_shadows` spike, `specialize_material_meshes` +
`queue_material_meshes` (~6.5 ms/frame combined in the post-fix 2-min aditi
trace) became the top `Render`-schedule cost. Part of that is the reflection-
probe capture paying a **redundant** per-view material re-specialization every
capture.

## Mechanism (confirmed from the Bevy 0.19 source)

- Material specialization is cached **per view**, not per material or per
  pipeline: `SpecializedMaterialPipelineCache` is
  `HashMap<RetainedViewEntity, HashMap<MainEntity, CachedRenderPipelineId>>`
  (`bevy_pbr/src/material.rs:816`).
- The stored value (`CachedRenderPipelineId`) is a **pure function of the mesh
  vertex layout, the material key, and the view key** (assembled at
  `material.rs:1036-1067`, then resolved through the global
  `SpecializedMeshPipelines` key-cache at `:1093`). It is invalidated only by
  change ticks (`changed_renderables` — a mesh/material actually changing) or a
  view-key change, **never by elapsed frames**. For an unchanged mesh + fixed
  camera it stays valid for the lifetime of the mesh.
- `specialize_material_meshes` **purges the per-view cache for any view that did
  not render this frame** (`material.rs:1107`,
  `retain(|view| all_views.contains(view))`) — the same purge-on-inactive policy
  as `specialize_shadows`. It fits truly ephemeral views (shadow cascades that
  despawn) but not a persistent camera that renders intermittently.
- Our probe capture (`probes.rs`) uses **six face-cameras per rig**, amortized
  to **one active per frame**. So each face view is purged between its turns and
  comes back with an empty cache — re-specializing **every mesh that face
  sees**, from cold, on every capture. The mirror readback test confirms it
  re-derives all of them (it reflects every neighbour, not just changed ones).

The re-derivation is **not** a shader compile (the compiled pipeline is cached
globally by key and hits on the common path). Per cold entity it is: reassemble
the key bits + `material.properties.clone()` (`material.rs:1074`, a real
allocation per visible mesh) + a global-cache lookup + a per-view-cache insert —
`O(meshes visible to the face)` cheap-ish ops per capture. It is pure waste for
unchanged content: the recomputed id is identical to the purged one.

Likely the queueing counterpart `queue_material_meshes` shares the same per-view
purge pattern (verify).

## Why change-detection does **not** fix this

Skipping a capture when a probe's frustum is unchanged only cuts capture
*frequency*, not the per-capture cost — and during active rez the scene is dirty
every frame, so we would capture every frame anyway. Change-detection helps only
the fully-static case, where the cost is already low. It is orthogonal to this.

## Levers

- **Local (in our control), modest:**
  - Fewer meshes per probe view — the `render_reflection_probe_dynamic_content`
    setting (avatars out of local probes) and a shorter probe capture far-clip
    both cut the entity count each face re-specializes.
  - Fewer probe views — the env-only default probe already helps (its faces see
    only sky/water/terrain, a handful of meshes); capping the local-probe pool
    does too.
- **Real fix (upstream), larger:** exempt a persistent-but-amortized capture
  view from the per-view specialization purge so its cache stays warm and only
  genuinely-changed entities re-specialize — exactly what the main view gets.
  That is a `bevy_pbr` change (the `retain` at `material.rs:1107`), so it is a
  candidate for the fork-and-upstream path (see the
  `sl-client-fork-upstream-for-upstream-bugs` memory), not a local edit.

## Measure first

The `specialize_material_meshes` mean rose 0.30 ms → 3.36 ms between the pre-
and post-fix traces. Those two captures are the **same aditi region with
essentially the same content** (only slightly different times), so the rise is a
real consequence of the changes (continuous shadow-free probe cadence), not a
scene artifact. What is still not isolated is the probe-view re-specialize share
versus other post-fix churn — so before investing, run a same-region A/B with
probe capture on vs. off (or in the gallery / a headless readback scene) to pin
the probe's exact contribution.
