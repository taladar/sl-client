//! The Texture tab's **Align planar faces** action (`viewer-prim-texture-editing`,
//! the reference's `checkbox planar align` + `LLFace::calcAlignedPlanarTE`):
//! align every other selected face's planar texture to the anchor face, so a
//! planar-mapped texture flows continuously across the faces (around a box's
//! corners, along a wall) — across the **whole selection**, not just one object.
//!
//! # Model
//!
//! The reference computes, for each face, a **planar projection frame** — a
//! world rotation, the object's world position, and a scale derived from the
//! face's normal and tangent (`face_projection`, a port of
//! `LLFace::getPlanarProjectedParams`). Each aligned face then takes a rotation
//! that carries the anchor face's texture axes into the aligned face's frame,
//! the anchor scale rescaled by the ratio of the two faces' projection scales,
//! and the anchor offset shifted by the distance between the two objects'
//! centres measured in the anchor's texture frame (zero when both faces sit on
//! the same object, which is why a single-object align never needed it).
//!
//! The **anchor** is the reference's `LLSelectedTE::getFace` pick: the primary
//! object's last-touched face while it is still selected
//! ([`SelectedNode::last_face`](crate::world_api::SelectedNode::last_face), the
//! reference's `LLSelectNode::getLastSelectedTE`), and otherwise the first face
//! the selection walk visits.
//!
//! An aligned face's placement is written to the **diffuse** channel and, when
//! the face carries a legacy (Blinn-Phong) material, copied onto that material's
//! **normal and specular** transforms as well — the reference's
//! `FSPanelFaceSetAlignedTEFunctor` calls `setNormalRotation` /
//! `setSpecularRotation` / `setNormal…Offset` / `setNormal…Repeat` alongside the
//! `setTEOffset` / `setTEScale` / `setTERotation` (`fspanelface.cpp:1442-1449`),
//! so a bump-mapped face does not keep an unaligned bump. A face with no
//! material is left without one: the reference builds a default material, applies
//! the transform, then drops it again because nothing needs it
//! (`FSPanelFace::LLSelectedTEMaterial::edit`'s `is_need_material` branch).
//!
//! # Quaternion conventions
//!
//! The reference's `LLQuaternion` composes in the **opposite** order to `glam`:
//! `a * b` there rotates by `a` and *then* by `b`, which is `b * a` here (its
//! `operator*` is literally the standard product with the arguments swapped),
//! and `v * q` is the ordinary `q v q⁻¹`. Its
//! `LLQuaternion(x_axis, y_axis, z_axis)` constructor goes through
//! `LLMatrix3::setRows` + `LLMatrix3::quaternion`, and that pair — rows, then a
//! deliberately inverted extraction (see the `SJB:` comment in `m3math.cpp`) —
//! comes back out as the rotation whose *columns* are the three axes, i.e.
//! [`Mat3::from_cols`]. The euler extraction is the reference's own
//! `LLQuaternion::getEulerAngles` (`ll_euler_yaw`); `glam`'s `to_euler` handles
//! gimbal lock differently, and the aligned rotation lands *on* the locked case
//! for the axis-aligned faces this action is mostly used on.
//!
//! The expected values in this module's tests come from compiling a verbatim
//! extract of `llquaternion.cpp` (`operator*`, `operator~`, `v * q`,
//! `getEulerAngles`), `m3math.cpp` (`LLMatrix3::setRows`, `::quaternion`) and
//! `llface.cpp` (`planarProjection`, `getPlanarProjectedParams`,
//! `calcAlignedPlanarTE`) with `g++ -O0` and printing its results at `%.9g`.
//! The extract itself is not committed.
//!
//! Reference (Firestorm, read-only): `llface.cpp` (`calcAlignedPlanarTE`,
//! `getPlanarProjectedParams`, `planarProjection`), `fspanelface.cpp`
//! (`FSPanelFaceSetAlignedTEFunctor`, `FSPanelFace::LLSelectedTE::getFace`).

use bevy::math::{Mat3, Quat, Vec2, Vec3};
use bevy::prelude::{Entity, GlobalTransform, MessageWriter, Query};
use sl_client_bevy::{
    Command, FaceMaterialPut, LegacyMaterial, PrimFace, PrimLod, PrimMesh, PrimShapeFloat,
    ScopedObjectId, SlCommand, TextureEntry, TextureFace, pcode, planar_texgen_uv, tessellate,
};
use sl_viewer_kit::coords::bevy_to_sl_vec;

use crate::edit_texture::PrimFaceLookup;
use crate::gizmos::sl_world_rotation;
use crate::legacy_materials::LegacyMaterialManager;
use crate::world_api::ObjectState;
use crate::world_api::SelectionSet;

/// The reference's gimbal-lock threshold (`GIMBAL_THRESHOLD`, `llmath.h`):
/// `sin(0.025°)`, the cosine of the pitch below which
/// `LLQuaternion::getEulerAngles` switches to its locked branch.
const GIMBAL_THRESHOLD: f32 = 0.000_436;

/// A face's planar projection frame, in Second Life world space: the rotation
/// that orients the planar texture axes, the owning object's centre, and the
/// scale of the projected basis.
#[derive(Debug, Clone, Copy)]
struct FaceProjection {
    /// The face's texture-frame rotation (the reference's `face_rot`).
    face_rot: Quat,
    /// The owning object's world position (the reference's `face_pos`, which is
    /// the object's world translation and so shared by all of its faces).
    face_pos: Vec3,
    /// The projected-basis scale (the reference's `proj_scale`).
    proj_scale: f32,
}

/// The texture placement an align reads off the anchor face (the reference's
/// `map_rot` / `map_scaleS,T` / `map_offsS,T`).
#[derive(Debug, Clone, Copy)]
struct MapPlacement {
    /// The anchor's texture rotation, in radians.
    rotation: f32,
    /// The anchor's texture repeats.
    scale: Vec2,
    /// The anchor's texture offset.
    offset: Vec2,
}

/// The placement an aligned face takes (the reference's `res_st_*` outputs).
#[derive(Debug, Clone, Copy, PartialEq)]
struct AlignedPlacement {
    /// The aligned texture offset, wrapped into (−1, 1).
    offset: Vec2,
    /// The aligned texture repeats.
    scale: Vec2,
    /// The aligned texture rotation, in radians.
    rotation: f32,
}

/// One selected object the align walk can touch: what it renders now, which of
/// its faces are selected, and the geometry / world transform its projection
/// frames come from.
struct AlignTarget {
    /// The object's region-scoped id — what the `ObjectImage` and the material
    /// PUT address.
    scoped: ScopedObjectId,
    /// The object's scene entity, so the anchor can be identified across objects
    /// that happen to share a face index.
    entity: Entity,
    /// The object's current per-face texture entry, rebuilt from what each face
    /// renders so the re-send preserves every untouched face.
    entry: TextureEntry,
    /// The selected Linden face indices, ascending — the walk order the
    /// reference's `applyToTEs` uses.
    selected: Vec<u16>,
    /// The object's tessellated geometry, the source of each face's normal and
    /// tangent.
    prim: PrimMesh,
    /// The object's world rotation, in Second Life space.
    rotation: Quat,
    /// The object's world position, in Second Life space.
    position: Vec3,
}

/// Align every other selected face of the selection to its anchor face, sending
/// the modified `TextureEntry` of each touched object as an `ObjectImage` and —
/// for every aligned face that carries one — its legacy material with the same
/// placement on the normal and specular channels.
///
/// A no-op unless at least two faces are selected across the selection and the
/// anchor face is planar-mapped; a non-planar target face is skipped (the
/// reference's `calcAlignedPlanarTE` returns false for it), as is any selected
/// object that is not a plain prim (a sculpt / mesh has no profile geometry to
/// project here).
pub(crate) fn align_planar_faces(
    selection: &SelectionSet,
    objects: &ObjectState,
    prim_faces: &PrimFaceLookup,
    legacy: &LegacyMaterialManager,
    globals: &Query<&GlobalTransform>,
    commands: &mut MessageWriter<SlCommand>,
) {
    let mut targets = collect_targets(selection, objects, prim_faces, globals);
    let face_count: usize = targets.iter().map(|target| target.selected.len()).sum();
    // Align only makes sense across an explicit multi-face selection.
    if face_count < 2 {
        return;
    }
    let Some((anchor_entity, anchor_face)) = anchor_of(selection, &targets) else {
        return;
    };
    let Some(anchor) = targets
        .iter()
        .find(|target| target.entity == anchor_entity)
        .and_then(|target| anchor_placement(target, anchor_face))
    else {
        return;
    };
    let (anchor_projection, map) = anchor;

    let mut material_updates: Vec<FaceMaterialPut> = Vec::new();
    for target in &mut targets {
        let AlignTarget {
            scoped,
            entity,
            entry,
            selected,
            prim,
            rotation,
            position,
        } = target;
        let mut touched = false;
        for &face_id in selected.iter() {
            if *entity == anchor_entity && face_id == anchor_face {
                continue;
            }
            let Some(te) = entry.face(usize::from(face_id)).copied() else {
                continue;
            };
            if !te.is_planar_texgen() {
                continue;
            }
            let Some(projection) = face_projection(prim, face_id, *rotation, *position) else {
                continue;
            };
            let aligned = aligned_planar_te(&anchor_projection, &projection, map);
            let Some(dst) = entry.faces.get_mut(usize::from(face_id)) else {
                continue;
            };
            dst.scale_s = aligned.scale.x;
            dst.scale_t = aligned.scale.y;
            dst.offset_s = aligned.offset.x;
            dst.offset_t = aligned.offset.y;
            dst.rotation = aligned.rotation;
            touched = true;
            if let Some(put) = aligned_material(*scoped, face_id, &te, legacy, aligned) {
                material_updates.push(put);
            }
        }
        if touched {
            commands.write(SlCommand(Command::SetObjectImage {
                local_id: *scoped,
                media_url: objects.media_url_of(scoped),
                texture_entry: entry.clone(),
            }));
        }
    }
    if !material_updates.is_empty() {
        commands.write(SlCommand(Command::SetRenderMaterials {
            updates: material_updates,
        }));
    }
}

/// The selected objects an align can touch, in selection order: a plain prim
/// with rendered faces, a known shape, and a resolvable world transform.
fn collect_targets(
    selection: &SelectionSet,
    objects: &ObjectState,
    prim_faces: &PrimFaceLookup,
    globals: &Query<&GlobalTransform>,
) -> Vec<AlignTarget> {
    let mut targets = Vec::new();
    for node in selection.iter() {
        let scoped = node.scoped();
        let Some(data) = objects.edit_data(&scoped) else {
            continue;
        };
        // Planar align works off a volume's tessellated faces; a sculpt / mesh has
        // no profile geometry to project here.
        if data.pcode != pcode::PRIMITIVE {
            continue;
        }
        // Rebuild the entry from the object's rendered per-face values (not by
        // re-decoding the blob), so an align preserves every unaligned face and
        // every attribute it does not touch.
        let faces = prim_faces.current_faces(node.entity);
        if faces.is_empty() {
            continue;
        }
        let Ok(global) = globals.get(node.entity) else {
            continue;
        };
        let count = u16::try_from(faces.len()).unwrap_or(u16::MAX);
        let selected: Vec<u16> = match node.faces.as_ref() {
            Some(set) => {
                let mut ids: Vec<u16> = set
                    .iter()
                    .map(|face| face.get())
                    .filter(|id| *id < count)
                    .collect();
                ids.sort_unstable();
                ids
            }
            None => (0..count).collect(),
        };
        if selected.is_empty() {
            continue;
        }
        targets.push(AlignTarget {
            scoped,
            entity: node.entity,
            entry: TextureEntry { faces },
            selected,
            prim: tessellate(&PrimShapeFloat::from_params(&data.shape), PrimLod::High),
            rotation: sl_world_rotation(global.rotation()),
            position: {
                let sl = bevy_to_sl_vec(global.translation());
                Vec3::new(sl.x, sl.y, sl.z)
            },
        });
    }
    targets
}

/// The anchor face the whole selection aligns to — the reference's
/// `LLSelectedTE::getFace`: the primary object's last-touched face while it is
/// still selected, and otherwise the first face the selection walk visits.
fn anchor_of(selection: &SelectionSet, targets: &[AlignTarget]) -> Option<(Entity, u16)> {
    if let Some(primary) = selection.primary()
        && let Some(target) = targets
            .iter()
            .find(|target| target.entity == primary.entity)
        && target.selected.contains(&primary.last_face.get())
    {
        return Some((target.entity, primary.last_face.get()));
    }
    let first = targets.first()?;
    Some((first.entity, *first.selected.first()?))
}

/// The anchor face's projection frame and the texture placement every aligned
/// face is derived from, or `None` if the face is missing, not planar-mapped, or
/// geometrically degenerate.
fn anchor_placement(target: &AlignTarget, face_id: u16) -> Option<(FaceProjection, MapPlacement)> {
    let te = target.entry.face(usize::from(face_id))?;
    if !te.is_planar_texgen() {
        return None;
    }
    let projection = face_projection(&target.prim, face_id, target.rotation, target.position)?;
    Some((
        projection,
        MapPlacement {
            rotation: te.rotation,
            scale: Vec2::new(te.scale_s, te.scale_t),
            offset: Vec2::new(te.offset_s, te.offset_t),
        },
    ))
}

/// The legacy-material update an aligned face needs, or `None` when the face has
/// no material — the reference creates one, applies the transform, and then
/// removes it again because a material with no maps and the default alpha mode is
/// not needed (`is_need_material`), so the net effect is to leave such a face
/// alone.
fn aligned_material(
    scoped: ScopedObjectId,
    face_id: u16,
    te: &TextureFace,
    legacy: &LegacyMaterialManager,
    aligned: AlignedPlacement,
) -> Option<FaceMaterialPut> {
    let mut material: LegacyMaterial = te
        .material_id
        .and_then(|id| legacy.decoded_material(&id).cloned())?;
    material.normal_rotation = aligned.rotation;
    material.normal_offset = (aligned.offset.x, aligned.offset.y);
    material.normal_repeat = (aligned.scale.x, aligned.scale.y);
    material.specular_rotation = aligned.rotation;
    material.specular_offset = (aligned.offset.x, aligned.offset.y);
    material.specular_repeat = (aligned.scale.x, aligned.scale.y);
    Some(FaceMaterialPut {
        local_id: scoped.id().0,
        face: u8::try_from(face_id).ok()?,
        material: Some(material),
    })
}

/// The placement `face` takes to align to `anchor`, whose channel currently sits
/// at `map` — a port of `LLFace::calcAlignedPlanarTE` past its channel pick.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "dense ported quaternion / vector algebra (calcAlignedPlanarTE); the glam operators \
              are the readable form and cannot overflow for the finite face geometry"
)]
fn aligned_planar_te(
    anchor: &FaceProjection,
    face: &FaceProjection,
    map: MapPlacement,
) -> AlignedPlacement {
    // orig_st_rot = the anchor's texture frame turned by its own map rotation.
    // The reference writes `LLQuaternion(map_rot, z_axis) * orig_face_rot`, which
    // in glam's order is `face_rot * rot_z`.
    let orig_st_rot = anchor.face_rot * Quat::from_axis_angle(Vec3::Z, map.rotation);
    // this_st_rot = orig_st_rot carried into this face's frame (the reference's
    // `orig_st_rot * ~this_face_rot`); its euler yaw is the aligned rotation.
    let this_st_rot = face.face_rot.conjugate() * orig_st_rot;
    let rotation = ll_euler_yaw(this_st_rot);

    // The distance between the two objects' centres, measured in the anchor's
    // texture frame and scaled by it — zero for two faces of one object.
    let centers_dist = orig_st_rot
        .conjugate()
        .mul_vec3(face.face_pos - anchor.face_pos);
    let st_scale = Vec3::new(map.scale.x, map.scale.y, 1.0) * anchor.proj_scale;
    let shifted = centers_dist * st_scale;
    let offset = map.offset + shifted.truncate();
    AlignedPlacement {
        offset: Vec2::new(wrap_unit(offset.x), wrap_unit(offset.y)),
        scale: (st_scale / face.proj_scale).truncate(),
        rotation,
    }
}

/// The `yaw` of the reference's `LLQuaternion::getEulerAngles`
/// (`indra/llmath/llquaternion.cpp`), gimbal-lock branch included.
///
/// `glam`'s `Quat::to_euler` disagrees with it exactly where it matters: an
/// aligned planar rotation between two axis-aligned faces lands *on* the locked
/// case, where the reference reads the yaw off a half-angle `atan2` of the
/// quaternion components rather than off a pitch of ±90°.
fn ll_euler_yaw(quat: Quat) -> f32 {
    let (x, y, z, w) = (quat.x, quat.y, quat.z, quat.w);
    // The sine of the roll, the sine of the pitch, and the two intermediate
    // cosines the reference names `ys` / `xz`.
    let sx = 2.0 * (x * w - y * z);
    let sy = 2.0 * (y * w + x * z);
    let ys = w * w - y * y;
    let xz = x * x - z * z;
    let cx = ys - xz;
    let cy = (sx * sx + cx * cx).sqrt();
    if cy > GIMBAL_THRESHOLD {
        (2.0 * (z * w - x * y)).atan2(ys + xz)
    } else if sy > 0.0 {
        2.0 * (z + x).atan2(w + y)
    } else {
        2.0 * (z - x).atan2(w - y)
    }
}

/// The planar projection frame of face `face_id` in the tessellated prim, carried
/// into Second Life world space by the object's `rotation` and `position`, or
/// `None` if the face is missing or degenerate. Ports the arithmetic of
/// `LLFace::getPlanarProjectedParams`: pick the face's normal and a tangent from
/// its geometry, project the reconstructed binormal to get the frame scale and
/// the in-plane angle, and build the rotation from the rotated basis.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "dense ported vector algebra (getPlanarProjectedParams); the glam operators are the \
              readable form and cannot overflow for the finite face geometry"
)]
fn face_projection(
    prim: &PrimMesh,
    face_id: u16,
    rotation: Quat,
    position: Vec3,
) -> Option<FaceProjection> {
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
    let local_rot = Quat::from_mat3(&Mat3::from_cols(
        binormal_rot.cross(normal),
        binormal_rot,
        normal,
    ));
    Some(FaceProjection {
        // The reference's `local_rot * vol_mat.quaternion()`, in glam's order.
        face_rot: rotation * local_rot,
        face_pos: position,
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

#[cfg(test)]
mod tests {
    use super::{
        AlignTarget, AlignedPlacement, FaceProjection, MapPlacement, aligned_planar_te, anchor_of,
        ll_euler_yaw, wrap_unit,
    };
    use bevy::math::{Mat3, Quat, Vec2, Vec3};
    use bevy::prelude::{Entity, World};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        CircuitId, ObjectKey, PrimFaceId, PrimMesh, RegionLocalObjectId, ScopedObjectId,
        TextureEntry, Uuid, planar_texgen_uv,
    };

    use crate::world_api::SelectionSet;

    /// The reference's `LLFace::getPlanarProjectedParams` over a face given
    /// directly by its (unit) normal and tangent, rather than by looking them up
    /// in a tessellated prim — the same arithmetic
    /// [`super::face_projection`](super::face_projection) runs once it has picked
    /// those two vectors out of the geometry, so a test can pin the frame against
    /// the reference without standing up a prim.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "the same ported vector algebra as `face_projection`, over the same finite face \
                  geometry"
    )]
    fn projection_of(
        normal: Vec3,
        tangent: Vec3,
        rotation: Quat,
        position: Vec3,
    ) -> FaceProjection {
        let binormal = normal.cross(tangent).normalize();
        let projected = planar_texgen_uv(binormal.to_array(), normal.to_array(), [1.0, 1.0, 1.0]);
        let projected = Vec2::new(projected[0], projected[1]) - Vec2::splat(0.5);
        let proj_scale = projected.length();
        let unit = projected / proj_scale;
        let mut ang = unit.y.clamp(-1.0, 1.0).acos();
        if unit.x < 0.0 {
            ang = -ang;
        }
        let binormal_rot = Quat::from_axis_angle(normal, ang) * binormal;
        let local_rot = Quat::from_mat3(&Mat3::from_cols(
            binormal_rot.cross(normal),
            binormal_rot,
            normal,
        ));
        FaceProjection {
            face_rot: rotation * local_rot,
            face_pos: position,
            proj_scale,
        }
    }

    /// A unit cube's `+X` face, as the reference tessellation gives it.
    fn face_px(rotation: Quat, position: Vec3) -> FaceProjection {
        projection_of(Vec3::X, Vec3::Y, rotation, position)
    }

    /// A unit cube's `+Y` face.
    fn face_py(rotation: Quat, position: Vec3) -> FaceProjection {
        projection_of(Vec3::Y, Vec3::NEG_X, rotation, position)
    }

    /// A unit cube's top (`+Z`) face.
    fn face_top(rotation: Quat, position: Vec3) -> FaceProjection {
        projection_of(Vec3::Z, Vec3::X, rotation, position)
    }

    /// Assert two floats agree to the precision the reference extract printed.
    #[track_caller]
    fn close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1.0e-5,
            "expected {expected}, got {actual}"
        );
    }

    /// Assert a whole placement against the reference extract's numbers.
    #[track_caller]
    fn close_placement(actual: AlignedPlacement, expected: AlignedPlacement) {
        close(actual.offset.x, expected.offset.x);
        close(actual.offset.y, expected.offset.y);
        close(actual.scale.x, expected.scale.x);
        close(actual.scale.y, expected.scale.y);
        close(actual.rotation, expected.rotation);
    }

    /// The projection frames themselves match `getPlanarProjectedParams`: the
    /// `+X` face of an unrotated cube comes out as the 120° turn about
    /// `(1, 1, 1)`, the top face as the identity, and both measure a projected
    /// basis of 2.
    #[test]
    fn projection_frames_match_the_reference() {
        let px = face_px(Quat::IDENTITY, Vec3::ZERO);
        close(px.face_rot.x, 0.5);
        close(px.face_rot.y, 0.5);
        close(px.face_rot.z, 0.5);
        close(px.face_rot.w, 0.5);
        close(px.proj_scale, 2.0);

        let top = face_top(Quat::IDENTITY, Vec3::ZERO);
        close(top.face_rot.x, 0.0);
        close(top.face_rot.y, 0.0);
        close(top.face_rot.z, 0.0);
        close(top.face_rot.w, 1.0);
        close(top.proj_scale, 2.0);

        let py = face_py(Quat::IDENTITY, Vec3::ZERO);
        close(py.face_rot.x, 0.0);
        close(py.face_rot.y, 0.707_106_77);
        close(py.face_rot.z, 0.707_106_77);
        close(py.face_rot.w, 0.0);
        close(py.proj_scale, 2.0);
    }

    /// One object, `+X` anchor → `+Y` face, an untouched anchor placement: the
    /// aligned face keeps the placement and takes no rotation.
    #[test]
    fn one_object_neighbouring_faces_align_flat() {
        let placement = aligned_planar_te(
            &face_px(Quat::IDENTITY, Vec3::ZERO),
            &face_py(Quat::IDENTITY, Vec3::ZERO),
            MapPlacement {
                rotation: 0.0,
                scale: Vec2::ONE,
                offset: Vec2::ZERO,
            },
        );
        close_placement(
            placement,
            AlignedPlacement {
                offset: Vec2::ZERO,
                scale: Vec2::ONE,
                rotation: 0.0,
            },
        );
    }

    /// One object, `+X` anchor → top face, with the anchor's own texture turned
    /// and repeated: the reference's rotation for this pair comes out of the
    /// **gimbal-locked** branch of `getEulerAngles`, which is why
    /// [`ll_euler_yaw`] is a port rather than a `to_euler` call.
    #[test]
    fn one_object_rotated_map_takes_the_locked_yaw() {
        let placement = aligned_planar_te(
            &face_px(Quat::IDENTITY, Vec3::ZERO),
            &face_top(Quat::IDENTITY, Vec3::ZERO),
            MapPlacement {
                rotation: 0.5,
                scale: Vec2::new(2.0, 3.0),
                offset: Vec2::new(0.25, -0.125),
            },
        );
        close_placement(
            placement,
            AlignedPlacement {
                offset: Vec2::new(0.25, -0.125),
                scale: Vec2::new(2.0, 3.0),
                rotation: 2.070_796_3,
            },
        );
    }

    /// Two objects: the aligned face sits on a prim turned 30° about `Z` and
    /// moved away, so the centre distance enters the offset — the term a
    /// single-object align never sees.
    #[test]
    fn two_objects_shift_the_offset_by_the_centre_distance() {
        let z30 = Quat::from_axis_angle(Vec3::Z, core::f32::consts::FRAC_PI_6);
        let placement = aligned_planar_te(
            &face_px(Quat::IDENTITY, Vec3::ZERO),
            &face_py(z30, Vec3::new(3.5, -2.25, 1.0)),
            MapPlacement {
                rotation: 0.0,
                scale: Vec2::ONE,
                offset: Vec2::ZERO,
            },
        );
        close_placement(
            placement,
            AlignedPlacement {
                offset: Vec2::new(-0.5, 0.0),
                scale: Vec2::ONE,
                rotation: 3.141_592_5,
            },
        );
    }

    /// Two objects, the aligned prim turned 40° about `Y` and the anchor's map
    /// rotated and repeated.
    #[test]
    fn two_objects_with_a_tilted_target_and_a_rotated_map() {
        let y40 = Quat::from_axis_angle(Vec3::Y, 0.698_131_7);
        let placement = aligned_planar_te(
            &face_px(Quat::IDENTITY, Vec3::ZERO),
            &face_top(y40, Vec3::new(-1.5, 4.0, 0.5)),
            MapPlacement {
                rotation: 0.5,
                scale: Vec2::new(2.0, 3.0),
                offset: Vec2::new(0.25, -0.125),
            },
        );
        close_placement(
            placement,
            AlignedPlacement {
                offset: Vec2::new(0.250_171_66, -0.998_464_6),
                scale: Vec2::new(2.0, 3.0),
                rotation: 2.070_796_3,
            },
        );
    }

    /// Two objects with the **anchor** itself on a turned, moved prim — the case
    /// that pins the order of the anchor's own world rotation in `orig_st_rot`.
    #[test]
    fn two_objects_with_a_moved_anchor() {
        let z30 = Quat::from_axis_angle(Vec3::Z, core::f32::consts::FRAC_PI_6);
        let y40 = Quat::from_axis_angle(Vec3::Y, 0.698_131_7);
        let placement = aligned_planar_te(
            &face_px(z30, Vec3::new(1.0, 2.0, 3.0)),
            &face_top(y40, Vec3::new(-1.5, 4.0, 0.5)),
            MapPlacement {
                rotation: 0.25,
                scale: Vec2::new(1.5, 0.75),
                offset: Vec2::new(-0.4, 0.6),
            },
        );
        close_placement(
            placement,
            AlignedPlacement {
                offset: Vec2::new(0.412_508_5, -0.140_078_07),
                scale: Vec2::new(1.5, 0.75),
                rotation: 2.358_178_3,
            },
        );
    }

    /// The euler port's two branches: a plain turn about `Z` reads straight off,
    /// and a quaternion whose pitch is ±90° takes the locked half-angle form.
    #[test]
    fn euler_yaw_covers_both_branches() {
        close(ll_euler_yaw(Quat::from_axis_angle(Vec3::Z, 0.75)), 0.75);
        // The `+X`-face frame turned by half a right angle about Z — the pair
        // from `one_object_rotated_map_takes_the_locked_yaw`, whose pitch locks.
        let locked = Quat::from_xyzw(0.608_15, 0.360_75, 0.608_15, 0.360_75).normalize();
        close(ll_euler_yaw(locked), 2.070_796_3);
    }

    /// An offset keeps only its fractional part, with the sign of the input (the
    /// reference's truncating `(S32)` cast).
    #[test]
    fn offsets_wrap_toward_zero() {
        close(wrap_unit(0.25), 0.25);
        close(wrap_unit(1.75), 0.75);
        close(wrap_unit(-1.75), -0.75);
    }

    /// A scoped id for the anchor tests.
    fn scoped(id: u32) -> ScopedObjectId {
        ScopedObjectId {
            circuit: CircuitId::new(1),
            id: RegionLocalObjectId(id),
        }
    }

    /// An align target that carries nothing but the identity and face list
    /// [`anchor_of`] reads.
    fn target(entity: Entity, id: u32, selected: &[u16]) -> AlignTarget {
        AlignTarget {
            scoped: scoped(id),
            entity,
            entry: TextureEntry { faces: Vec::new() },
            selected: selected.to_vec(),
            prim: PrimMesh::new(),
            rotation: Quat::IDENTITY,
            position: Vec3::ZERO,
        }
    }

    /// The anchor is the **primary** object's last-touched face, not the
    /// lowest-indexed one and not a face of an earlier-selected object — the
    /// reference's `getLastSelectedTE` on the primary node.
    #[test]
    fn the_anchor_is_the_primary_last_touched_face() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let primary = world.spawn_empty().id();
        let mut selection = SelectionSet::default();
        selection.toggle_face(
            scoped(1),
            ObjectKey::from(Uuid::from_u128(1)),
            first,
            PrimFaceId::new(0),
        );
        selection.toggle_face(
            scoped(2),
            ObjectKey::from(Uuid::from_u128(2)),
            primary,
            PrimFaceId::new(1),
        );
        selection.toggle_face(
            scoped(2),
            ObjectKey::from(Uuid::from_u128(2)),
            primary,
            PrimFaceId::new(4),
        );
        let targets = [target(first, 1, &[0]), target(primary, 2, &[1, 4])];
        assert_eq!(anchor_of(&selection, &targets), Some((primary, 4)));
    }

    /// Un-picking the last-touched face falls the anchor back to the first face
    /// the walk visits — the reference's `getLastSelectedTE` returning `-1`
    /// because the face it names is no longer selected.
    #[test]
    fn a_deselected_last_face_falls_back_to_the_first_of_the_walk() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let primary = world.spawn_empty().id();
        let mut selection = SelectionSet::default();
        selection.toggle_face(
            scoped(1),
            ObjectKey::from(Uuid::from_u128(1)),
            first,
            PrimFaceId::new(3),
        );
        selection.toggle_face(
            scoped(2),
            ObjectKey::from(Uuid::from_u128(2)),
            primary,
            PrimFaceId::new(1),
        );
        selection.toggle_face(
            scoped(2),
            ObjectKey::from(Uuid::from_u128(2)),
            primary,
            PrimFaceId::new(4),
        );
        // Shift-click face 4 again: it leaves the set but stays the last touched.
        selection.toggle_face(
            scoped(2),
            ObjectKey::from(Uuid::from_u128(2)),
            primary,
            PrimFaceId::new(4),
        );
        let targets = [target(first, 1, &[3]), target(primary, 2, &[1])];
        assert_eq!(anchor_of(&selection, &targets), Some((first, 3)));
    }

    /// A whole-object selection has no picked face, so its anchor is face `0` —
    /// the reference's `selectAllTEs` resetting `mLastTESelected`.
    #[test]
    fn a_whole_object_selection_anchors_on_face_zero() {
        let mut world = World::new();
        let primary = world.spawn_empty().id();
        let mut selection = SelectionSet::default();
        selection.insert(scoped(7), ObjectKey::from(Uuid::from_u128(7)), primary);
        let targets = [target(primary, 7, &[0, 1, 2, 3, 4, 5])];
        assert_eq!(anchor_of(&selection, &targets), Some((primary, 0)));
    }
}
