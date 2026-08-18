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
//! So we decouple it, but keep the cull's result **within the same frame** by
//! splitting the work across `PostUpdate`:
//!
//! * **dispatch** (early, right after the cascade frusta are built): fold this
//!   frame's caster changes into a **persistent snapshot** (only the casters that
//!   changed / spawned / despawned are re-extracted — O(changed), not O(all)),
//!   gather the cascade frusta, and spawn an [`AsyncComputeTaskPool`] task that
//!   frustum-tests the snapshot;
//! * **apply** (late, before extract): install that task's per-cascade lists into
//!   each light's `CascadesVisibleEntities` / `CascadesStaticVisibleEntities`;
//! * **mark** `ViewVisibility` visible for that result's casters (so off-camera
//!   shadow casters keep rendering — this must run every frame because Bevy
//!   resets `ViewVisibility` each frame).
//!
//! The heavy `intersects_obb` work runs on an async-compute thread (not the
//! render/compute pool render uses) while the intervening `PostUpdate` systems
//! run, and `apply` — later the same frame — `block_on`s it (which has almost
//! always already finished). Dispatching early and applying late means the cull
//! is applied in the **same** frame it was dispatched, not one frame later: that
//! zero lag is what keeps the static caster set consistent with the fork's
//! zero-lag static projection ([`viewer-perf-cached-static-shadow-map`]), so
//! cached static shadows do not blink as the camera pans across a coverage
//! rebuild. The snapshot is shared with the task via [`Arc`] (a refcount bump, no
//! copy) and updated in place next frame because the previous frame's `apply`
//! already dropped the finished task's `Arc`. The per-frame critical-path cost is
//! the O(changed) snapshot update + the `ViewVisibility` marking + (rarely) a
//! short `block_on`; the frustum tests otherwise leave the frame timeline, and the
//! extraction cost no longer scales with the (static) caster count.
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
    CascadesStaticVisibleEntities, CascadesVisibleEntities, NoCpuCulling, NoFrustumCulling,
    RenderLayers, SetViewVisibility as _, VisibilitySystems, VisibleMeshEntities,
};
use bevy::ecs::entity::EntityHashMap;
use bevy::light::{
    NotShadowCaster, SimulationLightSystems, StaticCascades, check_point_light_mesh_visibility,
};
use bevy::math::Affine3A;
use bevy::pbr::CachedStaticShadows;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on};

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
    /// The dispatch-frame index at which this caster last moved / spawned /
    /// changed shape (see [`ShadowCullPipeline::frame`]). Used by the
    /// cached-static shadow split ([`viewer-perf-cached-static-shadow-map`]) to
    /// classify a caster **dynamic** while it (or a client-side animation —
    /// avatars, flexi, spinners, physics) has moved within the settle window,
    /// and **static** once it has been still long enough. A caster that moves
    /// without a server `ObjectUpdate` (client animation) changes its
    /// `GlobalTransform` every frame, so it stays classified dynamic for free;
    /// one that settles ages out of the window and rejoins the retained static
    /// bake.
    last_moved_frame: u64,
}

/// One directional shadow view's cascade frusta + render-layer mask.
struct ViewInput {
    /// The directional light entity owning this view's cascades.
    light: Entity,
    /// The cascade-view entity (a camera Bevy fits the cascades to).
    view: Entity,
    /// The view's per-cascade **dynamic** frusta (re-fit to the camera each
    /// frame). Dynamic casters are culled against these.
    frusta: Vec<Frustum>,
    /// The view's per-cascade **static** frusta — the margin-expanded, retained
    /// projections (`StaticCascades`). Static casters are culled against these so
    /// a settled caster stays in the retained bake while the camera pans within
    /// the margin, instead of flickering in and out of the moving dynamic
    /// frustum. Same length as [`Self::frusta`]; falls back to `frusta` when the
    /// fork has not built static cascades for this view yet.
    static_frusta: Vec<Frustum>,
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
    /// The current dispatch-frame index, for the dynamic/static split.
    current_frame: u64,
    /// A caster counts as **dynamic** if it moved within this many dispatch
    /// frames of `current_frame`; otherwise it is **static** (retained bake).
    settle_frames: u64,
}

/// The result of one cull pass: per-light include-lists ready to drop straight
/// into each `CascadesVisibleEntities`, plus the union of visible casters for
/// `ViewVisibility` marking.
struct CullOutput {
    /// Per directional light: its per-view, per-cascade **dynamic** include-lists
    /// — already in the exact shape of [`CascadesVisibleEntities::entities`].
    /// When the split is disabled these hold every visible caster.
    lights: Vec<(Entity, EntityHashMap<Vec<VisibleMeshEntities>>)>,
    /// Per directional light: its per-view, per-cascade **static** include-lists,
    /// in the shape of `CascadesStaticVisibleEntities::entities`. Empty when the
    /// split is disabled.
    static_lights: Vec<(Entity, EntityHashMap<Vec<VisibleMeshEntities>>)>,
    /// Every caster visible in any cascade of any view (dynamic **or** static) —
    /// the full set to keep `ViewVisibility`-visible so both the dynamic and the
    /// static bake passes see their casters.
    visible: Vec<Entity>,
    /// The number of casters classified static this pass — diagnostics only.
    static_count: usize,
    /// Order-independent XOR hash of the static caster set, so the main thread
    /// can notice when the static set changes and trigger exactly one re-bake.
    static_hash: u64,
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
    let mut static_by_light: HashMap<Entity, EntityHashMap<Vec<VisibleMeshEntities>>> =
        HashMap::new();
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

    // Classify each caster once: dynamic if it moved within the settle window,
    // else static (retained bake). View-independent, so precompute in one scan.
    let is_dynamic: Vec<bool> = job
        .casters
        .iter()
        .map(|caster| {
            job.current_frame.saturating_sub(caster.last_moved_frame) <= job.settle_frames
        })
        .collect();
    // Count the static casters and fold their entities into an order-independent
    // XOR hash. A change in the hash means the static *set* changed — a caster
    // settled into, or moved out of, the retained bake — so the main thread must
    // re-bake exactly once. XOR is order-independent (the snapshot order is not
    // stable) and cheap; collisions are astronomically unlikely for this use.
    let mut static_count = 0usize;
    let mut static_hash = 0u64;
    for (caster, &dynamic) in job.casters.iter().zip(&is_dynamic) {
        if !dynamic {
            static_count = static_count.saturating_add(1);
            static_hash ^= caster.entity.to_bits();
        }
    }

    for view in &job.views {
        let mut dynamic_cascades: Vec<VisibleMeshEntities> =
            vec![VisibleMeshEntities::default(); view.frusta.len()];
        let mut static_cascades: Vec<VisibleMeshEntities> =
            vec![VisibleMeshEntities::default(); view.frusta.len()];
        for ((is_visible, caster), &dynamic) in
            seen.iter_mut().zip(job.casters.iter()).zip(&is_dynamic)
        {
            if !view.view_mask.intersects(&caster.layers) {
                continue;
            }
            // Bucket into the dynamic (per-frame) or static (retained bake) list,
            // each culled against its own frusta: dynamic casters against the
            // per-frame dynamic frusta, static casters against the margin-expanded
            // retained frusta so they do not flicker in/out of the moving dynamic
            // frustum as the camera pans.
            let (cascades, cull_frusta) = if dynamic {
                (&mut dynamic_cascades, &view.frusta)
            } else {
                (&mut static_cascades, &view.static_frusta)
            };
            match &caster.bounds {
                Some((aabb, world_from_local)) => {
                    let mask = compute_cascade_mask(
                        cull_frusta,
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
                    for cascade in cascades.iter_mut() {
                        cascade.entities.push(caster.entity);
                    }
                }
            }
        }
        // No per-cascade sort (see the `seen` comment above); just release the
        // spare capacity the pushes over-reserved.
        for cascade in dynamic_cascades
            .iter_mut()
            .chain(static_cascades.iter_mut())
        {
            cascade.shrink();
        }
        by_light
            .entry(view.light)
            .or_default()
            .insert(view.view, dynamic_cascades);
        static_by_light
            .entry(view.light)
            .or_default()
            .insert(view.view, static_cascades);
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
        static_lights: static_by_light.into_iter().collect(),
        visible,
        static_count,
        static_hash,
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
    /// Summed wall-clock of the applied passes (microseconds).
    cull_us_sum: u128,
    /// Caster count of the most recent pass.
    caster_count: usize,
    /// Static-caster count of the most recent pass (cached-static split).
    static_count: usize,
    /// Casters folded as `Changed` this window (spurious + real).
    changed_folded: u64,
    /// Of those, the ones whose bounds actually moved (reset their settle clock).
    real_moves: u64,
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

    // --- cached-static shadow split ([`viewer-perf-cached-static-shadow-map`]) ---
    /// The settle window (in dispatch frames): a caster still counts as dynamic
    /// this many frames after it last moved, then rejoins the static bake.
    settle_frames: u64,
    /// Monotonic dispatch-frame counter, stamped into a caster's
    /// [`CasterInput::last_moved_frame`] whenever it moves, and compared against
    /// the settle window to classify dynamic vs static.
    frame: u64,
    /// The static-set hash of the last applied pass; a change means a caster
    /// settled into, or moved out of, the retained bake, so re-bake once.
    last_static_hash: Option<u64>,
    /// A static-set change is awaiting a bake but has been deferred by the
    /// debounce (see [`BAKE_DEBOUNCE_FRAMES`]). Batches the per-frame churn of a
    /// rezzing region — where casters settle into the static set a few at a time
    /// every frame — into an occasional bake, since each bake force-requeues the
    /// whole static set (an expensive `specialize_shadows` / `queue_shadows`
    /// pass). Sparse changes (a single settle while parked) still bake promptly.
    static_bake_pending: bool,
    /// The dispatch frame of the last static bake, for the debounce window.
    last_bake_frame: u64,
    /// Whether the retained static shadow map must be (re-)baked this frame.
    /// Computed by [`apply_shadow_cull`] and consumed by
    /// [`drive_cached_shadow_config`] into the render-world [`CachedStaticShadows`]
    /// resource.
    bake_static: bool,
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
#[expect(
    clippy::type_complexity,
    reason = "the light query mirrors Bevy's own directional-light tuple, plus the \
              static visible-entities column this feature adds"
)]
fn apply_shadow_cull(
    mut pipeline: ResMut<ShadowCullPipeline>,
    mut lights: Query<
        (
            Entity,
            &DirectionalLight,
            &CascadesFrusta,
            &mut CascadesVisibleEntities,
            &mut CascadesStaticVisibleEntities,
            &ViewVisibility,
        ),
        Without<SpotLight>,
    >,
) {
    // 1. Install this frame's pass. `dispatch_shadow_cull` runs *earlier* in the
    //    same frame (right after the cascade frusta are built) and spawns the
    //    task off-thread; by the time we run — late in `PostUpdate`, after the
    //    intervening systems — it has almost always finished, so `block_on` waits
    //    only in the rare overrun. Applying the cull in the same frame it was
    //    dispatched (rather than one frame later) keeps the static caster set
    //    consistent with the fork's zero-lag static projection, which is what
    //    stops static shadows blinking as the camera pans across a coverage
    //    rebuild.
    let ready = pipeline.task.take().map(block_on);
    if let Some(output) = ready {
        let CullOutput {
            lights: light_lists,
            static_lights,
            visible,
            static_count,
            static_hash,
            caster_count,
            cull_us,
        } = output;
        let mut per_light: HashMap<Entity, EntityHashMap<Vec<VisibleMeshEntities>>> =
            light_lists.into_iter().collect();
        let mut per_light_static: HashMap<Entity, EntityHashMap<Vec<VisibleMeshEntities>>> =
            static_lights.into_iter().collect();
        for (light, _light, _frusta, mut cascades_visible, mut static_visible, _visibility) in
            &mut lights
        {
            if let Some(entities) = per_light.remove(&light) {
                cascades_visible.entities = entities;
            }
            if let Some(entities) = per_light_static.remove(&light) {
                static_visible.entities = entities;
            }
        }
        pipeline.visible = visible;

        // Re-bake the retained static map when the static caster **set** changed
        // (a caster settled into / moved out of the bake) — otherwise its last
        // bake is reused. The first applied pass has `last_static_hash == None`,
        // so the comparison already forces the initial bake. Projection
        // invalidation (camera / sun motion) is handled independently in the fork
        // (`StaticCascade`), which owns its own retained, margin-expanded
        // projection and re-bakes a cascade only when the camera leaves its
        // coverage — so it is deliberately *not* folded in here.
        let static_set_changed = pipeline.last_static_hash != Some(static_hash);
        pipeline.last_static_hash = Some(static_hash);
        if static_set_changed {
            pipeline.static_bake_pending = true;
        }
        // Debounce: coalesce rapid static-set churn (a rezzing region settles a
        // few casters into the bake every frame) into one bake per window, since
        // each bake force-requeues the *whole* static set. A change that has been
        // pending at least `BAKE_DEBOUNCE_FRAMES` bakes now; a lone change while
        // parked (its window already elapsed) still bakes immediately. The fork's
        // own projection invalidation (camera / sun motion) is independent and
        // not debounced.
        let debounced =
            pipeline.frame.saturating_sub(pipeline.last_bake_frame) >= BAKE_DEBOUNCE_FRAMES;
        pipeline.bake_static = pipeline.static_bake_pending && debounced;
        if pipeline.bake_static {
            pipeline.static_bake_pending = false;
            pipeline.last_bake_frame = pipeline.frame;
        }

        pipeline.diag.applied = pipeline.diag.applied.saturating_add(1);
        pipeline.diag.cull_us_sum = pipeline.diag.cull_us_sum.saturating_add(cull_us);
        pipeline.diag.caster_count = caster_count;
        pipeline.diag.static_count = static_count;
    } else {
        // No pass ran this frame (no shadow-casting views were dispatched): leave
        // the static map as it was (do not re-bake) so the retained content is
        // reused.
        pipeline.bake_static = false;
    }

    // 2. Reconcile every light's include-list structure (dynamic and static) to
    //    its current frusta — see this function's doc comment.
    for (
        _light,
        directional_light,
        frusta,
        mut cascades_visible,
        mut static_visible,
        light_visibility,
    ) in &mut lights
    {
        if !directional_light.shadow_maps_enabled || !light_visibility.get() {
            cascades_visible.entities.clear();
            static_visible.entities.clear();
            continue;
        }
        reconcile_cascade_structure(&mut cascades_visible.entities, frusta);
        reconcile_cascade_structure(&mut static_visible.entities, frusta);
    }
}

/// Reconcile a per-cascade include-list map's **structure** (which views exist
/// and how many cascades each has) to the light's current [`CascadesFrusta`],
/// preserving contents. Shared by the dynamic and static include-lists.
fn reconcile_cascade_structure(
    entities: &mut EntityHashMap<Vec<VisibleMeshEntities>>,
    frusta: &CascadesFrusta,
) {
    let mut views_to_remove = Vec::new();
    for (view, cascade_entities) in entities.iter_mut() {
        match frusta.frusta.get(view) {
            Some(view_frusta) => {
                cascade_entities.resize(view_frusta.len(), VisibleMeshEntities::default());
            }
            None => views_to_remove.push(*view),
        }
    }
    for (view, view_frusta) in &frusta.frusta {
        entities
            .entry(*view)
            .or_insert_with(|| vec![VisibleMeshEntities::default(); view_frusta.len()]);
    }
    for view in views_to_remove {
        entities.remove(&view);
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

/// Whether a caster's shadow-relevant bounds changed enough to count as a real
/// move (resetting its settle clock). Tolerates the sub-perceptible float noise
/// of a re-derived-but-unchanged transform so that a redundant `ObjectUpdate`
/// (the sim streams periodic terse updates for still objects) does not keep a
/// static caster perpetually out of the retained bake.
fn caster_bounds_changed(old: Option<&(Aabb, Affine3A)>, new: Option<&(Aabb, Affine3A)>) -> bool {
    /// Translation move (metres) below which a caster is treated as still.
    const TRANSLATION_EPSILON: f32 = 5.0e-3;
    /// Per-element tolerance for the rotation/scale matrix and the local AABB.
    const SHAPE_EPSILON: f32 = 1.0e-4;
    match (old, new) {
        (None, None) => false,
        (Some((old_aabb, old_xf)), Some((new_aabb, new_xf))) => {
            !old_xf
                .translation
                .abs_diff_eq(new_xf.translation, TRANSLATION_EPSILON)
                || !old_xf.matrix3.abs_diff_eq(new_xf.matrix3, SHAPE_EPSILON)
                || !old_aabb.center.abs_diff_eq(new_aabb.center, SHAPE_EPSILON)
                || !old_aabb
                    .half_extents
                    .abs_diff_eq(new_aabb.half_extents, SHAPE_EPSILON)
        }
        // Gained or lost bounds — treat as a change.
        _ => true,
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
/// mutates it in place because the **previous** frame's [`apply_shadow_cull`]
/// (which runs after this system each frame) already `block_on`-consumed and
/// dropped that pass's task `Arc`, leaving the snapshot uniquely owned here; only
/// a rare pass overrun forces a copy-on-write. Runs early in `PostUpdate` (right
/// after the cascade frusta are built) so the pass it spawns has the rest of the
/// frame to finish off-thread before `apply_shadow_cull` consumes it later this
/// same frame. Also flushes the once-a-second diagnostics.
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
            // The retained static cascade projections (fork), for culling the
            // static caster list against stable, margin-expanded frusta.
            &StaticCascades,
            Option<&RenderLayers>,
            &ViewVisibility,
        ),
        Without<SpotLight>,
    >,
    // Each shadow view's camera render layers, to skip views the sun does not
    // illuminate (probe-capture / gizmo / HUD / water-exclusion cameras): the
    // fork builds sun cascades for *every* camera but only renders shadows for
    // the ones whose layers intersect the sun's, so culling casters for the rest
    // is pure waste (it dominated the caster-cull cost on dense regions).
    view_render_layers: Query<Option<&RenderLayers>>,
) {
    flush_shadow_cull_diag(&mut pipeline);

    // Advance the dispatch-frame clock; a caster re-extracted this frame is
    // stamped with it (it just moved), and the settle window ages casters out of
    // the dynamic set relative to it.
    pipeline.frame = pipeline.frame.wrapping_add(1);
    let current_frame = pipeline.frame;

    // 1. Fold this frame's caster changes into the persistent snapshot. Split the
    //    borrow so the snapshot and its index can be updated together; `make_mut`
    //    is in-place unless a prior pass is still holding the shared snapshot.
    let mut changed_folded = 0u64;
    let mut real_moves = 0u64;
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
            let bounds = match (maybe_aabb, maybe_transform) {
                (Some(aabb), Some(transform)) => Some((*aabb, transform.affine())),
                _ => None,
            };
            match index.get(&entity).copied() {
                Some(position) => {
                    if let Some(slot) = casters.get_mut(position) {
                        // A `Changed<GlobalTransform>` fires even when the object
                        // system rewrites the *same* transform (the sim streams
                        // periodic terse `ObjectUpdate`s for still objects). Only
                        // treat the caster as having moved — resetting its settle
                        // clock, keeping it dynamic — when its bounds actually
                        // changed; otherwise a huge fraction of a region would
                        // never settle into the retained static bake.
                        let moved = caster_bounds_changed(slot.bounds.as_ref(), bounds.as_ref());
                        slot.bounds = bounds;
                        slot.layers = maybe_layers.cloned().unwrap_or_default();
                        slot.no_frustum_culling = no_frustum_culling;
                        changed_folded = changed_folded.saturating_add(1);
                        if moved {
                            slot.last_moved_frame = current_frame;
                            real_moves = real_moves.saturating_add(1);
                        }
                    }
                }
                None => {
                    index.insert(entity, casters.len());
                    casters.push(CasterInput {
                        entity,
                        bounds,
                        layers: maybe_layers.cloned().unwrap_or_default(),
                        no_frustum_culling,
                        // A newly-spawned caster starts dynamic until it settles.
                        last_moved_frame: current_frame,
                    });
                }
            }
        }
    }
    pipeline.diag.changed_folded = pipeline.diag.changed_folded.saturating_add(changed_folded);
    pipeline.diag.real_moves = pipeline.diag.real_moves.saturating_add(real_moves);

    // 2. Gather the shadow views' frusta (few — cheap to rebuild each frame).
    //    These drive the dynamic/static caster classification only; the retained
    //    static *projection* and its invalidation live in the fork
    //    (`StaticCascade`), so no projection hash is tracked here.
    let mut views: Vec<ViewInput> = Vec::new();
    for (light, directional_light, frusta, static_cascades, maybe_mask, light_visibility) in &lights
    {
        if !directional_light.shadow_maps_enabled || !light_visibility.get() {
            continue;
        }
        let view_mask = maybe_mask.cloned().unwrap_or_default();
        for (view, view_frusta) in &frusta.frusta {
            // Skip views the sun does not illuminate: the fork builds cascade
            // frusta for every camera, but `prepare_lights` only renders sun
            // shadows for views whose render layers intersect the light's, so
            // culling casters for the others (probe faces, gizmo/HUD/water masks)
            // produces lists nothing samples. Matching that filter here is the
            // single biggest cull-cost cut on a dense region (≈one view instead
            // of ~ten).
            let view_layers = view_render_layers
                .get(*view)
                .ok()
                .flatten()
                .cloned()
                .unwrap_or_default();
            if !view_mask.intersects(&view_layers) {
                continue;
            }
            // The static frusta come from the fork's retained `StaticCascades`
            // (margin-expanded, stable). Fall back to the dynamic frusta when the
            // fork has not built them for this view yet (e.g. the first frame).
            let static_frusta = static_cascades
                .cascades
                .get(view)
                .map(|cascades| {
                    cascades
                        .iter()
                        .map(|c| Frustum(ViewFrustum::from_clip_from_world(&c.clip_from_world)))
                        .collect::<Vec<_>>()
                })
                .filter(|f| f.len() == view_frusta.len())
                .unwrap_or_else(|| view_frusta.clone());
            views.push(ViewInput {
                light,
                view: *view,
                frusta: view_frusta.clone(),
                static_frusta,
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
        current_frame,
        settle_frames: pipeline.settle_frames,
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
            "async cull: fps~{} casters~{} (static~{}) changed/s={} moved/s={} pass mean={}us  applied={}",
            diag.frames, diag.caster_count, diag.static_count, diag.changed_folded, diag.real_moves,
            mean_cull_us, diag.applied
        );
    }
    pipeline.diag = ShadowCullDiag {
        window_start: Some(started),
        caster_count: pipeline.diag.caster_count,
        static_count: pipeline.diag.static_count,
        ..ShadowCullDiag::default()
    };
}

/// The settle window, in dispatch frames: a caster stays classified dynamic for
/// this many frames after it last moved, then rejoins the retained static bake.
/// ~0.5 s at 60 fps — long enough that a brief hitch does not thrash a caster
/// between the static and dynamic sets (each transition costs one re-bake), short
/// enough that a settled edit rejoins the cache promptly.
const DEFAULT_SETTLE_FRAMES: u64 = 30;

/// The static-bake debounce window, in dispatch frames (~0.2 s at 60 fps). While
/// a region rezzes, casters settle into the static set a few at a time every
/// frame; without a debounce each settle would force a full — expensive — static
/// re-queue. Coalescing those into one bake per window cuts the
/// `specialize_shadows` / `queue_shadows` spikes to an occasional cost, at the
/// price of a settled caster's shadow appearing up to this many frames late (only
/// while the region is actively rezzing — a sparse change while parked still
/// bakes on the next frame, since its window has long since elapsed).
const BAKE_DEBOUNCE_FRAMES: u64 = 12;

/// Push whether to re-bake the static shadow map this frame into the render world
/// (via the [`CachedStaticShadows`] extract-resource), so `bevy_pbr`'s forked
/// `prepare_lights`/`extract_lights` know when to render the static casters.
/// Written only on change, so it does not needlessly re-extract.
fn drive_cached_shadow_config(
    pipeline: Res<ShadowCullPipeline>,
    mut cached: ResMut<CachedStaticShadows>,
) {
    if cached.bake_static != pipeline.bake_static {
        cached.bake_static = pipeline.bake_static;
    }
}

/// Installs the off-thread sun shadow-caster cull, replacing Bevy's per-frame
/// `check_dir_light_mesh_visibility`. `SL_VIEWER_SHADOW_CULL=off` keeps stock
/// Bevy (the A/B baseline); `SL_VIEWER_LOG_SHADOW_CULL` enables the readout.
///
/// `SL_VIEWER_CACHED_SHADOW=on` additionally splits shadow casters into a retained
/// **static** bake (re-rendered only on invalidation) and a per-frame **dynamic**
/// pass ([`viewer-perf-cached-static-shadow-map`]); `=always` re-bakes the static
/// map every frame (the A/B correctness mode — the retained result must match).
pub(crate) struct ShadowVisibilityPlugin;

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

        info!(
            target: "shadow_cull",
            "cached-static shadow split active (settle={} frames)",
            DEFAULT_SETTLE_FRAMES,
        );

        app.insert_resource(ShadowCullPipeline {
            log_diag: std::env::var_os("SL_VIEWER_LOG_SHADOW_CULL").is_some(),
            settle_frames: DEFAULT_SETTLE_FRAMES,
            ..ShadowCullPipeline::default()
        });

        // Disable Bevy's own directional + point/spot caster-visibility systems
        // (both live in `CheckLightVisibility`) so ours runs in their place.
        app.configure_sets(
            PostUpdate,
            SimulationLightSystems::CheckLightVisibility.run_if(never),
        );

        // Dispatch the off-thread cull **early** (as soon as the cascade frusta
        // exist), then — later in the same frame — apply its result, mark
        // ViewVisibility, and push the static-bake config. Applying in the same
        // frame it was dispatched (rather than one frame later, as a
        // dispatch-last / apply-first order would) keeps the static caster set
        // consistent with the fork's zero-lag static projection, so static
        // shadows do not blink across a coverage rebuild while the camera pans.
        // `dispatch` and the apply group are ordered but **not** chained adjacent,
        // so Bevy can run the intervening `PostUpdate` systems in the gap while
        // the cull runs on the async pool — `apply` then usually finds it already
        // finished (it `block_on`s only in a rare overrun).
        app.add_systems(
            PostUpdate,
            (
                dispatch_shadow_cull,
                (
                    apply_shadow_cull,
                    mark_shadow_caster_visibility,
                    drive_cached_shadow_config,
                )
                    .chain()
                    .after(dispatch_shadow_cull),
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
            current_frame: 0,
            // Both casters moved this frame (age 0), so both are dynamic.
            settle_frames: 30,
            views: vec![ViewInput {
                light,
                view,
                frusta: vec![test_frustum()],
                static_frusta: vec![test_frustum()],
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
                    last_moved_frame: 0,
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
                    last_moved_frame: 0,
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

    /// An in-frustum unit-box caster with a chosen last-moved frame.
    fn in_frustum_caster(entity: Entity, last_moved_frame: u64) -> CasterInput {
        CasterInput {
            entity,
            bounds: Some((
                Aabb::from_min_max(Vec3::splat(-0.5), Vec3::splat(0.5)),
                Affine3A::IDENTITY,
            )),
            layers: RenderLayers::default(),
            no_frustum_culling: false,
            last_moved_frame,
        }
    }

    #[test]
    fn run_shadow_cull_splits_recently_moved_dynamic_from_settled_static() {
        let light = Entity::from_raw_u32(1).expect("valid entity");
        let view = Entity::from_raw_u32(2).expect("valid entity");
        // Both casters sit inside the same frustum; only their motion recency
        // differs.
        let mover = Entity::from_raw_u32(10).expect("valid entity");
        let settled = Entity::from_raw_u32(11).expect("valid entity");
        let job = CullJob {
            current_frame: 100,
            settle_frames: 10,
            views: vec![ViewInput {
                light,
                view,
                frusta: vec![test_frustum()],
                static_frusta: vec![test_frustum()],
                view_mask: RenderLayers::default(),
            }],
            casters: Arc::new(vec![
                // Moved this frame → within the settle window → dynamic.
                in_frustum_caster(mover, 100),
                // Last moved long ago → aged out of the window → static.
                in_frustum_caster(settled, 5),
            ]),
        };
        let output = run_shadow_cull(job);

        let dynamic = output
            .lights
            .iter()
            .find(|(l, _)| *l == light)
            .and_then(|(_, views)| views.get(&view))
            .and_then(|cascades| cascades.first())
            .expect("dynamic cascade for the view");
        assert_eq!(
            dynamic.entities,
            vec![mover],
            "the recently-moved caster is in the per-frame dynamic list"
        );

        let statics = output
            .static_lights
            .iter()
            .find(|(l, _)| *l == light)
            .and_then(|(_, views)| views.get(&view))
            .and_then(|cascades| cascades.first())
            .expect("static cascade for the view");
        assert_eq!(
            statics.entities,
            vec![settled],
            "the settled caster is in the retained static bake list"
        );

        // Both cast shadows, so both must stay `ViewVisibility`-visible.
        let mut visible = output.visible.clone();
        visible.sort_by_key(|entity| entity.to_bits());
        let mut expected = vec![mover, settled];
        expected.sort_by_key(|entity| entity.to_bits());
        assert_eq!(visible, expected);

        assert_eq!(output.static_count, 1, "one static caster");
        assert_eq!(
            output.static_hash,
            settled.to_bits(),
            "the static hash folds exactly the static set"
        );
    }

    #[test]
    fn run_shadow_cull_within_window_keeps_everything_dynamic() {
        let light = Entity::from_raw_u32(1).expect("valid entity");
        let view = Entity::from_raw_u32(2).expect("valid entity");
        let a = Entity::from_raw_u32(10).expect("valid entity");
        let b = Entity::from_raw_u32(11).expect("valid entity");
        let job = CullJob {
            current_frame: 1_000,
            // Both casters last moved at frame 0 (age 1000), still within the
            // window, so both are dynamic and the static set is empty.
            settle_frames: 1_500,
            views: vec![ViewInput {
                light,
                view,
                frusta: vec![test_frustum()],
                static_frusta: vec![test_frustum()],
                view_mask: RenderLayers::default(),
            }],
            casters: Arc::new(vec![in_frustum_caster(a, 0), in_frustum_caster(b, 0)]),
        };
        let output = run_shadow_cull(job);

        let mut dynamic = output
            .lights
            .iter()
            .find(|(l, _)| *l == light)
            .and_then(|(_, views)| views.get(&view))
            .and_then(|cascades| cascades.first())
            .expect("dynamic cascade")
            .entities
            .clone();
        dynamic.sort_by_key(|entity| entity.to_bits());
        let mut expected = vec![a, b];
        expected.sort_by_key(|entity| entity.to_bits());
        assert_eq!(dynamic, expected, "all casters are dynamic when split off");
        assert_eq!(output.static_count, 0, "no static casters when split off");
        assert_eq!(output.static_hash, 0, "empty static set hashes to zero");
    }
}
