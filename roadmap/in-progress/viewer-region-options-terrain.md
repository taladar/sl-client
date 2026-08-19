---
id: viewer-region-options-terrain
title: Region / Estate floater — terrain tab
topic: viewer
status: in-progress
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-region-options
blocked_by: [viewer-region-options-debug]
---

In progress (2026-07-28): the About Region **Terrain** tab is built and its
write path is wired — new `Command::SetRegionTerrain` sends the reference's four
`EstateOwnerMessage`s (`setregionterrain` + `texturedetail` + `textureheights` +
`texturecommit`) from the editable water / raise / lower fields, the four detail
texture swatches (reusable `spawn_texture_swatch` widget), and the per-corner
elevation fields. **Not yet closed:** the write path is unverified on a live
grid (needs an estate-owner login — the local OpenSim standalone or an owned SL
region), and the PBR-terrain material variant is not handled.

Context: [context/viewer.md](../context/viewer.md).

The Region / Estate floater **terrain** tab: the four terrain detail textures
and their elevation ranges (low / high per corner), water height, terrain raise
/ lower limits. Adds a tab to the floater shell from
[[viewer-region-options-debug]]; the terrain-texture and elevation edits overlap
with the terrain-editing brush work.

Reference (Firestorm, read-only): `llfloaterregioninfo`, `llpanelregion*`
(terrain panel).

Builds on: `protocol-14` estate / region.

Deps: [[viewer-region-options-debug]].

## Parity-audit addendum (2026-08-19)

Parity-audit additions to this tab: the **Download RAW / Upload RAW**
buttons — the Xfer protocol side is done
(`test-terrain-raw-transfer-download` / `-upload`) but
`sl-client-bevy-viewer/src/about_region.rs` has no raw
terrain-file UI at all; the estate **Bake Terrain** button (set the
current heightmap as the region's baked revert baseline — distinct from
the area-brush bake of viewer-terrain-edit-bake-revert); and the
PBR-terrain material variant's **elevation/transform sub-tabs**
(`panel_region_terrain_texture_transform.xml`, material_detail_*) on
top of the already-noted PBR material variant (refs viewer-pbr-terrain).
