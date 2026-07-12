---
id: protocol-25
title: PBR materials / GLTF (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**25. PBR materials / GLTF (done) ✅ — `GenericStreamingMessage` GLTF override,
`RenderMaterials` / `ModifyMaterialParams` CAPS, material/GLTF assets · 8 pts.**
The surface-material protocol layered on objects (#16) and the asset pipeline
(#19/#23). Per the asset-fetch scope (as with #19/#23), the GLTF document itself
is **not** parsed — material assets are fetched/uploaded as raw bytes and the
per-face GLTF *overrides* are surfaced as their raw notation-LLSD documents.
Implemented across both kinds of Second Life surface material (referenced per
face by a `TextureEntry`'s 16-byte material id):

- **Legacy materials (normal/specular) — `RenderMaterials` CAPS (OpenSim's
  path).** A new `sl-wire::material` module ports the cap's *zlib-compressed
  binary-LLSD* codec: a minimal header-less binary-LLSD reader/writer (built on
  the existing `Reader` + big-endian helpers) plus `miniz_oxide` zlib.
  `build_render_materials_request` zips a binary-LLSD array of the wanted
  material ids into the `{ "Zipped": … }` POST body OpenSim's `MaterialsModule`
  expects, and `parse_render_materials_response` unzips the reply into
  `RenderMaterialEntry { material_id, LegacyMaterial }` (normal/specular maps,
  the `*10000` fixed-point texture transforms un-scaled, spec colour/exponent,
  env intensity, diffuse-alpha mode/cutoff — cross-checked against OpenSim's
  `SOPMaterial`/`FaceMaterial.toOSD`). Driven by the runtimes'
  `RequestRenderMaterials` command → `Event::RenderMaterials`.
- **Modern GLTF (PBR) overrides — `GenericStreamingMessage` (receive) +
  `ModifyMaterialParams` (set).** Incoming material overrides arrive as a
  `GenericStreamingMessage` (method `0x4175`) carrying *notation* LLSD; a small
  notation-envelope scanner (`parse_gltf_material_override`) decodes the object
  local id and affected face indices and surfaces each per-face override as its
  raw, **undecoded** notation document — `Event::GltfMaterialOverride
  { region_handle, local_id, faces, overrides }`, decoded on the root *and*
  neighbour (child) circuits. Setting GLTF materials on object faces uses the
  `ModifyMaterialParams` cap (`build_modify_material_params_request`, an
  array of `{ object_id, side, gltf_json?, asset_id? }` with the JSON passed
  through opaque); the `{ success, message }` reply →
  `Event::MaterialParamsResult`. Driven by the runtimes' `ModifyMaterialParams`
  command.
- **Material / GLTF assets — fetch + upload over the existing pipeline.**
  `AssetType` gains `Material`/`Gltf`/`GltfBin` query keys and upload-cap names
  (`material_id`; `caps_asset_name`/`update_item_cap` →
  `UpdateMaterialAgentInventory`), plus an `InventoryType::Material`, so a
  material asset fetches (UDP `TransferRequest` by code, or the CAPS asset cap)
  and uploads (`NewFileAgentInventory` / `UpdateMaterialAgentInventory`) through
  the #19/#23 commands with no new surface. The three new caps
  (`RenderMaterials`, `ModifyMaterialParams`, `UpdateMaterialAgentInventory`)
  join the seed.

All wired as `Command`/`SlCommand` variants through both runtimes (the CAPS
POSTs run on a background task/thread; the binary `RenderMaterials` reply is
decoded off-thread into the event, the others route their LLSD reply through
`handle_caps_event`). New value types `GltfMaterialOverride`, `LegacyMaterial`,
`RenderMaterialEntry`, `MaterialOverrideUpdate` re-exported through both.
Covered by four `sl-wire` unit tests (binary-LLSD round-trip, the
`RenderMaterials` zip round-trip + response decode, the GLTF override envelope,
the `ModifyMaterialParams` body) and two `lifecycle.rs` tests (the
`GenericStreamingMessage` override → `Event::GltfMaterialOverride`, and the
`ModifyMaterialParams` reply → `Event::MaterialParamsResult`). *Live-checked
against the local OpenSim via the new `pbr_materials` tokio example: a clean
login → throttle → scene-stream → logout with the three material caps seeded and
no protocol error; the example harvests per-face material ids from the scene's
texture entries and POSTs a `RenderMaterials` fetch for them (the empty test
region returns none — stock OpenSim only serves a material once a viewer has set
one). The GLTF override + `ModifyMaterialParams` paths are SL-only (stock
OpenSim sends no overrides, nor serves the cap), so are
unit/lifecycle-tested only, as with #20's server-side bake and #23's `Update*`
caps. **All `ExtraParams` object sub-blocks are now decoded** (they are small
SL-specific structs, not standardised formats): a new `sl-proto::extra_params`
module walks the `ObjectUpdate` `ExtraParams` container (`u8` count, then per
entry a little-endian `u16` type / `u32` size / payload) into
`Object.extra: ObjectExtraParams` — `flexible` (`0x10`), `light` (`0x20`),
`sculpt`/mesh (`0x30`/`0x60`), `light_image` (`0x40`), `extended_mesh` (`0x70`),
per-face GLTF `render_material` refs (`0x80`, the `(face, material id)` list
tying #16 objects to the material assets here), and `reflection_probe` (`0x90`),
each mirroring its `unpack` in the viewer's `llprimitive.cpp`. A reflection
probe's content is a viewer-rendered cubemap, so there is nothing to fetch. The
GLTF material *document decode* (glTF 2.0) and J2C pixel decode remain out of
scope (those bytes/notation are surfaced raw).* *Test: a recent SL grid for the
GLTF paths; local OpenSim for the seed/caps + `RenderMaterials` round-trip.*

## Tier D — specialized (needs more than local OpenSim)
