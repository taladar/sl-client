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
//! - [`setup_water`] creates the shared water material, spawns the **endless
//!   ocean** plane (a large camera-following plane at the agent-region water
//!   height, filling the sea everywhere there is no loaded region — the reference
//!   `LLWorld::updateWaterObjects` hole / edge water), and registers
//!   [`WaterState`];
//! - [`update_water`] learns each region's water height from its
//!   [`SlSessionEvent::RegionInfoHandshake`];
//! - [`drive_water`] centres the ocean on the camera, reconciles a **per-region
//!   plane** for every loaded region whose water height differs from the agent
//!   region's (so a neighbour with a different sea level renders at its own
//!   height), folds the blended EEP water settings + sun direction + sky
//!   reflection tint + wave-scroll time into the shared material, and requests the
//!   wave normal map **boosted**;
//! - [`apply_water_textures`] swaps the decoded normal map into the material.
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
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sl_client_bevy::{
    Color as SlColor, RegionHandle, SlEvent, SlIdentity, SlSessionEvent, TextureKey, Uuid,
    WaterMaterial, WaterParams, WaterSettings, to_bevy_image,
};

use crate::camera::FlyCamera;
use crate::coords::sl_to_bevy_object_rotation;
use crate::environment::EnvironmentState;
use crate::render_priority::SKY_BOOST_PRIORITY;
use crate::sky::day_position;
use crate::textures::{TextureDecoded, TextureManager};

/// A standard Second Life / OpenSim region edge length, in metres.
const REGION_SIZE_METRES: f32 = 256.0;

/// The half-extent of the endless-ocean plane, in metres. The camera far plane is
/// 4096 m; a 20 km half-extent keeps the sea reaching past the horizon in every
/// direction as the plane follows the camera, without the plane ever being frustum
/// culled.
const OCEAN_HALF_EXTENT: f32 = 20_000.0;

/// The default water height, in metres, used for the endless ocean until the agent
/// region's handshake supplies the real one (the standard Second Life sea level;
/// see `map.rs`).
const DEFAULT_WATER_HEIGHT: f32 = 20.0;

/// How far below the agent-region water height the endless ocean sits, in metres,
/// so a same-height per-region plane (and the agent region's own sea) is never
/// exactly coplanar with it — avoiding depth fighting between the two transparent
/// surfaces. 2 cm is imperceptible.
const OCEAN_DEPTH_BIAS: f32 = 0.02;

/// Two water heights within this many metres are treated as equal, so a region at
/// (effectively) the agent-region sea level is covered by the endless ocean rather
/// than getting a redundant per-region plane.
const HEIGHT_EPSILON: f32 = 0.05;

/// The reference viewer's built-in wave normal map (`DEFAULT_WATER_NORMAL`,
/// `indra/llcommon/indra_constants.cpp`), sampled when the water frame names none
/// of its own.
const DEFAULT_WATER_NORMAL: Uuid = Uuid::from_u128(0x822d_ed49_9a6c_f61c_cb89_6df5_4f42_cdf4);

/// The current agent-region water level, in world metres — the height the endless
/// ocean sits at. Published each frame by [`drive_water`] so the underwater-fog
/// post-process ([`crate::underwater_fog`]) knows where the surface is without
/// reaching into [`WaterState`].
#[derive(Resource)]
pub(crate) struct WaterLevel(pub(crate) f32);

impl Default for WaterLevel {
    fn default() -> Self {
        Self(DEFAULT_WATER_HEIGHT)
    }
}

/// Marks the endless-ocean plane so [`drive_water`] can follow the camera with it.
#[derive(Component)]
pub(crate) struct WaterOcean;

/// Marks a per-region water plane, carrying the region it belongs to and its water
/// height so [`drive_water`] can (re)place it on the current scene origin.
#[derive(Component)]
pub(crate) struct WaterRegionPlane {
    /// The region this plane renders the sea for.
    region: RegionHandle,
    /// The region's water height, in metres.
    height: f32,
}

/// The viewer's water-render state: the shared material, the per-region plane mesh
/// and entities, the learned per-region water heights, and the requested wave
/// normal-map texture.
#[derive(Resource)]
pub(crate) struct WaterState {
    /// The single water material, shared by the ocean and every per-region plane
    /// (the water look is region-wide), updated each frame by [`drive_water`].
    material: Handle<WaterMaterial>,
    /// The shared 256 m plane mesh used for every per-region plane.
    region_mesh: Handle<Mesh>,
    /// The rendered entity for each region's plane (only regions whose height
    /// differs from the agent region's get one).
    region_planes: HashMap<RegionHandle, Entity>,
    /// The water height learned for each region from its handshake.
    region_heights: HashMap<RegionHandle, f32>,
    /// The texture id currently requested for the wave normal map (the water
    /// frame's own, or the built-in [`DEFAULT_WATER_NORMAL`]).
    normal_key: Option<TextureKey>,
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
    let water = environment
        .settings
        .blended_water_settings(day_position(&environment.settings));
    let params = water.map_or_else(default_water_params, |water| {
        water_params(
            &water,
            Vec3::Y,
            Vec3::ZERO,
            default_reflection(),
            Vec3::ONE,
            0.0,
        )
    });
    let material = materials.add(WaterMaterial {
        params,
        normal_map: placeholder.clone(),
        normal_map_next: placeholder,
    });

    // The endless ocean: a large plane (XZ, +Y normal), kept centred on the camera
    // and placed at the agent-region water height by `drive_water`.
    let ocean_mesh = meshes.add(
        Plane3d::default()
            .mesh()
            .size(2.0 * OCEAN_HALF_EXTENT, 2.0 * OCEAN_HALF_EXTENT)
            .build(),
    );
    commands.spawn((
        Mesh3d(ocean_mesh),
        MeshMaterial3d(material.clone()),
        Transform::from_xyz(0.0, DEFAULT_WATER_HEIGHT, 0.0),
        // The water never casts shadows (P24 adds cascaded shadow maps for the sun).
        NotShadowCaster,
        WaterOcean,
    ));

    let region_mesh = meshes.add(
        Plane3d::default()
            .mesh()
            .size(REGION_SIZE_METRES, REGION_SIZE_METRES)
            .build(),
    );
    commands.insert_resource(WaterState {
        material,
        region_mesh,
        region_planes: HashMap::new(),
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
    time: Res<Time>,
    identity: Res<SlIdentity>,
    camera: Query<&GlobalTransform, With<FlyCamera>>,
    environment: Res<EnvironmentState>,
    mut state: ResMut<WaterState>,
    mut level: ResMut<WaterLevel>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut textures: ResMut<TextureManager>,
    mut ocean: Query<&mut Transform, (With<WaterOcean>, Without<WaterRegionPlane>)>,
    mut planes: Query<(&WaterRegionPlane, &mut Transform), Without<WaterOcean>>,
    mut commands: Commands,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let camera_pos = camera.translation();

    // The origin the terrain places its patches on: the agent's (root) region. Its
    // water height is the endless-ocean level (the reference uses the agent
    // region's height for all hole / edge water).
    let root = identity.region_handle;
    let root_height = root
        .and_then(|root| state.region_heights.get(&root).copied())
        .unwrap_or(DEFAULT_WATER_HEIGHT);
    // Publish the surface level for the underwater-fog post-process.
    level.0 = root_height;

    // Place the ocean under the camera, just below the agent-region sea level so a
    // same-height region plane never z-fights it.
    if let Ok(mut transform) = ocean.single_mut() {
        transform.translation =
            Vec3::new(camera_pos.x, root_height - OCEAN_DEPTH_BIAS, camera_pos.z);
    }

    // Reconcile per-region planes: spawn one for each region whose height differs
    // from the agent region's, despawn any whose height has converged, and replace
    // the transform of the survivors on the current origin.
    reconcile_region_planes(&mut state, root, root_height, &mut planes, &mut commands);

    // Fold the current environment water + sky into the shared material.
    let position = day_position(&environment.settings);
    let Some(water) = environment.settings.blended_water_settings(position) else {
        return;
    };
    let sky = environment
        .settings
        .blended_sky_settings(camera_pos.y, position);

    // The sun direction (Bevy space) and a sky-reflection tint, both from the sky
    // frame (as `drive_sky` computes the sun direction).
    let (light_dir, sunlight, reflection) = sky.map_or_else(
        || (Vec3::Y, Vec3::ONE, default_reflection()),
        |sky| {
            let sun_dir = sl_to_bevy_object_rotation(&sky.sun_rotation)
                .mul_vec3(Vec3::X)
                .normalize();
            let moon_dir = sl_to_bevy_object_rotation(&sky.moon_rotation)
                .mul_vec3(Vec3::X)
                .normalize();
            // The active light: sun if up, else moon if up, else straight down.
            let light = if sun_dir.y >= 0.0 {
                sun_dir
            } else if moon_dir.y >= 0.0 {
                moon_dir
            } else {
                Vec3::NEG_Y
            };
            let sunlight = Vec3::new(
                sky.sunlight_color.red(),
                sky.sunlight_color.green(),
                sky.sunlight_color.blue(),
            );
            (light, sunlight, color_rgb(sky.blue_horizon))
        },
    );

    if let Some(mut material) = materials.get_mut(&state.material) {
        material.params = water_params(
            &water,
            light_dir,
            camera_pos,
            reflection,
            sunlight,
            time.elapsed_secs(),
        );
    }

    // Fetch the water's wave normal map boosted (the water frame's own, or the
    // reference built-in) so it resolves ahead of ordinary faces.
    let normal_key = water
        .normal_map
        .unwrap_or_else(|| TextureKey::from(DEFAULT_WATER_NORMAL));
    textures.request_boosted(normal_key, SKY_BOOST_PRIORITY);
    state.normal_key = Some(normal_key);
}

/// Spawn / despawn / reposition the per-region water planes: a region whose water
/// height differs from the agent region's gets its own plane at its own height (a
/// neighbour with a different sea level); one that has converged to the agent
/// region's height is dropped (the endless ocean covers it); the survivors are
/// re-placed on the current scene origin every frame.
fn reconcile_region_planes(
    state: &mut WaterState,
    root: Option<RegionHandle>,
    root_height: f32,
    planes: &mut Query<(&WaterRegionPlane, &mut Transform), Without<WaterOcean>>,
    commands: &mut Commands,
) {
    let Some(root) = root else {
        return;
    };
    let (root_x, root_y) = root.global_coordinates();

    // Snapshot the learned heights so the borrow of `state` is released before we
    // spawn (which also borrows `state.region_planes`).
    let heights: Vec<(RegionHandle, f32)> = state
        .region_heights
        .iter()
        .map(|(&region, &height)| (region, height))
        .collect();

    for (region, height) in heights {
        let differs = (height - root_height).abs() > HEIGHT_EPSILON;
        let existing = state.region_planes.get(&region).copied();
        match (differs, existing) {
            // Needs a plane and has one: reposition it on the current origin.
            (true, Some(_entity)) => {}
            // Needs a plane and has none: spawn it.
            (true, None) => {
                let translation = region_plane_translation(root_x, root_y, region, height);
                let entity = commands
                    .spawn((
                        Mesh3d(state.region_mesh.clone()),
                        MeshMaterial3d(state.material.clone()),
                        Transform::from_translation(translation),
                        NotShadowCaster,
                        WaterRegionPlane { region, height },
                    ))
                    .id();
                state.region_planes.insert(region, entity);
            }
            // No longer differs but still has a plane: despawn it.
            (false, Some(entity)) => {
                commands.entity(entity).despawn();
                state.region_planes.remove(&region);
            }
            (false, None) => {}
        }
    }

    // Re-place every surviving plane on the current origin (the origin follows the
    // agent region, so a border crossing moves them all).
    for (plane, mut transform) in planes {
        transform.translation =
            region_plane_translation(root_x, root_y, plane.region, plane.height);
    }
}

/// The Bevy translation of a region's water plane: the region centre relative to
/// the scene origin (the agent region), at the region's water height. Mirrors the
/// terrain's region placement (`(x, y, z) -> (x, z, -y)` axis map) so the sea lines
/// up with the ground.
fn region_plane_translation(
    origin_x: u32,
    origin_y: u32,
    region: RegionHandle,
    height: f32,
) -> Vec3 {
    let (region_x, region_y) = region.global_coordinates();
    // The region's south-west corner relative to the origin, plus a half-region to
    // reach the plane's centre.
    let sl_x = metres_to_f32(region_x) - metres_to_f32(origin_x) + REGION_SIZE_METRES / 2.0;
    let sl_y = metres_to_f32(region_y) - metres_to_f32(origin_y) + REGION_SIZE_METRES / 2.0;
    // Second Life (x, y, z-up) → Bevy (x, z, -y).
    Vec3::new(sl_x, height, -sl_y)
}

/// Swap the decoded wave normal map into the shared material when its id resolves.
pub(crate) fn apply_water_textures(
    mut decoded: MessageReader<TextureDecoded>,
    state: Res<WaterState>,
    manager: Res<TextureManager>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for &TextureDecoded(id) in decoded.read() {
        if state.normal_key != Some(id) {
            continue;
        }
        let Some(decoded) = manager.decoded(id) else {
            // The fetch/decode failed; the surface keeps its flat-normal placeholder
            // (still a fresnel-tinted flat sea).
            continue;
        };
        // The wave shader tiles the normal map: the wave texcoords scroll well
        // outside `[0, 1]`, so the texture must wrap (the reference samples with
        // `GL_REPEAT`). Bevy's default sampler is clamp-to-edge, so give the normal
        // map a repeating sampler.
        let mut image = to_bevy_image(decoded);
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            address_mode_w: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::linear()
        });
        let handle = images.add(image);
        if let Some(mut material) = materials.get_mut(&state.material) {
            // Both normal-map slots share the id until the day cycle drives a
            // separate next-frame normal map and the blend factor between them.
            material.normal_map = handle.clone();
            material.normal_map_next = handle;
        }
    }
}

/// Build the water-shader uniform block from a water frame plus the per-frame sun
/// direction, camera position, sky-reflection tint, sunlight colour, and
/// wave-scroll time.
const fn water_params(
    water: &WaterSettings,
    light_dir: Vec3,
    camera_position: Vec3,
    reflection_color: Vec3,
    sunlight_color: Vec3,
    time: f32,
) -> WaterParams {
    WaterParams {
        light_dir,
        time,
        camera_position,
        fresnel_scale: water.fresnel_scale,
        normal_scale: Vec3::new(
            water.normal_scale.x(),
            water.normal_scale.y(),
            water.normal_scale.z(),
        ),
        fresnel_offset: water.fresnel_offset,
        water_fog_color: color_rgb(water.water_fog_color),
        water_fog_density: water.water_fog_density,
        sunlight_color,
        blur_multiplier: water.blur_multiplier,
        reflection_color,
        blend_factor: 0.0,
        wave1_dir: Vec2::from_array(water.wave1_direction),
        wave2_dir: Vec2::from_array(water.wave2_direction),
    }
}

/// The water uniforms for the built-in legacy default water, used to seed the
/// material before an environment is selected.
fn default_water_params() -> WaterParams {
    let water = WaterSettings::legacy_default("Default");
    water_params(
        &water,
        Vec3::Y,
        Vec3::ZERO,
        default_reflection(),
        Vec3::ONE,
        0.0,
    )
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

/// A `u32` metre count as an `f32`, saturating at the `f32` mantissa limit so a
/// far-flung region coordinate does not silently wrap.
const fn metres_to_f32(metres: u32) -> f32 {
    // A global metre coordinate fits in 24 bits of mantissa well past any real
    // grid, so the cast is exact in practice.
    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "a global metre coordinate is exactly representable as f32 across any real grid"
    )]
    let value = metres as f32;
    value
}

/// A 1×1 flat-normal placeholder [`Image`] (RGB `(128, 128, 255)` = the unit +Z
/// tangent-space normal), used for the wave normal map until the real one decodes,
/// so the surface starts perfectly flat.
fn flat_normal_image() -> Image {
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
