---
id: viewer-region-options-terrain
title: Region / Estate floater — terrain tab
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-region-options
blocked_by: [viewer-region-options-debug]
refs: [viewer-pbr-terrain, gridspec-aditi-test-land]
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
terrain-file UI at all; the estate **Bake Terrain** button — **done 2026-09-19**
with [[viewer-terrain-edit-bake-revert]]: `Command::BakeRegionTerrain`
sends `EstateOwnerMessage`/`terrain` `["bake"]` from a button on this
tab (there is no per-area form of it — `ModifyLand` carries no bake
action); and the
PBR-terrain material variant's **elevation/transform sub-tabs**
(`panel_region_terrain_texture_transform.xml`, material_detail_*) on
top of the already-noted PBR material variant (refs viewer-pbr-terrain).

## Write path measured (gridspec-terrain-editing, 2026-10-18)

`Command::SetRegionTerrain`'s four messages were sent as OpenSim's estate
owner by the `terrain-editing` census: the limits and a corner's start height
took, and everybody was told five `RegionInfo`s and four handshakes. Second
Life's answer waits on [[gridspec-aditi-test-land]].

## Done (2026-11-05)

The tab's last reference controls, and the two confirmations it owes:

- **Download RAW terrain… / Upload RAW terrain…** — a save chooser offering
  `terrain.raw` and an open chooser filtered to `.raw`. The platform file
  dialog grew a third mode for this, `FileDialogSelection::Save { file_name }`
  (`rfd`'s `save_file`), the viewer's first "… to disk". A finished download
  is written to the chosen path and announced (`FinishedRawDownload`); an
  upload raises `RawUploadStarted`; a file that cannot be read or written, or
  an empty one, raises the catalogue's `GenericFile…Error`.
- **Only the region's owner or a god** may press those two and **Bake
  Terrain** (the reference's `owner_or_god`): an `OwnerOnlyButton` gate beside
  the managers' one, fed by `AboutRegionState::owner_or_god`, which a god grant
  re-evaluates without a new region record. Bake used to be open to any
  manager.
- **Bake asks first** (`ConfirmBakeTerrain`), and an **Apply** whose corner
  has its low elevation above its high one asks `ConfirmTextureHeights`; its
  "don't ask" turns off the new `RegionCheckTextureHeights` setting (default
  on, `[region]`), as the reference's does.
- `sl-viewer-places::region_terrain_actions` carries all four from the button
  to the wire, and sends **nothing if the agent left the region** while the
  chooser or the question was up — every one of them goes out on the current
  circuit, and a teleport mid-chooser would otherwise upload over the region
  the agent landed in.

One deliberate divergence: the reference tells the grid the picked file's
**full local path** as the transfer name. Here the grid hears only the file's
own name — enough for OpenSim, which picks its heightmap loader from the
extension, without sending the user's directory layout off the machine. The
full path stays local, keyed by that name, until the download arrives.

### The PBR-material variant

- **"PBR Metallic Roughness"** switches the four swatches between textures
  and materials (the reference's `terrain_material_type`); the other set is
  kept, as its `mLastSetTextures` / `mLastSetMaterials` are. The material
  swatches are the toolkit's material picker (`spawn_material_swatch`), with a
  sphere preview.
- **Which kind a region is on** is found the reference's way
  (`getMaterialType`): each detail id is asked for as a texture
  (`DecodedTextures`) and as a material (`MaterialManager`); any decoded
  texture means textures, else any decoded material means materials, else
  both sets show the same ids on textures. `MaterialManager::material_status`
  is new, to tell "loading" from "failed".
- **Transforms** (scale u/v, rotation, offset u/v per material) are a sub-tab
  beside **Elevation**, as the reference's `terrain_tabs` are, whose button
  shows only on materials; they are read from `ModifyRegion` once per region and
  written after the estate messages on Apply, the same transform on all four
  texture infos (`getOverrideLLSD` against the default). Where the region does
  not grant `ModifyRegion` (OpenSim) nothing is posted. The reference hides this
  behind two settings that default off; here it is simply part of the materials
  view.
- **Apply checks first**, as `sendUpdate` does: on textures every texture must
  be RGB or opaque RGBA and at most 2048 px (`InvalidTerrainBitDepth` /
  `InvalidTerrainAlpha` / `InvalidTerrainSize`, from the decoded store); on
  materials every one must have loaded, be single-sided and not blended
  (`InvalidTerrainMaterial*`); the blank material passes unloaded.

The Second Life write path is still [[gridspec-aditi-test-land]]'s.

### Verified

`cargo test --release -p sl-viewer-places -p sl-viewer-platform` — the
`terrain_pbr` tests (the kind follows what decoded; each texture and material
failure is named by slot; transforms round-trip through the overrides; Second
Life's four `undef`s read as identities), the 14 `region_terrain_actions`
tests (a download saved where the chooser said, one nobody asked for left
alone, an upload sent under its own name, an unreadable or empty file reported
not sent, a chooser or a Bake answered after leaving the region sending
nothing, Bake waiting for OK, the elevation check and its "don't ask", the
overrides following the estate messages), the About Region owner-gating test
(a manager greyed, a god and the owner not), and the save dialog's own.

`cargo test --release -p sl-client-bevy-viewer --test e2e_windows terrain` —
three fake-grid stages through the real binary: the owner downloads the RAW to
a chosen file, uploads a changed copy (the grid receives those bytes under
`hills.raw`), bakes after confirming, and applies an inverted range after
confirming (the grid's `textureheights` carries it); a second estate manager
who does not own the region sees the three owner buttons greyed while Apply
is live; and on a region whose slots are materials the tab ticks PBR by
itself, shows the material swatches and transforms, and an Apply puts the
material in `texturedetail` and the edited scale in the grid's overrides.
