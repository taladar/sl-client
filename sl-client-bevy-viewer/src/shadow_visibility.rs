//! Amortise the sun's shadow-caster visibility cull over several frames
//! (`viewer-perf-pbr-shadow-cluster-rez`).
//!
//! Bevy's `check_dir_light_mesh_visibility` (in `bevy_light`) re-tests **every**
//! shadow-casting mesh against the directional light's cascade frusta **every
//! frame**. Full-session Tracy captures of a rezzed Aditi region measured it at
//! ~8 ms/frame in the visible steady state — one serial system near the tail of
//! `PostUpdate`, on the main-thread-bound side of the frame, so nothing hides
//! its cost and it directly gates the frame rate.
//!
//! The retest is nearly all redundant: the cascade frusta only shift a little
//! frame-to-frame (the sun angle is texel-snapped and drifts slowly, the camera
//! drifts), so almost no *static* mesh's shadow-visibility actually flips. This
//! module replaces the Bevy system with one that:
//!
//! * re-runs the (expensive) per-cascade `intersects_obb` test only for a
//!   **round-robin `1/stride` slice** of casters each frame, cycling so every
//!   caster is re-tested at least once every `stride` frames — the same
//!   amortisation shape as the LOD-apply budget;
//! * re-tests **immediately** any caster that spawned, moved, was resized, or
//!   changed inherited visibility this frame (change detection), so dynamic
//!   content (walking avatars, scripted movers) is never stale;
//! * always re-tests casters carrying a [`VisibilityRange`] (their visibility is
//!   camera-distance dependent, so it can flip without the mesh itself
//!   changing);
//! * for every other (static) caster reuses its **cached per-cascade
//!   membership** from the last time it was tested.
//!
//! The cascade *lists* Bevy consumes (`CascadesVisibleEntities`) and the
//! per-caster [`ViewVisibility`] marking are still rebuilt in full every frame
//! from the fresh-or-cached membership, so a caster that stops being tested this
//! frame keeps rendering into the shadow map exactly as before — only the
//! frustum arithmetic is skipped. A static caster's cascade membership is
//! therefore at most `stride` frames stale, which is invisible at realistic sun
//! and camera speeds; a moved one is never stale.
//!
//! ## Why a viewer-side replacement rather than a `bevy_light` fork
//!
//! `check_dir_light_mesh_visibility` lives in `bevy_light`, but every type and
//! function it touches — the system itself, [`check_point_light_mesh_visibility`],
//! [`SimulationLightSystems`], [`CascadesFrusta`], [`CascadesVisibleEntities`],
//! [`VisibleMeshEntities`] — is `pub`, and the `CheckLightVisibility` set is
//! referenced nowhere else in Bevy. So we disable Bevy's copy with an
//! always-false run condition on that set and add our own directional system
//! (plus a re-add of the unchanged point/spot one, which shares the disabled
//! set) with the same ordering constraints. This keeps the amortisation as our
//! own rendering policy — not a Bevy bug fork — and needs no patched Bevy graph.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use bevy::camera::primitives::{Aabb, CascadesFrusta, Frustum};
use bevy::camera::visibility::{
    CascadesVisibleEntities, NoCpuCulling, NoFrustumCulling, RenderLayers, SetViewVisibility as _,
    VisibilityRange, VisibilitySystems, VisibleEntityRanges, VisibleMeshEntities,
};
use bevy::ecs::entity::EntityHashMap;
use bevy::light::{NotShadowCaster, SimulationLightSystems, check_point_light_mesh_visibility};
use bevy::prelude::*;
use bevy::utils::Parallel;

/// System set holding our replacement shadow-caster visibility systems.
///
/// Carries the same ordering constraints Bevy gives its own
/// `SimulationLightSystems::CheckLightVisibility` members, so downstream shadow
/// preparation sees the visible-entity lists at the same point in `PostUpdate`.
#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
struct ShadowVisibilitySet;

/// Round-robin period for the sun shadow-caster cull, from
/// `SL_VIEWER_SHADOW_CULL_STRIDE` (default [`DEFAULT_SHADOW_CULL_STRIDE`]).
///
/// `1` disables amortisation (every caster tested every frame, matching Bevy's
/// stock behaviour) — the A/B baseline. Higher values spread the frustum tests
/// over more frames: `stride` frames to re-test a fully static caster set.
const DEFAULT_SHADOW_CULL_STRIDE: usize = 4;

/// Per-cascade membership cache + round-robin cursor for the amortised sun
/// shadow-caster cull, plus its once-a-second self-diagnostics.
#[derive(Resource)]
struct ShadowCullState {
    /// Round-robin period; see [`DEFAULT_SHADOW_CULL_STRIDE`]. Runtime-mutable
    /// (the `Ctrl+Alt+S` cycle in [`cycle_shadow_cull_stride`]) so a single
    /// live session can A/B several strides against the *same* scene, which a
    /// startup-only env value can't (cross-run aditi captures differ in scene
    /// density and rez progress).
    stride: usize,
    /// Monotonic frame counter, advanced once per system run; its residue mod
    /// [`Self::stride`] selects this frame's round-robin bucket.
    frame: usize,
    /// Per (directional-light entity, cascade-view entity) cache of each
    /// caster's cascade membership as a bitmask (bit `i` = visible in cascade
    /// `i`). Persistent: only re-tested casters are re-written each frame, so a
    /// static scene pays no per-frame rebuild. Retired views are pruned; entries
    /// for despawned casters linger harmlessly (the `Entity` key carries the
    /// generation, so a reused index never false-hits) until [`Self::stride`]
    /// visits them or the view retires.
    caches: HashMap<(Entity, Entity), EntityHashMap<u32>>,
    /// Diagnostics accumulated since the last log line.
    diag: ShadowCullDiag,
    /// Whether to emit the once-a-second `shadow_cull` summary line
    /// (`SL_VIEWER_LOG_SHADOW_CULL`, off by default so a normal session is
    /// quiet). The `Ctrl+Alt+S` stride cycle always works; this only gates the
    /// readout.
    log_diag: bool,
}

/// Rolling diagnostics for the amortised cull, flushed to one `info!` line per
/// second. The `tested / total` ratio is the key signal: on a genuinely static
/// scene it should sit near `1/stride`; near `1.0` means change-detection is
/// re-testing (almost) every caster every frame, so amortisation can't help.
#[derive(Default)]
struct ShadowCullDiag {
    /// Frames accumulated into this window.
    frames: u64,
    /// Summed count of casters whose frustum test actually ran (`∑` over frames
    /// and views).
    tested: u64,
    /// Summed count of casters considered (reached the cull branch).
    total: u64,
    /// Summed wall-clock of the system body (matches its Tracy `system{}` zone).
    cull_ns: u128,
    /// Worst single-frame wall-clock in this window.
    cull_max_ns: u64,
    /// Wall-clock start of the current window (`None` until the first frame).
    window_start: Option<Instant>,
}

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
/// bounding box against each cascade frustum (the expensive path we amortise).
///
/// Mirrors Bevy's inner test: near-plane culling is disabled because a shadow
/// caster can legitimately lie before a cascade's near plane, and
/// `NoFrustumCulling` casters are visible in every cascade.
fn compute_cascade_mask(
    view_frusta: &[Frustum],
    aabb: &Aabb,
    world_from_local: &bevy::math::Affine3A,
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

/// Whether a caster must have its (expensive) per-cascade frustum test re-run
/// this frame, rather than reusing its cached membership.
///
/// A caster is re-tested when amortisation is off (`stride <= 1`), when it
/// `changed` (spawned / moved / resized / flipped inherited visibility) this
/// frame, when it carries a [`VisibilityRange`] (camera-distance dependent, so
/// it can flip without the mesh changing), when it has no `cached` result yet,
/// or when its index falls in this frame's round-robin bucket. Otherwise its
/// last-tested membership is reused, bounding a static caster's staleness to
/// `stride` frames.
const fn caster_needs_retest(
    stride: usize,
    frame_bucket: usize,
    entity_index: usize,
    changed: bool,
    has_visibility_range: bool,
    cached: bool,
) -> bool {
    if stride <= 1 || changed || has_visibility_range || !cached {
        return true;
    }
    match entity_index.checked_rem(stride) {
        Some(bucket) => bucket == frame_bucket,
        None => false,
    }
}

/// Always-false run condition used to disable Bevy's own
/// `SimulationLightSystems::CheckLightVisibility` set so our replacement runs
/// instead.
const fn never() -> bool {
    false
}

/// Amortised replacement for Bevy's `check_dir_light_mesh_visibility`.
///
/// See the module docs for the strategy. The view-bookkeeping, render-layer /
/// visibility-range gating, list collection and deferred `ViewVisibility`
/// marking are ported faithfully from Bevy 0.19's system; the only behavioural
/// change is that the per-cascade `intersects_obb` test is skipped (and its
/// prior result reused) for casters outside this frame's round-robin slice that
/// did not change.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected queries / resources / \
              thread-local scratch; this ports Bevy's own six plus the cache state"
)]
#[expect(
    clippy::type_complexity,
    reason = "the caster query mirrors Bevy's own tuple + filter set verbatim"
)]
fn check_dir_light_mesh_visibility_amortised(
    mut commands: Commands,
    mut state: ResMut<ShadowCullState>,
    mut directional_lights: Query<
        (
            Entity,
            &DirectionalLight,
            &CascadesFrusta,
            &mut CascadesVisibleEntities,
            Option<&RenderLayers>,
            &ViewVisibility,
        ),
        Without<SpotLight>,
    >,
    visible_entity_query: Query<
        (
            Entity,
            Ref<InheritedVisibility>,
            Option<&RenderLayers>,
            Option<Ref<Aabb>>,
            Option<Ref<GlobalTransform>>,
            Has<VisibilityRange>,
            Has<NoFrustumCulling>,
        ),
        (
            Without<NotShadowCaster>,
            Without<DirectionalLight>,
            Without<NoCpuCulling>,
            With<Mesh3d>,
        ),
    >,
    visible_entity_ranges: Option<Res<VisibleEntityRanges>>,
    mut defer_visible_entities_queue: Local<Parallel<Vec<Entity>>>,
    mut view_visible_entities_queue: Local<Parallel<Vec<Vec<Entity>>>>,
    mut cache_pairs_queue: Local<Parallel<Vec<(Entity, u32)>>>,
    // Per-thread (total, tested) caster counters for the once-a-second diag.
    mut counts_queue: Local<Parallel<(u64, u64)>>,
) {
    let started = Instant::now();
    let visible_entity_ranges = visible_entity_ranges.as_deref();

    state.frame = state.frame.wrapping_add(1);
    let stride = state.stride.max(1);
    let frame_bucket = state.frame.checked_rem(stride).unwrap_or(0);

    // Every (light, view) touched this frame; the cache retains only these, so a
    // despawned light or retired cascade view drops out.
    let mut seen_keys: HashSet<(Entity, Entity)> = HashSet::new();
    // Casters considered / actually frustum-tested this frame (all lights+views).
    let mut frame_total = 0u64;
    let mut frame_tested = 0u64;

    for (
        light_entity,
        directional_light,
        frusta,
        mut visible_entities,
        maybe_view_mask,
        light_view_visibility,
    ) in &mut directional_lights
    {
        let mut views_to_remove = Vec::new();
        for (view, cascade_view_entities) in &mut visible_entities.entities {
            match frusta.frusta.get(view) {
                Some(view_frusta) => {
                    cascade_view_entities.resize(view_frusta.len(), VisibleMeshEntities::default());
                }
                None => views_to_remove.push(*view),
            }
        }
        for (view, frusta) in &frusta.frusta {
            visible_entities
                .entities
                .entry(*view)
                .or_insert_with(|| vec![VisibleMeshEntities::default(); frusta.len()]);
        }
        for view in views_to_remove {
            visible_entities.entities.remove(&view);
        }

        // NOTE: If shadow mapping is disabled for the light then it must have no
        // visible entities. Its cache keys are simply not re-seen and get pruned
        // below, so re-enabling shadows re-tests every caster from cold.
        if !directional_light.shadow_maps_enabled || !light_view_visibility.get() {
            visible_entities.entities.clear();
            continue;
        }

        let view_mask = maybe_view_mask.unwrap_or_default();

        for (view, view_frusta) in &frusta.frusta {
            let view_entity = *view;
            let key = (light_entity, view_entity);
            let old_cache = state.caches.get(&key);

            visible_entity_query.par_iter().for_each_init(
                || {
                    let mut entities = view_visible_entities_queue.borrow_local_mut();
                    entities.resize(view_frusta.len(), Vec::default());
                    (
                        defer_visible_entities_queue.borrow_local_mut(),
                        entities,
                        cache_pairs_queue.borrow_local_mut(),
                        counts_queue.borrow_local_mut(),
                    )
                },
                |(defer_local, view_local, cache_local, counts_local),
                 (
                    entity,
                    inherited_visibility,
                    maybe_entity_mask,
                    maybe_aabb,
                    maybe_transform,
                    has_visibility_range,
                    has_no_frustum_culling,
                )| {
                    if !inherited_visibility.get() {
                        return;
                    }

                    let entity_mask = maybe_entity_mask.unwrap_or_default();
                    if !view_mask.intersects(entity_mask) {
                        return;
                    }

                    // Check visibility ranges.
                    if has_visibility_range
                        && visible_entity_ranges.is_some_and(|visible_entity_ranges| {
                            !visible_entity_ranges.entity_is_in_range_of_view(entity, view_entity)
                        })
                    {
                        return;
                    }

                    let cached = old_cache.and_then(|cache| cache.get(&entity).copied());
                    let moved = maybe_transform
                        .as_ref()
                        .is_some_and(DetectChanges::is_changed);
                    let resized = maybe_aabb.as_ref().is_some_and(DetectChanges::is_changed);
                    let changed = moved || resized || inherited_visibility.is_changed();
                    let index = usize::try_from(entity.index_u32()).unwrap_or(0);
                    let tested = caster_needs_retest(
                        stride,
                        frame_bucket,
                        index,
                        changed,
                        has_visibility_range,
                        cached.is_some(),
                    );

                    counts_local.0 = counts_local.0.saturating_add(1);
                    match (maybe_aabb, maybe_transform) {
                        (Some(aabb), Some(transform)) => {
                            let mask = if tested {
                                counts_local.1 = counts_local.1.saturating_add(1);
                                let fresh = compute_cascade_mask(
                                    view_frusta,
                                    &aabb,
                                    &transform.affine(),
                                    has_no_frustum_culling,
                                );
                                // Persist only the freshly-tested membership;
                                // untested casters keep their cached entry, so
                                // the cache costs O(tested), not O(casters).
                                cache_local.push((entity, fresh));
                                fresh
                            } else {
                                cached.unwrap_or(0)
                            };
                            let mut visible = false;
                            for (index, local) in view_local.iter_mut().enumerate() {
                                if mask & cascade_bit(index) != 0 {
                                    local.push(entity);
                                    visible = true;
                                }
                            }
                            if visible {
                                defer_local.push(entity);
                            }
                        }
                        _ => {
                            // No bounds: a caster we cannot cull is visible in
                            // every cascade, exactly as Bevy treats it. The mask
                            // is constant, so it needs no cache entry.
                            defer_local.push(entity);
                            for local in view_local.iter_mut() {
                                local.push(entity);
                            }
                        }
                    }
                },
            );

            // Collect entities from the parallel queue into the cascade lists.
            if let Some(view_dest_vec) = visible_entities.entities.get_mut(&view_entity) {
                for (view_dest_index, view_dest) in view_dest_vec.iter_mut().enumerate() {
                    view_dest.entities.clear();
                    for thread_entity_queue in view_visible_entities_queue.iter_mut() {
                        if let Some(src) = thread_entity_queue.get_mut(view_dest_index) {
                            view_dest.entities.append(src);
                        }
                    }
                    view_dest.shrink();
                    view_dest.entities.sort_unstable();
                }
            }

            // Apply only the freshly-tested casters to the persistent cache.
            let cache = state.caches.entry(key).or_default();
            for pairs in cache_pairs_queue.iter_mut() {
                for (entity, mask) in pairs.drain(..) {
                    cache.insert(entity, mask);
                }
            }
            seen_keys.insert(key);

            // Fold this view's per-thread caster counters into the frame totals.
            for counter in counts_queue.iter_mut() {
                frame_total = frame_total.saturating_add(counter.0);
                frame_tested = frame_tested.saturating_add(counter.1);
                *counter = (0, 0);
            }
        }
    }

    state.caches.retain(|key, _| seen_keys.contains(key));

    flush_shadow_cull_diag(&mut state, started, frame_total, frame_tested);

    // Defer marking view visibility so this system can run in parallel with
    // check_point_light_mesh_visibility.
    let mut defer_queue = std::mem::take(&mut *defer_visible_entities_queue);
    commands.queue(move |world: &mut World| {
        let mut query = world.query::<&mut ViewVisibility>();
        for entities in defer_queue.iter_mut() {
            let mut iter = query.iter_many_mut(world, entities.iter());
            while let Some(mut view_visibility) = iter.fetch_next() {
                view_visibility.set_visible();
            }
        }
    });
}

/// Accumulate this frame into the rolling diagnostics and, once a second, emit
/// one `shadow_cull` summary line and reset the window.
///
/// All integer math (the strict `as`-free lint set forbids float casts): times
/// are reported in microseconds, the tested share as an integer percent.
fn flush_shadow_cull_diag(
    state: &mut ShadowCullState,
    started: Instant,
    frame_total: u64,
    frame_tested: u64,
) {
    let stride = state.stride;
    let elapsed_ns = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let diag = &mut state.diag;
    diag.frames = diag.frames.saturating_add(1);
    diag.tested = diag.tested.saturating_add(frame_tested);
    diag.total = diag.total.saturating_add(frame_total);
    diag.cull_ns = diag.cull_ns.saturating_add(u128::from(elapsed_ns));
    diag.cull_max_ns = diag.cull_max_ns.max(elapsed_ns);

    let window_start = *diag.window_start.get_or_insert(started);
    if started.duration_since(window_start).as_secs() < 1 {
        return;
    }
    if !state.log_diag {
        // Reset the window without emitting, so the accumulators stay bounded.
        state.diag = ShadowCullDiag {
            window_start: Some(started),
            ..ShadowCullDiag::default()
        };
        return;
    }
    let diag = &state.diag;
    let frames = diag.frames.max(1);
    let mean_cull_us = diag
        .cull_ns
        .checked_div(u128::from(frames))
        .and_then(|mean_ns| mean_ns.checked_div(1000))
        .unwrap_or(0);
    let max_cull_us = diag.cull_max_ns.checked_div(1000).unwrap_or(0);
    let mean_total = diag.total.checked_div(frames).unwrap_or(0);
    let mean_tested = diag.tested.checked_div(frames).unwrap_or(0);
    let tested_pct = diag
        .tested
        .saturating_mul(100)
        .checked_div(diag.total)
        .unwrap_or(0);
    info!(
        target: "shadow_cull",
        "stride={stride} fps~{frames} cull mean={mean_cull_us}us max={max_cull_us}us  \
         casters total~{mean_total} tested~{mean_tested} ({tested_pct}%)"
    );

    state.diag = ShadowCullDiag {
        window_start: Some(started),
        ..ShadowCullDiag::default()
    };
}

/// Cycle the round-robin stride live with `Ctrl+Alt+S`, so one parked scene can
/// be A/B'd across several strides without the scene-density confound of
/// separate login runs.
fn cycle_shadow_cull_stride(keys: Res<ButtonInput<KeyCode>>, mut state: ResMut<ShadowCullState>) {
    const LADDER: [usize; 6] = [1, 2, 4, 8, 16, 60];
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    if !(ctrl && alt && keys.just_pressed(KeyCode::KeyS)) {
        return;
    }
    let next = LADDER
        .iter()
        .copied()
        .find(|&candidate| candidate > state.stride)
        .unwrap_or(1);
    state.stride = next;
    info!(target: "shadow_cull", "stride -> {next}");
}

/// Installs the amortised sun shadow-caster cull, replacing Bevy's per-frame
/// `check_dir_light_mesh_visibility`.
///
/// `SL_VIEWER_SHADOW_CULL_STRIDE` selects the initial stride (default
/// [`DEFAULT_SHADOW_CULL_STRIDE`]); `0` is a **passthrough** that installs
/// nothing and leaves stock Bevy in place, the A/B baseline against our system.
pub(crate) struct ShadowVisibilityPlugin;

impl Plugin for ShadowVisibilityPlugin {
    fn build(&self, app: &mut App) {
        let stride = std::env::var("SL_VIEWER_SHADOW_CULL_STRIDE")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(DEFAULT_SHADOW_CULL_STRIDE);

        if stride == 0 {
            info!(
                target: "shadow_cull",
                "passthrough: keeping stock bevy check_dir_light_mesh_visibility \
                 (SL_VIEWER_SHADOW_CULL_STRIDE=0)"
            );
            return;
        }

        app.insert_resource(ShadowCullState {
            stride,
            frame: 0,
            caches: HashMap::new(),
            diag: ShadowCullDiag::default(),
            log_diag: std::env::var_os("SL_VIEWER_LOG_SHADOW_CULL").is_some(),
        });

        // Disable Bevy's own directional + point/spot caster-visibility systems
        // (both live in `CheckLightVisibility`) so ours runs in their place.
        app.configure_sets(
            PostUpdate,
            SimulationLightSystems::CheckLightVisibility.run_if(never),
        );

        // Re-add the point/spot system unchanged alongside our directional
        // replacement, with the same ordering constraints Bevy gives the pair.
        app.add_systems(
            PostUpdate,
            (
                check_dir_light_mesh_visibility_amortised,
                check_point_light_mesh_visibility,
            )
                .in_set(ShadowVisibilitySet)
                .after(VisibilitySystems::CalculateBounds)
                .after(TransformSystems::Propagate)
                .after(SimulationLightSystems::UpdateLightFrusta)
                .after(VisibilitySystems::CheckVisibility)
                .before(VisibilitySystems::MarkNewlyHiddenEntitiesInvisible),
        );
        app.add_systems(Update, cycle_shadow_cull_stride);
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(cascade_bit(31), 1u32 << 31);
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
        let identity = bevy::math::Affine3A::IDENTITY;
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
    fn amortisation_off_always_retests() {
        for index in 0..10 {
            assert!(
                caster_needs_retest(1, 0, index, false, false, true),
                "stride 1 must re-test every caster every frame"
            );
        }
    }

    #[test]
    fn change_range_or_cold_cache_overrides_the_bucket() {
        // Index 3, stride 8, bucket 0: not this frame's round-robin slice ...
        assert!(!caster_needs_retest(8, 0, 3, false, false, true));
        // ... but a change, a visibility range, or a cold cache force a re-test.
        assert!(caster_needs_retest(8, 0, 3, true, false, true));
        assert!(caster_needs_retest(8, 0, 3, false, true, true));
        assert!(caster_needs_retest(8, 0, 3, false, false, false));
    }

    #[test]
    fn static_caster_is_retested_exactly_once_per_stride() {
        let stride = 4;
        for index in 0..20 {
            let retests = (0..stride)
                .filter(|&frame_bucket| {
                    caster_needs_retest(stride, frame_bucket, index, false, false, true)
                })
                .count();
            assert_eq!(
                retests, 1,
                "a static cached caster is re-tested exactly once every `stride` frames"
            );
        }
    }
}
