//! Water-relative transparency ordering (`viewer-particle-water-ordering`), the
//! **pre-water pass** the refracting water surface is built on
//! (`viewer-water-surface-alpha-not-refraction`) — translucent content below the
//! surface is drawn before the water, in a pass of its own, so it is in the screen
//! copy the water refracts — and the **sky-backdrop bucket**
//! (`viewer-nametags-occluded-by-clouds`), which keeps the camera-anchored sun,
//! moon, star, and cloud backdrops behind every world-anchored transparent overlay
//! instead of on top of them (the crate-private `SkyBackdrop` marker).
//!
//! **The problem this started as.** Bevy draws the whole [`Transparent3d`] phase
//! back-to-front by each item's *mesh centre*. The water plane follows the camera,
//! so its centre is always near the eye and the whole plane sorted last (on top) —
//! painting out a fountain's spray in front of it, exactly the decades-old
//! reference-viewer artifact this viewer must not reproduce. A single region-sized
//! (or endless) plane cannot be sorted per region either, so no per-object centre
//! sort can place it correctly.
//!
//! **The fix, and what it became.** The reference splits its alpha pool by the
//! region water height into `POOL_ALPHA_PRE_WATER` → `POOL_WATER` →
//! `POOL_ALPHA_POST_WATER`. This module ports that split. Every [`Transparent3d`]
//! item — particles, translucent prims, whoever queued them — is bucketed by its
//! centre height against the water level, and `sort_transparent_by_water` re-sorts
//! each view's phase by `(bucket, backdrop order, distance)` in
//! [`RenderSystems::PhaseSort`], after Bevy's own
//! [`sort_phase_system::<Transparent3d>`](sort_phase_system) has recomputed the
//! distances. The below-water items therefore sit at the head of the phase, in
//! back-to-front order.
//!
//! The water itself is no longer in this phase at all. It renders in Bevy's
//! [`Transmissive3d`](bevy::pbr::Transmissive3d) phase, opaque
//! and depth-writing, sampling the screen copy Bevy takes at the start of that pass
//! — which is what the reference's water does with its own copy
//! (`lldrawpoolwater.cpp:116`). That copy is taken *after* the opaque and alpha-mask
//! passes and *before* [`Transparent3d`], and the below-water translucency has to be
//! inside it or it is simply lost behind an opaque sea. So this module also draws
//! it, early:
//!
//! - `pre_water_transparent_pass_3d` runs in the `Core3d` main pass after the
//!   opaque pass and before the transmissive one, and renders the below-water head
//!   of each view's phase — the reference's `POOL_ALPHA_PRE_WATER`.
//! - `suppress_pre_water_items` then empties those items' batch ranges **for that
//!   view**, which is what
//!   [`SortedRenderPhase::render_range`](bevy::render::render_phase::SortedRenderPhase::render_range)
//!   skips on, so Bevy's own transparent pass draws only the rest and nothing is
//!   drawn twice. The ranges are rebuilt from scratch by the batching systems every
//!   frame (`batch_and_prepare_sorted_render_phase`), so this needs no undoing —
//!   and the items stay in the phase, which matters because they are *retained*
//!   there: removing one would drop it until its entity became visible again.
//!   Both systems are per view: Bevy runs the whole `Core3d` schedule once for each
//!   view, and suppressing across *all* views let the first one to run empty the
//!   ranges of views whose pass had not drawn yet, losing their below-water
//!   translucency outright (`viewer-underwater-name-tags-not-drawn`).
//!
//! **What this does and does not fix.** Combined with the water's depth write it is
//! per-pixel correct for the common cases: translucent content below the surface is
//! composited before the water and comes back through the refraction, and
//! above-water content (a fountain plume, a particle cloud) is occluded per pixel
//! where it dips behind the surface. It is **per-object** for the bucket assignment,
//! so a single large translucent prim that straddles the waterline is classified
//! whole by its centre (reference parity — the reference is per spatial-group,
//! likewise not per-pixel); a genuinely per-pixel split of one straddling mesh would
//! need order-independent transparency or a clip-plane double-draw, a separate
//! future effort.

use bevy::camera::MainPassResolutionOverride;
use bevy::core_pipeline::core_3d::{Transparent3d, TransparentSortingInfo3d};
use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::math::FloatOrd;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::diagnostic::RecordDiagnostics as _;
use bevy::render::extract_resource::ExtractResourcePlugin;
use bevy::render::render_phase::{PhaseItem as _, ViewSortedRenderPhases, sort_phase_system};
use bevy::render::render_resource::{RenderPassDescriptor, StoreOp};
use bevy::render::renderer::{RenderContext, ViewQuery};
use bevy::render::sync_world::{MainEntity, MainEntityHashMap};
use bevy::render::view::{ExtractedView, RetainedViewEntity, ViewDepthTexture, ViewTarget};
use bevy::render::{Extract, Render, RenderApp, RenderSystems};

use crate::water::{DEFAULT_WATER_HEIGHT, WaterLevel};

/// The system set the pre-water translucency pass runs in, so the above-water water
/// haze ([`crate::underwater_fog`]) can order itself before it without reaching for
/// the (private) system — the reference fogs the opaque scene first and draws its
/// pre-water alpha pool over the fogged result.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PreWaterPass;

/// The sort bucket for translucent content **below** the water surface: drawn first,
/// in [`pre_water_transparent_pass_3d`], before the water is drawn over it — so it
/// is in the screen copy the water refracts rather than hidden behind it.
const BELOW_WATER_BUCKET: u8 = 0;
/// The sort bucket for the camera-anchored sky backdrops ([`SkyBackdrop`]): drawn
/// before every world-anchored transparent overlay, because their depth is forced to
/// the far clip plane and their *centre* — the camera — makes Bevy's distance sort
/// place them last, on top of everything (`viewer-nametags-occluded-by-clouds`).
const BACKDROP_BUCKET: u8 = 1;
/// The sort bucket for translucent content **above** the water surface: left to
/// Bevy's transparent pass, which runs after the water and depth-tests against the
/// depth the water wrote.
const ABOVE_WATER_BUCKET: u8 = 2;
/// The sort bucket for [`TransparentSortingInfo3d::AlwaysOnTop`] items: drawn after
/// everything else so they stay on top, as their name promises (selection
/// highlights and the like).
const ALWAYS_ON_TOP_BUCKET: u8 = 3;

/// The buckets must ascend below → backdrop → above → always-on-top, because
/// `sort_transparent_by_water` sorts by bucket ascending and that is the order the
/// items are drawn in — and because [`PreWaterSplit`] takes the below-water items to
/// be a *prefix* of the sorted phase.
const _: () = assert!(
    BELOW_WATER_BUCKET < BACKDROP_BUCKET
        && BACKDROP_BUCKET < ABOVE_WATER_BUCKET
        && ABOVE_WATER_BUCKET < ALWAYS_ON_TOP_BUCKET,
    "water sort buckets must ascend below < backdrop < above < always-on-top"
);

/// Marks one of the camera-anchored **sky backdrops** — the sun / moon discs, the
/// star field, the cloud dome — and says where in the backdrop stack it belongs.
///
/// All three sit in the [`Transparent3d`] phase but are *not* world-anchored: their
/// fragment depth is forced to the far clip plane (`clouds.wgsl`, `stars.wgsl`) or,
/// for the discs, is a fixed 2000 m from the eye, and the meshes themselves are
/// centred on (or aimed from) the camera. Bevy sorts the phase by each item's mesh
/// centre, so a camera-centred dome has a sort distance of ~0 — the *nearest*
/// transparent object — and is drawn last, painting over every world-anchored
/// overlay in front of it (a name tag on a nearby avatar, hover text, a parcel
/// border, a particle system). [`BACKDROP_BUCKET`] takes them out of that sort
/// entirely and draws them, in the reference's own order, before the rest of the
/// phase.
///
/// The order is `LLDrawPoolWLSky::renderDeferred` (`lldrawpoolwlsky.cpp`): the sky
/// haze dome, then the heavenly bodies, then the stars, then the clouds. The sky
/// dome itself is opaque and so is drawn in the [`Opaque3d`](bevy::core_pipeline::core_3d::Opaque3d)
/// phase, before any of this; it needs no marker.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SkyBackdrop {
    /// The sun and moon discs — the reference's `renderHeavenlyBodies`, drawn first
    /// of the three so the stars twinkle over the moon and the clouds pass in front
    /// of the sun.
    HeavenlyBody,
    /// The star field — the reference's `renderStarsDeferred`.
    Stars,
    /// The cloud dome — the reference's `renderSkyCloudsDeferred`, drawn last of the
    /// backdrops (clouds occlude the sun, the moon, and the stars).
    Clouds,
}

impl SkyBackdrop {
    /// Where this backdrop sits in the backdrop stack: ascending is the order the
    /// reference draws them in, and therefore back to front.
    const fn draw_order(self) -> u8 {
        match self {
            Self::HeavenlyBody => 0,
            Self::Stars => 1,
            Self::Clouds => 2,
        }
    }
}

/// The [`SkyBackdrop`] of every backdrop entity, keyed by its main-world entity —
/// the render-world mirror `sort_transparent_by_water` looks each phase item up in.
///
/// Keyed by [`MainEntity`] rather than by the render-world entity because a
/// [`Transparent3d`] item's render entity is still `Entity::PLACEHOLDER` when the
/// phase is sorted (`queue_material_meshes` fills it in later, during batching); the
/// main entity is the half that is valid this early.
#[derive(Resource, Default, Debug)]
pub(crate) struct SkyBackdrops(MainEntityHashMap<SkyBackdrop>);

/// Mirror the main world's [`SkyBackdrop`] markers into the render world for
/// `sort_transparent_by_water`. A handful of entities, rebuilt each frame so a
/// despawned backdrop leaves nothing behind.
fn extract_sky_backdrops(
    mut backdrops: ResMut<SkyBackdrops>,
    markers: Extract<Query<(Entity, &SkyBackdrop)>>,
) {
    backdrops.0.clear();
    backdrops.0.extend(
        markers
            .iter()
            .map(|(entity, backdrop)| (MainEntity::from(entity), *backdrop)),
    );
}

/// How many items at the head of each view's [`Transparent3d`] phase are below the
/// water — the split point between the reference's `POOL_ALPHA_PRE_WATER` and
/// everything after the water.
///
/// Written by [`sort_transparent_by_water`], which has just put those items in front
/// by sorting on the bucket, and read by the two systems that draw and then suppress
/// them. A count rather than a list of items because the phase is a sorted
/// `IndexMap`: the head is addressable as a range, which is exactly what
/// `render_range` takes.
#[derive(Resource, Default, Debug)]
pub(crate) struct PreWaterSplit(HashMap<RetainedViewEntity, usize>);

/// Re-sort each view's [`Transparent3d`] phase by `(bucket, backdrop order,
/// distance)` so below-water translucency leads the phase, the sky backdrops follow
/// it, and above-water translucency comes after them — with each bucket kept in
/// Bevy's back-to-front distance order — and record where the first two meet in
/// [`PreWaterSplit`].
///
/// Runs in [`RenderSystems::PhaseSort`] after Bevy's
/// [`sort_phase_system::<Transparent3d>`](sort_phase_system), which has already
/// recomputed every item's `distance` this frame — so this pass reuses that distance
/// for the within-bucket order and only overrides the *across*-water order. Applied
/// to every view uniformly: a view with no water and no sky (the HUD camera) puts
/// everything in one bucket, leaving its order unchanged.
fn sort_transparent_by_water(
    water_level: Option<Res<WaterLevel>>,
    backdrops: Res<SkyBackdrops>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut split: ResMut<PreWaterSplit>,
) {
    let level = water_level.map_or(DEFAULT_WATER_HEIGHT, |water_level| water_level.0);
    split.0.clear();
    for (view, phase) in phases.iter_mut() {
        // Decorate-sort: compute each item's `(bucket, backdrop order, distance)` key
        // exactly once (`sort_by_cached_key` is stable, like the `sort_by` it
        // replaces), instead of re-running the bucket lookup for both operands of
        // every comparison.
        phase.items.sort_by_cached_key(|_key, item| {
            let (bucket, order) = classify_bucket(
                item.sorting_info,
                level,
                backdrops.0.get(&item.entity.1).copied(),
            );
            (bucket, order, FloatOrd(item.distance))
        });
        let below = phase
            .items
            .values()
            .take_while(|item| {
                classify_bucket(
                    item.sorting_info,
                    level,
                    backdrops.0.get(&item.entity.1).copied(),
                )
                .0 == BELOW_WATER_BUCKET
            })
            .count();
        if below > 0 {
            split.0.insert(*view, below);
        }
    }
}

/// The bucket decision, and the within-bucket order for the backdrops: a sky
/// backdrop is a backdrop wherever the camera happens to be, an always-on-top item is
/// topmost, and any other sorted item is above or below by whether its centre height
/// reaches the water `level`.
///
/// The backdrop test comes first because a backdrop's mesh centre is the *camera*:
/// left to the water test, the cloud dome and the star field would drop into the
/// below-water bucket — and so into the pre-water pass, to be refracted by the water
/// — the moment the camera dipped under the surface.
const fn classify_bucket(
    sorting_info: TransparentSortingInfo3d,
    level: f32,
    backdrop: Option<SkyBackdrop>,
) -> (u8, u8) {
    if let Some(backdrop) = backdrop {
        return (BACKDROP_BUCKET, backdrop.draw_order());
    }
    match sorting_info {
        TransparentSortingInfo3d::AlwaysOnTop => (ALWAYS_ON_TOP_BUCKET, 0),
        TransparentSortingInfo3d::Sorted { mesh_center, .. } => {
            if mesh_center.y >= level {
                (ABOVE_WATER_BUCKET, 0)
            } else {
                (BELOW_WATER_BUCKET, 0)
            }
        }
    }
}

/// Draw the below-water head of each view's [`Transparent3d`] phase, between the
/// opaque pass and the transmissive one — the reference's `POOL_ALPHA_PRE_WATER`,
/// and the reason the water's screen copy has anything translucent in it.
///
/// Modelled on Bevy's own `main_transparent_pass_3d`: same colour attachment, same
/// loaded-and-stored depth attachment (the water has not been drawn yet, so this
/// depth-tests against opaque geometry only, which is what the reference's pre-water
/// pool does too).
fn pre_water_transparent_pass_3d(
    world: &World,
    view: ViewQuery<(
        &ExtractedCamera,
        &ExtractedView,
        &ViewTarget,
        &ViewDepthTexture,
        Option<&MainPassResolutionOverride>,
    )>,
    phases: Res<ViewSortedRenderPhases<Transparent3d>>,
    split: Res<PreWaterSplit>,
    mut ctx: RenderContext,
) {
    let view_entity = view.entity();
    let (camera, extracted_view, target, depth, resolution_override) = view.into_inner();

    let Some(phase) = phases.get(&extracted_view.retained_view_entity) else {
        return;
    };
    let Some(&below) = split.0.get(&extracted_view.retained_view_entity) else {
        return;
    };
    // Defensive: the split is recorded from this same phase in `PhaseSort`, but the
    // range must be in bounds or `render_range` panics.
    let below = below.min(phase.items.len());
    if below == 0 {
        return;
    }

    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let mut render_pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("pre_water_transparent_pass_3d"),
        color_attachments: &[Some(target.get_color_attachment())],
        // Loaded, and stored as Bevy's transparent pass stores it (its own comment
        // cites bevy#3776: storing keeps wgpu from clearing the depth buffer).
        depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    let pass_span = diagnostics.pass_span(&mut render_pass, "pre_water_transparent_pass_3d");

    if let Some(viewport) = bevy::camera::Viewport::from_viewport_and_override(
        camera.viewport.as_ref(),
        resolution_override,
    ) {
        render_pass.set_camera_viewport(&viewport);
    }

    if let Err(err) = phase.render_range(&mut render_pass, world, view_entity, ..below) {
        error!("error rendering the pre-water transparent phase: {err:?}");
    }

    pass_span.end(&mut render_pass);
}

/// Empty the batch range of every item [`pre_water_transparent_pass_3d`] has just
/// drawn **for this view**, so Bevy's transparent pass skips them rather than
/// drawing them a second time (translucent content drawn twice is blended twice,
/// and reads as too dense).
///
/// `render_range` skips an item whose batch range is empty, which is the only seam
/// Bevy offers here: the phase is drawn whole by one system we do not own, and the
/// items cannot be moved out of it because they are retained there across frames.
/// The batching systems assign every item a fresh range each frame
/// (`batch_and_prepare_sorted_render_phase`), so emptying one is undone before it is
/// ever read again.
///
/// **Per view, and that is load-bearing** (`viewer-underwater-name-tags-not-drawn`).
/// Bevy 0.19 runs the `Core3d` schedule once for *each* view — that is what
/// [`ViewQuery`] resolves against — and every view owns a separate phase with its own
/// item list and its own batch ranges. Suppressing across all of them meant the first
/// view to run zeroed the ranges of views whose pre-water pass had not run yet: their
/// pass then drew nothing (`render_range` skips an empty range) *and* their
/// transparent pass skipped the items too, so below-water translucency vanished
/// outright. The viewer has several views — the main camera, the HUD camera, and the
/// reflection-probe capture cameras, which cycle every frame — so which view lost its
/// below-water content came down to schedule order. Name tags showed it first,
/// because submerged is the only time a tag is bucketed below the water at all.
fn suppress_pre_water_items(
    view: ViewQuery<&ExtractedView>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    split: Res<PreWaterSplit>,
) {
    let retained_view_entity = view.into_inner().retained_view_entity;
    suppress_view_pre_water_items(&mut phases, &retained_view_entity, &split);
}

/// Empty the batch ranges of one view's below-water prefix — the body of
/// [`suppress_pre_water_items`], split out so a test can drive it for a chosen view
/// without a render app.
fn suppress_view_pre_water_items(
    phases: &mut ViewSortedRenderPhases<Transparent3d>,
    view: &RetainedViewEntity,
    split: &PreWaterSplit,
) {
    let Some(&below) = split.0.get(view) else {
        return;
    };
    let Some(phase) = phases.get_mut(view) else {
        return;
    };
    for item in phase.items.values_mut().take(below) {
        *item.batch_range_mut() = 0..0;
    }
}

/// Wires the water-relative transparency ordering into the app: extract the
/// `WaterLevel` and the `SkyBackdrop` markers into the render world, add the
/// re-sort after Bevy's transparent sort, and add the pre-water pass and its
/// suppression to the `Core3d` main pass. Add once, after `DefaultPlugins`, like the
/// other viewer render plugins.
#[derive(Debug, Default)]
pub struct TransparencyOrderPlugin;

impl Plugin for TransparencyOrderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ExtractResourcePlugin::<WaterLevel>::default());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<PreWaterSplit>()
            .init_resource::<SkyBackdrops>()
            .add_systems(ExtractSchedule, extract_sky_backdrops)
            .add_systems(
                Render,
                sort_transparent_by_water
                    .in_set(RenderSystems::PhaseSort)
                    .after(sort_phase_system::<Transparent3d>),
            )
            .add_systems(
                Core3d,
                (pre_water_transparent_pass_3d, suppress_pre_water_items)
                    .chain()
                    .in_set(PreWaterPass)
                    .in_set(Core3dSystems::MainPass)
                    .after(bevy::core_pipeline::core_3d::main_opaque_pass_3d)
                    .before(bevy::pbr::main_transmissive_pass_3d),
            );
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]
    #![expect(
        clippy::arithmetic_side_effects,
        reason = "small literal fixture indices, nowhere near an overflow"
    )]

    use super::{
        ABOVE_WATER_BUCKET, ALWAYS_ON_TOP_BUCKET, BACKDROP_BUCKET, BELOW_WATER_BUCKET,
        PreWaterSplit, SkyBackdrop, classify_bucket, suppress_view_pre_water_items,
    };
    use bevy::core_pipeline::core_3d::{Transparent3d, TransparentSortingInfo3d};
    use bevy::ecs::entity::Entity;
    use bevy::math::Vec3;
    use bevy::render::render_phase::{
        DrawFunctionId, PhaseItem as _, PhaseItemExtraIndex, ViewSortedRenderPhases,
    };
    use bevy::render::render_resource::CachedRenderPipelineId;
    use bevy::render::sync_world::MainEntity;
    use bevy::render::view::RetainedViewEntity;
    use pretty_assertions::assert_eq;

    /// A `Sorted` sorting info centred at height `y` (the field the bucket reads).
    fn sorted_at(y: f32) -> TransparentSortingInfo3d {
        TransparentSortingInfo3d::Sorted {
            mesh_center: Vec3::new(0.0, y, 0.0),
            depth_bias: 0.0,
        }
    }

    /// A distinct entity for a test fixture, by index.
    fn entity(index: u32) -> Entity {
        Entity::from_raw_u32(index).expect("a small index is a valid entity")
    }

    /// A view key for a test fixture, by index.
    fn view(index: u32) -> RetainedViewEntity {
        RetainedViewEntity::new(MainEntity::from(entity(index)), None, 0)
    }

    /// A minimal [`Transparent3d`] item with a **non-empty** batch range, so a test
    /// can tell "suppressed" (emptied) from "left alone".
    fn phase_item(index: u32) -> Transparent3d {
        Transparent3d {
            sorting_info: sorted_at(0.0),
            distance: 0.0,
            pipeline: CachedRenderPipelineId::INVALID,
            entity: (entity(index), MainEntity::from(entity(index))),
            draw_function: DrawFunctionId(0),
            batch_range: 0..1,
            extra_index: PhaseItemExtraIndex::None,
            indexed: false,
        }
    }

    /// Two views, each with `items` items and a below-water prefix of `below`.
    fn two_view_phases(
        items: u32,
        below: usize,
    ) -> (ViewSortedRenderPhases<Transparent3d>, PreWaterSplit) {
        let mut phases = ViewSortedRenderPhases::<Transparent3d>::default();
        let mut split = PreWaterSplit::default();
        for which in 0..2 {
            let key = view(which);
            phases.prepare_for_new_frame(key);
            let phase = phases.get_mut(&key).expect("the phase was just prepared");
            for index in 0..items {
                // Distinct entities per view, so the two phases do not share keys.
                phase.add_retained(phase_item(which * items + index));
            }
            split.0.insert(key, below);
        }
        (phases, split)
    }

    /// The batch ranges of one view's items, in phase order.
    fn ranges(
        phases: &ViewSortedRenderPhases<Transparent3d>,
        key: &RetainedViewEntity,
    ) -> Vec<std::ops::Range<u32>> {
        phases
            .get(key)
            .expect("the view has a phase")
            .items
            .values()
            .map(|item| item.batch_range().clone())
            .collect()
    }

    /// Suppressing one view's below-water prefix must leave **every other view's**
    /// batch ranges alone (`viewer-underwater-name-tags-not-drawn`).
    ///
    /// Bevy runs the `Core3d` schedule once per view, so the pre-water pass and this
    /// suppression run once per view too. Suppressing across all views meant the
    /// first view to run emptied the ranges of views whose pass had not run yet —
    /// and an item with an empty range is skipped by `render_range` *and* by Bevy's
    /// transparent pass, so those views lost their below-water translucency
    /// altogether. Submerged name tags were the visible symptom.
    #[test]
    fn suppression_touches_only_its_own_view() {
        let (mut phases, split) = two_view_phases(3, 2);
        suppress_view_pre_water_items(&mut phases, &view(0), &split);
        assert_eq!(
            ranges(&phases, &view(0)),
            vec![0..0, 0..0, 0..1],
            "the drawn below-water prefix of view 0 must be emptied",
        );
        assert_eq!(
            ranges(&phases, &view(1)),
            vec![0..1, 0..1, 0..1],
            "view 1's pass has not run yet, so its items must still be drawable",
        );
    }

    /// A view with no recorded split has nothing drawn early, so nothing is emptied.
    #[test]
    fn a_view_without_a_split_is_untouched() {
        let (mut phases, mut split) = two_view_phases(2, 2);
        split.0.remove(&view(0));
        suppress_view_pre_water_items(&mut phases, &view(0), &split);
        assert_eq!(ranges(&phases, &view(0)), vec![0..1, 0..1]);
    }

    /// Content buckets above or below by whether its centre reaches the water level;
    /// content exactly at the level counts as above (drawn over the surface it sits
    /// on).
    #[test]
    fn content_buckets_above_or_below_the_level() {
        assert_eq!(
            classify_bucket(sorted_at(25.0), 20.0, None),
            (ABOVE_WATER_BUCKET, 0)
        );
        assert_eq!(
            classify_bucket(sorted_at(15.0), 20.0, None),
            (BELOW_WATER_BUCKET, 0)
        );
        assert_eq!(
            classify_bucket(sorted_at(20.0), 20.0, None),
            (ABOVE_WATER_BUCKET, 0)
        );
    }

    /// An always-on-top item stays topmost, wherever it is relative to the water.
    #[test]
    fn always_on_top_stays_topmost() {
        assert_eq!(
            classify_bucket(TransparentSortingInfo3d::AlwaysOnTop, 20.0, None),
            (ALWAYS_ON_TOP_BUCKET, 0)
        );
    }

    /// A sky backdrop is a backdrop wherever the camera is — including under the
    /// water, where its camera-centred mesh centre would otherwise put it in the
    /// below-water bucket and hand it to the pre-water pass to be refracted.
    #[test]
    fn a_backdrop_is_a_backdrop_under_water_too() {
        for backdrop in [
            SkyBackdrop::HeavenlyBody,
            SkyBackdrop::Stars,
            SkyBackdrop::Clouds,
        ] {
            for height in [25.0_f32, 15.0] {
                assert_eq!(
                    classify_bucket(sorted_at(height), 20.0, Some(backdrop)).0,
                    BACKDROP_BUCKET,
                    "{backdrop:?} at {height} m must stay a backdrop",
                );
            }
        }
    }

    /// The backdrops draw in the reference's own order
    /// (`LLDrawPoolWLSky::renderDeferred`): heavenly bodies, then stars, then clouds
    /// — so the clouds pass in front of the sun rather than behind it.
    #[test]
    fn backdrops_draw_in_the_reference_order() {
        let mut order = [
            classify_bucket(sorted_at(25.0), 20.0, Some(SkyBackdrop::Clouds)),
            classify_bucket(sorted_at(25.0), 20.0, Some(SkyBackdrop::HeavenlyBody)),
            classify_bucket(sorted_at(25.0), 20.0, Some(SkyBackdrop::Stars)),
        ];
        order.sort_unstable();
        assert_eq!(
            order,
            [
                classify_bucket(sorted_at(25.0), 20.0, Some(SkyBackdrop::HeavenlyBody)),
                classify_bucket(sorted_at(25.0), 20.0, Some(SkyBackdrop::Stars)),
                classify_bucket(sorted_at(25.0), 20.0, Some(SkyBackdrop::Clouds)),
            ],
        );
    }

    /// The below-water bucket must sort first, because the pre-water pass takes those
    /// items to be a prefix of the phase and draws them by range; the backdrops must
    /// then precede the above-water content, which is the whole point of the bucket
    /// (a name tag in front of a cloudy sky must not be painted over by the clouds).
    /// If a reordering of the constants ever broke either, the pass would draw the
    /// wrong items — this is the runtime companion to the compile-time assert on the
    /// constants.
    #[test]
    fn below_water_leads_and_backdrops_precede_the_world() {
        let mut buckets = [
            classify_bucket(TransparentSortingInfo3d::AlwaysOnTop, 20.0, None).0,
            classify_bucket(sorted_at(25.0), 20.0, None).0,
            classify_bucket(sorted_at(25.0), 20.0, Some(SkyBackdrop::Clouds)).0,
            classify_bucket(sorted_at(15.0), 20.0, None).0,
        ];
        buckets.sort_unstable();
        assert_eq!(
            buckets,
            [
                BELOW_WATER_BUCKET,
                BACKDROP_BUCKET,
                ABOVE_WATER_BUCKET,
                ALWAYS_ON_TOP_BUCKET
            ],
            "sorting by bucket must run below-water → backdrops → above-water → on top",
        );
    }
}
