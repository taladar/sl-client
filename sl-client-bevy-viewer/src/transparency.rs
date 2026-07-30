//! Water-relative transparency ordering (`viewer-particle-water-ordering`): a
//! render-world re-sort of the [`Transparent3d`] phase that fixes translucent
//! content ordering against the water surface — and, more generally, makes every
//! translucent object draw in a sensible order relative to the sea.
//!
//! **The problem.** The water surface is a camera-following alpha-blended plane
//! ([`WaterMaterial`](sl_client_bevy::WaterMaterial)), and translucent content
//! (particles, translucent prims) is alpha-blended too. Bevy draws the whole
//! [`Transparent3d`] phase back-to-front by each item's *mesh centre*. The water
//! plane follows the camera, so its centre is always near the eye and the whole
//! plane sorts last (on top) — painting out a fountain's spray in front of it,
//! exactly the decades-old reference-viewer artifact this viewer must not
//! reproduce. A single region-sized (or endless) plane cannot be sorted per region
//! either, so no per-object centre sort can place it correctly.
//!
//! **The fix (a port of `LLDrawPoolAlpha`'s pre/post-water split).** The reference
//! splits the alpha pool by the region water height into
//! `POOL_ALPHA_PRE_WATER` → `POOL_WATER` → `POOL_ALPHA_POST_WATER`: underwater
//! translucency is drawn first (already composited, so it shows *through* the
//! translucent surface), the water is drawn next **writing depth**
//! ([`WaterMaterial`](sl_client_bevy::WaterMaterial)'s `specialize`), and
//! above-water translucency is drawn last — where the water's depth write now gives
//! per-pixel occlusion of anything that dips behind the surface, rather than the
//! whole-plane sort that painted it out.
//!
//! Porting that here means bucketing **every** [`Transparent3d`] item — particles
//! *and* prims, whoever queued them — by its centre height relative to the water
//! level, with the water pinned to its own middle bucket. Rather than intercept
//! Bevy's `queue_material_meshes` (which owns the prim items) or add a bespoke
//! sub-phase, [`sort_transparent_by_water`] runs once in
//! [`RenderSystems::PhaseSort`], **after** Bevy's own
//! [`sort_phase_system::<Transparent3d>`](sort_phase_system), and re-sorts each
//! view's phase by `(water_bucket, distance)`: the bucket orders below-water → water
//! → above-water, and the distance (which Bevy just recomputed) preserves the
//! back-to-front order *within* each bucket. One interception point covers all
//! transparent content regardless of who queued it.
//!
//! **What this does and does not fix.** Combined with the water depth write it is
//! per-pixel correct for the common cases: translucent content clearly above the
//! surface draws over it, clearly below shows through it, and above-water content
//! (a fountain plume, a particle cloud) is occluded per pixel where it dips behind
//! the surface. It is **per-object** for the bucket assignment, so a single large
//! translucent prim that straddles the waterline is classified whole by its centre
//! (reference parity — the reference is per spatial-group, likewise not per-pixel);
//! a genuinely per-pixel split of one straddling mesh would need order-independent
//! transparency or a clip-plane double-draw, a separate future effort. Particle
//! clouds fare better than a lone prim because the water depth write refines the
//! above-water bucket per pixel.

use bevy::core_pipeline::core_3d::{Transparent3d, TransparentSortingInfo3d};
use bevy::ecs::query::QueryItem;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::ExtractResourcePlugin;
use bevy::render::render_phase::{ViewSortedRenderPhases, sort_phase_system};
use bevy::render::sync_component::SyncComponent;
use bevy::render::sync_world::MainEntity;
use bevy::render::{Render, RenderApp, RenderSystems};

use crate::water::{DEFAULT_WATER_HEIGHT, WaterLevel};

/// The sort bucket for translucent content **below** the water surface: drawn first
/// (before the water), so it is already composited and shows through the surface.
const BELOW_WATER_BUCKET: u8 = 0;
/// The sort bucket for the **water surface** itself: drawn between the below- and
/// above-water buckets, writing depth so the above-water bucket is occluded per
/// pixel where it dips behind the surface.
const WATER_BUCKET: u8 = 1;
/// The sort bucket for translucent content **above** the water surface: drawn last
/// (after the water), depth-tested against the water it draws over.
const ABOVE_WATER_BUCKET: u8 = 2;
/// The sort bucket for [`TransparentSortingInfo3d::AlwaysOnTop`] items: drawn after
/// everything else so they stay on top, as their name promises (selection
/// highlights and the like).
const ALWAYS_ON_TOP_BUCKET: u8 = 3;

/// The buckets must ascend below → water → above → always-on-top, because
/// [`sort_transparent_by_water`] sorts by bucket ascending and that is the order
/// the items are drawn in. Enforced at compile time so a reordering of the
/// constants can never silently invert the draw order.
const _: () = assert!(
    BELOW_WATER_BUCKET < WATER_BUCKET
        && WATER_BUCKET < ABOVE_WATER_BUCKET
        && ABOVE_WATER_BUCKET < ALWAYS_ON_TOP_BUCKET,
    "water sort buckets must ascend below < water < above < always-on-top"
);

/// Marks a water-surface entity (the endless ocean plane and each per-region plane,
/// tagged in [`crate::water`]) so [`sort_transparent_by_water`] can pin it to the
/// [`WATER_BUCKET`] regardless of its mesh centre — which, for a camera-following
/// or region-sized plane, is a useless sort reference.
///
/// Extracted into the render world (the entity is already synced there via its
/// `Mesh3d`), where the re-sort reads it.
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct WaterSurface;

impl ExtractComponent for WaterSurface {
    type QueryData = &'static Self;
    type QueryFilter = ();
    type Out = Self;

    /// Copy the marker into the render world for every water-surface entity.
    fn extract_component(item: QueryItem<'_, '_, Self::QueryData>) -> Option<Self> {
        Some(*item)
    }
}

impl SyncComponent for WaterSurface {
    type Target = Self;
}

/// Re-sort each view's [`Transparent3d`] phase by `(water_bucket, distance)` so
/// below-water translucency draws before the water and above-water translucency
/// after it, with each bucket kept in Bevy's back-to-front distance order.
///
/// Runs in [`RenderSystems::PhaseSort`] after Bevy's
/// [`sort_phase_system::<Transparent3d>`](sort_phase_system), which has already
/// recomputed every item's `distance` this frame — so this pass reuses that
/// distance for the within-bucket order and only overrides the *across*-water
/// order. Applied to every view uniformly: a view with no water (the HUD camera)
/// puts everything in one bucket, leaving its order unchanged.
fn sort_transparent_by_water(
    water_level: Option<Res<WaterLevel>>,
    water_surfaces: Query<&MainEntity, With<WaterSurface>>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
) {
    let level = water_level.map_or(DEFAULT_WATER_HEIGHT, |water_level| water_level.0);
    let water: HashSet<MainEntity> = water_surfaces.iter().copied().collect();
    for phase in phases.values_mut() {
        phase.items.sort_by(|a_key, a, b_key, b| {
            water_bucket(a, a_key.1, &water, level)
                .cmp(&water_bucket(b, b_key.1, &water, level))
                .then_with(|| a.distance.total_cmp(&b.distance))
        });
    }
}

/// The water bucket a transparent item sorts into: the [`WATER_BUCKET`] for a water
/// surface, else above / below by its mesh-centre height, with an always-on-top
/// item kept topmost.
fn water_bucket(
    item: &Transparent3d,
    main_entity: MainEntity,
    water: &HashSet<MainEntity>,
    level: f32,
) -> u8 {
    classify_bucket(item.sorting_info, water.contains(&main_entity), level)
}

/// The pure bucket decision behind [`water_bucket`], split out so it is testable
/// without a render world: a water surface is the [`WATER_BUCKET`]; otherwise an
/// always-on-top item is topmost and a sorted item is above / below by whether its
/// centre height reaches the water `level`.
fn classify_bucket(sorting_info: TransparentSortingInfo3d, is_water: bool, level: f32) -> u8 {
    if is_water {
        return WATER_BUCKET;
    }
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

/// Wires the water-relative transparency ordering into the app: extract the
/// [`WaterSurface`] markers and the [`WaterLevel`] into the render world, and add
/// the re-sort after Bevy's transparent sort. Add once, after `DefaultPlugins`,
/// like the other viewer render plugins.
#[derive(Debug, Default)]
pub(crate) struct TransparencyOrderPlugin;

impl Plugin for TransparencyOrderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<WaterSurface>::default(),
            ExtractResourcePlugin::<WaterLevel>::default(),
        ));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.add_systems(
            Render,
            sort_transparent_by_water
                .in_set(RenderSystems::PhaseSort)
                .after(sort_phase_system::<Transparent3d>),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ABOVE_WATER_BUCKET, ALWAYS_ON_TOP_BUCKET, BELOW_WATER_BUCKET, WATER_BUCKET, classify_bucket,
    };
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

    /// A water surface is pinned to the water bucket regardless of its (useless)
    /// mesh centre — even a plane whose centre sits far above or below the level.
    #[test]
    fn water_surface_is_always_the_water_bucket() {
        assert_eq!(classify_bucket(sorted_at(1000.0), true, 20.0), WATER_BUCKET);
        assert_eq!(
            classify_bucket(sorted_at(-1000.0), true, 20.0),
            WATER_BUCKET
        );
    }

    /// Non-water content buckets above or below by whether its centre reaches the
    /// water level; content exactly at the level counts as above (drawn over the
    /// surface it sits on).
    #[test]
    fn content_buckets_above_or_below_the_level() {
        assert_eq!(
            classify_bucket(sorted_at(25.0), false, 20.0),
            ABOVE_WATER_BUCKET
        );
        assert_eq!(
            classify_bucket(sorted_at(15.0), false, 20.0),
            BELOW_WATER_BUCKET
        );
        assert_eq!(
            classify_bucket(sorted_at(20.0), false, 20.0),
            ABOVE_WATER_BUCKET
        );
    }

    /// An always-on-top item stays topmost (it is not water and has no height).
    #[test]
    fn always_on_top_stays_topmost() {
        assert_eq!(
            classify_bucket(TransparentSortingInfo3d::AlwaysOnTop, false, 20.0),
            ALWAYS_ON_TOP_BUCKET
        );
    }
}
