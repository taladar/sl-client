//! Material protocol support: PBR/GLTF material overrides, the legacy
//! `RenderMaterials` capability, and the `ModifyMaterialParams` set request.
//!
//! Second Life carries two kinds of surface material referenced per face by a
//! `TextureEntry`'s 16-byte material id:
//!
//! - **Legacy materials** (normal/specular maps) exchanged over the
//!   `RenderMaterials` capability, whose payload is a *zlib-compressed binary
//!   LLSD* document — the only path stock OpenSim implements. This module ports
//!   that codec ([`build_render_materials_request`] /
//!   [`parse_render_materials_response`]); the binary-LLSD layer itself is
//!   `sl-llsd`'s, so the `RenderMaterials` bodies and the inventory cache share
//!   one codec.
//! - **Modern GLTF (PBR) materials**, where per-object/per-face *overrides* are
//!   pushed in a `GenericStreamingMessage` (method `0x4175`) as *notation* LLSD,
//!   and set with a `ModifyMaterialParams` capability POST. Per the project's
//!   asset-fetch scope, the GLTF document itself is **not** parsed here: the
//!   override envelope (object local id + affected faces) is decoded and each
//!   per-face override is surfaced as its raw notation bytes
//!   ([`parse_gltf_material_override`]), and the JSON a caller sets via
//!   [`build_modify_material_params_request`] is passed through opaque.
//!
//! The material *assets* themselves (`AT_MATERIAL` / `AT_GLTF`) are fetched and
//! uploaded over the generic asset pipeline (see the runtime asset commands);
//! only the surface-material protocol lives here.

mod gltf;
mod legacy;
#[cfg(test)]
mod tests;
mod types;

pub use gltf::{
    build_gltf_material_override, build_modify_material_params_request,
    build_modify_material_params_response, parse_gltf_material_override,
    parse_modify_material_params_request,
};
pub use legacy::{
    build_render_materials_put_request, build_render_materials_request,
    build_render_materials_response, parse_render_materials_put_request,
    parse_render_materials_request, parse_render_materials_response,
};
pub use types::{
    FaceMaterialPut, GLTF_MATERIAL_OVERRIDE_METHOD, GltfMaterialOverride, LegacyMaterial,
    MaterialOverrideUpdate, RenderMaterialEntry,
};
