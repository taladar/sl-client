//! On-screen render priority (P20.2): rank every queued texture and mesh fetch
//! by how large the thing appears on screen, so what the camera looks at loads
//! first.
//!
//! Everything is fetched through the LOD-aware texture / mesh store admission
//! gates, which order queued work by an opaque
//! [`Priority`]. This module computes that priority
//! from on-screen importance and feeds it back to the two managers each
//! throttled frame:
//!
//! - the pixel area an object covers is [`ScreenMetrics::pixel_area`] — the
//!   reference viewer's `LLPipeline::calcPixelArea`, driven by the object's world
//!   bounding radius, its distance from the camera, and the camera's vertical
//!   field of view;
//! - that area maps to a scheduling priority through [`Priority::from_pixel_area`]
//!   (the reference viewer's texture decode priority *is* the max on-screen
//!   virtual size, `LLViewerFetchedTexture::calcDecodePriority`);
//! - [`drive_render_priority`] recomputes it for every visible face and mesh a
//!   few times a second and calls [`TextureManager::set_priority`] /
//!   [`MeshManager::set_priority`], which re-rank the still-queued requests
//!   in place (a texture the camera turns toward rises, one it turns away sinks);
//! - the same pass also drives texture level-of-detail (P21.1): it calls
//!   [`TextureManager::set_lod_for_area`] with each face texture's on-screen
//!   pixel area, so a small / distant face is fetched (and kept) at a coarser
//!   discard level and upgraded as the camera approaches;
//! - mesh level-of-detail (P21.2): it computes each mesh object's
//!   [`MeshLod`] from its bounding radius and camera distance
//!   ([`MeshLod::for_distance`], the reference viewer's `LLVOVolume::calcLOD`)
//!   and calls [`MeshManager::set_lod_for_area`], so a small / distant mesh is
//!   fetched (and kept) at a coarser geometry block and upgraded as the camera
//!   approaches;
//! - and prim level-of-detail (P21.3): it computes each plain prim's
//!   [`PrimLod`] the same way ([`PrimLod::for_distance`], the same
//!   `LLVolumeLODGroup` tier selection) and records it in [`PrimLodTargets`], so
//!   `apply_prim_lod` re-tessellates a small / distant prim at a coarser detail
//!   and refines it as the camera approaches.
//!
//! Assets the pixel-area pass does not cover — terrain detail textures and avatar
//! textures / bakes — are requested at a fixed [boost](AVATAR_BOOST_PRIORITY)
//! instead (mirroring `LLGLTexture::BOOST_TERRAIN` / `BOOST_AVATAR`), so they are
//! not starved behind nearer prims. The own avatar / attachments / HUD would map
//! to the same full-resolution boost; those consumers arrive with later phases.

use bevy::prelude::*;
use std::collections::HashMap;

use sl_client_bevy::{
    DEFAULT_LOD_FACTOR, MeshKey, MeshLod, PrimLod, Priority, ScreenMetrics, TextureKey, TreeLod,
};

use crate::camera::FlyCamera;
use crate::meshes::MeshManager;
use crate::objects::{
    FaceTextureDebug, ObjectCategory, ObjectDebugInfo, PrimLodTargets, SceneObject, TreeLodTargets,
    TreeTier,
};
use crate::textures::TextureManager;

/// How often (seconds) the render-priority pass re-ranks the queued fetches. The
/// reference viewer re-derives every texture's virtual size once per frame; a few
/// times a second is ample here (a request only moves in the queue while it waits
/// behind the gate) and keeps the per-frame cost off the render thread.
const REPRIORITIZE_INTERVAL_SECS: f32 = 0.25;

/// The top of the pixel-area priority range: [`Priority::from_pixel_area`]
/// saturates here (`FULL_RESOLUTION_PIXEL_AREA` = `2048 * 2048`). Boost
/// priorities sit *strictly above* this, so a boosted asset always outranks even
/// the closest, largest prim face rather than merely tying with it on a region
/// dense with max-pixel-area content — mirroring how the reference viewer's
/// `BOOST_*` levels force a texture ahead of ordinary pixel-area-ranked content.
pub(crate) const PIXEL_AREA_CAP: u32 = 2048 * 2048;

/// Whether `priority` sits in the boost band strictly above the pixel-area range
/// (terrain / avatar / worn-attachment textures). A boosted texture is fetched
/// at full resolution and excluded from pixel-area LOD management (P21.1): its
/// skinned / joint-parented entity transform does not reflect its on-screen
/// size, so the face pass cannot rank it, and it is deliberately loaded at full
/// fidelity regardless of apparent size (the reference viewer's `BOOST_*`
/// textures likewise skip discard reduction).
pub(crate) const fn is_boost_priority(priority: Priority) -> bool {
    priority.get() > PIXEL_AREA_CAP
}

/// The fixed boost priority for a region's four terrain detail textures
/// (`LLGLTexture::BOOST_TERRAIN`): one step into the boost band, so the ground is
/// not starved behind nearer prims (the terrain textures are few and always
/// under the camera, and the on-screen face pass does not rank them — terrain is
/// a custom material, not a tessellated prim face).
pub(crate) const TERRAIN_BOOST_PRIORITY: Priority = Priority::new(PIXEL_AREA_CAP + 1);

/// The fixed boost priority for an avatar's textures and server-side bakes
/// (`LLGLTexture::BOOST_AVATAR` / `BOOST_AVATAR_BAKED`): above terrain, so the
/// avatars the camera is looking at resolve first even on a region dense with
/// max-pixel-area prims. The avatar is a skinned mesh, not a tessellated prim
/// face, so the on-screen face pass does not rank it — this boost is what keeps
/// its bakes ahead of the surrounding scene.
pub(crate) const AVATAR_BOOST_PRIORITY: Priority = Priority::new(PIXEL_AREA_CAP + 2);

/// The fixed boost priority for the sky's referenced textures — the rainbow /
/// halo (and, later, sun / moon / cloud / bloom) maps the atmospheric sky dome
/// samples (`LLGLTexture::BOOST_HIGH`). In the boost band so a sky texture
/// resolves ahead of ordinary pixel-area-ranked scene faces (the sky is drawn
/// behind everything and, like terrain, is a custom material the on-screen face
/// pass cannot rank), one step above the avatar boost.
pub(crate) const SKY_BOOST_PRIORITY: Priority = Priority::new(PIXEL_AREA_CAP + 3);

/// Re-rank every queued texture and mesh fetch by on-screen pixel area (P20.2),
/// throttled to [`REPRIORITIZE_INTERVAL_SECS`].
///
/// For each visible prim / sculpt / mesh face it computes the pixel area its
/// object covers (from the object's world bounding radius and camera distance)
/// and keeps the maximum area seen for each texture — the reference viewer's
/// `mMaxVirtualSize`, the largest any face using the texture reached this frame —
/// then feeds that through [`Priority::from_pixel_area`] to the texture manager.
/// Mesh geometry is ranked the same way from its owning object's pixel area.
///
/// Boosted assets (terrain, avatar) are requested at a fixed priority and are not
/// in these queries, so this pass leaves them at their boost.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system reading the camera, window, scene faces / objects, and both asset managers"
)]
pub(crate) fn drive_render_priority(
    time: Res<Time>,
    mut since_last: Local<f32>,
    // Qualified by `FlyCamera`: the reflection probes (P33.2) spawn a `Camera3d` per
    // probe-capture face, so a bare `With<Camera3d>` matches several and `single()`
    // fails — which silently switched this whole re-prioritisation off.
    camera: Query<(&GlobalTransform, &Projection), With<FlyCamera>>,
    windows: Query<&Window>,
    faces: Query<(&GlobalTransform, &FaceTextureDebug)>,
    objects: Query<(&GlobalTransform, &ObjectDebugInfo, &SceneObject)>,
    mut textures: ResMut<TextureManager>,
    mut meshes: ResMut<MeshManager>,
    mut prim_targets: ResMut<PrimLodTargets>,
    mut tree_targets: ResMut<TreeLodTargets>,
) {
    *since_last += time.delta_secs();
    if *since_last < REPRIORITIZE_INTERVAL_SECS {
        return;
    }
    *since_last = 0.0;

    let Ok((camera_transform, Projection::Perspective(perspective))) = camera.single() else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    let metrics = ScreenMetrics::new(window.height(), perspective.fov);
    let camera_position = camera_transform.translation();

    // The largest pixel area any face reached for each texture — the reference
    // viewer's per-texture `mMaxVirtualSize`.
    let mut texture_area: HashMap<TextureKey, f32> = HashMap::new();
    for (transform, FaceTextureDebug(face)) in &faces {
        let area = face_pixel_area(&metrics, transform, camera_position);
        let slot = texture_area.entry(face.texture_id).or_insert(0.0);
        *slot = slot.max(area);
    }

    // A mesh object's geometry is still fetching before its face entities exist,
    // so it is ranked from the object's own debug identity (its asset id + Second
    // Life scale) rather than a face. A mesh asset shared by several object
    // instances at different apparent sizes is aggregated the same way a texture
    // is: the largest pixel area (for priority) and the *finest* level any on-
    // screen instance warrants (for LOD) — so a shared mesh is not thrashed
    // between levels by whichever instance was visited last, and renders at the
    // fidelity its closest / largest use needs (P21.2). A sculpt's map is a
    // texture keyed by the same asset id, so its area is folded into the texture
    // aggregation; the store not fetching that id ignores it.
    let mut mesh_area: HashMap<MeshKey, f32> = HashMap::new();
    let mut mesh_lod: HashMap<MeshKey, MeshLod> = HashMap::new();
    // Fresh prim / tree LOD targets for this pass (P21.3 / P26.2); `apply_prim_lod`
    // / `apply_tree_lod` drain them.
    prim_targets.0.clear();
    tree_targets.0.clear();
    for (transform, info, scene) in &objects {
        // The object's full scale-vector length: its half is the bounding-sphere
        // radius for pixel area, while `LLVOVolume::calcLOD` ranks LOD against the
        // full length (`getScale().length()`), so the two uses differ (P21.2/P21.3).
        let scale_length = Vec3::from_array(info.scale()).length();
        let distance = camera_position.distance(transform.translation());
        let Some(asset) = info.render_asset() else {
            // A plain prim (no mesh / sculpt asset) is client-tessellation LOD
            // managed (P21.3): pick the tier its on-screen size warrants and hand
            // it to `apply_prim_lod`, which re-tessellates the prim on a change.
            // Each prim tessellates its own shape, so — unlike a shared mesh asset
            // — there is no cross-instance aggregation.
            if scene.category == ObjectCategory::Prim {
                let desired = PrimLod::for_distance(scale_length, distance, DEFAULT_LOD_FACTOR);
                prim_targets.0.insert(scene.scoped_id, desired);
            } else if scene.category == ObjectCategory::Tree {
                // A tree is procedurally generated (like a prim, no asset): pick the
                // branching tier its on-screen size warrants, or the billboard
                // imposter once it is tiny on screen (P26.2).
                let area = metrics.pixel_area(0.5 * scale_length, distance);
                let desired = tree_tier_for_size(scale_length, distance, area);
                tree_targets.0.insert(scene.scoped_id, desired);
            }
            continue;
        };
        let area = metrics.pixel_area(0.5 * scale_length, distance);
        let mesh_key = MeshKey::from(asset);
        let area_slot = mesh_area.entry(mesh_key).or_insert(0.0);
        *area_slot = area_slot.max(area);
        let desired = MeshLod::for_distance(scale_length, distance, DEFAULT_LOD_FACTOR);
        let lod_slot = mesh_lod.entry(mesh_key).or_insert(MeshLod::COARSEST);
        *lod_slot = lod_slot.finer_of(desired);
        // Offer the same area to the texture store, aggregated by the maximum, for
        // a sculpt map (or a mesh-asset id also used as a texture).
        let texture_slot = texture_area.entry(TextureKey::from(asset)).or_insert(0.0);
        *texture_slot = texture_slot.max(area);
    }

    for (id, area) in texture_area {
        textures.set_priority(id, Priority::from_pixel_area(area));
        // Pixel-area LOD (P21.1): pick the discard level the on-screen size of
        // the face warrants and upgrade / downgrade the store entry toward it.
        // A no-op for a boosted (full-resolution) or not-yet-decoded texture.
        textures.set_lod_for_area(id, area);
    }
    for (mesh_key, area) in mesh_area {
        meshes.set_priority(mesh_key, Priority::from_pixel_area(area));
    }
    for (mesh_key, desired) in mesh_lod {
        // Mesh LOD (P21.2): upgrade / downgrade the managed mesh toward the finest
        // level any on-screen instance warrants. A no-op for a boosted (finest,
        // unmanaged) or not-yet-decoded mesh.
        meshes.set_lod_for_area(mesh_key, desired);
    }
}

/// Pixel area below which a tree drops to the [`TreeTier::Billboard`] imposter —
/// once it covers only a small patch of screen, the crossed-quad billboard stands
/// in for the branch geometry (P26.2). Roughly a 24×24-pixel footprint.
const TREE_BILLBOARD_MAX_PIXEL_AREA: f32 = 576.0;

/// Select a tree's [`TreeTier`] from its on-screen size (P26.2): the crossed-quad
/// [`TreeTier::Billboard`] imposter once its pixel `area` is tiny, else the
/// branching [`TreeLod`] its distance warrants (the reference viewer's
/// `LLVOTree::mTrunkLOD` selection, reusing the shared `LLVOVolume::calcLOD` tier).
fn tree_tier_for_size(scale_length: f32, distance: f32, area: f32) -> TreeTier {
    if area < TREE_BILLBOARD_MAX_PIXEL_AREA {
        return TreeTier::Billboard;
    }
    // `MeshLod` indexes 0 (coarsest) ..= 3 (finest); `TreeLod` indexes 0 (finest,
    // `Highest`) ..= 3 (coarsest, `Low`), so the mesh tier maps by mirroring.
    let tier = MeshLod::for_distance(scale_length, distance, DEFAULT_LOD_FACTOR);
    TreeTier::Lod(TreeLod::from_index(
        3usize.saturating_sub(usize::from(tier.index())),
    ))
}

/// The pixel area a face's object covers: its world bounding radius is half the
/// diagonal of the object's world-space scale (carried on the face entity's
/// [`GlobalTransform`] by the object's geometry holder), and its distance is from
/// the camera to the object.
fn face_pixel_area(
    metrics: &ScreenMetrics,
    transform: &GlobalTransform,
    camera_position: Vec3,
) -> f32 {
    let (scale, _rotation, translation) = transform.to_scale_rotation_translation();
    let radius = 0.5 * scale.length();
    let distance = camera_position.distance(translation);
    metrics.pixel_area(radius, distance)
}
