//! HUD picking and clicking (P35.3): the half of Phase 35 that makes a rendered
//! HUD ([`crate::hud`]) *usable*.
//!
//! [`crate::hud`] draws a worn HUD in screen space but nothing can touch it — a
//! HUD exists to be clicked. This module adds the pick path the reference viewer
//! runs alongside its world pick (`LLViewerWindow::cursorIntersect` with the HUD
//! matrices): a second, orthographic ray *through the HUD camera*, tried before
//! the world so a HUD covering half the screen never leaks a click to the ground
//! behind it.
//!
//! - **A free cursor, by default.** Outside mouselook the pointer is free
//!   ([`crate::camera`] only captures it in mouselook), so a left click has a
//!   cursor to pick with directly — the reference's model, where third person
//!   clicks the world. In mouselook the cursor is centred, so the pick falls back
//!   to the screen centre (the crosshair). The old `H` free-cursor toggle is gone:
//!   it only existed to escape the debug fly-camera's permanent grab.
//! - **A HUD pick, HUD before world.** On a left click ([`pick_and_touch`]) an
//!   orthographic ray is cast through the [`HudCamera`] at
//!   the cursor, restricted to the HUD render layer. If it hits, that HUD face is
//!   touched; only if nothing HUD-ward is under the cursor does the click fall
//!   through to a perspective world ray from the fly camera, exactly the
//!   reference's HUD-first order.
//! - **The surface the sim needs.** A touch carries a [`SurfaceInfo`] built from
//!   the hit ([`surface_info_from_hit`]): the picked face index, its texture
//!   (`UV`) and surface (`ST`) coordinates, and the intersection point / normal /
//!   binormal — the reference's `LLPickInfo::getSurfaceInfo`, which is what a
//!   script reads back through `llDetectedTouchFace` / `llDetectedTouchST` /
//!   `llDetectedTouchUV` / `llDetectedTouchPos` and friends.
//!
//! The touch itself reuses the existing `Session::touch_object` path (an
//! `ObjectGrab` immediately followed by an `ObjectDeGrab`, both now carrying the
//! surface block), so a click reaches a scripted object's `touch_start` /
//! `touch_end`.

use std::collections::HashSet;

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use sl_client_bevy::{
    Command, PrimFaceId, SlCommand, SurfaceInfo, TextureFace, Vector, texture_face_uv_transform,
};

use crate::camera::ViewerCamera;
use crate::hud::{HudCamera, on_hud_layer};
use crate::objects::{FaceTextureDebug, PrimFaceEntity, SceneObject};

/// The mouse button a HUD (or fall-through world) touch is made with.
const TOUCH_BUTTON: MouseButton = MouseButton::Left;

/// Everything the pick needs to identify and touch the object under a hit face.
type FaceQuery<'world, 'state> =
    Query<'world, 'state, (&'static PrimFaceEntity, &'static FaceTextureDebug)>;

/// On a left click, touch the HUD face under the cursor — or, if none, the world
/// object under it (the reference's HUD-first pick order).
///
/// The cursor is free in every camera mode except mouselook, so the click has a
/// pointer position to use directly; in mouselook (a centred, captured cursor) the
/// pick falls back to the screen centre (the crosshair). The HUD ray is
/// orthographic (through the HUD camera) and restricted to the HUD render layer;
/// the world ray is the ordinary perspective ray from the world camera through the
/// pick point, restricted to everything *not* on the HUD layer, so the two passes
/// never poach each other's geometry.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the \
              mouse button, the Alt modifier, the window for the cursor, the two cameras to cast \
              from, the ray caster, the render-layer / face / object components a hit is resolved \
              through, and the command channel the touch is sent on"
)]
pub(crate) fn pick_and_touch(
    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    hud_camera: Query<(&Camera, &GlobalTransform), With<HudCamera>>,
    fly_camera: Query<(&Camera, &GlobalTransform), With<ViewerCamera>>,
    layers: Query<(Entity, &RenderLayers)>,
    mut ray_cast: MeshRayCast,
    faces: FaceQuery,
    scene: Query<&SceneObject>,
    globals: Query<&GlobalTransform>,
    parents: Query<&ChildOf>,
    mut writer: MessageWriter<SlCommand>,
) {
    // A plain left-click touches; an `Alt`-held left-click is the camera focus /
    // orbit gesture (`crate::camera::focus_on_object`), not a touch, so ignore it.
    let alt = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);
    if !buttons.just_pressed(TOUCH_BUTTON) || alt {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    // The free cursor's position, or the screen centre when it is captured
    // (mouselook) — the crosshair the first-person view aims with.
    let cursor = window
        .cursor_position()
        .unwrap_or_else(|| Vec2::new(window.width() * 0.5, window.height() * 0.5));

    // The HUD entities the orthographic pass may hit — the whole routed HUD
    // subtree carries the HUD render layer (propagated from the screen), and
    // world geometry carries no render layers at all, so this set cleanly splits
    // the two passes.
    let hud_entities: HashSet<Entity> = layers
        .iter()
        .filter(|(_entity, layers)| on_hud_layer(Some(layers)))
        .map(|(entity, _layers)| entity)
        .collect();

    // 1. HUD first: an orthographic ray through the HUD camera at the cursor,
    //    limited to the HUD subtree.
    if let Ok((camera, camera_transform)) = hud_camera.single()
        && let Ok(ray) = camera.viewport_to_world(camera_transform, cursor)
    {
        let hud_filter = |entity: Entity| hud_entities.contains(&entity);
        let settings = MeshRayCastSettings::default()
            // Inherited visibility, not per-view: the HUD is drawn by its own
            // camera, and a HUD entity's `ViewVisibility` from the world camera
            // (which never renders it) would read false.
            .with_visibility(bevy::picking::mesh_picking::ray_cast::RayCastVisibility::Visible)
            .with_filter(&hud_filter);
        if let Some((entity, hit)) = ray_cast.cast_ray(ray, &settings).first().cloned()
            && touch_hit(
                entity,
                &hit,
                &faces,
                &scene,
                &globals,
                &parents,
                &mut writer,
                "HUD",
            )
        {
            return;
        }
    }

    // 2. Fall through to the world: a perspective ray from the fly camera through
    //    the cursor, limited to everything *not* on the HUD layer.
    if let Ok((camera, camera_transform)) = fly_camera.single()
        && let Ok(ray) = camera.viewport_to_world(camera_transform, cursor)
    {
        let world_filter = |entity: Entity| !hud_entities.contains(&entity);
        let settings = MeshRayCastSettings::default().with_filter(&world_filter);
        if let Some((entity, hit)) = ray_cast.cast_ray(ray, &settings).first().cloned() {
            touch_hit(
                entity,
                &hit,
                &faces,
                &scene,
                &globals,
                &parents,
                &mut writer,
                "world",
            );
        }
    }
}

/// Resolve a ray hit to its object and touch it, carrying the surface the ray
/// struck. Returns whether a touch was sent (a hit that resolves to no object —
/// e.g. an avatar's own mesh — sends nothing).
#[expect(
    clippy::too_many_arguments,
    reason = "the several components a hit is resolved through, threaded from the pick system"
)]
fn touch_hit(
    entity: Entity,
    hit: &bevy::picking::mesh_picking::ray_cast::RayMeshHit,
    faces: &FaceQuery,
    scene: &Query<&SceneObject>,
    globals: &Query<&GlobalTransform>,
    parents: &Query<&ChildOf>,
    writer: &mut MessageWriter<SlCommand>,
    which: &str,
) -> bool {
    // The ray strikes a face/submesh child entity: its Linden face index and the
    // per-face texture placement give the surface coordinates the sim wants.
    let face = faces.get(entity).ok();

    // Walk up the linkset to the object entity carrying the scene identity, whose
    // global maps the world hit back into the object's own Second Life frame.
    let mut current = entity;
    let scoped = loop {
        if let Ok(scene) = scene.get(current) {
            break Some(scene.scoped_id);
        }
        let Ok(child_of) = parents.get(current) else {
            break None;
        };
        current = child_of.parent();
    };
    let (Some(scoped), Ok(object_global)) = (scoped, globals.get(current)) else {
        return false;
    };

    let surface = surface_info_from_hit(
        hit,
        face.map(|(marker, _tf)| marker.face_id),
        face.map(|(_marker, FaceTextureDebug(tf))| tf),
        object_global,
    );
    info!(
        "P35.3 touch ({which}) object {scoped:?} face={} pos=({:.2},{:.2},{:.2})",
        surface.face_index, surface.position.x, surface.position.y, surface.position.z,
    );
    writer.write(SlCommand(Command::TouchObject {
        local_id: scoped,
        surface: Some(surface),
    }));
    true
}

/// Component-wise vector subtraction (`a - b`), avoiding the glam `-` operator the
/// workspace `arithmetic_side_effects` lint trips on.
fn vsub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Component-wise vector scaling (`v * s`).
fn vscale(v: Vec3, s: f32) -> Vec3 {
    Vec3::new(v.x * s, v.y * s, v.z * s)
}

/// Build the [`SurfaceInfo`] a touch carries from a ray hit, the picked face, and
/// the touched object's world transform — the viewer's `LLPickInfo::getSurfaceInfo`.
///
/// - **Face** is the Linden face index the ray struck (`-1` when the hit is not on
///   a textured face, the reference's "no intersection" value).
/// - **ST** is the face's own `[0, 1]` surface coordinate: the mesh's stored
///   texture coordinate, un-flipped from the bottom-up→top-down convention this
///   viewer bakes into `ATTRIBUTE_UV_0` back into Second Life's bottom-up space.
/// - **UV** is `ST` with the face's texture placement (repeats / offset /
///   rotation, [`texture_face_uv_transform`]) applied — the coordinate as the
///   texture is actually sampled, matching the reference's `surfaceToTexture`.
/// - **Position / normal / binormal** are given in the object's own Second Life
///   frame (its global's inverse carries the world hit back into it). A HUD lives
///   in screen space with no meaningful region position, so the object-local
///   frame is the sensible finite choice; the reference instead reports region /
///   HUD-matrix coordinates, a deliberate simplification here. The binormal is
///   derived geometrically (perpendicular to the normal, along the hit triangle)
///   rather than from a texture tangent the ray hit does not carry.
pub(crate) fn surface_info_from_hit(
    hit: &bevy::picking::mesh_picking::ray_cast::RayMeshHit,
    face_id: Option<PrimFaceId>,
    texture_face: Option<&TextureFace>,
    object_global: &GlobalTransform,
) -> SurfaceInfo {
    let inverse = object_global.affine().inverse();
    // The hit point / normal in the object's own Second Life frame (the object
    // subtree lives in Second Life space under the root's basis change, so the
    // inverse of its global lands here directly).
    let position = inverse.transform_point3(hit.point);
    let normal = inverse.transform_vector3(hit.normal).normalize_or_zero();

    // The binormal: perpendicular to the normal and along the surface, derived
    // from the hit triangle's first edge projected off the normal.
    let binormal = hit
        .triangle
        .map(|tri| {
            let edge = inverse.transform_vector3(vsub(tri[1], tri[0]));
            let along = vsub(edge, vscale(normal, edge.dot(normal)));
            normal.cross(along).normalize_or_zero()
        })
        .filter(|binormal| *binormal != Vec3::ZERO)
        .unwrap_or_else(|| normal.any_orthonormal_vector());

    // ST: the mesh's stored surface coordinate, back in Second Life bottom-up
    // space (this viewer flips `v` when building the Bevy mesh).
    let bevy_uv = hit.uv.unwrap_or(Vec2::ZERO);
    let st = Vec2::new(bevy_uv.x, 1.0 - bevy_uv.y);
    // UV: ST with the face's texture placement applied, as sampled — the
    // `uv_transform` acts in the Bevy (flipped) UV space, so flip back after.
    let placed = texture_face.map_or(bevy_uv, |tf| {
        texture_face_uv_transform(tf).transform_point2(bevy_uv)
    });
    let uv = Vec2::new(placed.x, 1.0 - placed.y);

    SurfaceInfo {
        uv: [uv.x, uv.y],
        st: [st.x, st.y],
        face_index: face_id.map_or(-1, |face| i32::from(face.get())),
        position: Vector {
            x: position.x,
            y: position.y,
            z: position.z,
        },
        normal: Vector {
            x: normal.x,
            y: normal.y,
            z: normal.z,
        },
        binormal: Vector {
            x: binormal.x,
            y: binormal.y,
            z: binormal.z,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{surface_info_from_hit, vscale, vsub};
    use bevy::math::{Affine3A, Quat, Vec2, Vec3};
    use bevy::picking::mesh_picking::ray_cast::RayMeshHit;
    use bevy::transform::components::GlobalTransform;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::PrimFaceId;
    use sl_client_bevy::TextureFace;

    /// A ray hit with the given world point / normal / uv and a flat triangle in
    /// the XY plane, enough to exercise the surface-info build.
    fn hit(point: Vec3, normal: Vec3, uv: Option<Vec2>) -> RayMeshHit {
        RayMeshHit {
            point,
            normal,
            barycentric_coords: Vec3::ZERO,
            distance: 1.0,
            triangle: Some([
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ]),
            uv,
            triangle_index: Some(0),
        }
    }

    /// The face index passes through, and a hit with no face reports `-1` (the
    /// reference's "no intersection" value a script sees as `llDetectedTouchFace`).
    #[test]
    fn face_index_passes_through_or_defaults_to_minus_one() {
        let object = GlobalTransform::IDENTITY;
        let with_face = surface_info_from_hit(
            &hit(Vec3::ZERO, Vec3::Z, Some(Vec2::new(0.25, 0.5))),
            Some(PrimFaceId::new(4)),
            None,
            &object,
        );
        assert_eq!(with_face.face_index, 4);
        let no_face = surface_info_from_hit(&hit(Vec3::ZERO, Vec3::Z, None), None, None, &object);
        assert_eq!(no_face.face_index, -1);
    }

    /// ST un-flips the Bevy (top-down) mesh coordinate back into Second Life's
    /// bottom-up space, and with an identity texture placement UV equals ST.
    #[test]
    fn st_unflips_and_uv_matches_without_placement() {
        let object = GlobalTransform::IDENTITY;
        let default_face =
            TextureFace::new(sl_client_bevy::TextureKey::from(sl_client_bevy::Uuid::nil()));
        let info = surface_info_from_hit(
            &hit(Vec3::ZERO, Vec3::Z, Some(Vec2::new(0.25, 0.75))),
            Some(PrimFaceId::new(0)),
            Some(&default_face),
            &object,
        );
        // ST: v flipped (1 - 0.75 = 0.25), u unchanged.
        assert!((info.st[0] - 0.25).abs() < 1e-6, "{:?}", info.st);
        assert!((info.st[1] - 0.25).abs() < 1e-6, "{:?}", info.st);
        // The identity placement leaves UV equal to ST.
        assert!((info.uv[0] - info.st[0]).abs() < 1e-6, "{:?}", info.uv);
        assert!((info.uv[1] - info.st[1]).abs() < 1e-6, "{:?}", info.uv);
    }

    /// A doubled texture repeat tiles the sampled coordinate: UV moves twice as
    /// far from the face centre as ST does.
    #[test]
    fn uv_applies_the_face_texture_repeats() {
        let object = GlobalTransform::IDENTITY;
        let mut face =
            TextureFace::new(sl_client_bevy::TextureKey::from(sl_client_bevy::Uuid::nil()));
        face.scale_s = 2.0;
        face.scale_t = 2.0;
        let info = surface_info_from_hit(
            &hit(Vec3::ZERO, Vec3::Z, Some(Vec2::new(1.0, 1.0))),
            Some(PrimFaceId::new(0)),
            Some(&face),
            &object,
        );
        // ST is the un-flipped stored coordinate; UV tiles it about the centre, so
        // the two differ once the repeat is above one.
        let differs =
            (info.uv[0] - info.st[0]).abs() > 1e-3 || (info.uv[1] - info.st[1]).abs() > 1e-3;
        assert!(differs, "st {:?} uv {:?}", info.st, info.uv);
    }

    /// The hit point and normal are carried into the object's own Second Life
    /// frame: a translated / rotated object un-does its transform, so the surface
    /// data is object-relative rather than world.
    #[test]
    fn position_and_normal_are_object_local() {
        // An object translated to (10, 0, 0) and yawed 90° about Z.
        let object = GlobalTransform::from(Affine3A::from_rotation_translation(
            Quat::from_rotation_z(core::f32::consts::FRAC_PI_2),
            Vec3::new(10.0, 0.0, 0.0),
        ));
        // A world hit at the object's origin, normal along world +X.
        let info = surface_info_from_hit(
            &hit(Vec3::new(10.0, 0.0, 0.0), Vec3::X, Some(Vec2::ZERO)),
            Some(PrimFaceId::new(0)),
            None,
            &object,
        );
        // The hit is at the object's own origin.
        assert!(info.position.x.abs() < 1e-5, "{:?}", info.position);
        assert!(info.position.y.abs() < 1e-5, "{:?}", info.position);
        // World +X, un-yawed by 90° about Z, becomes object-local -Y.
        assert!((info.normal.y + 1.0).abs() < 1e-5, "{:?}", info.normal);
        // The normal is a unit vector.
        let len = (info.normal.x * info.normal.x
            + info.normal.y * info.normal.y
            + info.normal.z * info.normal.z)
            .sqrt();
        assert!((len - 1.0).abs() < 1e-5, "normal not unit: {len}");
    }

    /// The binormal comes out perpendicular to the normal and unit length, so the
    /// surface frame the sim receives is well-formed.
    #[test]
    fn binormal_is_unit_and_perpendicular_to_the_normal() {
        let object = GlobalTransform::IDENTITY;
        let info = surface_info_from_hit(
            &hit(Vec3::ZERO, Vec3::Z, Some(Vec2::ZERO)),
            Some(PrimFaceId::new(0)),
            None,
            &object,
        );
        let dot = info.normal.x * info.binormal.x
            + info.normal.y * info.binormal.y
            + info.normal.z * info.binormal.z;
        assert!(dot.abs() < 1e-5, "binormal not perpendicular: {dot}");
        let len = (info.binormal.x * info.binormal.x
            + info.binormal.y * info.binormal.y
            + info.binormal.z * info.binormal.z)
            .sqrt();
        assert!((len - 1.0).abs() < 1e-5, "binormal not unit: {len}");
    }

    /// The vector helpers do plain component arithmetic.
    #[test]
    fn vector_helpers() {
        assert_eq!(
            vsub(Vec3::new(3.0, 2.0, 1.0), Vec3::new(1.0, 1.0, 1.0)),
            Vec3::new(2.0, 1.0, 0.0)
        );
        assert_eq!(
            vscale(Vec3::new(1.0, 2.0, 3.0), 2.0),
            Vec3::new(2.0, 4.0, 6.0)
        );
    }
}
