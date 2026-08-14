---
id: viewer-mesh-lod-factor-preference
title: Expose the mesh/prim LOD factor as a preference (the reference "Mesh Detail" slider)
topic: viewer
status: done
origin: user noted full-LOD range feels short vs a dialed-up reference viewer (2026-08-11)
---

Context: [context/viewer.md](../context/viewer.md).

The viewer hardcodes `DEFAULT_LOD_FACTOR = 1.0` (`sl-proto/src/mesh_lod.rs`) at
the three `for_distance` call sites in `render_priority.rs` (mesh, prim, tree
tiers). That is a faithful match for the reference viewer's *default*
`RenderVolumeLODFactor`, but the reference exposes it as the **"Mesh Detail:
Objects"** slider (and the graphics presets set it — Ultra and power-user debug
push it to ~4×), which is why full LOD reaches much farther on a dialed-up
reference viewer. We have no equivalent knob, so we are pinned at the stock
range.

## Task

Add a persistent **LOD factor** graphics preference (default `1.0`, range
roughly `1.0`–`4.0`+ to mirror the reference slider), read by
`drive_render_priority` and passed as `lod_factor` to `MeshLod::for_distance` /
`PrimLod::for_distance` / the tree-tier selection instead of the hardcoded
`DEFAULT_LOD_FACTOR`. A larger value selects finer geometry at a given distance,
pushing full LOD farther out.

As a GUI-driven persistent setting it belongs in the preferences store + a
graphics-preferences control, not a CLI flag (the CLI is only for
non-GUI/startup options). Consider whether it should be part of, or feed from,
the graphics presets work ([[viewer-graphics-presets]]) — the reference sets
this factor per preset. Changing it must re-drive the LOD pass so on-screen
geometry re-ranks immediately. The recently-fixed warm/shared-mesh LOD path
([[viewer-mesh-stuck-low-lod-warm-cache]]) means every instance will now track
the new factor, not just cold-built ones.

## Done

`RenderVolumeLODFactor` is a persisted `[render]` setting (default 1.0
= the old hardcoded behaviour, range 1–4), registered and consumed in
`render_priority.rs`: `drive_render_priority` reads it each 0.25 s
pass and feeds it to the three `for_distance` tier selections (mesh,
prim, tree — `tree_tier_for_size` gained the parameter), so a change
re-ranks all on-screen geometry within a quarter second in both
directions (the warm/shared-mesh path tracks it too). Surfaced as the
"Mesh detail (LOD factor)" slider in the preferences graphics tab
([[viewer-preferences-graphics-tab]], whose quality tiers also set it)
and in the Quick Preferences panel. Verified live on the local grid
with a fixed-camera A/B: a small ball prim at ~50 m renders as a
coarse hexagon at factor 1 and a smooth sphere at factor 4, fresh
builds in both directions.
