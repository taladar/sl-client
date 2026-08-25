//! The Texture tab's **Align planar faces** action (`viewer-prim-texture-editing`,
//! the reference's `checkbox planar align` + `LLFace::calcAlignedPlanarTE`):
//! align every other selected face's planar texture to the primary selection's
//! reference face, so a planar-mapped texture flows continuously across the
//! faces (around a box's corners, along a wall).
//!
//! # Model
//!
//! The reference computes, for each face, a **planar projection frame** — a
//! rotation and a scale derived from the face's normal and tangent
//! (`face_projection`, a port of `LLFace::getPlanarProjectedParams` reduced to
//! what a single object needs: the per-face `face_pos` the reference reads is the
//! object's world position, identical for every face, so the inter-face
//! translation term is zero and the object world transform cancels out of the
//! rotation). Each aligned face then takes the **reference** face's texture
//! offset (unchanged, wrapped to −1..1), the reference scale rescaled by the
//! ratio of the two faces' projection scales, and a rotation that carries the
//! reference face's texture axes into the aligned face's frame.
//!
//! Reference (Firestorm, read-only): `llface.cpp` (`calcAlignedPlanarTE`,
//! `getPlanarProjectedParams`, `planarProjection`), `llpanelface.cpp`
//! (`LLPanelFaceSetAlignedTEFunctor`).

use bevy::math::{EulerRot, Mat3, Quat, Vec2, Vec3};
use bevy::prelude::MessageWriter;
use sl_client_bevy::{
    Command, PrimFace, PrimLod, PrimMesh, PrimShapeFloat, SlCommand, TextureEntry, pcode,
    planar_texgen_uv, tessellate,
};

use crate::edit_texture::PrimFaceLookup;
use crate::world_api::ObjectState;
use crate::world_api::SelectionSet;

/// A face's planar projection frame: the rotation that orients the planar
/// texture axes and the scale of the projected basis.
#[derive(Debug, Clone, Copy)]
struct FaceProjection {
    /// The face's local texture-frame rotation (the reference's `face_rot` with
    /// the object world rotation cancelled, since it appears on both sides).
    face_rot: Quat,
    /// The projected-basis scale (the reference's `proj_scale`).
    proj_scale: f32,
}

/// Align every other selected face of the primary object to its reference face
/// (the lowest-indexed selected face), then send the modified `TextureEntry` as
/// an `ObjectImage`. A no-op unless the primary is a plain prim with at least two
/// selected faces and the reference face is planar-mapped; a non-planar target
/// face is skipped (the reference's `calcAlignedPlanarTE` returns false for it).
#[expect(
    clippy::arithmetic_side_effects,
    reason = "dense ported quaternion / vector algebra (calcAlignedPlanarTE); the glam operators \
              are the readable form and cannot overflow for the finite face geometry"
)]
pub(crate) fn align_planar_faces(
    selection: &SelectionSet,
    objects: &ObjectState,
    prim_faces: &PrimFaceLookup,
    commands: &mut MessageWriter<SlCommand>,
) {
    let Some(primary) = selection.primary() else {
        return;
    };
    let scoped = primary.scoped();
    // Align only makes sense across an explicit multi-face selection.
    let Some(selected) = primary.faces.as_ref() else {
        return;
    };
    if selected.len() < 2 {
        return;
    }
    let Some(data) = objects.edit_data(&scoped) else {
        return;
    };
    // Planar align works off a volume's tessellated faces; a sculpt / mesh has no
    // profile geometry to project here.
    if data.pcode != pcode::PRIMITIVE {
        return;
    }
    // Rebuild the entry from the object's rendered per-face values (not by
    // re-decoding the blob), so an align preserves every unaligned face and every
    // attribute it does not touch.
    let faces = prim_faces.current_faces(primary.entity);
    if faces.is_empty() {
        return;
    }
    let mut entry = TextureEntry { faces };
    let prim = tessellate(&PrimShapeFloat::from_params(&data.shape), PrimLod::High);

    // The reference face: the lowest-indexed selected face. Its current texture
    // placement is what every other face is aligned to.
    let Some(ref_id) = selected.iter().map(|face| face.get()).min() else {
        return;
    };
    let Some(ref_te) = entry.face(usize::from(ref_id)).copied() else {
        return;
    };
    if !ref_te.is_planar_texgen() {
        return;
    }
    let Some(ref_proj) = face_projection(&prim, ref_id) else {
        return;
    };
    // orig_st_rot = rot(map_rot about Z) * reference face_rot.
    let orig_st_rot = Quat::from_axis_angle(Vec3::Z, ref_te.rotation) * ref_proj.face_rot;

    let mut touched = false;
    for &face in selected {
        let face_id = face.get();
        if face_id == ref_id {
            continue;
        }
        let Some(te) = entry.face(usize::from(face_id)).copied() else {
            continue;
        };
        if !te.is_planar_texgen() {
            continue;
        }
        let Some(proj) = face_projection(&prim, face_id) else {
            continue;
        };
        // this_st_rot = orig_st_rot * conj(this face_rot); its Z euler is the
        // aligned texture rotation.
        let this_st_rot = orig_st_rot * proj.face_rot.conjugate();
        let (_x_ang, _y_ang, z_ang) = this_st_rot.to_euler(EulerRot::XYZ);
        // Scale = reference scale × (reference proj scale / this proj scale).
        let scale_ratio = ref_proj.proj_scale / proj.proj_scale;
        if let Some(dst) = entry.faces.get_mut(usize::from(face_id)) {
            dst.scale_s = ref_te.scale_s * scale_ratio;
            dst.scale_t = ref_te.scale_t * scale_ratio;
            // Offset is the reference's, wrapped into (−1, 1) — the inter-face
            // translation term is zero for one object.
            dst.offset_s = wrap_unit(ref_te.offset_s);
            dst.offset_t = wrap_unit(ref_te.offset_t);
            dst.rotation = z_ang;
            touched = true;
        }
    }
    if !touched {
        return;
    }
    commands.write(SlCommand(Command::SetObjectImage {
        local_id: scoped,
        media_url: objects.media_url_of(&scoped),
        texture_entry: entry,
    }));
}

/// The planar projection frame of face `face_id` in the tessellated prim, or
/// `None` if the face is missing or degenerate. Ports the arithmetic of
/// `LLFace::getPlanarProjectedParams`: pick the face's normal and a tangent from
/// its geometry, project the reconstructed binormal to get the frame scale and
/// the in-plane angle, and build the rotation from the rotated basis.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "dense ported vector algebra (getPlanarProjectedParams); the glam operators are the \
              readable form and cannot overflow for the finite face geometry"
)]
fn face_projection(prim: &PrimMesh, face_id: u16) -> Option<FaceProjection> {
    let face = prim
        .faces
        .iter()
        .find(|face| face.face_id.get() == face_id)?;
    let normal = Vec3::from(*face.normals.first()?).normalize_or_zero();
    if normal == Vec3::ZERO {
        return None;
    }
    let tangent = face_tangent(face, normal)?;
    // The reconstructed binormal (the reference's `normal × tangent`).
    let binormal = normal.cross(tangent).normalize_or_zero();
    if binormal == Vec3::ZERO {
        return None;
    }
    // Project the binormal onto the face's planar basis (our `planar_texgen_uv`
    // is the reference's `planarProjection`); the reference then removes the
    // +0.5 texture-space bias before measuring.
    let projected = planar_texgen_uv(binormal.to_array(), normal.to_array(), [1.0, 1.0, 1.0]);
    let projected = Vec2::new(projected[0], projected[1]) - Vec2::splat(0.5);
    let proj_scale = projected.length();
    if proj_scale <= f32::EPSILON {
        return None;
    }
    let unit = projected / proj_scale;
    // The signed in-plane angle of the projected binormal.
    let mut ang = unit.y.clamp(-1.0, 1.0).acos();
    if unit.x < 0.0 {
        ang = -ang;
    }
    // Rotate the binormal by that angle about the normal, then build the frame.
    let binormal_rot = Quat::from_axis_angle(normal, ang) * binormal;
    let face_rot = Quat::from_mat3(&Mat3::from_cols(
        binormal_rot.cross(normal),
        binormal_rot,
        normal,
    ));
    Some(FaceProjection {
        face_rot,
        proj_scale,
    })
}

/// A tangent (texture-`u` direction) for `face`, from its first triangle's
/// position / UV gradient, orthogonalized against `normal`. Falls back to any
/// vector perpendicular to the normal when the triangle's UVs are degenerate.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "dense ported vector algebra (tangent from the position / UV gradient); the glam \
              operators are the readable form and cannot overflow for the finite face geometry"
)]
fn face_tangent(face: &PrimFace, normal: Vec3) -> Option<Vec3> {
    let i0 = usize::try_from(*face.indices.first()?).ok()?;
    let i1 = usize::try_from(*face.indices.get(1)?).ok()?;
    let i2 = usize::try_from(*face.indices.get(2)?).ok()?;
    let p0 = Vec3::from(*face.positions.get(i0)?);
    let p1 = Vec3::from(*face.positions.get(i1)?);
    let p2 = Vec3::from(*face.positions.get(i2)?);
    let uv0 = Vec2::from(*face.uvs.get(i0)?);
    let uv1 = Vec2::from(*face.uvs.get(i1)?);
    let uv2 = Vec2::from(*face.uvs.get(i2)?);
    let edge1 = p1 - p0;
    let edge2 = p2 - p0;
    let d1 = uv1 - uv0;
    let d2 = uv2 - uv0;
    let det = d1.x * d2.y - d2.x * d1.y;
    let tangent = if det.abs() > 1.0e-8 {
        (edge1 * d2.y - edge2 * d1.y) / det
    } else if normal.x.abs() < 0.9 {
        // Degenerate UVs: any axis not parallel to the normal.
        Vec3::X
    } else {
        Vec3::Y
    };
    // Orthogonalize against the normal and normalize.
    let tangent = (tangent - normal * tangent.dot(normal)).normalize_or_zero();
    (tangent != Vec3::ZERO).then_some(tangent)
}

/// Wrap a texture offset into (−1, 1) by dropping its integer part (the
/// reference's `offset -= (S32)offset`).
fn wrap_unit(value: f32) -> f32 {
    value - value.trunc()
}
