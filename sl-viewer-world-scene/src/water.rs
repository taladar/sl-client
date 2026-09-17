//! Water-surface rendering (P23.1): render the Second Life sea as a flat
//! horizontal plane at the region water height, shaded from the region's
//! Extended-Environment (EEP) water settings.
//!
//! The heavy lifting is a port of the reference viewer's water shaders
//! ([`WaterMaterial`] / `water.wgsl`, `LLDrawPoolWater`,
//! `class1/environment/waterV.glsl` + `class3/environment/waterF.glsl`): scrolling
//! wave normals, a fresnel-blended sky reflection, the water-fog deep-water tint,
//! and a sun specular highlight. This module drives that material and places the
//! planes:
//!
//! - `setup_water` creates the shared water material, spawns the **endless
//!   ocean** plane (a large camera-following plane at the agent-region water
//!   height, filling the sea everywhere there is no loaded region — the reference
//!   `LLWorld::updateWaterObjects` hole / edge water), and registers
//!   [`WaterState`];
//! - `update_water` learns each region's water height from its
//!   [`SlSessionEvent::RegionInfoHandshake`];
//! - `drive_water` centres the ocean on the camera, reconciles a **per-region
//!   plane** for every loaded region whose water height differs from the agent
//!   region's (so a neighbour with a different sea level renders at its own
//!   height), folds the blended EEP water settings + sun direction + sky
//!   reflection tint + wave-scroll time into the shared material, and requests the
//!   wave normal map **boosted**;
//! - `apply_water_textures` swaps the decoded normal map into the material.
//!
//! **Model (matches the reference).** Per `LLDrawPoolWater::render`, the water
//! **colour / waves / fresnel are region-wide** — a single `getCurrentWater()`
//! (the agent's current, position-selected EEP environment) binds the whole water
//! pass, applying the same look everywhere for a consistent scene. Only the water
//! **height** varies per region. So this uses one shared material (the current
//! [`EnvironmentState`] water) for every plane, and varies only the plane height.
//!
//! **Simplification.** The endless-ocean plane spans the whole visible area at the
//! agent-region height, including under loaded regions; a per-region plane is only
//! spawned where a region's height *differs* from the agent region's, so the
//! common all-same-height case is a single clean surface. Where heights genuinely
//! differ, the differing region's plane sits at its own height over the ocean (the
//! reference instead omits hole / edge water inside a region footprint); the water
//! is alpha-blended, so any overlap reads as a faint double surface — an accepted
//! trade for not tiling the ocean around every region footprint.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sl_client_bevy::{
    Color as SlColor, DecodedTexture, RegionHandle, SlEvent, SlIdentity, SlSessionEvent,
    TextureKey, WaterMaterial, WaterParams, WaterSettings,
};

use crate::environment::EnvironmentState;
use crate::probe_layers::environment_render_layers;
use crate::sky::day_position;
use crate::textures::{TextureDecoded, TextureManager};
use crate::water_fog::WaterFogSettings;
use crate::world_api::world_scoped::WorldScopedAppExt as _;
use crate::world_api::{DecodedTextures, SKY_BOOST_PRIORITY, ViewerCamera, WorldPhase};

/// The water surface's own scheduling: the endless ocean and the per-region
/// planes, spawned at `Startup` and driven every frame.
///
/// `drive_water` centres the ocean on the viewpoint, so it orders against
/// [`WorldPhase::CameraPositioned`] rather than naming the camera system —
/// which is what lets the ocean markers and the water state stay private here.
#[derive(Debug, Default)]
pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        // `WaterState` is inserted by `setup_water` (it needs the shared material
        // and mesh), so only its purge is registered here.
        app.init_resource::<WaterLevel>()
            .register_world_scoped::<WaterState>()
            .add_systems(Startup, setup_water)
            .add_systems(
                Update,
                (
                    // Learn each region's water height, then centre the endless
                    // ocean on the camera and place a per-region plane where a
                    // neighbour's sea level differs, and swap in the decoded wave
                    // normal map.
                    update_water,
                    drive_water.after(WorldPhase::CameraPositioned),
                    apply_water_textures,
                ),
            );
    }
}

/// A standard Second Life / OpenSim region edge length, in metres.
const REGION_SIZE_METRES: f32 = 256.0;

/// How far the sea grid reaches from the camera, in **cells** of one region each.
///
/// The camera far plane is 4096 m, and nothing is drawn past it, so 16 cells cover
/// everything visible; one more absorbs the half-cell the camera sits inside and the
/// frustum corners. 35x35 cells share one mesh and one material, so they batch into
/// a single draw.
const SEA_GRID_RADIUS_CELLS: u32 = 17;

/// The default water height, in metres, used for the endless ocean until the agent
/// region's handshake supplies the real one (the standard Second Life sea level;
/// see `map.rs`).
///
/// Public because every other reader of a region's sea level needs the same
/// fallback for a region that has not handshaked — the minimap's above-/below-water
/// colour split among them.
pub const DEFAULT_WATER_HEIGHT: f32 = 20.0;

/// Two water heights within this many metres are treated as one level, when voting
/// on what a void cell should inherit from the regions around it.
const HEIGHT_EPSILON: f32 = 0.05;

/// The current agent-region water level, in world metres — the height the endless
/// ocean sits at. Published each frame by [`drive_water`] so the underwater-fog
/// post-process ([`crate::underwater_fog`]) knows where the surface is without
/// reaching into [`WaterState`].
///
/// Extracted into the render world too, where the transparency-ordering re-sort
/// ([`crate::transparency`]) buckets every translucent item above / below it.
#[derive(Debug, Resource, Clone, Copy, ExtractResource)]
pub(crate) struct WaterLevel(pub(crate) f32);

impl Default for WaterLevel {
    fn default() -> Self {
        Self(DEFAULT_WATER_HEIGHT)
    }
}

/// One 256 m square of sea, on the region grid: either a loaded region's own water
/// or void water, carrying the grid cell it covers so `drive_water` can re-place it
/// when the scene origin moves.
///
/// The whole sea is these and nothing else. That is the reference's rule — every
/// square metre of sea belongs to exactly one surface, a region's own
/// (`LLSurface::createObjects`) or a hole's (`LLWorld::updateWaterObjects`) — and it
/// is what keeps two water surfaces from ever being drawn over the same ground,
/// which is what used to hide a region whose sea sat lower than the agent's under an
/// ocean plane spanning everything.
///
/// One region per square also keeps the numbers small: the wave texcoords are built
/// from world coordinates, and a single 40 km plane loses enough precision across
/// one triangle to show a seam and to make the scroll quantise.
#[derive(Debug, Component)]
pub struct WaterCell {
    /// The cell this square covers, in region units relative to the agent region.
    cell: IVec2,
}

/// The viewer's water-render state: the shared material, the per-region plane mesh
/// and entities, the learned per-region water heights, and the requested wave
/// normal-map texture.
#[derive(Debug, Resource)]
pub struct WaterState {
    /// The single water material, shared by the ocean and every per-region plane
    /// (the water look is region-wide), updated each frame by [`drive_water`].
    material: Handle<WaterMaterial>,
    /// The shared 256 m plane mesh every cell of the sea grid is drawn with.
    cell_mesh: Handle<Mesh>,
    /// The rendered entity for each cell of the sea grid, by cell.
    cells: HashMap<IVec2, Entity>,
    /// The water height learned for each region from its handshake.
    region_heights: HashMap<RegionHandle, f32>,
    /// The texture id currently requested for the wave normal map (the water
    /// frame's own, or the built-in [`sl_client_bevy::DEFAULT_WATER_NORMAL_TEXTURE`]).
    normal_key: Option<TextureKey>,
}

impl WaterState {
    /// The water height learned for `region` from its handshake, if any — the
    /// minimap's above-/below-water object colour split reads this.
    #[must_use]
    pub fn height_of(&self, region: RegionHandle) -> Option<f32> {
        self.region_heights.get(&region).copied()
    }

    /// The shared water material handle, so the water-exclusion pass
    /// ([`crate::water_exclusion`]) can bind its screen-space mask into it.
    pub(crate) const fn material(&self) -> &Handle<WaterMaterial> {
        &self.material
    }

    /// A state carrying nothing but `material`, for a sibling module's tests: the
    /// screen-space passes that bind into the shared water material reach it only
    /// through [`WaterState`], and building a real one would need the whole
    /// environment / mesh / region-height setup they have no use for.
    #[cfg(test)]
    pub(crate) fn material_only(material: Handle<WaterMaterial>) -> Self {
        Self {
            material,
            cell_mesh: Handle::default(),
            cells: HashMap::new(),
            region_heights: HashMap::new(),
            normal_key: None,
        }
    }
}

impl crate::world_api::world_scoped::WorldScoped for WaterState {
    /// Despawn every per-region plane and forget every learned water height.
    ///
    /// Both are keyed by regions the distant teleport just disconnected. The
    /// heights map used to be insert-only, so after a few hops
    /// `reconcile_region_planes` measured every region *ever visited* against
    /// the new agent region's sea level and spawned a 256 m alpha plane for each
    /// one that differed — scattered across the grid, permanently in the
    /// transparency sort, and re-collected into a `Vec` every frame.
    ///
    /// The shared material, the plane mesh and the requested normal-map key are
    /// kept: they are the region-independent water *look*, which
    /// `drive_water` refreshes from the destination's environment anyway.
    fn purge_world(
        &mut self,
        _purge: crate::world_api::world_scoped::WorldPurge,
        commands: &mut Commands,
    ) {
        for (_cell, entity) in self.cells.drain() {
            commands.entity(entity).try_despawn();
        }
        self.region_heights.clear();
    }
}

/// Startup: create the shared water material (on a flat-normal placeholder), spawn
/// the endless-ocean plane, and register [`WaterState`].
pub(crate) fn setup_water(
    mut commands: Commands,
    environment: Res<EnvironmentState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let placeholder = images.add(flat_normal_image());
    // Seed the material from the current environment water at the current day
    // position; `drive_water` refines it every frame.
    let water = environment.water_at(day_position(&environment));
    let params = water.map_or_else(default_water_params, |water| {
        let fog = WaterFogSettings::from_water(&water, DEFAULT_WATER_HEIGHT);
        water_params(&water, default_water_lighting(), false, fog)
    });
    let material = materials.add(WaterMaterial {
        params,
        normal_map: placeholder.clone(),
        normal_map_next: placeholder,
        // A white 1×1 placeholder — "water everywhere" — until the water-exclusion
        // pass ([`crate::water_exclusion`]) swaps in its real screen-space mask, so
        // the sea is unaffected until an exclusion surface is in view.
        exclusion_mask: images.add(white_mask_image()),
        // A 1×1 placeholder depth buffer until [`crate::water_scene_depth`] sizes
        // the real one to the view. The shader reads the bound depth only when it
        // measures the same as the view being shaded, so a 1×1 one rejects no
        // refraction sample.
        scene_depth: images.add(crate::water_scene_depth::placeholder_scene_depth_image()),
    });

    // No sea is spawned here: `drive_water` builds the grid on the first frame it
    // has a camera and an agent region, and keeps it under the camera after that.
    let cell_mesh = meshes.add(
        Plane3d::default()
            .mesh()
            .size(REGION_SIZE_METRES, REGION_SIZE_METRES)
            .build(),
    );
    commands.insert_resource(WaterState {
        material,
        cell_mesh,
        cells: HashMap::new(),
        region_heights: HashMap::new(),
        normal_key: None,
    });
}

/// Learn each region's water height from its handshake, so [`drive_water`] can
/// place the sea at the right level per region.
pub(crate) fn update_water(mut events: MessageReader<SlEvent>, mut state: ResMut<WaterState>) {
    for event in events.read() {
        if let SlSessionEvent::RegionInfoHandshake(identity) = &event.0 {
            state
                .region_heights
                .insert(identity.region_handle, identity.water_height);
        }
    }
}

/// Centre the ocean on the camera at the agent-region water height, reconcile a
/// per-region plane for every loaded region whose height differs from the agent
/// region's, fold the blended EEP water settings into the shared material, and
/// (re)request the wave normal map boosted.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected ECS resources and queries; \
              placing the ocean and per-region planes needs the camera, identity, \
              environment, meshes, and the water material together"
)]
pub(crate) fn drive_water(
    identity: Res<SlIdentity>,
    fog_settings: Res<WaterFogSettings>,
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
    environment: Res<EnvironmentState>,
    mut state: ResMut<WaterState>,
    mut level: ResMut<WaterLevel>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut textures: ResMut<TextureManager>,
    mut cells: Query<(&WaterCell, &mut Transform)>,
    mut commands: Commands,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let camera_pos = camera.translation();

    // The origin the terrain places its patches on: the agent's (root) region. Its
    // water height is the level the sea falls back to where nothing better is
    // known — a void cell with no loaded region anywhere to inherit from.
    let root = identity.region_handle;
    let root_height = root
        .and_then(|root| state.region_heights.get(&root).copied())
        .unwrap_or(DEFAULT_WATER_HEIGHT);
    // Publish the surface level for the underwater-fog post-process (only when it
    // moved — a per-frame same-value write would mark the resource changed and
    // re-extract it every frame).
    if level.0.to_bits() != root_height.to_bits() {
        level.0 = root_height;
    }

    // Reconcile the sea grid: one 256 m square per region cell around the camera, at
    // the loaded region's own water height or, for a cell with no region, at the
    // level of the nearest ones.
    reconcile_sea_grid(
        &mut state,
        root,
        root_height,
        camera_pos,
        &mut cells,
        &mut commands,
    );

    // Fold the current environment water + sky into the shared material.
    let position = day_position(&environment);
    let Some(water) = environment.water_at(position) else {
        return;
    };
    let sky = environment.sky_at(camera_pos.y, position);

    // Everything the surface needs from the sky frame: where the active light is,
    // what colour its specular highlight is, what the atmosphere does to that
    // colour, and what tint the surface mirrors when no probe is bound. All of it
    // through `resolve_sky`, so the sea's idea of where the sun is and the sky
    // dome's are the same derivation rather than two copies of it.
    let lighting = sky.as_ref().map_or_else(default_water_lighting, |sky| {
        let resolved = crate::sky::resolve_sky(sky);
        WaterLighting {
            light_dir: resolved.light_dir,
            specular_color: water_specular_color(
                // The pool reads the frame's **raw** colour, not the (classic-mode
                // normalised) one the shaders are bound with — and then discards
                // its magnitude anyway, so only the hue it carries survives.
                Vec3::new(
                    sky.sunlight_color.red(),
                    sky.sunlight_color.green(),
                    sky.sunlight_color.blue(),
                ),
                resolved.light_dir,
                resolved.sun_up || resolved.moon_up,
            ),
            sunlit_color: resolved.sunlit,
            haze_atten_coef: resolved.haze_atten_coef,
            reflection_color: color_rgb(sky.blue_horizon),
        }
    });

    // Compare-then-`get_mut` (the texture_anim idiom): under a static sky the
    // params are identical every frame — the waves animate GPU-side from
    // `globals.time` — so the shared water material is never re-prepared. Under
    // a live day cycle they are identical for a whole day-cycle sampling step
    // (`sky::DAY_POSITION_STEPS`), which is what makes this float-equality
    // compare hold at all off the screenshot harness.
    // The reference asks this of the eye, not of the fragment
    // (`llsettingsvo.cpp:1128`, `eyedepth = camera.z - water_height; underwater =
    // eyedepth <= 0`), and measures it against the *environment's* water height —
    // here the agent region's, the same level `WaterLevel` publishes.
    let submerged = camera_pos.y <= root_height;
    // The fog the surface applies to its own underside. Read from the resource
    // rather than resolved from `water` here, so the surface, the haze pass and the
    // face materials are fogging with one set of values — including when the
    // `SL_VIEWER_DISABLE_UNDERWATER_FOG` knob has zeroed them. It is a frame old
    // (`update_water_fog_settings` runs after this, since the level it publishes is
    // this system's own output), which costs nothing: what it carries changes when
    // the region or its environment does, not while anyone is looking.
    let params = water_params(&water, lighting, submerged, *fog_settings);
    if materials
        .get(&state.material)
        .is_some_and(|material| material.params != params)
        && let Some(mut material) = materials.get_mut(&state.material)
    {
        material.params = params;
    }

    // Fetch the water's wave normal map boosted (the water frame's own, or the
    // reference built-in) so it resolves ahead of ordinary faces.
    let normal_key = water
        .normal_map
        .unwrap_or_else(|| TextureKey::from(sl_client_bevy::DEFAULT_WATER_NORMAL_TEXTURE));
    // Only on a key change: the boost request is persistent in the store, and a
    // per-frame re-request marks `TextureManager` and `WaterState` changed with
    // identical values.
    if state.normal_key != Some(normal_key) {
        textures.request_boosted(normal_key, SKY_BOOST_PRIORITY);
        state.normal_key = Some(normal_key);
    }
}

/// Spawn / despawn / re-place the sea grid: one 256 m square per region cell within
/// [`SEA_GRID_RADIUS_CELLS`] of the camera, each at the water height that cell
/// should have.
///
/// The grid is anchored on the **region grid**, not on the camera: a cell covers one
/// region's footprint exactly, so a loaded region's sea is that region's own square
/// and nothing else is ever drawn over it. The camera only decides which cells
/// exist.
fn reconcile_sea_grid(
    state: &mut WaterState,
    root: Option<RegionHandle>,
    root_height: f32,
    camera_pos: Vec3,
    cells: &mut Query<(&WaterCell, &mut Transform)>,
    commands: &mut Commands,
) {
    // Which cells the loaded regions occupy, in the same relative coordinates the
    // grid is keyed by, so the height rule can work in cell space. Before the agent
    // region's handshake there is no origin to measure against and so nothing is
    // loaded — the grid is still built, at the default sea level, rather than
    // leaving the world with no sea at all for the first few frames.
    let loaded = root.map(|root| loaded_cells(&state.region_heights, root));
    let loaded = loaded.unwrap_or_default();

    // The camera's own cell, and the square of cells around it.
    let camera_cell = cell_of(camera_pos);
    let wanted = |cell: IVec2| cell_distance(cell, camera_cell) <= SEA_GRID_RADIUS_CELLS;
    let radius = i32::try_from(SEA_GRID_RADIUS_CELLS).unwrap_or(0);

    // Drop the cells that have fallen out of range.
    state.cells.retain(|&cell, entity| {
        if wanted(cell) {
            return true;
        }
        commands.entity(*entity).try_despawn();
        false
    });

    // Spawn the ones that have come into range.
    for x in camera_cell.x.saturating_sub(radius)..=camera_cell.x.saturating_add(radius) {
        for y in camera_cell.y.saturating_sub(radius)..=camera_cell.y.saturating_add(radius) {
            let cell = IVec2::new(x, y);
            if state.cells.contains_key(&cell) {
                continue;
            }
            let entity = commands
                .spawn((
                    Mesh3d(state.cell_mesh.clone()),
                    MeshMaterial3d(state.material.clone()),
                    Transform::from_translation(cell_translation(
                        cell,
                        cell_height(cell, &loaded, root_height),
                    )),
                    // The water never casts shadows (P24 adds cascaded shadow maps
                    // for the sun).
                    NotShadowCaster,
                    // Environment content: also visible to probe captures.
                    environment_render_layers(),
                    WaterCell { cell },
                ))
                .id();
            state.cells.insert(cell, entity);
        }
    }

    // Re-place every surviving cell. Write-on-change: with a stable origin, stable
    // heights and a parked camera this writes nothing and propagates nothing.
    for (cell, mut transform) in cells {
        let translation = cell_translation(cell.cell, cell_height(cell.cell, &loaded, root_height));
        if transform.translation != translation {
            transform.translation = translation;
        }
    }
}

/// The loaded regions as grid cells relative to `root`, with their water heights —
/// the input the height rule votes over.
fn loaded_cells(heights: &HashMap<RegionHandle, f32>, root: RegionHandle) -> HashMap<IVec2, f32> {
    let (root_x, root_y) = root.global_coordinates();
    heights
        .iter()
        .filter_map(|(&region, &height)| {
            let (region_x, region_y) = region.global_coordinates();
            let x = region_grid_delta(region_x, root_x)?;
            let y = region_grid_delta(region_y, root_y)?;
            Some((IVec2::new(x, y), height))
        })
        .collect()
}

/// A region-grid offset in cells: `(coordinate - origin) / 256`, in `i32`, or `None`
/// for a region so far away the difference does not fit — which cannot be within
/// draw distance, so it has nothing to say about the sea around the camera.
fn region_grid_delta(coordinate: u32, origin: u32) -> Option<i32> {
    let delta = i64::from(coordinate).checked_sub(i64::from(origin))?;
    let cells = delta.checked_div(256)?;
    i32::try_from(cells).ok()
}

/// The cell a world position falls in, in the same relative coordinates as
/// [`loaded_cells`] (the scene origin is the agent region's south-west corner).
fn cell_of(position: Vec3) -> IVec2 {
    // Bevy (x, y-up, z) → Second Life (x, -z); a cell is one region across.
    let x = (position.x / REGION_SIZE_METRES).floor();
    let y = (-position.z / REGION_SIZE_METRES).floor();
    // Clamped before the conversion: a camera position that has gone wild (or NaN)
    // must not land the grid a hemisphere away, and must not turn the spawn loop
    // below into an unbounded one.
    Vec2::new(x, y)
        .clamp(Vec2::splat(-CELL_LIMIT), Vec2::splat(CELL_LIMIT))
        .as_ivec2()
}

/// How far from the scene origin, in cells, the grid is allowed to be anchored — far
/// past any draw distance, and only there to bound [`cell_of`] against a nonsense
/// camera position.
const CELL_LIMIT: f32 = 100_000.0;

/// A cell's Bevy translation: the centre of its square, at `height`.
fn cell_translation(cell: IVec2, height: f32) -> Vec3 {
    let cell = cell.as_vec2();
    let half = REGION_SIZE_METRES / 2.0;
    // Second Life (x, y, z-up) → Bevy (x, z, -y).
    let sl_x = cell.x * REGION_SIZE_METRES + half;
    let sl_y = cell.y * REGION_SIZE_METRES + half;
    Vec3::new(sl_x, height, -sl_y)
}

/// The water height for one cell of the sea grid, in metres: the region's own if a
/// region is loaded there, else the level of the **nearest** loaded regions.
///
/// The nearest-region rule is a deliberate improvement on the reference, which uses
/// the agent region's height for every cell with no region
/// (`LLWorld::updateWaterObjects`: *"Use the water height of the region we're on for
/// areas where there is no region"*). Take an agent region ringed by eight regions
/// whose sea is lower, with void beyond: the reference puts that void back at the
/// agent's level, so the outer edge of the ring gets a step in the water that
/// nothing standing there can explain. Inheriting from the nearest region instead
/// puts void water at the level of whatever it actually adjoins, and the step is
/// gone.
///
/// Looking only at a cell's immediate neighbours would not be enough: the second
/// ring of void has no loaded neighbour at all and would fall back to the agent's
/// height, which moves the step outward rather than removing it. Distance is
/// **Chebyshev**, so a diagonal neighbour counts as adjacent — "surrounded by eight
/// regions" is one ring, which is how it looks.
///
/// Ties (a cell equidistant from regions at different levels, one east and one west
/// say) go to the majority, and then to the **lower** level: void water that is too
/// high reads as a wall standing over the neighbouring sea, while too low only
/// reveals a little more of the void it was covering.
///
/// One set of cells is decided differently: those on the outward **diagonal** of a
/// corner region (one with no loaded region beside it on that side, in either axis).
/// Chebyshev distance makes that corner the *only* nearest region there, while the
/// cells either side of the diagonal each tie it with one of its edge neighbours.
/// Left to the plain vote, the diagonal would be a line one cell wide running to the
/// horizon at the corner's level, with the tie-broken sea on both sides of it. Such
/// a cell is where the two edge sides of the corner meet, so it takes the **lower**
/// of the levels its two outward neighbours (one cell further out along each axis)
/// get — the same "too low beats too high" rule a tie follows.
fn cell_height(cell: IVec2, loaded: &HashMap<IVec2, f32>, agent_height: f32) -> f32 {
    if let Some(&height) = loaded.get(&cell) {
        return height;
    }
    let nearest = nearest_regions(cell, loaded);
    if let [(region, _height)] = nearest.as_slice()
        && let Some(outward) = diagonal_outward_step(*region, cell)
    {
        let across = nearest_height(cell.saturating_add(IVec2::new(outward.x, 0)), loaded);
        let along = nearest_height(cell.saturating_add(IVec2::new(0, outward.y)), loaded);
        if let (Some(across), Some(along)) = (across, along) {
            return across.min(along);
        }
    }
    let heights: Vec<f32> = nearest.iter().map(|&(_region, height)| height).collect();
    majority_height(&heights).unwrap_or(agent_height)
}

/// The loaded regions nearest to `cell` by [`cell_distance`] (every one at that
/// distance), with their water heights. Empty when nothing is loaded.
fn nearest_regions(cell: IVec2, loaded: &HashMap<IVec2, f32>) -> Vec<(IVec2, f32)> {
    let mut nearest: Vec<(IVec2, f32)> = Vec::new();
    let mut best = u32::MAX;
    for (&other, &height) in loaded {
        let distance = cell_distance(other, cell);
        if distance < best {
            best = distance;
            nearest.clear();
        }
        if distance == best {
            nearest.push((other, height));
        }
    }
    nearest
}

/// The plain nearest-ring vote for `cell` — [`majority_height`] over
/// [`nearest_regions`] — with no diagonal handling. `None` when nothing is loaded.
fn nearest_height(cell: IVec2, loaded: &HashMap<IVec2, f32>) -> Option<f32> {
    let heights: Vec<f32> = nearest_regions(cell, loaded)
        .iter()
        .map(|&(_region, height)| height)
        .collect();
    majority_height(&heights)
}

/// When `cell` lies exactly on a diagonal out from `region` (as far off in x as in
/// y, and not on it), the unit step pointing further out along that diagonal, one
/// sign per axis; `None` otherwise.
fn diagonal_outward_step(region: IVec2, cell: IVec2) -> Option<IVec2> {
    let offset = cell.saturating_sub(region);
    (offset.x != 0 && offset.x.unsigned_abs() == offset.y.unsigned_abs()).then(|| offset.signum())
}

/// The Chebyshev distance between two cells, in cells: the number of rings out one
/// is from the other, so a diagonal neighbour is one ring like an edge neighbour.
/// Saturating, because a cell index is only as sane as the camera position it came
/// from.
fn cell_distance(a: IVec2, b: IVec2) -> u32 {
    let x = a.x.saturating_sub(b.x).unsigned_abs();
    let y = a.y.saturating_sub(b.y).unsigned_abs();
    x.max(y)
}

/// The most common height in `heights`, counting two within [`HEIGHT_EPSILON`] as
/// the same level, with the lower winning a tie. `None` for an empty slice.
fn majority_height(heights: &[f32]) -> Option<f32> {
    let mut best: Option<(usize, f32)> = None;
    for &height in heights {
        let votes = heights
            .iter()
            .filter(|&&other| (other - height).abs() <= HEIGHT_EPSILON)
            .count();
        let better = best.is_none_or(|(best_votes, best_height)| {
            votes > best_votes || (votes == best_votes && height < best_height)
        });
        if better {
            best = Some((votes, height));
        }
    }
    best.map(|(_votes, height)| height)
}

/// Swap the decoded wave normal map into the shared material when its id resolves.
pub(crate) fn apply_water_textures(
    mut decoded: MessageReader<TextureDecoded>,
    state: Res<WaterState>,
    store: Res<DecodedTextures>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for &TextureDecoded(id) in decoded.read() {
        if state.normal_key != Some(id) {
            continue;
        }
        let Some(decoded) = store.get(id) else {
            // The fetch/decode failed; the surface keeps its flat-normal placeholder
            // (still a fresnel-tinted flat sea).
            continue;
        };
        let handle = images.add(water_normal_image(decoded));
        if let Some(mut material) = materials.get_mut(&state.material) {
            // Both normal-map slots share the id until the day cycle drives a
            // separate next-frame normal map and the blend factor between them.
            material.normal_map = handle.clone();
            material.normal_map_next = handle;
        }
    }
}

/// What the **sky** frame contributes to the water surface, resolved once per
/// frame: where the active light is, the specular highlight's base colour, the
/// atmospheric sun colour that highlight is scaled by, the haze it is dimmed by,
/// and the tint the surface mirrors when no reflection probe is bound.
///
/// Grouped rather than passed as five positional arguments because four of the
/// five are colours of the same dimension — a call site that swapped two of them
/// would compile and would be a rendering bug with no symptom but the wrong
/// colour, which is exactly the class of bug this module has already paid for
/// once.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WaterLighting {
    /// The direction toward the sun, or toward the moon at night, Bevy Y-up
    /// (`getLightDirection`).
    pub(crate) light_dir: Vec3,
    /// The specular base colour — the pool's `light_diffuse`, see
    /// [`water_specular_color`].
    pub(crate) specular_color: Vec3,
    /// The atmospheric sun colour the specular highlight is scaled by
    /// (`calcAtmosphericVarsLinear`'s `sunlit`).
    pub(crate) sunlit_color: Vec3,
    /// The per-metre haze attenuation coefficient the highlight fades with over
    /// distance.
    pub(crate) haze_atten_coef: Vec3,
    /// The sky-reflection tint (`blue_horizon`) the surface mirrors where no
    /// reflection probe is bound.
    pub(crate) reflection_color: Vec3,
}

/// The water lighting for a scene with no sky frame selected yet: a light
/// overhead, a white highlight, no haze, and a pale horizon blue to mirror.
const fn default_water_lighting() -> WaterLighting {
    WaterLighting {
        light_dir: Vec3::Y,
        specular_color: Vec3::ONE,
        sunlit_color: Vec3::ONE,
        haze_atten_coef: Vec3::ZERO,
        reflection_color: default_reflection(),
    }
}

/// The base colour of the sun's specular highlight on the sea: the reference's
/// `specular` uniform, which `lldrawpoolwater.cpp` builds as its own
/// `light_diffuse` and **not** from the frame's sunlight colour directly.
///
/// ```cpp
/// light_dir.normalize();
/// F32 ground_proj_sq = light_dir.mV[0] * light_dir.mV[0] + light_dir.mV[1] * light_dir.mV[1];
/// if (0.f < light_diffuse.normalize())  // Normalizing a color? Puzzling...
/// {
///     light_diffuse *= (1.5f + (6.f * ground_proj_sq));
/// }
/// ```
///
/// Two things follow, and both matter to how the streak looks. The colour is
/// normalised to **unit length**, so the frame's authored brightness is discarded
/// entirely and only its hue reaches the shader — feeding the raw colour instead
/// puts a legacy sunset's `2.8` into a highlight that then clamps to white. And
/// what replaces that brightness is a function of the sun's *elevation*:
/// `ground_proj²` is the light direction's horizontal extent, so the factor runs
/// from `1.5` with the sun overhead to `7.5` with it on the horizon. The sea's
/// highlight is brightest exactly when the sun is lowest, which is the effect the
/// comment calls magic numbers.
///
/// `lit` is whether either body is above the horizon: with neither, the pool's
/// `light_diffuse` is never added to and stays black.
///
/// Per-component `f32` arithmetic (the glam vector operators trip the workspace
/// `arithmetic_side_effects` lint).
pub(crate) fn water_specular_color(sunlight: Vec3, light_dir: Vec3, lit: bool) -> Vec3 {
    if !lit {
        return Vec3::ZERO;
    }
    let magnitude =
        (sunlight.x * sunlight.x + sunlight.y * sunlight.y + sunlight.z * sunlight.z).sqrt();
    if magnitude <= 0.0 {
        return Vec3::ZERO;
    }
    // The light direction's projection onto the ground plane — Second Life's `x`
    // and `y`, which are Bevy's `x` and `z`. It arrives normalised.
    let ground_proj_sq = light_dir.x * light_dir.x + light_dir.z * light_dir.z;
    let scale = (1.5 + 6.0 * ground_proj_sq) / magnitude;
    Vec3::new(sunlight.x * scale, sunlight.y * scale, sunlight.z * scale)
}

/// Build the water-shader uniform block from a water frame plus the sky's
/// contribution ([`WaterLighting`]). (The wave-scroll clock and the camera
/// position are read GPU-side — `globals.time` and the view's `world_position` —
/// so they are not part of the uniform block.)
///
/// `submerged` is whether the eye is under the surface, which the reference asks
/// before binding this shader's fog density
/// (`lldrawpoolwater.cpp:242 getModifiedWaterFogDensity(underwater)`): submerged, the
/// density is raised to the frame's underwater fog modifier. It is the one input
/// here that follows the camera rather than the environment, and it is a step
/// function of it — it changes only when the eye crosses the waterline, so it does
/// not turn the material's compare-then-`get_mut` into a per-frame re-prepare.
pub(crate) fn water_params(
    water: &WaterSettings,
    lighting: WaterLighting,
    submerged: bool,
    fog: WaterFogSettings,
) -> WaterParams {
    WaterParams {
        light_dir: lighting.light_dir,
        fresnel_scale: water.fresnel_scale,
        normal_scale: Vec3::new(
            water.normal_scale.x(),
            water.normal_scale.y(),
            water.normal_scale.z(),
        ),
        fresnel_offset: water.fresnel_offset,
        specular_color: lighting.specular_color,
        // The reference doubles it on the way to the shader
        // (`uniform1f(WATER_BLUR_MULTIPLIER, fmaxf(0, getBlurMultiplier()) * 2)`),
        // and what the shader then calls it is `perceptualRoughness` — so the
        // doubling belongs here, where the binding is, rather than in the one place
        // downstream that happened to remember it.
        blur_multiplier: water.blur_multiplier.max(0.0) * 2.0,
        sunlit_color: lighting.sunlit_color,
        // `refScale`, the reference's screen-space refraction displacement: the
        // frame's `scaleBelow` when the eye is under the surface, `scaleAbove` when
        // it is over it (`lldrawpoolwater.cpp:299`).
        ref_scale: if submerged {
            water.scale_below
        } else {
            water.scale_above
        },
        haze_atten_coef: lighting.haze_atten_coef,
        blend_factor: 0.0,
        reflection_color: lighting.reflection_color,
        water_fog_density: if submerged {
            fog.density_submerged
        } else {
            fog.density_above
        },
        // The fog the surface applies to its own underside — from the scene's
        // resolved [`WaterFogSettings`] rather than from `water` directly, so that
        // it is the same fog the haze pass and the face materials use (and so that
        // `SL_VIEWER_DISABLE_UNDERWATER_FOG` turns this half off too).
        water_fog_color: fog.color,
        wave1_dir: Vec2::from_array(water.wave1_direction),
        wave2_dir: Vec2::from_array(water.wave2_direction),
    }
}

/// The water uniforms for the built-in legacy default water, used to seed the
/// material before an environment is selected.
///
/// Seeded as seen from above: the agent is not in the water at login, and
/// `drive_water` replaces this from the real camera on the first frame anyway.
pub(crate) fn default_water_params() -> WaterParams {
    let water = WaterSettings::legacy_default("Default");
    let fog = WaterFogSettings::from_water(&water, DEFAULT_WATER_HEIGHT);
    water_params(&water, default_water_lighting(), false, fog)
}

/// A neutral sky-reflection tint used before a sky frame is selected (a pale
/// horizon blue).
const fn default_reflection() -> Vec3 {
    Vec3::new(0.5, 0.6, 0.8)
}

/// A Second Life [`SlColor`] as a linear RGB triple.
const fn color_rgb(color: SlColor) -> Vec3 {
    Vec3::new(color.red(), color.green(), color.blue())
}

/// Upload a decoded water normal map: **linear**, and tiling.
///
/// Both halves are load-bearing and one of them was wrong. This used to be
/// `to_bevy_image`, which builds `Rgba8UnormSrgb` — and a normal map is not a
/// colour. Read back through the sRGB transfer a flat `(0.5, 0.5, 1.0)` texel
/// decodes to about `(0.21, 0.21, 1.0)`, which unpacks to a normal tilted well off
/// the surface rather than along it; every wavelet in the sea was skewed the same
/// way, and the flatter the water the more wrong it was.
///
/// Every other normal map in this viewer is already careful about exactly this —
/// [`sl_viewer_world_objects::legacy_materials`]'s `build_linear_image` ("the linear colour space a
/// normal map needs"), [`sl_viewer_world_objects::materials`]'s `build_pbr_image`, [`sl_viewer_world_objects::bump`]'s
/// generator — and so is this module's own [`flat_normal_image`], which is
/// `Rgba8Unorm`. So the water was the one path out of step with its neighbours *and*
/// with its own placeholder: the sea changed colour space the moment its texture
/// arrived.
///
/// The sampler must repeat because the wave shader scrolls its texcoords well
/// outside `[0, 1]` (the reference samples with `GL_REPEAT`) and Bevy's default is
/// clamp-to-edge — the R22h class.
///
/// Found by [`crate::render_scene`]'s `water-surface` scene, which had to build one
/// of these without a grid to have any waves at all.
pub(crate) fn water_normal_image(decoded: &DecodedTexture) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: decoded.width,
            height: decoded.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        decoded.pixels.to_vec(),
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// A 1×1 all-white placeholder [`Image`] for the water-exclusion mask: `1` means
/// "water present", so a water material wearing this placeholder renders the sea
/// everywhere (no exclusion) until [`crate::water_exclusion`] wires in the real
/// screen-space mask. Single-channel [`TextureFormat::R8Unorm`] to match the mask
/// render target the water shader samples.
pub(crate) fn white_mask_image() -> Image {
    Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![255],
        TextureFormat::R8Unorm,
        RenderAssetUsages::default(),
    )
}

/// A 1×1 flat-normal placeholder [`Image`] (RGB `(128, 128, 255)` = the unit +Z
/// tangent-space normal), used for the wave normal map until the real one decodes,
/// so the surface starts perfectly flat.
pub(crate) fn flat_normal_image() -> Image {
    Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![128, 128, 255, 255],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::{WaterCell, WaterState, cell_height, default_water_lighting, water_params};
    use crate::world_api::world_scoped::{WorldPurge, WorldScoped as _};
    use bevy::ecs::world::CommandQueue;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::RegionHandle;
    use std::collections::HashMap;

    /// A region handle at the given grid coordinates (in region units).
    fn region(x: u32, y: u32) -> RegionHandle {
        RegionHandle::from_global(x.saturating_mul(256), y.saturating_mul(256))
    }

    /// A distant teleport must leave no learned height and no spawned sea behind:
    /// the heights map is what the grid's void cells vote over, so a surviving entry
    /// from a grid we are no longer connected to would pull the sea around the
    /// destination to a level from the region we left.
    #[test]
    fn a_world_reset_drops_every_sea_cell_and_height() {
        let mut world = World::new();
        let far = world
            .spawn(WaterCell {
                cell: IVec2::new(3, -2),
            })
            .id();

        let mut state = WaterState {
            material: Handle::default(),
            cell_mesh: Handle::default(),
            cells: HashMap::from([(IVec2::new(3, -2), far)]),
            region_heights: HashMap::from([(region(1000, 1000), 42.0), (region(1001, 1000), 20.0)]),
            normal_key: None,
        };

        let mut queue = CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, &world);
            state.purge_world(WorldPurge::default(), &mut commands);
        }
        queue.apply(&mut world);

        assert!(state.cells.is_empty());
        assert!(state.region_heights.is_empty());
        assert!(
            world.get_entity(far).is_err(),
            "the departed world's sea cells must be despawned"
        );
        // The region-independent water look survives — the destination's
        // environment refines it rather than rebuilding it.
        assert_eq!(state.normal_key, None);
    }

    /// A cell with a loaded region takes that region's own water height, whatever
    /// the neighbours or the agent are at. This is the case the old model got wrong
    /// in one direction: a region whose sea sat *below* the agent's was drawn over
    /// by the ocean plane and vanished.
    #[test]
    fn a_loaded_region_keeps_its_own_level() {
        let loaded = HashMap::from([(IVec2::new(0, 0), 20.0), (IVec2::new(1, 0), 5.0)]);
        assert_height(cell_height(IVec2::new(1, 0), &loaded, 20.0), 5.0);
        assert_height(cell_height(IVec2::new(0, 0), &loaded, 20.0), 20.0);
    }

    /// The reporter's case, and the reason this does not follow the reference: an
    /// agent region ringed by eight regions whose sea is lower, void beyond. The
    /// reference would put every void cell back at the agent's level, stepping the
    /// water up again at the outer edge of the ring for no reason visible from
    /// there. Inheriting from the nearest regions instead leaves the whole void at
    /// the level it adjoins.
    #[test]
    fn void_beyond_a_ring_inherits_the_ring_not_the_agent() {
        let mut loaded = HashMap::from([(IVec2::new(0, 0), 30.0)]);
        for x in -1..=1 {
            for y in -1..=1 {
                if (x, y) != (0, 0) {
                    loaded.insert(IVec2::new(x, y), 10.0);
                }
            }
        }
        // The first ring of void, just outside the loaded ring.
        assert_height(cell_height(IVec2::new(2, 0), &loaded, 30.0), 10.0);
        // And the second, which has no loaded neighbour at all — the case that
        // makes "look at the neighbours" insufficient and "nearest region"
        // necessary.
        assert_height(cell_height(IVec2::new(3, 0), &loaded, 30.0), 10.0);
        // Far out in every direction, still the ring's level rather than the
        // agent's.
        assert_height(cell_height(IVec2::new(-9, 7), &loaded, 30.0), 10.0);
    }

    /// A void cell equidistant from two levels goes to the majority, and to the
    /// lower level when the vote is even: too high reads as a wall of water standing
    /// over the neighbouring sea, too low only shows a little more void.
    #[test]
    fn a_tied_void_cell_takes_the_lower_level() {
        // One region either side, at different levels: an even vote.
        let split = HashMap::from([(IVec2::new(-1, 0), 25.0), (IVec2::new(1, 0), 15.0)]);
        assert_height(cell_height(IVec2::new(0, 0), &split, 40.0), 15.0);
        // Two against one at the same distance: the majority wins even though it is
        // the higher level, so this is a vote and not simply a minimum.
        let majority = HashMap::from([
            (IVec2::new(-1, 0), 25.0),
            (IVec2::new(1, 0), 15.0),
            (IVec2::new(0, 1), 25.0),
        ]);
        assert_height(cell_height(IVec2::new(0, 0), &majority, 40.0), 25.0);
    }

    /// The aditi report: a 2x2 block whose south-east region's sea sits above the
    /// other three. Every void cell around the block either ties that corner with
    /// one of its edge neighbours or is nearer the lower three — except the cells on
    /// the corner's outward diagonal, which saw the corner alone and ran a raised
    /// strip of sea out to the horizon. None of the void may take the corner's level.
    #[test]
    fn no_void_cell_on_a_raised_corner_diagonal_is_raised() {
        let loaded = HashMap::from([
            (IVec2::new(0, 0), 10.0),
            (IVec2::new(1, 0), 30.0),
            (IVec2::new(0, 1), 10.0),
            (IVec2::new(1, 1), 10.0),
        ]);
        // The diagonal itself, out to well past any draw distance.
        for step in 1_i32..=12 {
            let cell = IVec2::new(step.saturating_add(1), -step);
            assert_height(cell_height(cell, &loaded, 30.0), 10.0);
        }
        // And every void cell in a window around the block.
        for x in -8..=9 {
            for y in -8..=9 {
                let cell = IVec2::new(x, y);
                if !loaded.contains_key(&cell) {
                    assert_height(cell_height(cell, &loaded, 30.0), 10.0);
                }
            }
        }
    }

    /// The same corner, lowered instead: the tie rule already puts the void off the
    /// diagonal at the corner's level, so the diagonal must not be pulled *up* to the
    /// other three's — that would be the same strip, inverted.
    #[test]
    fn a_lowered_corner_diagonal_matches_the_void_beside_it() {
        let loaded = HashMap::from([
            (IVec2::new(0, 0), 30.0),
            (IVec2::new(1, 0), 10.0),
            (IVec2::new(0, 1), 30.0),
            (IVec2::new(1, 1), 30.0),
        ]);
        for step in 1_i32..=12 {
            let diagonal = cell_height(IVec2::new(step.saturating_add(1), -step), &loaded, 30.0);
            let beside = cell_height(IVec2::new(step.saturating_add(2), -step), &loaded, 30.0);
            assert_height(diagonal, 10.0);
            assert_height(beside, 10.0);
        }
    }

    /// A region with nothing loaded around it keeps its level on its diagonals too:
    /// the rule only lowers a diagonal towards a level that is actually nearby.
    #[test]
    fn a_lone_region_diagonal_keeps_its_level() {
        let loaded = HashMap::from([(IVec2::new(0, 0), 30.0)]);
        assert_height(cell_height(IVec2::new(3, 3), &loaded, 10.0), 30.0);
        assert_height(cell_height(IVec2::new(-2, 2), &loaded, 10.0), 30.0);
    }

    /// With nothing loaded there is nothing to inherit from, so the sea falls back
    /// to the agent region's level — which is the reference's rule, kept for exactly
    /// the case where it is the only answer available.
    #[test]
    fn with_no_regions_the_sea_is_the_agent_level() {
        assert_height(cell_height(IVec2::new(4, 4), &HashMap::new(), 21.5), 21.5);
    }

    /// The surface shader's refraction displacement is the **eye-state** one, as the
    /// reference picks it (`lldrawpoolwater.cpp:299`): the water frame's `scaleAbove`
    /// over the surface and `scaleBelow` under it. It decides how far the wave normal
    /// moves the screen sample, and the two differ by nearly an order of magnitude in
    /// the legacy default — a diver given the surface-dweller's value gets a sea with
    /// almost no ripple in it.
    #[test]
    fn the_eye_state_picks_the_refraction_scale() {
        let water = sl_client_bevy::WaterSettings::legacy_default("Default");
        let fog =
            crate::water_fog::WaterFogSettings::from_water(&water, super::DEFAULT_WATER_HEIGHT);
        let above = water_params(&water, default_water_lighting(), false, fog);
        let below = water_params(&water, default_water_lighting(), true, fog);

        assert!(
            (above.ref_scale - water.scale_above).abs() <= 1e-6,
            "above water the frame's `scaleAbove` is bound, got {}",
            above.ref_scale,
        );
        assert!(
            (below.ref_scale - water.scale_below).abs() <= 1e-6,
            "submerged, the frame's `scaleBelow` is bound, got {}",
            below.ref_scale,
        );
        // The legacy default's two scales genuinely differ, so this is not vacuous.
        assert!(
            (above.ref_scale - below.ref_scale).abs() > 1e-3,
            "the default water frame no longer distinguishes the two eye states",
        );
    }

    /// The specular base colour keeps the frame's **hue** and throws its brightness
    /// away: `lldrawpoolwater.cpp` normalises the colour to unit length before it
    /// scales it, so what a legacy sunset authors (`sunlight_color` well past 1) can
    /// never reach the shader as a magnitude.
    ///
    /// This is the half of the fix the frames complained about as *white*: fed the
    /// raw colour, the highlight ran so far over range that it clamped to white
    /// wherever it was visible at all.
    #[test]
    fn the_specular_base_keeps_the_hue_and_discards_the_brightness() {
        // A warm sunset colour, at a legacy sunset's brightness.
        let authored = Vec3::new(2.8386, 1.9, 0.9);
        let dim = Vec3::new(0.28386, 0.19, 0.09);
        // Straight overhead, so both go through the same elevation factor.
        let up = Vec3::Y;

        let bright = super::water_specular_color(authored, up, true);
        let faint = super::water_specular_color(dim, up, true);

        assert!(
            (bright - faint).length() < 1e-5,
            "a ten-times brighter frame must give the same specular base: {bright:?} vs {faint:?}",
        );
        // Overhead: `1.5 + 6 * 0` — the whole magnitude is the elevation factor.
        assert!(
            (bright.length() - 1.5).abs() < 1e-4,
            "expected a unit colour scaled by 1.5, got a length of {}",
            bright.length(),
        );
    }

    /// The lower the sun, the brighter the sea's highlight: the pool's "magic
    /// numbers translating light direction into intensities" scale the specular base
    /// by `1.5 + 6·groundProj²`, which is `1.5` overhead and `7.5` on the horizon.
    ///
    /// The horizon end of that curve is the frame the cross-check compares — a
    /// setting sun — so the factor there is the one that carries the highlight.
    #[test]
    fn a_lower_sun_gives_a_brighter_specular_base() {
        let authored = Vec3::new(1.0, 0.8, 0.6);
        let overhead = super::water_specular_color(authored, Vec3::Y, true);
        // Just above the horizon, in the Bevy frame where `y` is up and `x`/`z` are
        // the ground plane the projection is taken on.
        let horizon =
            super::water_specular_color(authored, Vec3::new(1.0, 0.02, 0.0).normalize(), true);

        assert!(
            (overhead.length() - 1.5).abs() < 1e-3,
            "overhead the factor is 1.5, got {}",
            overhead.length(),
        );
        assert!(
            (horizon.length() - 7.5).abs() < 1e-2,
            "on the horizon the factor is 7.5, got {}",
            horizon.length(),
        );
    }

    /// With neither body above the horizon the pool never adds a colour to its
    /// `light_diffuse`, so the sea carries no highlight at all — not a dim one, and
    /// not the last one it had.
    #[test]
    fn an_unlit_sky_gives_no_specular_highlight() {
        let unlit = super::water_specular_color(Vec3::new(1.0, 0.8, 0.6), Vec3::NEG_Y, false);
        assert_eq!(unlit, Vec3::ZERO);
    }

    /// The blur multiplier reaches the shader **doubled**, as the reference binds it
    /// (`uniform1f(WATER_BLUR_MULTIPLIER, fmaxf(0, getBlurMultiplier()) * 2)`). It is
    /// the shader's `perceptualRoughness`, so a missing factor of two is a highlight
    /// of the wrong width — and the doubling used to live in one of the two places
    /// downstream that used it.
    #[test]
    fn the_blur_multiplier_is_bound_doubled() {
        let water = sl_client_bevy::WaterSettings::legacy_default("Default");
        let fog =
            crate::water_fog::WaterFogSettings::from_water(&water, super::DEFAULT_WATER_HEIGHT);
        let params = water_params(&water, default_water_lighting(), false, fog);

        assert!(
            (params.blur_multiplier - water.blur_multiplier * 2.0).abs() < 1e-6,
            "expected {} (twice the frame's {}), got {}",
            water.blur_multiplier * 2.0,
            water.blur_multiplier,
            params.blur_multiplier,
        );
        // The default frame's own value is non-zero, so this is not vacuous.
        assert!(water.blur_multiplier > 0.0);
    }

    /// A height is the expected one, to within a tolerance far under the epsilon two
    /// levels are considered the same within.
    #[track_caller]
    fn assert_height(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-3,
            "expected a water height of {expected}, got {actual}",
        );
    }
}
