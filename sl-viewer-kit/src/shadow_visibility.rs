//! Run the sun's shadow-caster visibility cull **off the per-frame critical
//! path** (`viewer-perf-pbr-shadow-cluster-rez`, the background/double-buffer
//! follow-up).
//!
//! Bevy's `check_dir_light_mesh_visibility` (in `bevy_light`) tests every shadow
//! caster against the directional light's cascade frusta **synchronously in
//! `PostUpdate`**, so the frame blocks on it (~6-16 ms on a rezzed aditi region,
//! on the main-thread-bound side). But — the crucial point — that work is only
//! the **frustum-culling decision** (which casters go into each cascade's
//! include-list); the shadow map itself is re-rendered from the casters' **live**
//! transforms every frame. So the include-list can lag by several frames with no
//! visible effect: a caster with a stale list entry still casts a correct shadow
//! at its current position, and the only artifact is a brief missing/extra
//! contribution when a caster crosses a cascade boundary — invisible in practice,
//! and far better than the shadows-off users fall back to.
//!
//! So we decouple it. Each frame the main schedule does only cheap work:
//!
//! * **apply** the most recently completed cull result — move its per-cascade
//!   lists into each light's `CascadesVisibleEntities`;
//! * **mark** `ViewVisibility` visible for that result's casters (so off-camera
//!   shadow casters keep rendering — this must run every frame because Bevy
//!   resets `ViewVisibility` each frame);
//! * **dispatch** the next cull: fold this frame's caster changes into a
//!   **persistent snapshot** (only the casters that changed / spawned / despawned
//!   are re-extracted — O(changed), not O(all)), gather the cascade frusta, and
//!   spawn an [`AsyncComputeTaskPool`] task that frustum-tests the snapshot.
//!
//! The heavy `intersects_obb` work runs on an async-compute thread (not the
//! render/compute pool render uses) and is picked up next frame — a one-frame
//! pipeline in the steady state, more frames stale only if a pass overruns
//! (then the previous result is reused). The snapshot is shared with the task via
//! [`Arc`] (a refcount bump, no copy), and updated in place next frame because
//! `apply` drops the finished task's `Arc` before `dispatch` mutates it. So the
//! per-frame critical-path cost is just the O(changed) snapshot update + the
//! `ViewVisibility` marking; the frustum tests leave the frame timeline entirely,
//! and the extraction cost no longer scales with the (static) caster count.
//!
//! ## Why a viewer-side replacement rather than a `bevy_light` fork
//!
//! Every type/fn this needs — [`check_point_light_mesh_visibility`],
//! [`SimulationLightSystems`], [`CascadesFrusta`], [`CascadesVisibleEntities`],
//! [`VisibleMeshEntities`] — is `pub`, and the `CheckLightVisibility` set is
//! referenced nowhere else in Bevy. So we disable Bevy's copy with an
//! always-false run condition on that set and add our own systems (plus a re-add
//! of the unchanged point/spot one, which shares the disabled set) with the same
//! ordering constraints — our own rendering policy, no patched Bevy graph.
//!
//! Known v1 limitations (documented, not bugs): `VisibilityRange` LOD is not
//! honoured for shadow casters on the async path (very rare on SL content), and
//! the round-robin amortisation of the earlier in-frame version is dropped — a
//! full cull runs per pass, which is cheap enough off the critical path.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use bevy::camera::primitives::{Aabb, CascadesFrusta, Frustum};
use bevy::camera::visibility::{
    CascadesVisibleEntities, NoCpuCulling, NoFrustumCulling, RenderLayers, SetViewVisibility as _,
    VisibilitySystems, VisibleMeshEntities,
};
use bevy::ecs::entity::EntityHashMap;
use bevy::light::{NotShadowCaster, SimulationLightSystems, check_point_light_mesh_visibility};
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

/// System set holding our replacement shadow-caster visibility systems.
///
/// Carries the same ordering constraints Bevy gives its own
/// `SimulationLightSystems::CheckLightVisibility` members, so downstream shadow
/// preparation sees the visible-entity lists at the same point in `PostUpdate`.
#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
struct ShadowVisibilitySet;

/// The cascade bit for cascade index `i` (`0` for `i >= 32`, which never occurs
/// for realistic cascade counts but keeps the shift total).
fn cascade_bit(index: usize) -> u32 {
    u32::try_from(index)
        .ok()
        .and_then(|shift| 1u32.checked_shl(shift))
        .unwrap_or(0)
}

/// A bitmask with the low `count` cascade bits set (all cascades visible).
fn all_cascades(count: usize) -> u32 {
    let mut mask = 0u32;
    for index in 0..count {
        mask |= cascade_bit(index);
    }
    mask
}

/// Compute a caster's cascade-membership bitmask by testing its oriented
/// bounding box against each cascade frustum (the expensive path, now off-thread).
///
/// Mirrors Bevy's inner test: near-plane culling is disabled because a shadow
/// caster can legitimately lie before a cascade's near plane, and
/// `NoFrustumCulling` casters are visible in every cascade.
fn compute_cascade_mask(
    view_frusta: &[Frustum],
    aabb: &Aabb,
    world_from_local: &Affine3A,
    no_frustum_culling: bool,
) -> u32 {
    if no_frustum_culling {
        return all_cascades(view_frusta.len());
    }
    let mut mask = 0u32;
    for (index, frustum) in view_frusta.iter().enumerate() {
        if frustum.intersects_obb(aabb, world_from_local, false, true) {
            mask |= cascade_bit(index);
        }
    }
    mask
}

/// Always-false run condition used to disable Bevy's own
/// `SimulationLightSystems::CheckLightVisibility` set so our replacement runs
/// instead.
const fn never() -> bool {
    false
}

/// One caster's immutable inputs, snapshotted out of the ECS for the off-thread
/// cull. Only inherited-visible casters are snapshotted. `Clone` for the rare
/// copy-on-write when a cull pass overruns and still holds the shared snapshot.
#[derive(Clone)]
struct CasterInput {
    /// The caster mesh entity.
    entity: Entity,
    /// `None` for a caster with no bounds — visible in every cascade, as Bevy
    /// treats it.
    bounds: Option<(Aabb, Affine3A)>,
    /// The caster's render layers, intersected against each view's mask.
    layers: RenderLayers,
    /// Whether the caster opts out of frustum culling (always in every cascade).
    no_frustum_culling: bool,
}

/// One directional shadow view's cascade frusta + render-layer mask.
struct ViewInput {
    /// The directional light entity owning this view's cascades.
    light: Entity,
    /// The cascade-view entity (a camera Bevy fits the cascades to).
    view: Entity,
    /// The view's per-cascade frusta.
    frusta: Vec<Frustum>,
    /// The view's render-layer mask.
    view_mask: RenderLayers,
}

/// The complete input handed to one off-thread cull pass.
struct CullJob {
    /// Every inherited-visible shadow caster to test — the pipeline's persistent
    /// snapshot, shared (not copied) with the task via [`Arc`].
    casters: Arc<Vec<CasterInput>>,
    /// Every shadow view to test them against.
    views: Vec<ViewInput>,
}

/// The result of one cull pass: per-light include-lists ready to drop straight
/// into each `CascadesVisibleEntities`, plus the union of visible casters for
/// `ViewVisibility` marking.
struct CullOutput {
    /// Per directional light: its per-view, per-cascade include-lists — already
    /// in the exact shape of [`CascadesVisibleEntities::entities`].
    lights: Vec<(Entity, EntityHashMap<Vec<VisibleMeshEntities>>)>,
    /// Every caster visible in any cascade (sorted, deduped).
    visible: Vec<Entity>,
    /// Casters considered — diagnostics only.
    caster_count: usize,
    /// Wall-clock of the pass — diagnostics only.
    cull_us: u128,
}

/// Run one full shadow-caster cull off-thread: for each shadow view, bucket every
/// caster into the cascades whose frustum its bounding box intersects.
fn run_shadow_cull(job: CullJob) -> CullOutput {
    let started = Instant::now();
    let mut by_light: HashMap<Entity, EntityHashMap<Vec<VisibleMeshEntities>>> = HashMap::new();
    // A caster is `ViewVisibility`-visible if it lands in any cascade of any
    // view. Track that per snapshot position in a bool vec (parallel to
    // `job.casters`) rather than pushing to a Vec and sort+dedup-ing at the end.
    // On a dense region the two sorts below — the per-cascade sort and this
    // visible-set sort+dedup — were ~half of this pass's CPU, and neither result
    // needs ordering: Bevy's own `check_dir_light_mesh_visibility` pushes both
    // cascade contents and its visible set unordered, and the shadow phase
    // re-sorts / batches downstream. The bool scan also yields each caster at
    // most once, so [`mark_shadow_caster_visibility`]'s `iter_many_mut` still
    // sees a unique set without a dedup.
    let mut seen = vec![false; job.casters.len()];

    for view in &job.views {
        let mut cascades: Vec<VisibleMeshEntities> =
            vec![VisibleMeshEntities::default(); view.frusta.len()];
        for (is_visible, caster) in seen.iter_mut().zip(job.casters.iter()) {
            if !view.view_mask.intersects(&caster.layers) {
                continue;
            }
            match &caster.bounds {
                Some((aabb, world_from_local)) => {
                    let mask = compute_cascade_mask(
                        &view.frusta,
                        aabb,
                        world_from_local,
                        caster.no_frustum_culling,
                    );
                    if mask != 0 {
                        *is_visible = true;
                    }
                    for (index, cascade) in cascades.iter_mut().enumerate() {
                        if mask & cascade_bit(index) != 0 {
                            cascade.entities.push(caster.entity);
                        }
                    }
                }
                None => {
                    *is_visible = true;
                    for cascade in &mut cascades {
                        cascade.entities.push(caster.entity);
                    }
                }
            }
        }
        // No per-cascade sort (see the `seen` comment above); just release the
        // spare capacity the pushes over-reserved.
        for cascade in &mut cascades {
            cascade.shrink();
        }
        by_light
            .entry(view.light)
            .or_default()
            .insert(view.view, cascades);
    }

    // Collect the visible set in one O(casters) scan — already unique, no sort.
    let visible: Vec<Entity> = job
        .casters
        .iter()
        .zip(&seen)
        .filter_map(|(caster, &is_visible)| is_visible.then_some(caster.entity))
        .collect();
    CullOutput {
        lights: by_light.into_iter().collect(),
        visible,
        caster_count: job.casters.len(),
        cull_us: started.elapsed().as_micros(),
    }
}

/// Rolling diagnostics for the async cull, flushed to one `info!` line per
/// second when `SL_VIEWER_LOG_SHADOW_CULL` is set.
#[derive(Default)]
struct ShadowCullDiag {
    /// Frames elapsed in this window.
    frames: u64,
    /// Cull passes that completed and were applied this window.
    applied: u64,
    /// Frames the in-flight pass wasn't ready and the previous result was reused.
    reused: u64,
    /// Summed wall-clock of the applied passes (microseconds).
    cull_us_sum: u128,
    /// Caster count of the most recent pass.
    caster_count: usize,
    /// Wall-clock start of the current window (`None` until the first frame).
    window_start: Option<Instant>,
}

/// Double-buffered pipeline for the off-thread shadow-caster cull.
#[derive(Resource, Default)]
struct ShadowCullPipeline {
    /// The in-flight cull, if any (at most one pass runs at a time).
    task: Option<Task<CullOutput>>,
    /// The casters visible in the most recently applied result — re-marked
    /// `ViewVisibility` every frame (empty until the first pass completes, so the
    /// scene is briefly shadowless right after login).
    visible: Vec<Entity>,
    /// Whether to emit the once-a-second `shadow_cull` line
    /// (`SL_VIEWER_LOG_SHADOW_CULL`, off by default).
    log_diag: bool,
    /// The persistent caster snapshot, maintained **incrementally**: each frame
    /// only the entries for casters that changed / spawned / despawned are
    /// updated, so the per-frame extraction cost is O(changed), not O(all).
    /// Shared with the in-flight cull pass via [`Arc`] (a refcount bump, no copy);
    /// [`Arc::make_mut`] mutates it in place because [`apply_shadow_cull`] drops
    /// the finished task — releasing its `Arc` — before [`dispatch_shadow_cull`]
    /// updates it. Only a rare pass *overrun* forces a copy-on-write.
    snapshot: Arc<Vec<CasterInput>>,
    /// Position of each caster in [`Self::snapshot`], for O(1) incremental
    /// update / swap-remove.
    index: EntityHashMap<usize>,
    /// Rolling once-a-second diagnostics.
    diag: ShadowCullDiag,
}

/// Apply the most recently completed cull pass, if ready, and — every frame —
/// reconcile each light's `CascadesVisibleEntities` **structure** to its current
/// cascade frusta.
///
/// The pass supplies the include-list *contents* (which casters are in each
/// cascade), keyed by the stable cascade-view entities. The *structure* (which
/// views exist, and how many cascades each has) must match **this** frame's
/// `CascadesFrusta` before extract, because the shadow render extracts one
/// subview per cascade of every current view and `expect()`s a matching entry.
/// This is exactly the bookkeeping Bevy's own system does at the top of every
/// run; skipping it (writing only the last pass's views) panics the render
/// (`bevy_pbr` `light.rs`: "Failed to get directional light visible entities for
/// cascade"). Never blocks — an unfinished pass just leaves the previous
/// contents in place (one more frame stale).
fn apply_shadow_cull(
    mut pipeline: ResMut<ShadowCullPipeline>,
    mut lights: Query<
        (
            Entity,
            &DirectionalLight,
            &CascadesFrusta,
            &mut CascadesVisibleEntities,
            &ViewVisibility,
        ),
        Without<SpotLight>,
    >,
) {
    // 1. Install a completed pass's contents, if one is ready.
    let ready = pipeline
        .task
        .as_mut()
        .and_then(|task| block_on(poll_once(task)));
    if let Some(output) = ready {
        pipeline.task = None;
        let CullOutput {
            lights: light_lists,
            visible,
            caster_count,
            cull_us,
        } = output;
        let mut per_light: HashMap<Entity, EntityHashMap<Vec<VisibleMeshEntities>>> =
            light_lists.into_iter().collect();
        for (light, _light, _frusta, mut cascades_visible, _visibility) in &mut lights {
            if let Some(entities) = per_light.remove(&light) {
                cascades_visible.entities = entities;
            }
        }
        pipeline.visible = visible;
        pipeline.diag.applied = pipeline.diag.applied.saturating_add(1);
        pipeline.diag.cull_us_sum = pipeline.diag.cull_us_sum.saturating_add(cull_us);
        pipeline.diag.caster_count = caster_count;
    } else if pipeline.task.is_some() {
        pipeline.diag.reused = pipeline.diag.reused.saturating_add(1);
    }

    // 2. Reconcile every light's include-list structure to its current frusta.
    for (_light, directional_light, frusta, mut cascades_visible, light_visibility) in &mut lights {
        if !directional_light.shadow_maps_enabled || !light_visibility.get() {
            cascades_visible.entities.clear();
            continue;
        }
        let mut views_to_remove = Vec::new();
        for (view, cascade_entities) in &mut cascades_visible.entities {
            match frusta.frusta.get(view) {
                Some(view_frusta) => {
                    cascade_entities.resize(view_frusta.len(), VisibleMeshEntities::default());
                }
                None => views_to_remove.push(*view),
            }
        }
        for (view, view_frusta) in &frusta.frusta {
            cascades_visible
                .entities
                .entry(*view)
                .or_insert_with(|| vec![VisibleMeshEntities::default(); view_frusta.len()]);
        }
        for view in views_to_remove {
            cascades_visible.entities.remove(&view);
        }
    }
}

/// Mark `ViewVisibility` visible for the last applied result's casters, every
/// frame — so off-camera shadow casters keep rendering into the shadow map (Bevy
/// resets `ViewVisibility` each frame, so this cannot be skipped between passes).
fn mark_shadow_caster_visibility(
    pipeline: Res<ShadowCullPipeline>,
    mut view_visibilities: Query<&mut ViewVisibility>,
) {
    if pipeline.visible.is_empty() {
        return;
    }
    let mut iter = view_visibilities.iter_many_mut(&pipeline.visible);
    while let Some(mut view_visibility) = iter.fetch_next() {
        view_visibility.set_visible();
    }
}

/// Remove `entity` from the persistent snapshot (and its index) if present, via
/// an O(1) swap-remove — repointing the index of whatever caster the swap moved.
fn remove_caster(index: &mut EntityHashMap<usize>, casters: &mut Vec<CasterInput>, entity: Entity) {
    if let Some(position) = index.remove(&entity) {
        casters.swap_remove(position);
        // Whatever was last is now at `position` (unless `position` was itself
        // the last) — repoint its index.
        if let Some(moved) = casters.get(position) {
            index.insert(moved.entity, position);
        }
    }
}

/// Keep the persistent caster snapshot current — **incrementally**, updating only
/// the casters that changed / spawned / despawned this frame — gather the cascade
/// frusta, and spawn the next off-thread cull pass over the shared snapshot.
///
/// This is the critical-path work, and the incremental update keeps it O(changed)
/// rather than O(all casters): most casters (buildings, trees) never change, so
/// their snapshot entries are left untouched, and only the handful that moved
/// (an avatar, a scripted mover) are re-extracted. The snapshot is shared with
/// the task via [`Arc`] — a refcount bump, no copy — and [`Arc::make_mut`] below
/// mutates it in place because [`apply_shadow_cull`] (chained before this) has
/// already dropped the finished task's `Arc`; only a rare pass overrun forces a
/// copy-on-write. Also flushes the once-a-second diagnostics.
#[expect(
    clippy::type_complexity,
    reason = "the changed-caster query mirrors Bevy's own tuple + filter set (minus the range test)"
)]
fn dispatch_shadow_cull(
    mut pipeline: ResMut<ShadowCullPipeline>,
    changed_casters: Query<
        (
            Entity,
            &InheritedVisibility,
            Option<&RenderLayers>,
            Option<&Aabb>,
            Option<&GlobalTransform>,
            Has<NoFrustumCulling>,
        ),
        (
            Or<(
                Changed<GlobalTransform>,
                Changed<Aabb>,
                Changed<RenderLayers>,
                Added<Mesh3d>,
                Changed<InheritedVisibility>,
            )>,
            Without<NotShadowCaster>,
            Without<DirectionalLight>,
            Without<NoCpuCulling>,
            With<Mesh3d>,
        ),
    >,
    mut removed_casters: RemovedComponents<Mesh3d>,
    lights: Query<
        (
            Entity,
            &DirectionalLight,
            &CascadesFrusta,
            Option<&RenderLayers>,
            &ViewVisibility,
        ),
        Without<SpotLight>,
    >,
) {
    flush_shadow_cull_diag(&mut pipeline);

    // 1. Fold this frame's caster changes into the persistent snapshot. Split the
    //    borrow so the snapshot and its index can be updated together; `make_mut`
    //    is in-place unless a prior pass is still holding the shared snapshot.
    {
        let ShadowCullPipeline {
            snapshot, index, ..
        } = &mut *pipeline;
        let casters = Arc::make_mut(snapshot);
        for entity in removed_casters.read() {
            remove_caster(index, casters, entity);
        }
        for (entity, inherited, maybe_layers, maybe_aabb, maybe_transform, no_frustum_culling) in
            &changed_casters
        {
            if !inherited.get() {
                // A caster that went hidden leaves the cull set.
                remove_caster(index, casters, entity);
                continue;
            }
            let input = CasterInput {
                entity,
                bounds: match (maybe_aabb, maybe_transform) {
                    (Some(aabb), Some(transform)) => Some((*aabb, transform.affine())),
                    _ => None,
                },
                layers: maybe_layers.cloned().unwrap_or_default(),
                no_frustum_culling,
            };
            match index.get(&entity).copied() {
                Some(position) => {
                    if let Some(slot) = casters.get_mut(position) {
                        *slot = input;
                    }
                }
                None => {
                    index.insert(entity, casters.len());
                    casters.push(input);
                }
            }
        }
    }

    // 2. Gather the shadow views' frusta (few — cheap to rebuild each frame).
    let mut views: Vec<ViewInput> = Vec::new();
    for (light, directional_light, frusta, maybe_mask, light_visibility) in &lights {
        if !directional_light.shadow_maps_enabled || !light_visibility.get() {
            continue;
        }
        let view_mask = maybe_mask.cloned().unwrap_or_default();
        for (view, view_frusta) in &frusta.frusta {
            views.push(ViewInput {
                light,
                view: *view,
                frusta: view_frusta.clone(),
                view_mask: view_mask.clone(),
            });
        }
    }
    if views.is_empty() {
        // No shadow-casting views: nothing to render, drop the stale visible set.
        pipeline.visible.clear();
        return;
    }

    // 3. Spawn the pass over the shared snapshot, unless one is still running.
    if pipeline.task.is_some() {
        return;
    }
    let job = CullJob {
        casters: Arc::clone(&pipeline.snapshot),
        views,
    };
    pipeline.task = Some(AsyncComputeTaskPool::get().spawn(async move { run_shadow_cull(job) }));
}

/// Emit the once-a-second `shadow_cull` diagnostic line and reset the window.
fn flush_shadow_cull_diag(pipeline: &mut ShadowCullPipeline) {
    pipeline.diag.frames = pipeline.diag.frames.saturating_add(1);
    let started = Instant::now();
    let window_start = *pipeline.diag.window_start.get_or_insert(started);
    if started.duration_since(window_start).as_secs() < 1 {
        return;
    }
    if pipeline.log_diag {
        let diag = &pipeline.diag;
        let applied = diag.applied.max(1);
        let mean_cull_us = diag
            .cull_us_sum
            .checked_div(u128::from(applied))
            .unwrap_or(0);
        info!(
            target: "shadow_cull",
            "async cull: fps~{} casters~{} pass mean={}us  applied={} reused={}",
            diag.frames, diag.caster_count, mean_cull_us, diag.applied, diag.reused
        );
    }
    pipeline.diag = ShadowCullDiag {
        window_start: Some(started),
        caster_count: pipeline.diag.caster_count,
        ..ShadowCullDiag::default()
    };
}

/// Installs the off-thread sun shadow-caster cull, replacing Bevy's per-frame
/// `check_dir_light_mesh_visibility`. `SL_VIEWER_SHADOW_CULL=off` keeps stock
/// Bevy (the A/B baseline); `SL_VIEWER_LOG_SHADOW_CULL` enables the readout.
#[derive(Debug)]
pub struct ShadowVisibilityPlugin;

impl Plugin for ShadowVisibilityPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("SL_VIEWER_SHADOW_CULL").ok().as_deref() == Some("off") {
            info!(
                target: "shadow_cull",
                "passthrough: keeping stock bevy check_dir_light_mesh_visibility \
                 (SL_VIEWER_SHADOW_CULL=off)"
            );
            return;
        }

        app.insert_resource(ShadowCullPipeline {
            log_diag: std::env::var_os("SL_VIEWER_LOG_SHADOW_CULL").is_some(),
            ..ShadowCullPipeline::default()
        });

        // Disable Bevy's own directional + point/spot caster-visibility systems
        // (both live in `CheckLightVisibility`) so ours runs in their place.
        app.configure_sets(
            PostUpdate,
            SimulationLightSystems::CheckLightVisibility.run_if(never),
        );

        // apply last result → mark ViewVisibility → dispatch next pass, then the
        // re-added point/spot system, all with the ordering Bevy gives the pair.
        app.add_systems(
            PostUpdate,
            (
                (
                    apply_shadow_cull,
                    mark_shadow_caster_visibility,
                    dispatch_shadow_cull,
                )
                    .chain(),
                check_point_light_mesh_visibility,
            )
                .in_set(ShadowVisibilitySet)
                .after(VisibilitySystems::CalculateBounds)
                .after(TransformSystems::Propagate)
                .after(SimulationLightSystems::UpdateLightFrusta)
                .after(VisibilitySystems::CheckVisibility)
                .before(VisibilitySystems::MarkNewlyHiddenEntitiesInvisible),
        );
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use pretty_assertions::assert_eq;

    use super::*;

    /// A real perspective frustum: a camera at `z = 10` looking down `-Z` at the
    /// origin, built through the same `CameraProjection::compute_frustum` path
    /// Bevy uses for every camera.
    fn test_frustum() -> Frustum {
        let projection = bevy::camera::PerspectiveProjection::default();
        let camera = GlobalTransform::from(
            Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        );
        bevy::camera::CameraProjection::compute_frustum(&projection, &camera)
    }

    #[test]
    fn cascade_bit_sets_the_expected_bit() {
        assert_eq!(cascade_bit(0), 0b1);
        assert_eq!(cascade_bit(1), 0b10);
        assert_eq!(cascade_bit(3), 0b1000);
        assert_eq!(cascade_bit(31), 0x8000_0000);
        assert_eq!(
            cascade_bit(32),
            0,
            "an out-of-range cascade index contributes no bit"
        );
    }

    #[test]
    fn all_cascades_sets_the_low_bits() {
        assert_eq!(all_cascades(0), 0);
        assert_eq!(all_cascades(1), 0b1);
        assert_eq!(all_cascades(4), 0b1111);
        assert_eq!(all_cascades(32), u32::MAX);
    }

    #[test]
    fn compute_cascade_mask_matches_frustum_membership() {
        let frusta = [test_frustum()];
        let identity = Affine3A::IDENTITY;
        let inside = Aabb::from_min_max(Vec3::splat(-0.5), Vec3::splat(0.5));
        let outside = Aabb::from_min_max(
            Vec3::new(9_999.5, -0.5, -0.5),
            Vec3::new(10_000.5, 0.5, 0.5),
        );
        assert_eq!(
            compute_cascade_mask(&frusta, &inside, &identity, false),
            0b1,
            "a caster in front of the camera is in the single cascade"
        );
        assert_eq!(
            compute_cascade_mask(&frusta, &outside, &identity, false),
            0,
            "a caster far off to the side is in no cascade"
        );
        assert_eq!(
            compute_cascade_mask(&frusta, &outside, &identity, true),
            0b1,
            "a NoFrustumCulling caster is in every cascade regardless of position"
        );
    }

    #[test]
    fn run_shadow_cull_buckets_and_marks_visible() {
        let light = Entity::from_raw_u32(1).expect("valid entity");
        let view = Entity::from_raw_u32(2).expect("valid entity");
        let inside = Entity::from_raw_u32(10).expect("valid entity");
        let outside = Entity::from_raw_u32(11).expect("valid entity");
        let job = CullJob {
            views: vec![ViewInput {
                light,
                view,
                frusta: vec![test_frustum()],
                view_mask: RenderLayers::default(),
            }],
            casters: Arc::new(vec![
                CasterInput {
                    entity: inside,
                    bounds: Some((
                        Aabb::from_min_max(Vec3::splat(-0.5), Vec3::splat(0.5)),
                        Affine3A::IDENTITY,
                    )),
                    layers: RenderLayers::default(),
                    no_frustum_culling: false,
                },
                CasterInput {
                    entity: outside,
                    bounds: Some((
                        Aabb::from_min_max(
                            Vec3::new(9_999.5, -0.5, -0.5),
                            Vec3::new(10_000.5, 0.5, 0.5),
                        ),
                        Affine3A::IDENTITY,
                    )),
                    layers: RenderLayers::default(),
                    no_frustum_culling: false,
                },
            ]),
        };
        let output = run_shadow_cull(job);
        assert_eq!(
            output.visible,
            vec![inside],
            "only the in-frustum caster is visible"
        );
        let (out_light, views) = output.lights.first().expect("one light");
        assert_eq!(*out_light, light);
        let cascades = views.get(&view).expect("the view's cascades");
        assert_eq!(
            cascades.first().expect("one cascade").entities,
            vec![inside],
            "the single cascade lists only the in-frustum caster"
        );
    }
}
