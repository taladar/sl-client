//! The region terrain the world layers share.
//!
//! The land and water patches the simulator streams, folded per region into
//! the heightfield the scene tessellates, the minimap shades and the camera
//! and movement code ask to stand on. One store, so a height read anywhere in
//! the viewer answers from the same patches.

#![expect(
    clippy::module_name_repetitions,
    reason = "`TerrainState` and `RegionTerrain` are what the whole viewer calls \
              them; renaming them to satisfy a style rule this codebase does not \
              follow would churn every call site"
)]

use std::collections::HashMap;

use bevy::prelude::*;
use sl_client_bevy::{RegionHandle, TerrainPatch, TextureKey};
use sl_terrain::TerrainComposition;

/// The number of ground ("detail") textures a region blends between.
pub const DETAIL_COUNT: usize = 4;

/// A patch's key: its region plus grid position within that region.
pub type PatchKey = (RegionHandle, u32, u32);

/// Resolve the land height at region-local `(x, y)` from a patch map (the live
/// [`TerrainState::raw_patches`] or the retained [`TerrainState::land_cache`]).
/// `None` when no land patch in that map covers the point.
fn land_height_in(
    patches: &HashMap<PatchKey, TerrainPatch>,
    region: RegionHandle,
    x: f32,
    y: f32,
) -> Option<f32> {
    for (&(patch_region, _, _), patch) in patches {
        if patch_region != region || !patch.layer.is_land() {
            continue;
        }
        let span = f32::from(u16::try_from(patch.size).unwrap_or(u16::MAX));
        let x0 = f32::from(u16::try_from(patch.patch_x).unwrap_or(u16::MAX)) * span;
        let y0 = f32::from(u16::try_from(patch.patch_y).unwrap_or(u16::MAX)) * span;
        if x < x0 || y < y0 || x >= x0 + span || y >= y0 + span {
            continue;
        }
        // The floored offset into the patch, in `0..size`. The subtraction is
        // non-negative (guarded above) and below `size`, so the truncating cast
        // is exact.
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the offset is in 0.0..size, so it fits u32 exactly"
        )]
        let (cell_x, cell_y) = ((x - x0).floor() as u32, (y - y0).floor() as u32);
        let last = patch.size.saturating_sub(1);
        return patch.value(cell_x.min(last), cell_y.min(last));
    }
    None
}

/// Per-region terrain-compositing state: the elevation bands and the
/// detail-texture keys requested for the region's shared splat material. The
/// material handle itself is machinery and lives beside the systems that build
/// it, keyed by the same region handle.
#[derive(Debug, Default)]
pub struct RegionTerrain {
    /// The region's terrain-compositing parameters, once its `RegionHandshake`
    /// has been seen; `None` until then (patches render with flat weights).
    pub composition: Option<TerrainComposition>,
    /// The texture key requested for each detail slot (`None` for a nil slot),
    /// used to route an arriving texture to the right material slot.
    pub detail_keys: [Option<TextureKey>; DETAIL_COUNT],
    /// Whether this region's detail textures have been requested yet.
    pub requested: bool,
}

/// Viewer-side terrain bookkeeping across the home region and its neighbours.
#[derive(Debug, Resource, Default)]
pub struct TerrainState {
    /// The scene origin: the region whose south-west corner is Bevy `(0, 0)`.
    /// Follows the root region so coordinates stay small near the camera.
    pub origin: Option<RegionHandle>,
    /// Per-region compositing state, keyed by region handle.
    pub regions: HashMap<RegionHandle, RegionTerrain>,
    /// The rendered entity for each patch; a repeat patch replaces its mesh.
    pub patches: HashMap<PatchKey, Entity>,
    /// The most recent raw patch for each key, kept so a patch's mesh can be
    /// rebuilt (with real weights, or when a neighbour arrives) after the fact.
    pub raw_patches: HashMap<PatchKey, TerrainPatch>,
    /// The **last known land patch** for each key, retained across a region
    /// teardown / rebuild (and, with the disk layer, across sessions) so
    /// [`Self::land_height`] can still answer while the live [`Self::raw_patches`]
    /// are gone — a region's ground height is stable, so a stale cached value is a
    /// far better ground-floor answer than `None`. This is what keeps the avatar
    /// ground floor (`physics.rs`) working through the login window and the
    /// region-disappears-then-rebuilds flicker, so the avatar does not fall through
    /// the terrain while its patches are absent. Land layers only.
    pub land_cache: HashMap<PatchKey, TerrainPatch>,
    /// A monotonically increasing counter bumped whenever height or compositing
    /// data changes (a patch arrives, a handshake's composition lands), so a
    /// derived consumer (the minimap's terrain backdrop) can cheaply notice
    /// staleness without hashing the patch maps.
    map_revision: u64,
    /// Per-region version of [`map_revision`](Self::map_revision), bumped only
    /// for the region whose data actually changed, so a per-region consumer (the
    /// parcel-border bands, which ground-follow) can rebuild only the terraformed
    /// / newly-streamed region instead of all of them.
    region_revisions: HashMap<RegionHandle, u64>,
}

impl TerrainState {
    /// The scene origin region (whose south-west corner is Bevy `(0, 0)`), or
    /// `None` before the first terrain patch arrives.
    #[must_use]
    pub const fn origin(&self) -> Option<RegionHandle> {
        self.origin
    }

    /// Despawn every rendered land patch and forget all per-region terrain state —
    /// the terrain half of the distant-teleport scene purge
    /// ([`Event::RegionChanged`](sl_client_bevy::SlSessionEvent)'s `world_reset`).
    /// The destination streams its terrain fresh. Drops the origin so the
    /// recentring pass re-anchors on the destination without a spurious camera
    /// shift. The region materials and the decoded-detail-texture cache are the
    /// world layer's to purge (a texture shared with the destination need not be
    /// refetched).
    pub fn purge(&mut self, commands: &mut Commands) {
        for (_key, entity) in self.patches.drain() {
            commands.entity(entity).try_despawn();
        }
        self.regions.clear();
        self.raw_patches.clear();
        self.map_revision = self.map_revision.wrapping_add(1);
        // The purged regions are gone; a re-streamed region restarts at revision
        // 1, which a stale per-region stamp (its pre-purge revision) will not
        // match, so its bands rebuild.
        self.region_revisions.clear();
        self.origin = None;
    }

    /// The terrain-data revision — bumped on every ingested patch and learned
    /// composition, so a derived map texture knows when to rebuild.
    #[must_use]
    pub const fn map_revision(&self) -> u64 {
        self.map_revision
    }

    /// This region's own terrain revision, bumped only when *its* height or
    /// compositing data changes; `0` before any of it has arrived.
    #[must_use]
    pub fn region_revision(&self, region: RegionHandle) -> u64 {
        self.region_revisions.get(&region).copied().unwrap_or(0)
    }

    /// Bump both the global [`map_revision`](Self::map_revision) and `region`'s
    /// per-region revision — call wherever `region`'s height / compositing data
    /// changes.
    pub fn bump_revision(&mut self, region: RegionHandle) {
        self.map_revision = self.map_revision.wrapping_add(1);
        let revision = self.region_revisions.entry(region).or_insert(0);
        *revision = revision.wrapping_add(1);
    }

    /// Every decoded land patch of `region`, for compositing a top-down map.
    pub fn land_patches_of(&self, region: RegionHandle) -> impl Iterator<Item = &TerrainPatch> {
        self.raw_patches.iter().filter_map(move |(key, patch)| {
            (key.0 == region && patch.layer.is_land()).then_some(patch)
        })
    }

    /// The terrain-compositing parameters of `region`, once its handshake has
    /// been seen.
    #[must_use]
    pub fn composition_of(&self, region: RegionHandle) -> Option<&TerrainComposition> {
        self.regions
            .get(&region)
            .and_then(|entry| entry.composition.as_ref())
    }

    /// The ground height at region-local metre position (`x`, `y`) in `region`,
    /// read from the nearest decoded land-patch cell, or `None` when that region's
    /// terrain has not been ingested (or the point is off-region / non-finite).
    ///
    /// A land patch holds `size`×`size` height samples spanning `size` metres (one
    /// sample per metre for a standard 16-cell patch), so cell `(⌊x⌋, ⌊y⌋)` within
    /// the patch is the nearest sample. Used by the physics dead-reckoning (P31.2)
    /// as the ground floor an extrapolating object is clamped to
    /// (`getMinAllowedZ`).
    #[must_use]
    pub fn land_height(&self, region: RegionHandle, x: f32, y: f32) -> Option<f32> {
        if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
            return None;
        }
        // The live patches first; fall back to the retained cache when they are
        // absent (a region mid-rebuild, or not yet streamed after login), so the
        // avatar ground floor keeps a stable answer instead of dropping to `None`.
        land_height_in(&self.raw_patches, region, x, y)
            .or_else(|| land_height_in(&self.land_cache, region, x, y))
    }
}

impl crate::world_scoped::WorldScoped for TerrainState {
    fn purge_world(&mut self, _purge: crate::world_scoped::WorldPurge, commands: &mut Commands) {
        self.purge(commands);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{PatchKey, TerrainState};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{RegionHandle, TerrainLayerType, TerrainPatch};

    /// The region and grid position the terrain test patches use.
    const KEY: PatchKey = (RegionHandle(0), 1, 2);

    /// A single-patch map for the land patch of the given edge size whose height
    /// is `f(x, y)`, at [`KEY`].
    fn one_patch_map(
        size: u32,
        mut height: impl FnMut(u32, u32) -> f32,
    ) -> HashMap<PatchKey, TerrainPatch> {
        let mut values = Vec::new();
        for y in 0..size {
            for x in 0..size {
                values.push(height(x, y));
            }
        }
        let (region, patch_x, patch_y) = KEY;
        let patch = TerrainPatch {
            region_handle: region,
            layer: TerrainLayerType::Land,
            patch_x,
            patch_y,
            size,
            values,
        };
        let mut map = HashMap::new();
        map.insert(KEY, patch);
        map
    }

    #[test]
    fn per_region_revision_bumps_only_its_region() {
        let mut state = TerrainState::default();
        let a = RegionHandle(256_000);
        let b = RegionHandle(256_256);
        assert_eq!(state.region_revision(a), 0);
        assert_eq!(state.region_revision(b), 0);

        state.bump_revision(a);
        state.bump_revision(a);
        assert_eq!(state.region_revision(a), 2, "region a bumped twice");
        assert_eq!(state.region_revision(b), 0, "region b left untouched");

        let global_before = state.map_revision();
        state.bump_revision(b);
        assert_eq!(state.region_revision(b), 1);
        assert_eq!(state.region_revision(a), 2, "bumping b leaves a alone");
        assert!(
            state.map_revision() > global_before,
            "the global revision advances on any per-region bump"
        );
    }

    #[test]
    fn land_height_falls_back_to_the_retained_cache() {
        let mut state = TerrainState::default();
        // A patch whose height is a recognisable function of the cell.
        let map = one_patch_map(16, |x, y| {
            100.0 + f32::from(u16::try_from(x + y).unwrap_or(0))
        });
        // Only in the retained cache — the live patches are gone (a region mid-
        // rebuild, or not yet streamed after login), so `raw_patches` misses.
        state.land_cache = map;
        // Point (20.5, 35.5) is cell (4, 3) of the patch at grid (1, 2): height
        // 100 + (4 + 3) = 107. The cache answers even with no live patch.
        let height = state.land_height(RegionHandle(0), 20.5, 35.5);
        assert!(
            height.is_some_and(|height| (height - 107.0).abs() <= 1.0e-4),
            "land_height should fall back to the retained cache, got {height:?}"
        );
        // A region with no cached patch still returns `None` (nothing to stand on).
        assert!(
            state
                .land_height(RegionHandle(999_000), 20.5, 35.5)
                .is_none()
        );
    }
}
