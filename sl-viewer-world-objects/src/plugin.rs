//! The object layer's schedule: [`WorldObjectsPlugin`].
//!
//! Everything this crate contributes to a frame — the stores it owns, the
//! messages it publishes, and the pipelines that turn a region's object stream
//! into drawn faces — is registered here, in the crate that owns it, rather
//! than in the binary that happens to compose it. That is the point: an
//! ordering edge between two of this crate's systems is a fact about this
//! crate, and a viewer, a headless harness and a test `App` should not each
//! have to restate it (see the roadmap task
//! `viewer-audit-plugins-own-their-schedule`).
//!
//! # What is scheduled here
//!
//! - **`PreUpdate`**: the copy-on-write detach net that gives an interned face
//!   a private material ahead of this frame's mutators, and the refill of the
//!   two shared per-frame upload budgets.
//! - **`Update`**: the texture and mesh stores' fetch pipelines, the object
//!   fold, the geometry builds the decodes feed, the face-material pipeline,
//!   the level-of-detail pass and the small per-frame drivers (flexi, texture
//!   animation, cache pruning).
//! - **`PostUpdate`**: the PBR half of the transparency cull, before Bevy
//!   propagates visibility.
//!
//! # What is **not**
//!
//! The render-side pipelines this crate carries — the name-tag billboard
//! material, the floating-text renderer over it, the material-preview rig — are
//! their own plugins, because a CPU-only harness takes the fold without them.
//! Likewise the avatar layer's own use of this crate: `apply_rigged_attachments`
//! and the bake input pipeline live in `sl-viewer-world-avatar` and order
//! themselves against this crate's systems by name, which they may, because
//! that crate sits above this one.

use bevy::camera::visibility::VisibilitySystems;
use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::ScheduleSystem;
use bevy::prelude::*;
use bevy::time::common_conditions::on_timer;
use sl_viewer_world_api::world_scoped::{WorldResetSystems, WorldScopedAppExt as _};
use sl_viewer_world_api::{BoostTexture, DecodedTextures, ObjectState, SelectionSet, WorldPhase};

use crate::asset_budget::{MeshUploadBudget, reset_mesh_upload_budget};
use crate::bump::{BumpManager, apply_bump_normals, register_bump_faces};
use crate::legacy_materials::{
    LegacyMaterialManager, apply_legacy_materials, apply_legacy_normal_maps,
    apply_legacy_specular_maps, drive_legacy_material_requests, receive_legacy_materials,
    register_legacy_materials,
};
use crate::material_cache::MaterialCache;
use crate::materials::{
    MaterialManager, apply_blinn_phong_hide, apply_material_overrides, apply_pbr_face_visibility,
    apply_pbr_textures, poll_materials, register_changed_render_materials, register_pbr_materials,
    revert_removed_render_materials, update_material_caps,
};
use crate::meshes::{MeshDecoded, MeshManager, poll_meshes, update_mesh_caps};
use crate::objects::{
    ObjectDiagnosticsPlugin, PendingDecodedMeshes, PendingDecodedSculpts, PendingObjectEvents,
    PrimLodTargets, TreeLodTargets, apply_object_meshes, apply_object_sculpts, apply_prim_lod,
    apply_tree_lod, recenter_objects, update_objects,
};
use crate::render_priority::drive_render_priority;
use crate::texture_anim::{drive_texture_animations, restore_stopped_animations};
use crate::textures::{
    DeferredFaceTextures, PrimTextures, TextureApplyBudget, TextureDecoded, TextureManager,
    apply_prim_textures, drain_deferred_face_textures, drain_lod_reuploads, poll_textures,
    reset_texture_apply_budget, serve_texture_boosts, sync_texture_blacklist, update_texture_caps,
};

/// The object layer: the prim / mesh stores, the textures and materials that
/// dress their faces, and everything that turns a region's `ObjectUpdate`
/// stream into drawn geometry.
///
/// Add it to any app that wants a world built out of objects. It needs no login
/// parameters and touches no render app of its own, so a headless fixture world
/// takes exactly this.
#[derive(Debug, Default, Clone, Copy)]
pub struct WorldObjectsPlugin;

impl Plugin for WorldObjectsPlugin {
    fn build(&self, app: &mut App) {
        register_state(app);
        schedule_frame(app);
        // The per-object land-impact model (GetObjectCost), read by the hover
        // tooltip and the build floater, and the object-store statistics the
        // `F3` overlay and the Tracy plots read.
        app.add_plugins(ObjectDiagnosticsPlugin);
        app.add_plugins(crate::object_cost::ObjectCostPlugin);
    }
}

/// The stores and messages the object layer owns.
fn register_state(app: &mut App) {
    // The two stores a **distant** teleport throws away: their keys are
    // scoped object ids of a region the session has just dropped.
    app.init_world_scoped::<ObjectState>();
    app.init_world_scoped::<PendingObjectEvents>();
    app.init_resource::<PendingDecodedMeshes>();
    app.init_resource::<PendingDecodedSculpts>();
    app.init_resource::<PrimLodTargets>();
    app.init_resource::<TreeLodTargets>();
    app.init_resource::<MeshUploadBudget>();
    app.init_resource::<MeshManager>();
    // The asset caches are keyed by grid-wide UUIDs, so they survive a
    // teleport on purpose: the destination may well want the same texture.
    app.init_resource::<TextureManager>();
    app.init_resource::<DecodedTextures>();
    app.init_resource::<PrimTextures>();
    app.init_resource::<TextureApplyBudget>();
    app.init_resource::<DeferredFaceTextures>();
    app.insert_resource(MaterialManager::new());
    app.init_resource::<LegacyMaterialManager>();
    app.init_resource::<BumpManager>();
    // The cross-instance caches: shared mesh handles for identical prim /
    // sculpt / mesh geometry (`viewer-perf-prim-tessellation-cache`) and
    // shared face-material handles for identical face content, so matched
    // copies batch into instanced draws (`viewer-perf-material-intern`).
    app.init_resource::<sl_viewer_kit::geometry_cache::GeometryCache>();
    app.init_resource::<MaterialCache>();
    // The build tools' selection, read by this crate's material systems
    // (`detach_shared_face_materials` gives a selected object's faces
    // private materials for the editors' live previews;
    // `apply_blinn_phong_hide` renders a selected linkset's PBR faces as
    // Blinn-Phong) — which the object layer schedules without depending on
    // the edit layer, so it must not assume the edit plugins are in the
    // app. An empty selection is the right default, and `init_resource`
    // leaves the edit layer's own registration in charge where both are
    // present.
    app.init_resource::<SelectionSet>();
    app.add_message::<TextureDecoded>();
    app.add_message::<BoostTexture>();
    app.add_message::<MeshDecoded>();
}

/// The object layer's frame.
fn schedule_frame(app: &mut App) {
    // The material cache's copy-on-write detach net: give any interned
    // (shared-material) face a private material before this frame's
    // `Update` mutators — texture animation, PBR registration, HUD
    // fullbright, the edit floaters' live previews — can write into the
    // shared asset. Scheduled in `PreUpdate` so the swap's commands are
    // applied at the schedule boundary, ahead of every mutator.
    app.add_systems(
        PreUpdate,
        crate::material_cache::detach_shared_face_materials,
    );
    // Refill the shared per-frame asset-upload budgets in `PreUpdate`,
    // ahead of every `Update` apply system that spends from them — the
    // image lane (`TextureApplyBudget`, drawn by the texture / PBR-map /
    // bump / legacy / bake systems) and the mesh lane (`MeshUploadBudget`,
    // drawn by object spawn / geometry / LOD / terrain). Resetting here
    // rather than inside the scattered `Update` tuples guarantees the
    // refill precedes all consumers regardless of their relative order.
    app.add_systems(
        PreUpdate,
        (reset_texture_apply_budget, reset_mesh_upload_budget),
    );
    // A distant teleport replaced the world: `ObjectState` empties itself in
    // `WorldResetSystems::Purge`, and that has to happen before the
    // re-centring pass — the purge drops the layer's origin anchor, so
    // re-centring afterwards simply anchors on the destination instead of
    // shifting the (already purged) scene by a delta from the region we
    // left. A crossing or a neighbour teleport keeps the world and never
    // purges at all.
    app.configure_sets(Update, WorldResetSystems::Purge.before(recenter_objects));
    app.add_systems(
        Update,
        (
            texture_store_pipeline(),
            mesh_store_pipeline(),
            // Re-base world-root objects onto the new origin (a crossing or
            // a teleport to an already-connected region) before folding
            // object events, so a static object stays put and a new object
            // is placed against the current origin.
            (recenter_objects, update_objects)
                .chain()
                .in_set(WorldPhase::ObjectsUpdated),
            // Build the geometry of any mesh object whose asset just
            // decoded, and of any sculpted prim whose sculpt map just
            // decoded — both spend from the shared `MeshUploadBudget`
            // (refilled in `PreUpdate`) so a decode burst's builds spread
            // across frames; the avatar layer's `apply_rigged_attachments`
            // spends from the same pool via its `.after(apply_object_meshes)`
            // edge.
            (apply_object_meshes, apply_object_sculpts).chain(),
            face_material_pipeline(),
            level_of_detail_pipeline(),
            // Flexi prims (P32.2): step each flexible prim's CPU chain
            // simulation and rewrite its deformed geometry in place, after
            // `update_objects` so this frame's spawns / rebuilds have seeded
            // their chain state.
            sl_viewer_kit::flexi::simulate_flexi.after(update_objects),
            // Animated textures (P28.2): advance every prim's
            // `llSetTextureAnim` and fold the current frame's UV / flipbook
            // placement into its faces, then reset a face to its static
            // placement when the animation stops.
            (drive_texture_animations, restore_stopped_animations),
            // Periodically drop cache entries whose shared meshes or
            // materials all died (every face entity despawned) — the caches
            // hold only weak asset ids, so that is bookkeeping, not asset
            // freeing.
            sl_viewer_kit::geometry_cache::prune_geometry_cache
                .run_if(on_timer(sl_viewer_kit::geometry_cache::PRUNE_INTERVAL)),
            crate::material_cache::prune_material_cache
                .run_if(on_timer(crate::material_cache::PRUNE_INTERVAL)),
        ),
    );
    // The glTF half of the transparency cull: write each PBR face's composed
    // verdict onto its `Visibility`. In `PostUpdate` rather than beside the
    // material systems above, so it is unambiguously after both the systems
    // that queue a verdict and the object build that re-describes a rebuilt
    // face with the *legacy* one — and before Bevy propagates visibility, so
    // the answer reaches this frame's draw.
    app.add_systems(
        PostUpdate,
        apply_pbr_face_visibility.before(VisibilitySystems::VisibilityPropagate),
    );
}

// ---------------------------------------------------------------------------
// The object layer's pipelines
// ---------------------------------------------------------------------------
//
// Each of the groups below is a *pipeline*: stage N's output is stage N+1's
// input, and the prose above each one says so. They used to be scheduled as
// plain tuples, which states no order at all -- Bevy is then free to run them
// in any order it likes, and with a multithreaded executor it does. A stage's
// output was therefore visible to the next stage a nondeterministic one to six
// frames later, in an order that could differ from frame to frame. See the
// roadmap task `viewer-audit-system-ordering-claims`.
//
// They are functions rather than inline tuples so the order can be *tested*:
// `ScheduleConfigs` is a value, so a test can put one into a bare `Schedule`,
// initialize it, and read back the order the executor will actually use. A
// comment claiming an order cannot be tested; this can.

/// The texture store's fetch pipeline: keep the `GetTexture` capability URL
/// current (and re-issue whatever was parked while it was unknown), mirror the
/// derender blacklist so a blacklisted asset is refused *before* any fetch,
/// poll the finished decodes, and drain the [`BoostTexture`] requests raised by
/// the crates that only show textures and cannot reach the manager directly.
///
/// Chained because the cap refresh is what un-parks the requests the poll then
/// finishes, and because the blacklist mirror is only a filter if it is in place
/// before the requests go out. Note the consumer edge lives on
/// [`face_material_pipeline`], which orders itself after this: the decodes this
/// announces are what it drapes onto faces.
fn texture_store_pipeline() -> ScheduleConfigs<ScheduleSystem> {
    (
        update_texture_caps,
        sync_texture_blacklist,
        poll_textures,
        serve_texture_boosts,
    )
        .chain()
}

/// The mesh store's fetch pipeline: keep the `GetMesh2` / `GetMesh` cap current
/// and poll the decodes.
///
/// Chained for the same reason as [`texture_store_pipeline`] — a cap refresh
/// un-parks the requests the poll finishes — and ordered before
/// [`apply_object_meshes`], the consumer of the [`MeshDecoded`] messages it
/// writes, so a mesh that decodes this frame is built this frame.
fn mesh_store_pipeline() -> ScheduleConfigs<ScheduleSystem> {
    (update_mesh_caps, poll_meshes)
        .chain()
        .before(apply_object_meshes)
}

/// Apply decoded diffuse textures to parked faces, then the PBR (GLTF)
/// render-material pipeline (P27.1), the legacy normal / specular one (P27.3),
/// and the per-face bump flags (P27.4).
///
/// The whole group is one chain because it is one pipeline, stage by stage:
/// keep the material store's `ViewerAsset` cap current, register each face that
/// carries a material, fold the finished fetches in, layer the simulator's and
/// the build tool's overrides on top, and only then drop each decoded texture
/// map into its slot. Running `poll_materials` before `update_material_caps`,
/// or `apply_pbr_textures` before the override that decides which map belongs
/// in the slot, does not lose the work — it defers it to some later frame, in
/// an order that can differ from one frame to the next.
///
/// The three sub-pipelines are ordered against each other too, and that is not
/// incidental: `revert_removed_render_materials` brings a face's legacy
/// specular / normal back when its PBR material is cleared, and the bump pass
/// runs last so a face's real `LLMaterial` normal map takes precedence over a
/// generated bump map.
///
/// The group's external edges are the two its prose always claimed: after
/// [`texture_store_pipeline`], whose decodes it drapes, and after the
/// face-spawning systems, so a face's material is registered in the frame the
/// face appears rather than the frame after.
fn face_material_pipeline() -> ScheduleConfigs<ScheduleSystem> {
    (
        // Amortise face-material re-preps across frames: refill the per-frame
        // budget, drape freshly decoded textures (deferring the overflow past a
        // decode burst), patch faces parked on an already-decoded texture (a
        // build-tool live-preview pre-fetch, then a commit re-tessellation) that
        // the decode-event-driven `apply_prim_textures` alone would strand, then
        // drain the deferred backlog (face drapes, then the lower-priority LOD
        // re-uploads) with whatever budget is left. Chained so each drain sees
        // the budget the earlier steps spent (see `TextureApplyBudget`).
        (
            apply_prim_textures,
            crate::textures::patch_parked_decoded_textures,
            drain_deferred_face_textures,
            drain_lod_reuploads,
        )
            .chain(),
        (
            update_material_caps,
            register_pbr_materials,
            // A render material assigned to an existing prim (build tool /
            // in-world retexture) refreshes its holder without re-tessellating
            // its faces, so register the change here — `register_pbr_materials`
            // only sees freshly-spawned faces.
            register_changed_render_materials,
            // Phase 3: a render material cleared in-world removes the holder, so
            // revert each of its faces to Blinn-Phong / diffuse (and bring back
            // their legacy specular / normal, no longer superseded).
            revert_removed_render_materials,
            poll_materials,
            apply_material_overrides,
            crate::materials::drive_local_overrides,
            apply_pbr_textures,
            // A map uploaded from a coarser decode than the store now holds is
            // rebuilt, after the first-use builds have had the image budget.
            crate::materials::refresh_pbr_textures,
            // FIRE-35138: while the build tool's Texture tab is on the
            // Blinn-Phong mode, render each selected linkset's PBR faces as
            // Blinn-Phong so they can be judged as edited; restore PBR on
            // deselect / PBR tab / leaving build mode.
            apply_blinn_phong_hide,
        )
            .chain(),
        (
            // The legacy (normal/specular) render-material pipeline (P27.3):
            // register each face carrying a `TextureEntry` material id, batch the
            // `RenderMaterials` cap requests, fold in the replies, and apply the
            // materials + their normal maps to the faces.
            register_legacy_materials,
            drive_legacy_material_requests,
            receive_legacy_materials,
            apply_legacy_materials,
            apply_legacy_normal_maps,
            apply_legacy_specular_maps,
            crate::legacy_materials::refresh_legacy_map_images,
            // The legacy per-face bump / shiny / glow / fullbright flags (P27.4):
            // register each newly-spawned bumped face and, once its diffuse
            // texture decodes, generate and assign its normal map (fullbright /
            // glow / shiny are folded in at material-build time by
            // `face_material`). Runs after the legacy material path so a face's
            // real `LLMaterial` normal map takes precedence over bump.
            register_bump_faces,
            apply_bump_normals,
            crate::bump::refresh_bump_normals,
        )
            .chain(),
    )
        .chain()
        .after(poll_textures)
        .after(WorldPhase::ObjectsUpdated)
        .after(apply_object_meshes)
}

/// On-screen render priority (P20.2) and the level-of-detail passes it drives.
///
/// `drive_render_priority` re-ranks the queued texture / mesh fetches by the
/// pixel area each object covers, so what the camera looks at loads first
/// (throttled internally). It also picks each plain prim's tessellation level of
/// detail (P21.3), which is why the two appliers run after it.
///
/// Budget the LOD re-tessellations across frames: `apply_prim_lod` and (P26.2)
/// `apply_tree_lod` — which regenerates any tree whose branching / billboard
/// tier the driver changed — each spend from the shared `MeshUploadBudget`
/// (refilled in `PreUpdate`), so a tick's whole batch spreads over frames
/// instead of a single command-flush spike. Chained so tree sees the budget prim
/// spent.
fn level_of_detail_pipeline() -> ScheduleConfigs<ScheduleSystem> {
    (drive_render_priority, apply_prim_lod, apply_tree_lod).chain()
}

#[cfg(test)]
mod tests {
    use super::{face_material_pipeline, mesh_store_pipeline, texture_store_pipeline};

    use bevy::prelude::*;
    use sl_viewer_world_api::WorldPhase;
    use sl_viewer_world_api::schedule_order::ScheduleOrder;
    use sl_viewer_world_api::stage;

    use crate::bump::{apply_bump_normals, register_bump_faces};
    use crate::legacy_materials::{
        apply_legacy_materials, apply_legacy_normal_maps, apply_legacy_specular_maps,
        drive_legacy_material_requests, receive_legacy_materials, register_legacy_materials,
    };
    use crate::materials::{
        apply_blinn_phong_hide, apply_material_overrides, apply_pbr_textures, poll_materials,
        register_changed_render_materials, register_pbr_materials, revert_removed_render_materials,
        update_material_caps,
    };
    use crate::meshes::{poll_meshes, update_mesh_caps};
    use crate::objects::{
        apply_object_meshes, apply_object_sculpts, recenter_objects, update_objects,
    };
    use crate::textures::{
        apply_prim_textures, drain_deferred_face_textures, drain_lod_reuploads, poll_textures,
        serve_texture_boosts, sync_texture_blacklist, update_texture_caps,
    };

    /// The object layer's fold, reduced to what an ordering assertion needs.
    ///
    /// The object-fold systems the pipelines order themselves against are
    /// registered too, in the shape [`super::WorldObjectsPlugin`] gives them:
    /// those edges are half of what is under test, and an edge naming a system
    /// that is not in the schedule constrains nothing.
    fn fold() -> ScheduleOrder {
        ScheduleOrder::of((
            texture_store_pipeline(),
            mesh_store_pipeline(),
            face_material_pipeline(),
            (apply_object_meshes, apply_object_sculpts).chain(),
            (recenter_objects, update_objects)
                .chain()
                .in_set(WorldPhase::ObjectsUpdated),
        ))
    }

    /// The texture store keeps its cap current and mirrors the blacklist before
    /// it polls, so a fetch that starts this frame starts against the current
    /// cap and a blacklisted asset is refused before it is ever requested.
    #[test]
    fn texture_store_runs_as_a_pipeline() {
        fold().assert_pipeline(&[
            stage!(update_texture_caps),
            stage!(sync_texture_blacklist),
            stage!(poll_textures),
            stage!(serve_texture_boosts),
        ]);
    }

    /// The mesh store polls against the cap it just refreshed, and the decodes
    /// it announces reach `apply_object_meshes` in the frame they land.
    #[test]
    fn mesh_store_runs_as_a_pipeline() {
        fold().assert_pipeline(&[
            stage!(update_mesh_caps),
            stage!(poll_meshes),
            stage!(apply_object_meshes),
        ]);
    }

    /// The face-material group is one pipeline end to end: diffuse drapes, then
    /// PBR, then the legacy normal / specular path, then bump — so a face's real
    /// `LLMaterial` normal map still beats a generated bump map.
    #[test]
    fn face_materials_run_as_a_pipeline() {
        fold().assert_pipeline(&[
            stage!(apply_prim_textures),
            stage!(crate::textures::patch_parked_decoded_textures),
            stage!(drain_deferred_face_textures),
            stage!(drain_lod_reuploads),
            stage!(update_material_caps),
            stage!(register_pbr_materials),
            stage!(register_changed_render_materials),
            stage!(revert_removed_render_materials),
            stage!(poll_materials),
            stage!(apply_material_overrides),
            stage!(crate::materials::drive_local_overrides),
            stage!(apply_pbr_textures),
            stage!(apply_blinn_phong_hide),
            stage!(register_legacy_materials),
            stage!(drive_legacy_material_requests),
            stage!(receive_legacy_materials),
            stage!(apply_legacy_materials),
            stage!(apply_legacy_normal_maps),
            stage!(apply_legacy_specular_maps),
            stage!(register_bump_faces),
            stage!(apply_bump_normals),
        ]);
    }

    /// The face-material group's external edges: it drapes the decodes
    /// `poll_textures` announced this frame, onto the faces the object fold and
    /// the mesh build spawned this frame — not next frame's.
    #[test]
    fn face_materials_run_after_their_inputs() {
        let fold = fold();
        fold.assert_pipeline(&[stage!(poll_textures), stage!(apply_prim_textures)]);
        fold.assert_pipeline(&[stage!(update_objects), stage!(apply_prim_textures)]);
        fold.assert_pipeline(&[stage!(apply_object_meshes), stage!(apply_prim_textures)]);
    }
}
