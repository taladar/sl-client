//! Decoders for an object's `ExtraParams` block.
//!
//! An `ObjectUpdate`'s `ExtraParams` field is a list of optional typed
//! parameters — each a Linden `LLNetworkData` subtype (flexi, light, sculpt,
//! light-image, extended-mesh, render-material, reflection-probe). The container
//! framing (per the reference viewer's `LLViewerObject::unpackParameterEntry`)
//! is a `u8` parameter count, then for each parameter a little-endian `u16` type
//! code, a `u32` payload byte size, and that many payload bytes. This module
//! walks that container once and decodes each known parameter into the typed
//! [`ObjectExtraParams`] fields, mirroring each subtype's `unpack` in the
//! viewer's `llprimitive.cpp`.

use sl_types::lsl::Vector;
use sl_wire::Reader;

use crate::types::{
    ExtendedMesh, FlexibleData, LightData, LightImage, ObjectExtraParams, ReflectionProbe,
    RenderMaterialRef, SculptData,
};

/// `ExtraParams` type code for flexible-path data (`PARAMS_FLEXIBLE`).
const PARAMS_FLEXIBLE: u16 = 0x10;
/// `ExtraParams` type code for light data (`PARAMS_LIGHT`).
const PARAMS_LIGHT: u16 = 0x20;
/// `ExtraParams` type code for sculpt data (`PARAMS_SCULPT`).
const PARAMS_SCULPT: u16 = 0x30;
/// `ExtraParams` type code for projected-light texture data
/// (`PARAMS_LIGHT_IMAGE`).
const PARAMS_LIGHT_IMAGE: u16 = 0x40;
/// `ExtraParams` type code for a mesh prim (`PARAMS_MESH`); carried in the same
/// block as sculpt data.
const PARAMS_MESH: u16 = 0x60;
/// `ExtraParams` type code for extended-mesh flags (`PARAMS_EXTENDED_MESH`).
const PARAMS_EXTENDED_MESH: u16 = 0x70;
/// `ExtraParams` type code for per-face GLTF render materials
/// (`PARAMS_RENDER_MATERIAL`).
const PARAMS_RENDER_MATERIAL: u16 = 0x80;
/// `ExtraParams` type code for reflection-probe data
/// (`PARAMS_REFLECTION_PROBE`).
const PARAMS_REFLECTION_PROBE: u16 = 0x90;

/// Walks an object's raw `ExtraParams` blob and decodes each known parameter
/// into [`ObjectExtraParams`]. Best-effort: unknown parameters are skipped and a
/// truncated blob stops the walk, returning whatever decoded so far.
#[must_use]
pub(crate) fn decode_extra_params(blob: &[u8]) -> ObjectExtraParams {
    let mut out = ObjectExtraParams::default();
    let mut reader = Reader::new(blob);
    let Ok(count) = reader.u8() else {
        return out;
    };
    for _ in 0..count {
        let Ok(param_type) = reader.u16() else {
            break;
        };
        let Ok(size) = reader.u32() else {
            break;
        };
        let Ok(size) = usize::try_from(size) else {
            break;
        };
        let Ok(payload) = reader.take(size) else {
            break;
        };
        let mut param = Reader::new(payload);
        match param_type {
            PARAMS_FLEXIBLE => out.flexible = decode_flexible(&mut param),
            PARAMS_LIGHT => out.light = decode_light(&mut param),
            PARAMS_SCULPT | PARAMS_MESH => out.sculpt = decode_sculpt(&mut param),
            PARAMS_LIGHT_IMAGE => out.light_image = decode_light_image(&mut param),
            PARAMS_EXTENDED_MESH => out.extended_mesh = decode_extended_mesh(&mut param),
            PARAMS_RENDER_MATERIAL => out.render_material = decode_render_material(&mut param),
            PARAMS_REFLECTION_PROBE => out.reflection_probe = decode_reflection_probe(&mut param),
            _ => {}
        }
    }
    out
}

/// Decodes `LLFlexibleObjectData`: four packed bytes (tension, drag, gravity,
/// wind) and an optional trailing user-force vector.
fn decode_flexible(reader: &mut Reader<'_>) -> Option<FlexibleData> {
    let tension_byte = reader.u8().ok()?;
    let friction_byte = reader.u8().ok()?;
    let gravity_byte = reader.u8().ok()?;
    let wind_byte = reader.u8().ok()?;
    // The two simulate-LOD ("softness") bits are stashed in the high bits of the
    // tension and drag bytes (per the viewer's unpack).
    let softness = ((tension_byte >> 6) & 2) | ((friction_byte >> 7) & 1);
    let tension = f32::from(tension_byte & 0x7f) / 10.0;
    let air_friction = f32::from(friction_byte & 0x7f) / 10.0;
    let gravity = f32::from(gravity_byte) / 10.0 - 10.0;
    let wind_sensitivity = f32::from(wind_byte) / 10.0;
    // The user force is only present on a full (16-byte) flexi block.
    let user_force = reader.vector3().unwrap_or(Vector {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    });
    Some(FlexibleData {
        softness,
        tension,
        air_friction,
        gravity,
        wind_sensitivity,
        user_force,
    })
}

/// Decodes `LLLightParams`: an RGBA colour followed by radius, cutoff, falloff.
fn decode_light(reader: &mut Reader<'_>) -> Option<LightData> {
    let color = reader.take_array::<4>().ok()?;
    let radius = reader.f32().ok()?;
    let cutoff = reader.f32().ok()?;
    let falloff = reader.f32().ok()?;
    Some(LightData {
        color,
        radius,
        cutoff,
        falloff,
    })
}

/// Decodes `LLSculptParams`: a sculpt/mesh asset id and a type byte.
fn decode_sculpt(reader: &mut Reader<'_>) -> Option<SculptData> {
    let texture = reader.uuid().ok()?;
    let sculpt_type = reader.u8().ok()?;
    Some(SculptData {
        texture,
        sculpt_type,
    })
}

/// Decodes `LLLightImageParams`: a projected texture id and its parameters.
fn decode_light_image(reader: &mut Reader<'_>) -> Option<LightImage> {
    let texture = reader.uuid().ok()?;
    let params = reader.vector3().ok()?;
    Some(LightImage { texture, params })
}

/// Decodes `LLExtendedMeshParams`: a single flags word.
fn decode_extended_mesh(reader: &mut Reader<'_>) -> Option<ExtendedMesh> {
    let flags = reader.u32().ok()?;
    Some(ExtendedMesh { flags })
}

/// Decodes `LLRenderMaterialParams`: a count then that many `(face, material id)`
/// entries.
fn decode_render_material(reader: &mut Reader<'_>) -> Vec<RenderMaterialRef> {
    let Ok(count) = reader.u8() else {
        return Vec::new();
    };
    let mut entries = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let (Ok(face), Ok(material_id)) = (reader.u8(), reader.uuid()) else {
            break;
        };
        entries.push(RenderMaterialRef { face, material_id });
    }
    entries
}

/// Decodes `LLReflectionProbeParams`: ambiance, clip distance, and a flags byte.
fn decode_reflection_probe(reader: &mut Reader<'_>) -> Option<ReflectionProbe> {
    let ambiance = reader.f32().ok()?;
    let clip_distance = reader.f32().ok()?;
    let flags = reader.u8().ok()?;
    Some(ReflectionProbe {
        ambiance,
        clip_distance,
        is_box: flags & 0x01 != 0,
        is_dynamic: flags & 0x02 != 0,
        is_mirror: flags & 0x04 != 0,
    })
}
