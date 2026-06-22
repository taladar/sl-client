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

use sl_types::key::TextureKey;
use sl_types::lsl::Vector;
use sl_wire::{Reader, ReflectionProbeFlags, Writer};

use crate::types::{
    ExtendedMesh, FlexibleData, LightData, LightImage, ObjectExtraParams, ReflectionProbe,
    RenderMaterialRef, SculptData,
};

/// A single `ExtraParams` parameter's `u16` type code (Linden's `LLNetworkData`
/// subtype tag, the leading `u16` of each container entry). Unlike a bitfield
/// these codes are mutually exclusive — one per entry — so this is a plain
/// newtype with named constants rather than a flag set: it lets the decoder
/// match by name (`ExtraParamType::FLEXIBLE`) and the encoder write codes by name
/// instead of scattering magic `0x10`/`0x20`/… literals.
///
/// Like the other protocol type wrappers it stays private to this codec: the
/// codes only ever appear inside the raw `ExtraParams` blob this module walks.
/// `code()`/`from_code` are transparent, so wire bytes are byte-identical to the
/// previous bare-`u16` form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExtraParamType(u16);

impl ExtraParamType {
    /// Flexible-path ("flexi") data (`PARAMS_FLEXIBLE`).
    const FLEXIBLE: Self = Self(0x10);
    /// Point/spot-light data (`PARAMS_LIGHT`).
    const LIGHT: Self = Self(0x20);
    /// Sculpt data (`PARAMS_SCULPT`).
    const SCULPT: Self = Self(0x30);
    /// Projected-light texture data (`PARAMS_LIGHT_IMAGE`).
    const LIGHT_IMAGE: Self = Self(0x40);
    /// A mesh prim (`PARAMS_MESH`); carried in the same block as sculpt data.
    const MESH: Self = Self(0x60);
    /// Extended-mesh flags (`PARAMS_EXTENDED_MESH`).
    const EXTENDED_MESH: Self = Self(0x70);
    /// Per-face GLTF render materials (`PARAMS_RENDER_MATERIAL`).
    const RENDER_MATERIAL: Self = Self(0x80);
    /// Reflection-probe data (`PARAMS_REFLECTION_PROBE`).
    const REFLECTION_PROBE: Self = Self(0x90);

    /// Wraps a raw wire type code.
    const fn from_code(code: u16) -> Self {
        Self(code)
    }

    /// Returns the raw wire type code.
    const fn code(self) -> u16 {
        self.0
    }
}

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
        match ExtraParamType::from_code(param_type) {
            ExtraParamType::FLEXIBLE => out.flexible = decode_flexible(&mut param),
            ExtraParamType::LIGHT => out.light = decode_light(&mut param),
            ExtraParamType::SCULPT | ExtraParamType::MESH => {
                out.sculpt = decode_sculpt(&mut param);
            }
            ExtraParamType::LIGHT_IMAGE => out.light_image = decode_light_image(&mut param),
            ExtraParamType::EXTENDED_MESH => out.extended_mesh = decode_extended_mesh(&mut param),
            ExtraParamType::RENDER_MATERIAL => {
                out.render_material = decode_render_material(&mut param);
            }
            ExtraParamType::REFLECTION_PROBE => {
                out.reflection_probe = decode_reflection_probe(&mut param);
            }
            _ => {}
        }
    }
    out
}

/// Measures how many leading bytes of `blob` the `ExtraParams` container
/// occupies (its `u8` count plus each parameter's `u16` type, `u32` size, and
/// payload). Used by the compressed-object decoder, where the container is
/// embedded in a larger length-prefix-less stream and the bytes that follow it
/// (sound, name-values, shape, texture entry) can only be reached once the
/// container's extent is known. A truncated container is clamped to `blob.len()`.
#[must_use]
pub(crate) fn extra_params_len(blob: &[u8]) -> usize {
    let mut reader = Reader::new(blob);
    let Ok(count) = reader.u8() else {
        return 0;
    };
    for _ in 0..count {
        if reader.u16().is_err() {
            break;
        }
        let Ok(size) = reader.u32() else {
            break;
        };
        let Ok(size) = usize::try_from(size) else {
            break;
        };
        if reader.take(size).is_err() {
            // A declared payload that runs past the buffer means the container
            // is truncated; treat it as consuming the rest of the blob so the
            // caller does not misread later fields out of overflow bytes.
            return blob.len();
        }
    }
    blob.len().saturating_sub(reader.remaining())
}

/// Encodes an [`ObjectExtraParams`] into the raw `ExtraParams` blob — the exact
/// inverse of `decode_extra_params`. Emits the container framing (a `u8`
/// count of present parameters, then for each a little-endian `u16` type code, a
/// little-endian `u32` payload size, and that many payload bytes) with one
/// parameter per set field, in ascending type-code order — the order the
/// reference viewer's parameter list (keyed by `type >> 4`) iterates in
/// `LLViewerObject`. Each subtype payload is a faithful port of the matching
/// `LLNetworkData::pack` in `indra/llprimitive/llprimitive.cpp`.
///
/// A field that is `None` (or, for render materials, an empty list) is simply
/// omitted, so an [`ObjectExtraParams::default`] round-trips to a lone zero
/// count byte. The `PARAMS_MESH` alias is not re-emitted: sculpt/mesh data is
/// always written under the canonical `PARAMS_SCULPT` code (the decoder accepts
/// both).
#[must_use]
pub fn encode_extra_params(params: &ObjectExtraParams) -> Vec<u8> {
    // Collect each present parameter as a (type code, payload) pair, in
    // ascending type-code order.
    let mut entries: Vec<(ExtraParamType, Vec<u8>)> = Vec::new();
    if let Some(flexible) = &params.flexible {
        entries.push((ExtraParamType::FLEXIBLE, encode_flexible(flexible)));
    }
    if let Some(light) = &params.light {
        entries.push((ExtraParamType::LIGHT, encode_light(light)));
    }
    if let Some(sculpt) = &params.sculpt {
        entries.push((ExtraParamType::SCULPT, encode_sculpt(sculpt)));
    }
    if let Some(light_image) = &params.light_image {
        entries.push((ExtraParamType::LIGHT_IMAGE, encode_light_image(light_image)));
    }
    if let Some(extended_mesh) = &params.extended_mesh {
        entries.push((
            ExtraParamType::EXTENDED_MESH,
            encode_extended_mesh(extended_mesh),
        ));
    }
    if !params.render_material.is_empty() {
        entries.push((
            ExtraParamType::RENDER_MATERIAL,
            encode_render_material(&params.render_material),
        ));
    }
    if let Some(reflection_probe) = &params.reflection_probe {
        entries.push((
            ExtraParamType::REFLECTION_PROBE,
            encode_reflection_probe(reflection_probe),
        ));
    }

    let mut writer = Writer::new();
    // The container count is a single byte; an object can carry at most one of
    // each of the seven subtypes, so it never overflows.
    writer.put_u8(u8::try_from(entries.len()).unwrap_or(u8::MAX));
    for (param_type, payload) in entries {
        writer.put_u16(param_type.code());
        writer.put_u32(u32::try_from(payload.len()).unwrap_or(u32::MAX));
        writer.bytes(&payload);
    }
    writer.into_bytes()
}

/// Truncates a non-negative `f32` toward zero into a `u8`, the inverse of the
/// flexi decoder's `byte / 10.0` de-quantization. `as` saturates out-of-range
/// values, matching the viewer's in-range `(U8)` cast for the small values
/// (≤ 25.5) these fields hold.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "values pre-bounded to 0..=255; truncate-toward-zero matches LL's (U8) cast"
)]
const fn trunc_to_u8(value: f32) -> u8 {
    value as u8
}

/// Encodes `LLFlexibleObjectData`: four packed bytes (tension, drag, gravity,
/// wind) and the trailing user-force vector — the inverse of [`decode_flexible`]
/// and a port of `LLFlexibleObjectData::pack`. The two simulate-LOD ("softness")
/// bits are stashed back in the high bits of the tension and drag bytes, and the
/// `* 10.01` factor (then truncate) matches the viewer so a value just under an
/// integer tenth still rounds up.
fn encode_flexible(data: &FlexibleData) -> Vec<u8> {
    let mut writer = Writer::new();
    let bit1 = (data.softness & 2).wrapping_shl(6);
    let bit2 = (data.softness & 1).wrapping_shl(7);
    writer.put_u8(trunc_to_u8(data.tension * 10.01).wrapping_add(bit1));
    writer.put_u8(trunc_to_u8(data.air_friction * 10.01).wrapping_add(bit2));
    writer.put_u8(trunc_to_u8((data.gravity + 10.0) * 10.01));
    writer.put_u8(trunc_to_u8(data.wind_sensitivity * 10.01));
    writer.put_vector3(&data.user_force);
    writer.into_bytes()
}

/// Encodes `LLLightParams`: an RGBA colour then radius, cutoff, falloff — the
/// inverse of [`decode_light`].
fn encode_light(data: &LightData) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.bytes(&data.color);
    writer.put_f32(data.radius);
    writer.put_f32(data.cutoff);
    writer.put_f32(data.falloff);
    writer.into_bytes()
}

/// Encodes `LLSculptParams`: a sculpt/mesh asset id and a type byte — the
/// inverse of [`decode_sculpt`].
fn encode_sculpt(data: &SculptData) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.put_uuid(data.texture);
    writer.put_u8(data.sculpt_type);
    writer.into_bytes()
}

/// Encodes `LLLightImageParams`: a projected texture id and its parameters — the
/// inverse of [`decode_light_image`].
fn encode_light_image(data: &LightImage) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.put_uuid(data.texture.uuid());
    writer.put_vector3(&data.params);
    writer.into_bytes()
}

/// Encodes `LLExtendedMeshParams`: a single flags word — the inverse of
/// [`decode_extended_mesh`].
fn encode_extended_mesh(data: &ExtendedMesh) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.put_u32(data.flags);
    writer.into_bytes()
}

/// Encodes `LLRenderMaterialParams`: a count then that many `(face, material id)`
/// entries — the inverse of [`decode_render_material`]. The viewer caps the
/// block at 14 entries (the wire count is a single byte and each entry is 17
/// bytes); any beyond that are dropped so the written count matches the entries.
fn encode_render_material(entries: &[RenderMaterialRef]) -> Vec<u8> {
    /// The reference viewer's per-block entry cap (`llmin(size, 14)`).
    const MAX_ENTRIES: usize = 14;
    let mut writer = Writer::new();
    let written = entries.len().min(MAX_ENTRIES);
    writer.put_u8(u8::try_from(written).unwrap_or(u8::MAX));
    for entry in entries.iter().take(written) {
        writer.put_u8(entry.face);
        writer.put_uuid(entry.material_id);
    }
    writer.into_bytes()
}

/// Encodes `LLReflectionProbeParams`: ambiance, clip distance, and a flags byte
/// — the inverse of [`decode_reflection_probe`]. The flag byte is written back
/// verbatim from the typed [`ReflectionProbeFlags`] set, so a decode/encode round
/// trip is byte-identical even for bits the viewer does not yet name.
fn encode_reflection_probe(data: &ReflectionProbe) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.put_f32(data.ambiance);
    writer.put_f32(data.clip_distance);
    writer.put_u8(data.flags.bits());
    writer.into_bytes()
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
    Some(LightImage {
        texture: TextureKey::from(texture),
        params,
    })
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
        flags: ReflectionProbeFlags::from_bits(flags),
    })
}

#[cfg(test)]
mod encode_tests {
    use pretty_assertions::assert_eq;
    use sl_types::key::TextureKey;
    use sl_types::lsl::Vector;
    use uuid::Uuid;

    use sl_wire::ReflectionProbeFlags;

    use super::{decode_extra_params, encode_extra_params};
    use crate::types::{
        ExtendedMesh, FlexibleData, LightData, LightImage, ObjectExtraParams, ReflectionProbe,
        RenderMaterialRef, SculptData,
    };

    /// A fully-populated [`ObjectExtraParams`] whose floating-point fields are in
    /// the decoder's canonical forms (`byte / 10` for the flexi fields,
    /// exactly-representable values elsewhere), so a decode of its encoding is
    /// bit-identical to it.
    fn sample() -> ObjectExtraParams {
        ObjectExtraParams {
            flexible: Some(FlexibleData {
                softness: 3,
                // Each chosen so `decode`'s `byte / 10` reproduces it exactly:
                // 12 → 1.2, 7 → 0.7, 65 → 6.5 (gravity offset by −10), 24 → 2.4.
                tension: 12.0 / 10.0,
                air_friction: 7.0 / 10.0,
                gravity: 65.0 / 10.0 - 10.0,
                wind_sensitivity: 24.0 / 10.0,
                user_force: Vector {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
            }),
            light: Some(LightData {
                color: [10, 20, 30, 255],
                radius: 10.0,
                cutoff: 0.0,
                falloff: 1.0,
            }),
            sculpt: Some(SculptData {
                texture: Uuid::from_u128(0x1111_2222_3333_4444_5555_6666_7777_8888),
                sculpt_type: 5,
            }),
            light_image: Some(LightImage {
                texture: TextureKey::from(Uuid::from_u128(
                    0x9999_aaaa_bbbb_cccc_dddd_eeee_ffff_0000,
                )),
                params: Vector {
                    x: 0.5,
                    y: 0.25,
                    z: 0.75,
                },
            }),
            extended_mesh: Some(ExtendedMesh { flags: 0x0000_0001 }),
            render_material: vec![
                RenderMaterialRef {
                    face: 0,
                    material_id: Uuid::from_u128(0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10),
                },
                RenderMaterialRef {
                    face: 3,
                    material_id: Uuid::from_u128(0x1112_1314_1516_1718_191a_1b1c_1d1e_1f20),
                },
            ],
            reflection_probe: Some(ReflectionProbe {
                ambiance: 0.5,
                clip_distance: 2.0,
                flags: ReflectionProbeFlags::BOX_VOLUME | ReflectionProbeFlags::MIRROR,
            }),
        }
    }

    #[test]
    fn default_encodes_to_a_lone_zero_count_byte() {
        let blob = encode_extra_params(&ObjectExtraParams::default());
        assert_eq!(blob, vec![0]);
        // …and that lone byte decodes back to the default (no parameters).
        assert_eq!(decode_extra_params(&blob), ObjectExtraParams::default());
    }

    #[test]
    fn encode_then_decode_round_trips_every_subtype() {
        let original = sample();
        let blob = encode_extra_params(&original);
        // Seven parameters, so the count byte is 7.
        assert_eq!(blob.first().copied(), Some(7));
        let decoded = decode_extra_params(&blob);
        assert_eq!(decoded, original);
        // The encoder is deterministic and the exact inverse of the decoder, so
        // re-encoding the decoded form reproduces the identical blob.
        assert_eq!(encode_extra_params(&decoded), blob);
    }

    #[test]
    fn extra_param_type_codes_round_trip() {
        use super::ExtraParamType;
        // Each named code wraps and unwraps to the exact wire value the bare
        // `PARAMS_*` consts carried, so the framing is byte-identical.
        for (ty, code) in [
            (ExtraParamType::FLEXIBLE, 0x10_u16),
            (ExtraParamType::LIGHT, 0x20),
            (ExtraParamType::SCULPT, 0x30),
            (ExtraParamType::LIGHT_IMAGE, 0x40),
            (ExtraParamType::MESH, 0x60),
            (ExtraParamType::EXTENDED_MESH, 0x70),
            (ExtraParamType::RENDER_MATERIAL, 0x80),
            (ExtraParamType::REFLECTION_PROBE, 0x90),
        ] {
            assert_eq!(ty.code(), code);
            assert_eq!(ExtraParamType::from_code(code), ty);
        }
    }

    #[test]
    fn render_material_caps_at_fourteen_entries() {
        let params = ObjectExtraParams {
            render_material: (0..20_u8)
                .map(|face| RenderMaterialRef {
                    face,
                    material_id: Uuid::from_u128(u128::from(face) + 1),
                })
                .collect(),
            ..ObjectExtraParams::default()
        };
        let blob = encode_extra_params(&params);
        let decoded = decode_extra_params(&blob);
        // The block tops out at the viewer's 14-entry cap; the rest are dropped.
        assert_eq!(decoded.render_material.len(), 14);
        assert_eq!(
            Some(decoded.render_material.as_slice()),
            params.render_material.get(..14)
        );
    }
}

#[cfg(test)]
mod len_tests {
    use pretty_assertions::assert_eq;

    use super::extra_params_len;

    /// Appends `value` to `out` as `width` little-endian bytes (avoiding the
    /// crate's endian-byte-method lint).
    fn push_le(out: &mut Vec<u8>, value: u32, width: u32) {
        let mut emitted = 0_u32;
        while emitted < width {
            let shift = emitted.saturating_mul(8);
            out.push(u8::try_from((value >> shift) & 0xFF).unwrap_or(0));
            emitted = emitted.saturating_add(1);
        }
    }

    /// Builds an `ExtraParams` blob: a count byte then `(type, payload)` entries.
    fn build(entries: &[(u16, &[u8])]) -> Vec<u8> {
        let mut out = vec![u8::try_from(entries.len()).unwrap_or(0)];
        for &(param_type, payload) in entries {
            push_le(&mut out, u32::from(param_type), 2);
            push_le(&mut out, u32::try_from(payload.len()).unwrap_or(0), 4);
            out.extend_from_slice(payload);
        }
        out
    }

    #[test]
    fn measures_empty_and_populated_containers() {
        // A lone zero count byte: just the one byte.
        assert_eq!(extra_params_len(&[0]), 1);
        // One 16-byte light parameter: 1 (count) + 2 (type) + 4 (size) + 16.
        let blob = build(&[(0x20, &[0_u8; 16])]);
        assert_eq!(extra_params_len(&blob), 23);
        assert_eq!(extra_params_len(&blob), blob.len());
        // Trailing bytes after the container are not counted.
        let mut with_tail = blob.clone();
        with_tail.extend_from_slice(&[0xAB, 0xCD]);
        assert_eq!(extra_params_len(&with_tail), blob.len());
    }

    #[test]
    fn clamps_a_truncated_container_to_the_available_bytes() {
        // count=1, type=0x20, size=16, but only 3 payload bytes follow: the
        // declared payload overruns the blob, so the whole blob is consumed.
        let truncated = [1, 0x20, 0, 16, 0, 0, 0, 1, 2, 3];
        assert_eq!(extra_params_len(&truncated), truncated.len());
        // An empty blob has no count byte at all.
        assert_eq!(extra_params_len(&[]), 0);
    }
}
