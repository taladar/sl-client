---
id: protocol-64
title: Materials service pairing (extends #25, Tier C). DONE
topic: protocol
status: done
origin: ROADMAP.md — Tier F
---

Context: [context/protocol.md](../context/protocol.md).

**64. Materials service pairing (extends #25, Tier C). ✅ DONE.**
`sl-wire/src/material.rs` was partially bidirectional; this added the three
server-side gaps, each the exact inverse of an existing client-side
function. **`build_gltf_material_override`** — inverse of
`parse_gltf_material_override`: writes the notation-LLSD envelope
`{'id':i<local id>,'te':[i<face>…],'od':[<override>…]}` the simulator pushes in
a `GenericStreamingMessage`, emitting each per-face override document's raw
notation bytes verbatim (GLTF documents stay opaque, per the asset-fetch scope).
**`parse_modify_material_params_request`** — inverse of
`build_modify_material_params_request`: parses the `<llsd><array>` of per-face
assignments back into `MaterialOverrideUpdate`s (object id required; `side`
defaults to `-1`; the optional `gltf_json`/`asset_id` surfaced only when
present). **`build_render_materials_response`** — inverse of
`parse_render_materials_response`: re-applies the `*10000` fixed-point scale on
the texture transforms, builds the binary-LLSD array of
`{ "ID": <binary>, "Material": <map> }` entries (`NormMap`/`SpecMap` as `u`
UUIDs so the `as_uuid` reader round-trips them), and zlib-compresses it into the
`{ "Zipped": <binary> }` reply OpenSim's `MaterialsModule` returns — reusing the
module's existing header-less binary-LLSD writer. All three re-exported from
`sl-wire` and `sl-proto`; no runtime wiring (server-side binary sub-codec).
*Test: 3 new round-trip unit tests in `material.rs` (GLTF override
build→parse, `ModifyMaterialParams` build→parse, `RenderMaterials`
response build→parse), alongside the 4 existing client-side tests.*
