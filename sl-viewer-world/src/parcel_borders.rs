//! In-world parcel borders (property lines): the banded vertical strips the
//! reference viewer draws along parcel boundaries, colour-coded by ownership and
//! toggled with the `ShowPropertyLines` setting.
//!
//! This consumes the decoded 64×64 parcel-overlay grid
//! ([`SlParcelOverlay`], one [`ParcelOverlayGrid`] per region) and drapes short
//! vertical bands over the terrain heightfield ([`TerrainState::land_height`])
//! along every parcel boundary the grid marks with a `west_line` / `south_line`
//! bit (plus the derived east / north edges, exactly as the reference's
//! `LLViewerParcelOverlay::renderPropertyLines` does). Each band's colour is the
//! owning square's ownership class (self green, group teal, someone-else's red,
//! for-sale orange, auction violet — the reference `PropertyColor*` palette).
//! Unlike the reference, public / unassigned land is also drawn (in grey) so its
//! extent is legible, and the **region's outer rim is always drawn in white** — a
//! sim crossing, shown even between two public regions, so where a region crossing
//! happens is unmistakable. The band fades from opaque at the ground to
//! transparent at its top (the vertical banding) and the shader fades it out with
//! camera distance (the reference's 256 m clip).
//!
//! Multi-region: the overlay and terrain are per-region and neighbour regions are
//! already streamed, so one band-mesh entity is spawned per region, placed at the
//! region's south-west corner relative to the moving scene origin — the same
//! placement the terrain patches use (`crate::terrain::patch_transform`). The
//! mesh is rebuilt when the overlay changes (parcels split / join / sell), when
//! the terrain heights change, or when the scene origin moves (a border
//! crossing); the frequent terrain updates during streaming are coalesced behind
//! a short cooldown.
//!
//! Modelled on `sl_client_bevy`'s `TerrainMaterial` (the repo's custom
//! `AsBindGroup` + `load_internal_asset!` + `MaterialPlugin` template): a tiny
//! unlit, alpha-blended `ParcelBorderMaterial` whose per-vertex colour carries
//! the ownership tint and the band fade, and whose fragment shader adds the
//! camera-distance fade from the view bind group (so nothing mutates per frame).

use std::collections::{HashMap, HashSet};

use bevy::asset::{Asset, RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

use sl_client_bevy::{
    ParcelOverlayGrid, ParcelOwnership, RegionHandle, SlParcelOverlay, SlRegion, Vector,
};

use crate::coords::{metres_to_f32, sl_to_bevy_rotation, sl_to_bevy_vec};
use crate::settings::ViewerSettings;
use crate::terrain::TerrainState;
use crate::terrain::update_terrain;
use crate::water::WaterState;

/// The setting name gating the in-world property lines (the reference viewer's
/// `ShowPropertyLines`). Registered in [`register_settings`].
pub const SETTING_SHOW_PROPERTY_LINES: &str = "ShowPropertyLines";

/// The settings section the property-lines toggle lives under.
const PARCEL_SECTION: &[&str] = &["world"];

/// The internal handle the property-line shader (`parcel_borders.wgsl`) is loaded
/// under, so the material can reference it without an on-disk asset path.
const PARCEL_BORDER_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("b3f0c9a4-6e21-4d7b-9a83-5c1f2e8d4a60");

/// Side length, in metres, of one parcel-overlay grid square (the reference's
/// `PARCEL_GRID_STEP_METERS`). A local copy of `sl_proto::PARCEL_GRID_STEP_METRES`
/// (that crate is only a dev-dependency here); the 4 m step is fixed protocol.
const PARCEL_GRID_STEP_METRES: f32 = 4.0;

/// The height, in metres, each property-line band rises from the ground. The
/// reference's modern `renderPropertyLines` draws a flat ribbon, but the classic
/// banded look (and the reference's parcel-boundary posts, `PARCEL_POST_HEIGHT`
/// = 0.666 m) is a short vertical wall; one metre reads clearly at avatar scale
/// while staying a low band rather than a fence.
const BAND_HEIGHT_METRES: f32 = 1.0;

/// The base alpha at a band's foot (its top fades to zero). Matches the
/// reference `PropertyColor*` alpha (`0.4`).
const BAND_BASE_ALPHA: f32 = 0.4;

/// How far, in metres, each band is inset from the exact parcel boundary toward
/// the owning square's interior (the reference's `LINE_WIDTH` tick). This offsets
/// the two bands of a shared boundary a hair apart, so adjacent parcels show
/// their own colour on their own side rather than z-overlapping into one.
const BAND_INSET_METRES: f32 = 0.0625;

/// How many sub-segments each 4 m band is split into, so it drapes over terrain
/// undulations within a cell (one sample per metre, matching the terrain grid).
const BAND_SUBDIVISIONS: usize = 4;

/// How many region band meshes to rebuild per frame. A multi-region refresh
/// (first enable, or crossing into an area where several neighbours appear at
/// once) then spreads over a few frames instead of one hitch; property lines
/// filling in a few frames late is invisible.
const PARCEL_REBUILD_BUDGET: usize = 2;

/// How far, in metres, a water-clamped band foot sits above the water surface —
/// the reference's `+0.01` fudge, keeping the band from z-fighting the water
/// plane where a boundary rides on the sea.
const WATER_SURFACE_EPSILON: f32 = 0.01;

/// How far, in metres, inside the region edge a band's terrain height is sampled
/// when the band vertex sits on (or beyond) the exact region boundary — where
/// `TerrainState::land_height` returns nothing (its patch bounds are exclusive at
/// the far edge). The band vertex stays on the boundary; only the height read is
/// nudged inside, so region-rim bands are not dropped near the corners.
const EDGE_SAMPLE_INSET_METRES: f32 = 0.1;

/// The tint for public / unowned parcel boundaries. The reference draws none
/// (`PropertyColorAvail` is transparent), but we draw them in a neutral grey so
/// public land's extent is legible — you can see where unowned land begins and
/// ends, and where two public parcels meet.
const PUBLIC_COLOR: [f32; 3] = [0.55, 0.55, 0.55];

/// The tint for the region's outer edge — a **sim crossing**. Always drawn (even
/// between two public regions) in a distinct white so the region boundary, and
/// hence where a region crossing happens, is unmistakable.
const SIM_CROSSING_COLOR: [f32; 3] = [1.0, 1.0, 1.0];

/// A tiny unlit, alpha-blended material for the property-line bands: the ownership
/// tint and the band's vertical fade ride the mesh's per-vertex colour, and the
/// fragment shader adds a camera-distance fade from the view bind group, so the
/// material itself carries no per-frame state (an empty bind group).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, Default)]
#[expect(
    clippy::empty_structs_with_brackets,
    reason = "the AsBindGroup derive requires the braced struct form; the bind group is \
              deliberately empty (all per-vertex, no material uniforms)"
)]
pub(crate) struct ParcelBorderMaterial {}

impl Material for ParcelBorderMaterial {
    /// The bundled property-line shader carries the per-vertex colour through to
    /// the fragment stage.
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(PARCEL_BORDER_SHADER_HANDLE)
    }

    /// The bundled property-line shader applies the camera-distance fade and emits
    /// the ownership colour.
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(PARCEL_BORDER_SHADER_HANDLE)
    }

    /// Alpha-blended: the bands are translucent coloured overlays, so they sort in
    /// the transparent phase (depth-tested against the world, no depth write).
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    /// No depth / normal prepass: the mesh carries only position + colour (no
    /// normal / UV), so the default prepass vertex shader's required inputs would
    /// be unsatisfied — and a translucent overlay belongs in neither prepass.
    fn enable_prepass() -> bool {
        false
    }

    /// The bands cast no shadows (a translucent overlay, not solid geometry).
    fn enable_shadows() -> bool {
        false
    }

    /// Pin the vertex layout to position + colour (matching the shader's
    /// `@location`s) and draw both faces of each band (no back-face culling), so a
    /// band is visible from either side.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(1),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        // Keep the border lines' coverage out of the scene alpha (the glow mask) so
        // they do not bloom under the glow pass.
        sl_client_bevy::preserve_glow_mask_alpha(descriptor);
        Ok(())
    }
}

/// A marker on a spawned per-region property-line band entity.
#[derive(Component, Debug)]
struct ParcelBorderSurface;

/// Viewer-side bookkeeping for the in-world property lines: the per-region band
/// entities and the per-region stamps that decide, change-driven, which regions
/// to rebuild.
#[derive(Resource, Default)]
struct ParcelBorderState {
    /// The spawned band entity per region.
    entities: HashMap<RegionHandle, Entity>,
    /// The shared band material, built once on the first rebuild.
    material: Option<Handle<ParcelBorderMaterial>>,
    /// The scene origin the current bands were placed against.
    last_origin: Option<RegionHandle>,
    /// Whether the bands are currently shown (the setting was on last frame).
    active: bool,
    /// Per-region stamp of everything a rebuild depends on; a region is rebuilt
    /// only when its current stamp differs (or it is newly present).
    stamps: HashMap<RegionHandle, RegionStamp>,
    /// Regions marked dirty and awaiting a (budgeted) rebuild.
    pending: HashSet<RegionHandle>,
}

/// The inputs a region's border bands depend on: its parcel-overlay grid, its
/// water height (as raw bits, so it compares without a float `==`), and its
/// per-region terrain revision. A rebuild is needed exactly when one of these
/// differs from the last build.
struct RegionStamp {
    /// The parcel-overlay grid the bands were tessellated from.
    grid: ParcelOverlayGrid,
    /// The region's water height at build time (`f32::to_bits`), or `None`.
    water_bits: Option<u32>,
    /// The region's [`TerrainState::region_revision`] at build time.
    terrain_revision: u64,
}

/// Register the property-lines settings (the master `ShowPropertyLines` toggle).
/// The reference default is off, but we default it **on** because the viewer has
/// no preferences UI to turn it on yet; the World ▸ Property Lines menu entry
/// toggles it.
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        PARCEL_SECTION,
        SETTING_SHOW_PROPERTY_LINES,
        sl_settings::SettingValue::Bool(true),
        "Show the in-world parcel property lines (colour-coded by ownership)",
    );
}

/// The property-line tint for an ownership class. The owned classes mirror the
/// reference `PropertyColorSelf` / `Group` / `Other` / `ForSale` / `Auction`;
/// public / unassigned land — which the reference draws nothing for — is drawn in
/// [`PUBLIC_COLOR`] so its extent stays legible (unlike the reference, every
/// parcel boundary is shown).
const fn ownership_color(ownership: ParcelOwnership) -> [f32; 3] {
    match ownership {
        ParcelOwnership::SelfOwned => [0.0, 1.0, 0.0],
        ParcelOwnership::Group => [0.0, 0.72, 0.72],
        ParcelOwnership::Owned => [1.0, 0.0, 0.0],
        ParcelOwnership::ForSale => [1.0, 0.5, 0.0],
        ParcelOwnership::Auction => [0.5, 0.0, 1.0],
        _ => PUBLIC_COLOR,
    }
}

/// The region-local metre coordinate of overlay-grid line `index` (a column's
/// western edge or a row's southern edge), `index × 4 m`.
fn grid_metres(index: usize) -> f32 {
    f32::from(u16::try_from(index).unwrap_or(u16::MAX)) * PARCEL_GRID_STEP_METRES
}

/// The growing vertex / index buffers a region's property-line bands accumulate
/// into, before they are handed to a [`Mesh`].
#[derive(Default)]
struct BandMesh {
    /// Band vertex positions (Second Life space, relative to the region corner).
    positions: Vec<[f32; 3]>,
    /// Per-vertex colour: the ownership tint in `rgb`, the ground→top band fade
    /// in `a`.
    colors: Vec<[f32; 4]>,
    /// Triangle indices into [`positions`](Self::positions).
    indices: Vec<u32>,
}

/// The per-region context [`add_band`] drapes a band over: the terrain
/// heightfield, the region it samples, the region's water surface (when present),
/// and the region's width in metres (for clamping edge samples inside the region).
struct DrapeContext<'terrain> {
    /// The terrain heightfield to read the band's ground height from.
    terrain: &'terrain TerrainState,
    /// The region whose terrain / water this band belongs to.
    region: RegionHandle,
    /// The region's water surface plus a hair, if it has water — a band foot below
    /// it is lifted onto it (rides the surface rather than the seabed).
    water_floor: Option<f32>,
    /// The region's width in metres (`grids_per_edge × 4`), the far edge terrain
    /// samples are clamped just inside.
    width_metres: f32,
}

impl DrapeContext<'_> {
    /// The band-foot height at region-local `(x, y)`, or `None` if that point's
    /// terrain patch is not loaded yet. The height is sampled at a coordinate
    /// clamped just inside the region (`land_height` returns nothing exactly on
    /// the far edge), then lifted onto the water surface where submerged.
    fn foot_height(&self, x: f32, y: f32) -> Option<f32> {
        let far = self.width_metres - EDGE_SAMPLE_INSET_METRES;
        let sample_x = x.clamp(0.0, far);
        let sample_y = y.clamp(0.0, far);
        let terrain_z = self.terrain.land_height(self.region, sample_x, sample_y)?;
        Some(match self.water_floor {
            Some(floor) => terrain_z.max(floor),
            None => terrain_z,
        })
    }
}

/// Append one property-line band — a terrain-draped vertical strip running from
/// region-local `start` to `end` (metres, `x` east / `y` north) — to `mesh`,
/// tinted `color`. The strip is sub-divided so it follows the ground, its foot at
/// the terrain height and its top [`BAND_HEIGHT_METRES`] above (fading to
/// transparent). A foot below the region water surface is lifted onto it, so a
/// boundary crossing water rides on the surface rather than sinking to the seabed
/// (the reference's above-water behaviour). If any sample point's terrain is not
/// loaded yet the band is skipped entirely; it reappears when the region's patches
/// stream in and the mesh is rebuilt.
fn add_band(
    mesh: &mut BandMesh,
    drape: &DrapeContext,
    start: (f32, f32),
    end: (f32, f32),
    color: [f32; 3],
) {
    let (start_x, start_y) = start;
    let (end_x, end_y) = end;
    let steps = f32::from(u16::try_from(BAND_SUBDIVISIONS).unwrap_or(1)).max(1.0);
    // Collect every sub-point's ground position first; bail (drawing nothing) if
    // any of them is off a not-yet-loaded patch.
    let mut ground: Vec<(f32, f32, f32)> = Vec::with_capacity(BAND_SUBDIVISIONS.saturating_add(1));
    for step in 0..=BAND_SUBDIVISIONS {
        let t = f32::from(u16::try_from(step).unwrap_or(0)) / steps;
        let x = start_x + (end_x - start_x) * t;
        let y = start_y + (end_y - start_y) * t;
        let Some(z) = drape.foot_height(x, y) else {
            return;
        };
        ground.push((x, y, z));
    }
    let [r, g, b] = color;
    let mut previous: Option<(f32, f32, f32)> = None;
    for &(cur_x, cur_y, cur_z) in &ground {
        if let Some((prev_x, prev_y, prev_z)) = previous {
            let base = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
            let base1 = base.saturating_add(1);
            let base2 = base.saturating_add(2);
            let base3 = base.saturating_add(3);
            // Two ground vertices (opaque foot) and two top vertices (transparent
            // crown), forming the band quad between this pair of sub-points.
            mesh.positions.push([prev_x, prev_y, prev_z]);
            mesh.positions.push([cur_x, cur_y, cur_z]);
            mesh.positions
                .push([cur_x, cur_y, cur_z + BAND_HEIGHT_METRES]);
            mesh.positions
                .push([prev_x, prev_y, prev_z + BAND_HEIGHT_METRES]);
            mesh.colors.push([r, g, b, BAND_BASE_ALPHA]);
            mesh.colors.push([r, g, b, BAND_BASE_ALPHA]);
            mesh.colors.push([r, g, b, 0.0]);
            mesh.colors.push([r, g, b, 0.0]);
            mesh.indices
                .extend_from_slice(&[base, base1, base2, base, base2, base3]);
        }
        previous = Some((cur_x, cur_y, cur_z));
    }
}

/// One property-line band to draw: a boundary segment in region-local metres and
/// its ownership tint. [`add_band`] drapes it over the terrain into geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BorderEdge {
    /// The segment start (`x` east, `y` north), inset toward the owning square.
    start: (f32, f32),
    /// The segment end, likewise inset.
    end: (f32, f32),
    /// The owning square's ownership tint.
    color: [f32; 3],
}

/// Derive every property-line band from a region's overlay grid (pure — no
/// terrain). Each square contributes a band on any edge that carries a parcel
/// boundary: its own `west_line` / `south_line`, and the derived east / north
/// edges where the neighbouring square carries the shared boundary — the four
/// cases the reference `renderPropertyLines` handles. Interior boundaries take the
/// square's ownership tint (public land in [`PUBLIC_COLOR`], drawn — unlike the
/// reference — so its extent is legible). Every edge on the **region's outer rim**
/// is drawn unconditionally in [`SIM_CROSSING_COLOR`]: the region boundary (a sim
/// crossing) is always shown, even between two public regions. Each band is inset
/// [`BAND_INSET_METRES`] toward the owning square so a shared boundary shows each
/// parcel's colour on its own side.
fn region_border_edges(grid: &ParcelOverlayGrid) -> Vec<BorderEdge> {
    let edge = grid.grids_per_edge();
    let last = edge.saturating_sub(1);
    let mut edges: Vec<BorderEdge> = Vec::new();
    for (row, col, cell) in grid.cells() {
        let color = ownership_color(cell.ownership);
        let left = grid_metres(col);
        let right = left + PARCEL_GRID_STEP_METRES;
        let bottom = grid_metres(row);
        let top = bottom + PARCEL_GRID_STEP_METRES;
        // West edge (south→north along the square's western side): a parcel
        // boundary bit, or the region's western rim.
        let west_rim = col == 0;
        if cell.west_line || west_rim {
            let x = left + BAND_INSET_METRES;
            edges.push(BorderEdge {
                start: (x, bottom),
                end: (x, top),
                color: if west_rim { SIM_CROSSING_COLOR } else { color },
            });
        }
        // East edge: the region's eastern rim, or where the square to the east
        // carries the shared boundary as its western line.
        let east_rim = col >= last;
        let east_neighbour_line = grid
            .cell(row, col.saturating_add(1))
            .is_some_and(|neighbour| neighbour.west_line);
        if east_rim || east_neighbour_line {
            let x = right - BAND_INSET_METRES;
            edges.push(BorderEdge {
                start: (x, bottom),
                end: (x, top),
                color: if east_rim { SIM_CROSSING_COLOR } else { color },
            });
        }
        // South edge (west→east along the square's southern side): a parcel
        // boundary bit, or the region's southern rim.
        let south_rim = row == 0;
        if cell.south_line || south_rim {
            let y = bottom + BAND_INSET_METRES;
            edges.push(BorderEdge {
                start: (left, y),
                end: (right, y),
                color: if south_rim { SIM_CROSSING_COLOR } else { color },
            });
        }
        // North edge: the region's northern rim, or where the square to the north
        // carries the shared boundary as its southern line.
        let north_rim = row >= last;
        let north_neighbour_line = grid
            .cell(row.saturating_add(1), col)
            .is_some_and(|neighbour| neighbour.south_line);
        if north_rim || north_neighbour_line {
            let y = top - BAND_INSET_METRES;
            edges.push(BorderEdge {
                start: (left, y),
                end: (right, y),
                color: if north_rim { SIM_CROSSING_COLOR } else { color },
            });
        }
    }
    edges
}

/// Build one region's property-line band mesh from its overlay grid, or `None`
/// when the region has no coloured boundary drawn yet (or none of it is over
/// loaded terrain). Vertices are in Second Life space relative to the region's
/// south-west corner (the entity transform carries them into Bevy space, like the
/// terrain patches).
fn build_region_border_mesh(
    grid: &ParcelOverlayGrid,
    region: RegionHandle,
    terrain: &TerrainState,
    water_floor: Option<f32>,
) -> Option<Mesh> {
    let drape = DrapeContext {
        terrain,
        region,
        water_floor,
        width_metres: grid_metres(grid.grids_per_edge()),
    };
    let mut band = BandMesh::default();
    for border in region_border_edges(grid) {
        add_band(&mut band, &drape, border.start, border.end, border.color);
    }
    if band.positions.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, band.positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, band.colors);
    mesh.insert_indices(Indices::U32(band.indices));
    Some(mesh)
}

/// The entity transform placing a region's band mesh in the scene: the region's
/// south-west corner relative to the scene `origin`, converted to Bevy's Y-up
/// world — the same offset the terrain patches use, so the bands sit exactly on
/// their region's terrain.
fn region_transform(origin: Option<RegionHandle>, region: RegionHandle) -> Transform {
    let (region_x, region_y) = region.global_coordinates();
    let (origin_x, origin_y) = origin.unwrap_or(region).global_coordinates();
    let position = Vector {
        x: metres_to_f32(region_x) - metres_to_f32(origin_x),
        y: metres_to_f32(region_y) - metres_to_f32(origin_y),
        z: 0.0,
    };
    Transform {
        translation: sl_to_bevy_vec(&position),
        rotation: sl_to_bevy_rotation(),
        ..default()
    }
}

/// Despawn every spawned band entity and forget them.
fn despawn_all(state: &mut ParcelBorderState, commands: &mut Commands) {
    for (_region, entity) in state.entities.drain() {
        commands.entity(entity).despawn();
    }
}

/// Keep the in-world property-line bands current, **change-driven per region**:
/// each frame, rebuild only the regions whose stamp (parcel grid + terrain
/// revision + water height) changed or that are newly present, despawn regions
/// that left, and spread multi-region rebuilds over a few frames. A parked scene
/// with a static parcel layout does no rebuilds at all. Tears the bands down when
/// the `ShowPropertyLines` setting is off.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the setting, \
              the per-region overlay grids, the terrain heightfield, the per-region water \
              heights, the region entities, the mesh + material asset stores, this feature's \
              state, and the command buffer to spawn / despawn band entities"
)]
fn update_parcel_borders(
    settings: Res<ViewerSettings>,
    overlay: Res<SlParcelOverlay>,
    terrain: Res<TerrainState>,
    water: Res<WaterState>,
    regions: Query<&SlRegion>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ParcelBorderMaterial>>,
    mut state: ResMut<ParcelBorderState>,
    mut commands: Commands,
) {
    let show = settings
        .store()
        .get_bool(SETTING_SHOW_PROPERTY_LINES)
        .unwrap_or(true);
    if !show {
        if state.active || !state.entities.is_empty() {
            despawn_all(&mut state, &mut commands);
            state.stamps.clear();
            state.pending.clear();
            state.active = false;
        }
        return;
    }
    // On (re)enable the stamps are empty, so every region reads as new below.
    state.active = true;

    let origin = terrain.origin();
    let overlay_changed = overlay.is_changed();
    let current: HashSet<RegionHandle> = regions.iter().map(|region| region.handle).collect();

    // Despawn (and forget) regions that are no longer loaded.
    let gone: Vec<RegionHandle> = state
        .entities
        .keys()
        .copied()
        .filter(|region| !current.contains(region))
        .collect();
    for region in gone {
        if let Some(entity) = state.entities.remove(&region) {
            commands.entity(entity).despawn();
        }
        state.stamps.remove(&region);
        state.pending.remove(&region);
    }

    // An origin shift re-places every region; the band meshes are region-relative,
    // so only their transforms move — rewrite those, never rebuild the geometry.
    if origin != state.last_origin {
        for (region, entity) in &state.entities {
            commands
                .entity(*entity)
                .insert(region_transform(origin, *region));
        }
        state.last_origin = origin;
    }

    // Mark regions whose stamp changed (or that are new) for a rebuild. In the
    // steady state nothing changed → nothing is marked → no work below.
    for &region in &current {
        let dirty = match state.stamps.get(&region) {
            None => true,
            Some(stamp) => {
                terrain.region_revision(region) != stamp.terrain_revision
                    || water.height_of(region).map(f32::to_bits) != stamp.water_bits
                    // The grid compare is O(cells), so only run it when the
                    // overlay resource actually changed this frame.
                    || (overlay_changed && overlay.grid_of(region) != Some(&stamp.grid))
            }
        };
        if dirty {
            state.pending.insert(region);
        }
    }
    if state.pending.is_empty() {
        return;
    }

    // Rebuild a budgeted number of dirty regions this frame; the rest wait.
    let material = state
        .material
        .get_or_insert_with(|| materials.add(ParcelBorderMaterial::default()))
        .clone();
    let to_build: Vec<RegionHandle> = state
        .pending
        .iter()
        .copied()
        .take(PARCEL_REBUILD_BUDGET)
        .collect();
    for region in to_build {
        state.pending.remove(&region);
        let Some(grid) = overlay.grid_of(region) else {
            continue; // grid not streamed yet; re-detected as dirty next frame
        };
        let water_bits = water.height_of(region).map(f32::to_bits);
        // The region's water surface (plus a hair), so a boundary crossing water
        // rides on it rather than sinking to the seabed.
        let water_floor = water
            .height_of(region)
            .map(|height| height + WATER_SURFACE_EPSILON);
        let Some(mesh) = build_region_border_mesh(grid, region, &terrain, water_floor) else {
            continue;
        };
        let handle = meshes.add(mesh);
        let entity = commands
            .spawn((
                Mesh3d(handle),
                MeshMaterial3d(material.clone()),
                region_transform(origin, region),
                ParcelBorderSurface,
            ))
            .id();
        if let Some(old) = state.entities.insert(region, entity) {
            commands.entity(old).despawn();
        }
        state.stamps.insert(
            region,
            RegionStamp {
                grid: grid.clone(),
                water_bits,
                terrain_revision: terrain.region_revision(region),
            },
        );
    }
}

/// The plugin wiring the in-world property lines: it loads the band shader,
/// registers the `ParcelBorderMaterial`, and runs `update_parcel_borders`
/// after the terrain fold (so the heightfield it drapes over is current).
#[derive(Debug, Default)]
pub struct ParcelBordersPlugin;

impl Plugin for ParcelBordersPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            PARCEL_BORDER_SHADER_HANDLE,
            "parcel_borders.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins(MaterialPlugin::<ParcelBorderMaterial>::default())
            .init_resource::<ParcelBorderState>()
            .add_systems(Update, update_parcel_borders.after(update_terrain));
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use sl_client_bevy::ParcelOverlayGrid;

    use super::{
        BAND_INSET_METRES, PUBLIC_COLOR, ParcelOwnership, SIM_CROSSING_COLOR, grid_metres,
        ownership_color, region_border_edges,
    };

    /// The low-bits ownership class for "owned by you".
    const SELF_OWNED: u8 = 3;
    /// The `PARCEL_WEST_LINE` bit.
    const WEST: u8 = 0x40;
    /// The `PARCEL_SOUTH_LINE` bit.
    const SOUTH: u8 = 0x80;
    /// Self-owned green.
    const GREEN: [f32; 3] = [0.0, 1.0, 0.0];

    /// Whether two region-local points are within a hair of each other (the band
    /// coordinates are exact, but `float_cmp` forbids `==` on floats).
    fn near(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4
    }

    /// Whether two colours match component-wise within a hair.
    fn near_color(a: [f32; 3], b: [f32; 3]) -> bool {
        let [ar, ag, ab] = a;
        let [br, bg, bb] = b;
        (ar - br).abs() < 1e-4 && (ag - bg).abs() < 1e-4 && (ab - bb).abs() < 1e-4
    }

    /// Build a grid `edge` squares on a side from `(row, col, packed byte)`
    /// triples (row 0 = south, col 0 = west), the rest public.
    fn grid_from(edge: usize, cells: &[(usize, usize, u8)]) -> ParcelOverlayGrid {
        let mut data = vec![0_u8; edge.saturating_mul(edge)];
        for &(row, col, byte) in cells {
            let index = row.saturating_mul(edge).saturating_add(col);
            if let Some(slot) = data.get_mut(index) {
                *slot = byte;
            }
        }
        let mut grid = ParcelOverlayGrid::new(edge);
        grid.ingest_chunk(0, &data).expect("a full-grid chunk fits");
        grid
    }

    #[test]
    fn public_and_reserved_squares_take_the_public_grey() {
        assert!(near_color(
            ownership_color(ParcelOwnership::Public),
            PUBLIC_COLOR
        ));
        assert!(near_color(
            ownership_color(ParcelOwnership::Reserved(6)),
            PUBLIC_COLOR
        ));
        assert!(near_color(
            ownership_color(ParcelOwnership::SelfOwned),
            GREEN
        ));
        assert!(near_color(
            ownership_color(ParcelOwnership::Auction),
            [0.5, 0.0, 1.0]
        ));
    }

    #[test]
    fn an_interior_boundary_takes_the_owning_squares_colour() {
        // A 4×4 grid, an interior self-owned square (1, 1) with west + south
        // boundaries — well away from the rim, so its bands take the ownership
        // colour, not the sim-crossing white.
        let grid = grid_from(4, &[(1, 1, SELF_OWNED | WEST | SOUTH)]);
        let edges = region_border_edges(&grid);
        let x1 = grid_metres(1);
        // West edge of (1, 1): x = 4 + inset, spanning y 4..8, self-green.
        assert!(
            edges
                .iter()
                .any(|e| near(e.start, (x1 + BAND_INSET_METRES, x1))
                    && near(e.end, (x1 + BAND_INSET_METRES, grid_metres(2)))
                    && near_color(e.color, GREEN)),
            "an interior west edge in the ownership colour"
        );
        // The public square (1, 0) to its west derives the shared boundary as its
        // east edge, in the public grey — so public land's extent is visible too.
        assert!(
            edges
                .iter()
                .any(|e| near(e.start, (x1 - BAND_INSET_METRES, x1))
                    && near_color(e.color, PUBLIC_COLOR)),
            "the public neighbour draws the shared boundary in grey"
        );
    }

    #[test]
    fn a_public_region_still_shows_its_rim_as_a_sim_crossing() {
        // A 1×1 fully public region: no ownership colour, but all four outer edges
        // are the region rim — a sim crossing — so all four are drawn in white.
        let grid = grid_from(1, &[]);
        let edges = region_border_edges(&grid);
        assert!(edges.len() == 4, "all four region-rim edges");
        assert!(
            edges
                .iter()
                .all(|e| near_color(e.color, SIM_CROSSING_COLOR)),
            "every rim edge is the sim-crossing colour"
        );
        let far = grid_metres(1) - BAND_INSET_METRES;
        assert!(
            edges
                .iter()
                .any(|e| near(e.start, (BAND_INSET_METRES, 0.0)))
        );
        assert!(edges.iter().any(|e| near(e.start, (far, 0.0))));
    }

    #[test]
    fn an_interior_shared_boundary_is_drawn_from_both_sides() {
        // Two interior self-owned squares side by side, the eastern one carrying
        // the shared west line: the western square derives an east edge for it and
        // the eastern square draws its own west edge — the two-sided line, inset a
        // hair to either side of the boundary at x = 8.
        let grid = grid_from(4, &[(1, 1, SELF_OWNED), (1, 2, SELF_OWNED | WEST)]);
        let edges = region_border_edges(&grid);
        let boundary = grid_metres(2);
        assert!(
            edges.iter().any(
                |e| near(e.start, (boundary - BAND_INSET_METRES, grid_metres(1)))
                    && near_color(e.color, GREEN)
            ),
            "the west square draws the shared boundary as its east edge"
        );
        assert!(
            edges.iter().any(
                |e| near(e.start, (boundary + BAND_INSET_METRES, grid_metres(1)))
                    && near_color(e.color, GREEN)
            ),
            "the east square draws the shared boundary as its west edge"
        );
    }
}
