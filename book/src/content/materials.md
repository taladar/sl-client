# Materials

Materials control how object surfaces are shaded beyond a flat diffuse texture
(see [Textures & the Asset Pipeline](textures.md) for how those textures are
fetched, decoded, and cached). The protocol carries two generations side by
side: the **legacy** materials system (normal + specular maps) and the modern
**GLTF / PBR** system. A current client must understand both, because content of
both kinds coexists in the world.

## Legacy materials

A legacy material adds, per face, a **normal map** and a **specular map** with
parameters (specular colour and exponent, environment intensity, alpha mode and
cutoff, and texture offset/scale/rotation). These attach to the faces described
in an object's texture entry (see [3D World Information](world.md#objects)).

## GLTF / PBR materials

The modern system is **physically-based rendering** using the glTF material
model: a base-colour map, a metallic-roughness map, a normal map, an emissive
map, and the associated factors. Two related concepts:

- a **material asset** is a stored glTF material (referenced by UUID, fetched
  like any asset),
- a **material override** is per-face/per-object tweaks layered on top of the
  base material, delivered as LLSD.

## Fetching and modifying

Both live on the [CAPS](../comms/caps.md) side:

- **`RenderMaterials`** fetches material data (`Command::RequestRenderMaterials`
  → `Event::RenderMaterials`).
- **`ModifyMaterialParams`** applies PBR changes
  (`Command::ModifyMaterialParams` → `Event::MaterialParamsResult`).
- Per-object override updates arrive as `Event::GltfMaterialOverride`, typically
  surfaced from the
  [event queue](../comms/caps.md#the-event-queue-eventqueueget).

---

> **In this codebase**
>
> - Wire types are in `sl-wire/src/material.rs` and its `sl-wire/src/material/`
>   submodules (split into legacy and glTF forms): the legacy `Material` and the
>   PBR/override types (`MediaEntry` aside, the render-material/override
>   structs). `sl-proto` re-exports `Material`, `RenderMaterialRef`, and the
>   `MaterialOverrideUpdate` command input.
> - Caps `CAP_RENDER_MATERIALS` (`RenderMaterials`) and
>   `CAP_MODIFY_MATERIAL_PARAMS` (`ModifyMaterialParams`); the CAPS driver is
>   `sl-client-tokio/src/materials.rs`.
> - Commands `RequestRenderMaterials`, `ModifyMaterialParams`; events
>   `RenderMaterials`, `MaterialParamsResult`, `GltfMaterialOverride`. Worked
>   example: `sl-client-tokio/examples/pbr_materials.rs`.
