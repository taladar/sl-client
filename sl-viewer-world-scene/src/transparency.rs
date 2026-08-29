//! Water-relative transparency ordering (`viewer-particle-water-ordering`) and the
//! **pre-water pass** the refracting water surface is built on
//! (`viewer-water-surface-alpha-not-refraction`): translucent content below the
//! surface is drawn before the water, in a pass of its own, so it is in the screen
//! copy the water refracts.
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
//! each view's phase by `(water_bucket, distance)` in
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
//! - `suppress_pre_water_items` then empties those items' batch ranges, which is
//!   what [`SortedRenderPhase::render_range`](bevy::render::render_phase::SortedRenderPhase::render_range)
//!   skips on, so Bevy's own transparent pass draws only the rest and nothing is
//!   drawn twice. The ranges are rebuilt from scratch by the batching systems every
//!   frame (`batch_and_prepare_sorted_render_phase`), so this needs no undoing —
//!   and the items stay in the phase, which matters because they are *retained*
//!   there: removing one would drop it until its entity became visible again.
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
use bevy::render::view::{ExtractedView, RetainedViewEntity, ViewDepthTexture, ViewTarget};
use bevy::render::{Render, RenderApp, RenderSystems};

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
/// The sort bucket for translucent content **above** the water surface: left to
/// Bevy's transparent pass, which runs after the water and depth-tests against the
/// depth the water wrote.
const ABOVE_WATER_BUCKET: u8 = 1;
/// The sort bucket for [`TransparentSortingInfo3d::AlwaysOnTop`] items: drawn after
/// everything else so they stay on top, as their name promises (selection
/// highlights and the like).
const ALWAYS_ON_TOP_BUCKET: u8 = 2;

/// The buckets must ascend below → above → always-on-top, because
/// `sort_transparent_by_water` sorts by bucket ascending and that is the order the
/// items are drawn in — and because [`PreWaterSplit`] takes the below-water items to
/// be a *prefix* of the sorted phase.
const _: () = assert!(
    BELOW_WATER_BUCKET < ABOVE_WATER_BUCKET && ABOVE_WATER_BUCKET < ALWAYS_ON_TOP_BUCKET,
    "water sort buckets must ascend below < above < always-on-top"
);

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

/// Re-sort each view's [`Transparent3d`] phase by `(water_bucket, distance)` so
/// below-water translucency leads the phase and above-water translucency follows it,
/// with each bucket kept in Bevy's back-to-front distance order, and record where the
/// two meet in [`PreWaterSplit`].
///
/// Runs in [`RenderSystems::PhaseSort`] after Bevy's
/// [`sort_phase_system::<Transparent3d>`](sort_phase_system), which has already
/// recomputed every item's `distance` this frame — so this pass reuses that distance
/// for the within-bucket order and only overrides the *across*-water order. Applied
/// to every view uniformly: a view with no water (the HUD camera) puts everything in
/// one bucket, leaving its order unchanged.
fn sort_transparent_by_water(
    water_level: Option<Res<WaterLevel>>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut split: ResMut<PreWaterSplit>,
) {
    let level = water_level.map_or(DEFAULT_WATER_HEIGHT, |water_level| water_level.0);
    split.0.clear();
    for (view, phase) in phases.iter_mut() {
        // Decorate-sort: compute each item's `(bucket, distance)` key exactly once
        // (`sort_by_cached_key` is stable, like the `sort_by` it replaces), instead
        // of re-running the bucket lookup for both operands of every comparison.
        phase.items.sort_by_cached_key(|_key, item| {
            (
                classify_bucket(item.sorting_info, level),
                FloatOrd(item.distance),
            )
        });
        let below = phase
            .items
            .values()
            .take_while(|item| classify_bucket(item.sorting_info, level) == BELOW_WATER_BUCKET)
            .count();
        if below > 0 {
            split.0.insert(*view, below);
        }
    }
}

/// The bucket decision: an always-on-top item is topmost, and a sorted item is above
/// or below by whether its centre height reaches the water `level`.
const fn classify_bucket(sorting_info: TransparentSortingInfo3d, level: f32) -> u8 {
    match sorting_info {
        TransparentSortingInfo3d::AlwaysOnTop => ALWAYS_ON_TOP_BUCKET,
        TransparentSortingInfo3d::Sorted { mesh_center, .. } => {
            if mesh_center.y >= level {
                ABOVE_WATER_BUCKET
            } else {
                BELOW_WATER_BUCKET
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
/// drawn, so Bevy's transparent pass skips them rather than drawing them a second
/// time (translucent content drawn twice is blended twice, and reads as too dense).
///
/// `render_range` skips an item whose batch range is empty, which is the only seam
/// Bevy offers here: the phase is drawn whole by one system we do not own, and the
/// items cannot be moved out of it because they are retained there across frames.
/// The batching systems assign every item a fresh range each frame
/// (`batch_and_prepare_sorted_render_phase`), so emptying one is undone before it is
/// ever read again.
fn suppress_pre_water_items(
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    split: Res<PreWaterSplit>,
) {
    for (view, phase) in phases.iter_mut() {
        let Some(&below) = split.0.get(view) else {
            continue;
        };
        for item in phase.items.values_mut().take(below) {
            *item.batch_range_mut() = 0..0;
        }
    }
}

/// Wires the water-relative transparency ordering into the app: extract the
/// `WaterLevel` into the render world, add the re-sort after Bevy's transparent
/// sort, and add the pre-water pass and its suppression to the `Core3d` main pass.
/// Add once, after `DefaultPlugins`, like the other viewer render plugins.
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
    use super::{ABOVE_WATER_BUCKET, ALWAYS_ON_TOP_BUCKET, BELOW_WATER_BUCKET, classify_bucket};
    use bevy::core_pipeline::core_3d::TransparentSortingInfo3d;
    use bevy::math::Vec3;
    use pretty_assertions::assert_eq;

    /// A `Sorted` sorting info centred at height `y` (the field the bucket reads).
    fn sorted_at(y: f32) -> TransparentSortingInfo3d {
        TransparentSortingInfo3d::Sorted {
            mesh_center: Vec3::new(0.0, y, 0.0),
            depth_bias: 0.0,
        }
    }

    /// Content buckets above or below by whether its centre reaches the water level;
    /// content exactly at the level counts as above (drawn over the surface it sits
    /// on).
    #[test]
    fn content_buckets_above_or_below_the_level() {
        assert_eq!(classify_bucket(sorted_at(25.0), 20.0), ABOVE_WATER_BUCKET);
        assert_eq!(classify_bucket(sorted_at(15.0), 20.0), BELOW_WATER_BUCKET);
        assert_eq!(classify_bucket(sorted_at(20.0), 20.0), ABOVE_WATER_BUCKET);
    }

    /// An always-on-top item stays topmost, wherever it is relative to the water.
    #[test]
    fn always_on_top_stays_topmost() {
        assert_eq!(
            classify_bucket(TransparentSortingInfo3d::AlwaysOnTop, 20.0),
            ALWAYS_ON_TOP_BUCKET
        );
    }

    /// The below-water bucket must sort first, because the pre-water pass takes those
    /// items to be a prefix of the phase and draws them by range. If a reordering of
    /// the constants ever broke that, the pass would draw the wrong items — this is
    /// the runtime companion to the compile-time assert on the constants.
    #[test]
    fn below_water_leads_the_order() {
        let mut buckets = [
            classify_bucket(TransparentSortingInfo3d::AlwaysOnTop, 20.0),
            classify_bucket(sorted_at(25.0), 20.0),
            classify_bucket(sorted_at(15.0), 20.0),
        ];
        buckets.sort_unstable();
        assert_eq!(
            buckets,
            [BELOW_WATER_BUCKET, ABOVE_WATER_BUCKET, ALWAYS_ON_TOP_BUCKET],
            "sorting by bucket must put the below-water items at the head of the phase",
        );
    }
}
