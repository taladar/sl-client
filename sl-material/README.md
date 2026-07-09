# sl-material

A pure (sans-I/O) decoder for Second Life / OpenSim **GLTF 2.0 (PBR) render
materials** — the `AT_MATERIAL` asset a face references through its
`LLRenderMaterialParams` entry (`sl_proto::RenderMaterialRef`). It is the
material counterpart of `sl-mesh` / `sl-texture`: those crates decode a mesh /
texture asset; this one decodes a material asset into a renderer-agnostic
[`GltfMaterial`] the viewer maps onto its PBR material.

The reference implementation is Firestorm's `LLGLTFMaterial`
(`indra/llprimitive/llgltfmaterial.cpp`).

## Asset format

A material asset is an **LLSD** envelope (serialized `LLSD_BINARY`, so the bytes
lead with the `<? LLSD/Binary ?>` header the fetch returns verbatim) wrapping a
minified glTF 2.0 document:

```text
{
  "version": "1.1",        // accepted: "1.0" | "1.1"
  "type": "GLTF 2.0",
  "data": "<glTF 2.0 JSON>"
}
```

The `data` string is a standard glTF 2.0 JSON document carrying a single
material. Its texture slots reference `images[].uri`, which — unlike a
file-backed glTF — holds the **texture asset UUID** (not a path or a data URI),
so a decoded [`GltfTexture`] resolves straight to a `TextureKey` the texture
pipeline fetches over `GetTexture`.

## What it maps

- Base colour factor (linear RGBA) + base-colour texture
- Metallic / roughness factors + the packed metallic-roughness (ORM) texture
- Normal texture
- Emissive factor (linear RGB) + emissive texture
- Per-texture `KHR_texture_transform` (offset / scale / rotation)
- Alpha mode (`OPAQUE` / `MASK` / `BLEND`) + alpha cutoff
- Double-sided flag

Per-face **overrides** (`GltfMaterialOverride`, delivered over the material
cap / `GenericStreamingMessage`) are a separate concern layered on top of the
base material and are not applied here.
