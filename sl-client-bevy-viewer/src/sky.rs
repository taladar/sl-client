//! Atmospheric sky rendering (P22.2): render the Second Life sky dome from the
//! region's Extended-Environment (EEP) settings and drive the scene's sun / moon
//! light from the same sky.
//!
//! The heavy lifting is a faithful port of the reference viewer's deferred sky
//! shaders ([`SkyMaterial`] / `sky.wgsl`, `LLVOSky` / `class1/deferred/skyV.glsl`
//! + `skyF.glsl`). This module drives that material:
//!
//! - [`setup_sky`] spawns a large inward-facing dome carrying the sky material,
//!   plus the scene's single directional light (the sun / moon);
//! - [`center_sky_on_camera`] keeps the dome centred on the camera every frame so
//!   the atmosphere always surrounds the viewpoint;
//! - [`drive_sky`] selects the active [`SkySettings`] for the camera's altitude
//!   (the reference `LLEnvironment::calculateSkyTrackForAltitude`), computes the
//!   sun / moon direction and the scene light + ambient the way
//!   `LLSettingsSky::calculateLightSettings` does, and folds them into the sky
//!   material, the directional light, and the ambient light. It also fetches the
//!   sky's rainbow / halo textures **boosted** through the shared texture manager;
//! - [`apply_sky_textures`] swaps each decoded sky texture into the material.
//!
//! On top of the dome it renders the **sun and moon discs** (P22.3), textured
//! billboards at the computed sun / moon directions (the reference
//! `LLDrawPoolWLSky::renderHeavenlyBodies` / `sunDiscV/F.glsl` + `moonV/F.glsl`):
//!
//! - [`setup_sun_moon_discs`] spawns the two billboard entities (a shared unit
//!   quad + a [`SunDiscMaterial`] each) and registers [`DiscState`];
//! - [`drive_sun_moon_discs`] aims, scales, colours, and shows / hides each disc
//!   for the active sky frame, and fetches its sun / moon textures **boosted**;
//! - [`apply_disc_textures`] swaps each decoded disc texture into its material.
//!
//! It also renders the **star field** (P22.5), a sphere of small camera-facing
//! quads that fade in at night with the sky frame's `star_brightness` (the
//! reference `LLDrawPoolWLSky::renderStarsDeferred` / `LLVOWLSky::drawStars`):
//!
//! - [`setup_stars`] builds the 1000-star quad mesh and spawns it with a
//!   [`StarMaterial`] (initially hidden) and registers [`StarState`];
//! - [`drive_stars`] centres and slowly rotates the field on the camera, folds
//!   `star_brightness` and the twinkle time into the material, shows / hides the
//!   field for the active sky frame, and fetches its bloom texture **boosted**;
//! - [`apply_star_textures`] swaps the decoded bloom texture into the material.
//!
//! Every frame the sky, discs, clouds, and stars pull the **blended**
//! [`SkySettings`] for the current region time
//! (`EnvironmentSettings::blended_sky_settings`) — the smooth interpolation
//! between the two day-cycle keyframes bounding the moment (P22.6), so the
//! atmosphere and the sun / moon animate continuously through the day rather
//! than snapping between keyframes.

use std::time::{SystemTime, UNIX_EPOCH};

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{CascadeShadowConfig, CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sl_client_bevy::{
    CloudMaterial, CloudParams, Color as SlColor, ColorAlpha, Glow, SkyMaterial, SkyParams,
    SkySettings, StarMaterial, StarParams, SunDiscMaterial, SunDiscParams, TextureKey, Uuid,
    to_bevy_image,
};

use crate::camera::ViewerCamera;
use crate::coords::sl_to_bevy_object_rotation;
use crate::environment::EnvironmentState;
use crate::render_priority::SKY_BOOST_PRIORITY;
use crate::textures::{TextureDecoded, TextureManager};

/// The radius of the sky dome, in metres. The dome's *depth* is forced to the far
/// clip plane by `sky.wgsl` (a skybox backdrop, occluded by real geometry at any
/// altitude), so this radius only needs to enclose the camera and stay comfortably
/// within the camera's far plane (4096 m) so the sphere is never frustum-culled.
pub(crate) const SKY_DOME_RADIUS: f32 = 3000.0;

/// The scene directional light's illuminance (lux). Held constant; the sky's
/// computed sun / moon diffuse colour carries the day↔night brightness change
/// (a night moon diffuse is a fraction of the daytime sun diffuse), so the light
/// dims naturally as the colour darkens without re-scaling the illuminance.
pub(crate) const SCENE_LIGHT_ILLUMINANCE: f32 = 10_000.0;

/// Maps the sky's ambient colour luminance to the Bevy ambient-light brightness
/// (lux). The reference default ambient (`0.25` grey) lands at a soft fill.
const AMBIENT_BRIGHTNESS_SCALE: f32 = 400.0;

/// Cascaded-shadow-map coverage for the scene sun / moon (P24.1). Tuned to a
/// Second Life region's scale (256 m): the last cascade reaches to a region's
/// diagonal (~362 m) so an avatar's shadow, nearby prims, and terrain relief all
/// receive the sun, while the first (near) cascade is kept tight so avatar-close
/// detail gets most of the shadow-map resolution. The reference
/// `LLPipeline::renderShadow` uses four split sun cascades likewise.
pub(crate) fn shadow_cascades() -> CascadeShadowConfig {
    CascadeShadowConfigBuilder {
        num_cascades: 4,
        // The camera can push right up to an avatar's face (2 cm near plane), so
        // start the near cascade close.
        minimum_distance: 0.1,
        // A region diagonal — beyond this, distant relief goes unshadowed.
        maximum_distance: 384.0,
        // Keep the near cascade tight so avatar-close geometry is crisp.
        first_cascade_far_bound: 24.0,
        overlap_proportion: 0.2,
    }
    .build()
}

/// The directional shadow map's resolution (`DirectionalLightShadowMap.size`, set
/// to 4096 in `main`). The shadow-direction snap step is derived from it so a
/// direction step never moves a shadow by more than ~one texel on any cascade.
const SHADOW_MAP_SIZE: f32 = 4096.0;

/// Snap a shadow-caster light direction to a stable angular grid (R20).
///
/// Bevy's cascaded shadow maps already texel-snap the cascade origin, but that
/// only stabilises the shadow while the light *direction* is fixed. The real-time
/// day cycle rotates the sun a hair every frame, rotating the light-space texel
/// grid and making the ground shadows shimmer / oscillate. Rounding the direction
/// components to a grid and re-normalising holds the direction **bit-identical**
/// across the frames whose true direction falls in the same cell, so the shadow
/// sits perfectly still, and a step to the next cell changes the unit direction by
/// at most the grid step — which, for a receiver at distance `R`, moves its shadow
/// by at most `R · step`. Choosing the step as `1 / shadow_map_size` keeps that
/// bounded to ~one shadow-map texel on every cascade (a cascade's texel is its
/// diameter / size, and the receiver distance scales with the diameter), so each
/// step is imperceptible while the continuous shimmer is gone.
///
/// Component-rounding + re-normalise is used rather than snapping spherical angles
/// so it stays well-behaved when the sun passes near the zenith (where an azimuth
/// is ill-defined).
fn snap_shadow_direction(direction: Vec3) -> Vec3 {
    let step = 1.0 / SHADOW_MAP_SIZE;
    let snapped = Vec3::new(
        (direction.x / step).round() * step,
        (direction.y / step).round() * step,
        (direction.z / step).round() * step,
    );
    // Re-normalise so it stays a unit direction; fall back to the input if the
    // rounding collapsed it to zero (only possible for a near-zero input, which a
    // light direction never is).
    snapped.try_normalize().unwrap_or(direction)
}

/// The reference viewer's built-in rainbow texture (`IMG_RAINBOW`,
/// `llsettingssky.cpp`), sampled by the sky's rainbow overlay when the sky frame
/// names none of its own.
const IMG_RAINBOW: Uuid = Uuid::from_u128(0x11b4_c57c_56b3_04ed_1f82_2004_3638_82e4);

/// The reference viewer's built-in 22° ice-halo texture (`IMG_HALO`,
/// `llsettingssky.cpp`).
const IMG_HALO: Uuid = Uuid::from_u128(0x1214_9143_f599_91a7_77ac_b52a_3c0f_59cd);

/// The reference `LLSettingsSky::calculateLightSettings` `LIMIT` floor on the
/// light's up component, so the altitude attenuation term stays finite.
const LIGHT_UP_LIMIT: f32 = f32::EPSILON * 8.0;

/// The distance, in metres, at which the sun / moon disc billboards are placed
/// from the camera. Unlike the sky / cloud / star domes (whose depth is forced to
/// the far clip plane), the discs keep their real world-space depth, so at 2000 m
/// they depth-test in front of the far-plane sky backdrop and the star field (a
/// disc occludes the stars behind it) while still sitting inside the camera's far
/// plane. The disc angular size is independent of this distance (the half-extent
/// scales with it), so it only fixes where the billboard sits relative to the dome.
pub(crate) const DISC_DISTANCE: f32 = 2000.0;

/// The reference `HEAVENLY_BODY_FACTOR` (`llvosky.h`): the disc half-extent is
/// `sun_scale * distance * HEAVENLY_BODY_FACTOR * disk_radius`, so a unit-scale
/// disc subtends `HEAVENLY_BODY_FACTOR * disk_radius` radians (half-angle).
const HEAVENLY_BODY_FACTOR: f32 = 0.1;

/// The reference sun-disc radius (`SUN_DISK_RADIUS`, `llvosky.cpp`).
pub(crate) const SUN_DISK_RADIUS: f32 = 0.5;

/// The reference moon-disc radius (`MOON_DISK_RADIUS = SUN_DISK_RADIUS * 0.9`).
pub(crate) const MOON_DISK_RADIUS: f32 = 0.45;

/// The reference viewer's built-in sun-disc texture (`DEFAULT_SUN_ID`,
/// `llsettingssky.cpp`), used when the sky frame names none of its own.
const DEFAULT_SUN_ID: Uuid = Uuid::from_u128(0x32bf_bcea_24b1_fb9d_1ef9_48a2_8a63_730f);

/// The reference viewer's built-in moon-disc texture (`DEFAULT_MOON_ID`,
/// `llsettingssky.cpp`).
const DEFAULT_MOON_ID: Uuid = Uuid::from_u128(0xd07f_6eed_b96a_47cd_b51d_400a_d4a1_c428);

/// The reference viewer's built-in cloud-noise texture (`DEFAULT_CLOUD_ID`,
/// `llsettingssky.cpp`), sampled when the sky frame names none of its own.
const DEFAULT_CLOUD_ID: Uuid = Uuid::from_u128(0x1dc1_368f_e8fe_f02d_a08d_9d9f_11c1_af6b);

/// The radius of the cloud dome, in metres — the reference `LLSettingsSky::
/// DOME_RADIUS`. The cloud layer's *depth* is forced to the far clip plane by
/// `clouds.wgsl` (a skybox backdrop), so this large radius does not need to fit
/// inside the camera far plane; it sets the directional layout and the lighting
/// ray length (`rel_pos_len`) to match the reference.
pub(crate) const CLOUD_DOME_RADIUS: f32 = 15000.0;

/// The fraction of [`CLOUD_DOME_RADIUS`] the camera sits *above* the dome centre —
/// the reference `LLSettingsSky::DOME_OFFSET` (`getCamHeight = dome_offset ×
/// dome_radius`). The reference renders the dome with the camera this high inside
/// it, so the shallow `[0, π/8]` zenith cap ([`calc_cloud_phi`]) wraps down to fill
/// the whole visible sky rather than a small overhead circle. The viewer bakes
/// this offset into the dome vertices so the camera-centred dome entity places the
/// cap the same way.
const CLOUD_DOME_OFFSET: f32 = 0.96;

/// The number of stacks (rings from the zenith to the dome edge) in the cloud
/// dome, mirroring the reference `LLVOWLSky` sky-dome tessellation
/// (`getNumStacks`, `WLSkyDetail`). The stacks are distributed by
/// [`calc_cloud_phi`] over the reference's `[0, π/8]` zenith cap.
const CLOUD_DOME_STACKS: usize = 32;

/// The number of slices (segments around the dome) in the cloud dome, matching the
/// reference `getNumSlices` = `2 × getNumStacks`.
const CLOUD_DOME_SLICES: usize = 64;

/// The reference cloud-scroll accumulation divisor (`LLEnvironment::
/// updateCloudScroll`): the scroll delta grows by `dt * cloud_scroll_rate / 100`
/// each frame.
const CLOUD_SCROLL_DIVISOR: f32 = 100.0;

/// The reference viewer's built-in bloom / star texture (`IMG_BLOOM1`,
/// `llsettingssky.cpp`), sampled by the star field when the sky frame names none
/// of its own.
const IMG_BLOOM1: Uuid = Uuid::from_u128(0x3c59_f7fe_9dc8_47f9_8aaf_a9dd_1fbc_3bef);

/// The number of stars in the field (the reference `LLVOWLSky::getStarsNumVerts`).
const STAR_COUNT: usize = 1000;

/// The radius of the star sphere, in metres, at which the star quads sit for
/// screen projection. Their *depth* is forced to the far clip plane by `stars.wgsl`
/// (a skybox backdrop, occluded by real geometry at any altitude), so this radius
/// only sets the directional layout and — with [`REFERENCE_DOME_RADIUS`] — the
/// per-star screen size; it is kept well inside the camera's 4096 m far plane so
/// the sphere is not frustum-culled.
pub(crate) const STAR_DOME_RADIUS: f32 = 2900.0;

/// The reference sky-dome radius (`LLSettingsSky::DOME_RADIUS`), at which the
/// reference sizes the star quads (`sc = 16 + frand * 20`). Our field sits at the
/// much smaller [`STAR_DOME_RADIUS`] for screen projection, so the per-star size is
/// scaled by `STAR_DOME_RADIUS / REFERENCE_DOME_RADIUS` to keep the same *angular*
/// size the reference draws — otherwise the stars look ~5× too large.
const REFERENCE_DOME_RADIUS: f32 = 15000.0;

/// The reference star-brightness → `custom_alpha` divisor
/// (`renderStarsDeferred`: `getStarBrightness() / 500`).
const STAR_BRIGHTNESS_DIVISOR: f32 = 500.0;

/// Below this `custom_alpha` the reference skips the star pass entirely
/// (`renderStarsDeferred`); the viewer hides the field instead.
const STAR_ALPHA_THRESHOLD: f32 = 0.001;

/// The reference slow star-field rotation rate, about the up axis
/// (`renderStarsDeferred`: `rotatef(gFrameTimeSeconds * 0.01, …)`). `glRotatef`
/// takes *degrees*, so this is degrees per second — a very slow drift (a full turn
/// takes ~10 hours); it is converted to radians at use.
const STAR_ROTATION_RATE_DEG: f32 = 0.01;

/// The reference twinkle-time scale (`renderStarsDeferred`: `sStarTime =
/// getElapsedSeconds() * 0.5`).
const STAR_TIME_SCALE: f32 = 0.5;

/// The seed for the deterministic star-placement PRNG, so the star field is
/// identical across runs (the reference seeds from the global `ll_frand`).
const STAR_RNG_SEED: u64 = 0x5142_4152_5354_4152;

/// Marks the sky-dome entity so [`center_sky_on_camera`] can follow the camera.
#[derive(Component)]
pub(crate) struct SkyDome;

/// Marks the scene's sun / moon directional light so [`drive_sky`] can aim and
/// colour it from the sky.
#[derive(Component)]
pub(crate) struct SceneSun;

/// The viewer's sky-render state: the shared sky material and the decoded /
/// requested rainbow / halo overlay textures.
#[derive(Resource)]
pub(crate) struct SkyState {
    /// The single sky-dome material, updated each frame by [`drive_sky`].
    material: Handle<SkyMaterial>,
    /// The texture id currently requested for the rainbow overlay (from the active
    /// sky frame, or the built-in [`IMG_RAINBOW`]).
    rainbow_key: Option<TextureKey>,
    /// The texture id currently requested for the halo overlay.
    halo_key: Option<TextureKey>,
}

/// Marks the sun-disc billboard entity so [`drive_sun_moon_discs`] can aim it.
#[derive(Component)]
pub(crate) struct SunDisc;

/// Marks the moon-disc billboard entity so [`drive_sun_moon_discs`] can aim it.
#[derive(Component)]
pub(crate) struct MoonDisc;

/// The viewer's sun / moon disc state: the two disc materials and the disc
/// textures currently requested for them.
#[derive(Resource)]
pub(crate) struct DiscState {
    /// The sun-disc material, updated each frame by [`drive_sun_moon_discs`].
    sun_material: Handle<SunDiscMaterial>,
    /// The moon-disc material.
    moon_material: Handle<SunDiscMaterial>,
    /// The texture id currently requested for the sun disc (the active sky
    /// frame's, or the built-in [`DEFAULT_SUN_ID`]).
    sun_key: Option<TextureKey>,
    /// The texture id currently requested for the moon disc.
    moon_key: Option<TextureKey>,
}

/// Marks the cloud-dome entity so [`center_sky_on_camera`] can follow the camera.
#[derive(Component)]
pub(crate) struct CloudDome;

/// The viewer's cloud-layer state: the cloud material, the requested cloud-noise
/// texture, and the accumulated scroll offset.
#[derive(Resource)]
pub(crate) struct CloudState {
    /// The single cloud-dome material, updated each frame by [`drive_clouds`].
    material: Handle<CloudMaterial>,
    /// The texture id currently requested for the cloud noise (the active sky
    /// frame's, or the built-in [`DEFAULT_CLOUD_ID`]).
    cloud_key: Option<TextureKey>,
    /// The accumulated cloud-scroll offset (the reference
    /// `LLEnvironment::mCloudScrollDelta`), grown each frame from the sky frame's
    /// `cloud_scroll_rate` and folded into `cloud_pos_density1` so the layer
    /// drifts. Persists across sky-frame changes, like the reference.
    scroll: Vec2,
    /// The next time (`Time::elapsed_secs`) the opt-in cloud-param debug log
    /// (`SL_VIEWER_LOG_CLOUDS`) may fire, throttling it to a readable cadence.
    next_log_at: f32,
}

/// Marks the star-field entity so [`drive_stars`] can centre / rotate it.
#[derive(Component)]
pub(crate) struct StarField;

/// The viewer's star-field state: the star material and the bloom texture
/// currently requested for it.
#[derive(Resource)]
pub(crate) struct StarState {
    /// The single star-field material, updated each frame by [`drive_stars`].
    material: Handle<StarMaterial>,
    /// The texture id currently requested for the bloom / star texture (the active
    /// sky frame's, or the built-in [`IMG_BLOOM1`]).
    star_key: Option<TextureKey>,
}

/// Everything one sky frame implies for the scene it lights: the shader uniforms,
/// where the two bodies are, which of them is up, and the light and ambient the
/// atmosphere yields.
///
/// Extracted because **three systems were deriving it, identically**:
/// [`drive_sky`], [`drive_clouds`] and [`drive_sun_moon_discs`] each recomputed the
/// sun and moon directions, the up tests, the active light direction, the glow
/// ladder and the clamped light-norm from the same `SkySettings` — the comments in
/// two of them said "as in `drive_sky`", which is a copy admitting it is one. Three
/// copies of a derivation that must agree is three chances for them not to.
///
/// It is also what makes a sky **reachable without a session**: the derivation used
/// to be welded to `Res<EnvironmentState>` and a camera query, so the only way to
/// get a sky's uniforms was to be inside a running viewer. Now it is a function of
/// a `SkySettings`, which is a plain value — so `crate::render_scene`'s four
/// time-of-day scenes render the real atmosphere rather than four hand-copied
/// uniform blocks.
pub(crate) struct ResolvedSky {
    /// The atmosphere shader's uniform block.
    pub(crate) params: SkyParams,
    /// The clamped light-norm the shaders dot against (`getClampedLightNorm`).
    pub(crate) lightnorm: Vec3,
    /// The sun's direction, in Bevy space.
    pub(crate) sun_dir: Vec3,
    /// The moon's direction, in Bevy space.
    pub(crate) moon_dir: Vec3,
    /// Whether the sun is above the horizon (`getIsSunUp`).
    pub(crate) sun_up: bool,
    /// Whether the moon is above the horizon (`getIsMoonUp`).
    pub(crate) moon_up: bool,
    /// `1.0` by day, `0.0` by night — the shaders' `sun_up_factor`.
    pub(crate) sun_up_factor: f32,
    /// The sun/moon glow factor (`getSunMoonGlowFactor`).
    pub(crate) glow_factor: f32,
    /// The active light's direction: the sun if it is up, else the moon if it is,
    /// else straight down (`getLightDirection`).
    pub(crate) light_dir: Vec3,
    /// The active body's atmospheric diffuse colour — the scene's directional
    /// light.
    pub(crate) diffuse: [f32; 3],
    /// The sky's total ambient colour.
    pub(crate) ambient: [f32; 3],
}

/// Resolve one sky frame into everything the scene needs from it. See
/// [`ResolvedSky`].
pub(crate) fn resolve_sky(sky: &SkySettings) -> ResolvedSky {
    // Sun / moon directions in Bevy space, and which body is up (the reference
    // tests the Second Life up component, which maps to Bevy `y`).
    let sun_dir = sl_to_bevy_object_rotation(&sky.sun_rotation)
        .mul_vec3(Vec3::X)
        .normalize();
    let moon_dir = sl_to_bevy_object_rotation(&sky.moon_rotation)
        .mul_vec3(Vec3::X)
        .normalize();
    let sun_up = sun_dir.y >= 0.0;
    let moon_up = moon_dir.y >= 0.0;

    // The active light direction (`getLightDirection`): sun if up, else moon if
    // up, else straight down.
    let light_dir = if sun_up {
        sun_dir
    } else if moon_up {
        moon_dir
    } else {
        Vec3::NEG_Y
    };

    let sun_up_factor = if sun_up { 1.0 } else { 0.0 };
    // `getSunMoonGlowFactor`: full by day, a small moon-brightness fraction by
    // night, none when neither body is up.
    let glow_factor = if sun_up {
        1.0
    } else if moon_up {
        sky.moon_brightness * 0.25
    } else {
        0.0
    };

    // The clamped light-norm the shader dots against (`getClampedLightNorm`
    // floors the up component at -0.1).
    let lightnorm = Vec3::new(light_dir.x, light_dir.y.max(-0.1), light_dir.z);

    // Scene lighting from the sky (`calculateLightSettings`).
    let lighting = calculate_light_settings(sky, light_dir.y, moon_up);
    let diffuse = if sun_up {
        lighting.sun_diffuse
    } else if moon_up {
        lighting.moon_diffuse
    } else {
        [1.0, 1.0, 1.0]
    };

    ResolvedSky {
        params: sky_params(sky, lightnorm, sun_up_factor, glow_factor),
        lightnorm,
        sun_dir,
        moon_dir,
        sun_up,
        moon_up,
        sun_up_factor,
        glow_factor,
        light_dir,
        diffuse,
        ambient: lighting.total_ambient,
    }
}

/// Startup: spawn the sky dome (with its material) and the scene's directional
/// light, and register [`SkyState`].
pub(crate) fn setup_sky(
    mut commands: Commands,
    environment: Res<EnvironmentState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let placeholder = images.add(placeholder_image());
    // Seed the material from the current environment at ground level and the
    // current day position; `drive_sky` refines it every frame.
    let sky = environment
        .settings
        .blended_sky_settings(0.0, day_position(&environment.settings));
    let params = sky.map_or_else(default_sky_params, |sky| {
        sky_params(&sky, Vec3::Y, 1.0, 1.0)
    });
    let material = materials.add(SkyMaterial {
        params,
        rainbow: placeholder.clone(),
        halo: placeholder.clone(),
    });
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(SKY_DOME_RADIUS))),
        MeshMaterial3d(material.clone()),
        Transform::default(),
        // The sky never casts shadows (P24 adds cascaded shadow maps for the sun).
        NotShadowCaster,
        SkyDome,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: SCENE_LIGHT_ILLUMINANCE,
            // P24.1: cast cascaded shadow maps from the sun / moon.
            shadow_maps_enabled: true,
            ..default()
        },
        // Cascades tuned to region scale so shadows cover an avatar plus nearby
        // prims and terrain (`drive_sky` keeps the direction on the active body).
        shadow_cascades(),
        Transform::default().looking_to(Vec3::new(-0.4, -1.0, -0.3), Vec3::Y),
        SceneSun,
    ));
    commands.insert_resource(SkyState {
        material,
        rainbow_key: None,
        halo_key: None,
    });
}

/// Keep the sky and cloud domes centred on the camera each frame, so the
/// atmosphere always surrounds the viewpoint (the reference renders the domes
/// camera-relative).
#[expect(
    clippy::type_complexity,
    reason = "a Bevy query filter selecting both dome markers so they follow the camera together"
)]
pub(crate) fn center_sky_on_camera(
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
    mut domes: Query<&mut Transform, Or<(With<SkyDome>, With<CloudDome>)>>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let translation = camera.translation();
    for mut transform in &mut domes {
        transform.translation = translation;
    }
}

/// Fold the current environment + camera altitude into the sky material, the
/// directional light, and the ambient light, and (re)request the sky's rainbow /
/// halo overlay textures boosted.
pub(crate) fn drive_sky(
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
    environment: Res<EnvironmentState>,
    mut state: ResMut<SkyState>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut textures: ResMut<TextureManager>,
    mut sun: Query<(&mut Transform, &mut DirectionalLight), With<SceneSun>>,
    mut ambient: ResMut<GlobalAmbientLight>,
) {
    let altitude = camera.single().map_or(0.0, |camera| camera.translation().y);
    let position = day_position(&environment.settings);
    let Some(sky) = environment
        .settings
        .blended_sky_settings(altitude, position)
    else {
        return;
    };

    // Every derivation this system used to do inline. See `ResolvedSky`.
    let resolved = resolve_sky(&sky);
    let light_dir = resolved.light_dir;
    let diffuse = resolved.diffuse;

    if let Some(mut material) = materials.get_mut(&state.material) {
        material.params = resolved.params;
    }

    if let Ok((mut transform, mut light)) = sun.single_mut() {
        // The light travels *toward* its forward axis, i.e. away from the body, so
        // its forward is the negated light direction. Snap the *shadow-caster*
        // direction to a texel-equivalent angular grid first (R20): the real-time
        // day cycle rotates the sun a hair every frame, which rotates the cascaded
        // shadow map's light-space texel grid and makes the ground shadows shimmer
        // / oscillate — Bevy already texel-snaps the cascade origin, but a
        // per-frame-rotating light defeats it. Snapping holds the direction
        // bit-stable between steps, so the shadow sits still, and each step moves
        // it by at most ~one shadow-map texel (imperceptible). The visible sun
        // disc, sky, and light colour keep the un-snapped direction, so only the
        // shadow projection is affected. Pick a safe up when the body is near the
        // zenith (forward near-parallel to +Y).
        let shadow_dir = snap_shadow_direction(light_dir);
        let forward = Vec3::new(-shadow_dir.x, -shadow_dir.y, -shadow_dir.z);
        let up = if forward.dot(Vec3::Y).abs() > 0.99 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        *transform = Transform::default().looking_to(forward, up);
        light.color = Color::linear_rgb(
            diffuse[0].clamp(0.0, 1.0),
            diffuse[1].clamp(0.0, 1.0),
            diffuse[2].clamp(0.0, 1.0),
        );
    }

    // Ambient from the sky's total ambient: its luminance sets the fill strength,
    // its (normalised) hue the tint.
    let amb = resolved.ambient;
    let luminance = 0.2126 * amb[0] + 0.7152 * amb[1] + 0.0722 * amb[2];
    let peak = amb[0].max(amb[1]).max(amb[2]).max(1.0e-4);
    ambient.color = Color::linear_rgb(amb[0] / peak, amb[1] / peak, amb[2] / peak);
    ambient.brightness = luminance * AMBIENT_BRIGHTNESS_SCALE;

    // Fetch the sky's referenced rainbow / halo textures boosted (the sky frame's
    // own, or the reference built-ins) so they resolve ahead of ordinary faces.
    let rainbow_key = Some(
        sky.rainbow_texture
            .unwrap_or_else(|| TextureKey::from(IMG_RAINBOW)),
    );
    let halo_key = Some(
        sky.halo_texture
            .unwrap_or_else(|| TextureKey::from(IMG_HALO)),
    );
    if let Some(key) = rainbow_key {
        textures.request_boosted(key, SKY_BOOST_PRIORITY);
    }
    if let Some(key) = halo_key {
        textures.request_boosted(key, SKY_BOOST_PRIORITY);
    }
    state.rainbow_key = rainbow_key;
    state.halo_key = halo_key;
}

/// Swap a decoded sky texture into the material when its rainbow / halo id
/// resolves.
pub(crate) fn apply_sky_textures(
    mut decoded: MessageReader<TextureDecoded>,
    state: Res<SkyState>,
    manager: Res<TextureManager>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for &TextureDecoded(id) in decoded.read() {
        let is_rainbow = state.rainbow_key == Some(id);
        let is_halo = state.halo_key == Some(id);
        if !is_rainbow && !is_halo {
            continue;
        }
        let Some(decoded) = manager.decoded(id) else {
            // The fetch/decode failed; the overlay keeps its placeholder (and the
            // default moisture / ice of 0 makes it a no-op anyway).
            continue;
        };
        let handle = images.add(to_bevy_image(decoded));
        let Some(mut material) = materials.get_mut(&state.material) else {
            return;
        };
        if is_rainbow {
            material.rainbow = handle.clone();
        }
        if is_halo {
            material.halo = handle;
        }
    }
}

/// Startup: spawn the sun / moon disc billboards (a shared unit quad + a
/// [`SunDiscMaterial`] each, initially hidden) and register [`DiscState`].
pub(crate) fn setup_sun_moon_discs(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SunDiscMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let placeholder = images.add(placeholder_image());
    // A shared 1×1 quad (centred, +Z normal); the billboards scale it to the disc
    // size and orient it toward the camera each frame.
    let quad = meshes.add(Rectangle::new(1.0, 1.0));

    let sun_material = materials.add(SunDiscMaterial {
        params: SunDiscParams {
            brightness: 1.0,
            blend_factor: 0.0,
            moon_mode: 0.0,
            up_component: 0.0,
        },
        diffuse: placeholder.clone(),
        alt_diffuse: placeholder.clone(),
    });
    let moon_material = materials.add(SunDiscMaterial {
        params: SunDiscParams {
            brightness: 1.0,
            blend_factor: 0.0,
            moon_mode: 1.0,
            up_component: 0.0,
        },
        diffuse: placeholder.clone(),
        alt_diffuse: placeholder.clone(),
    });

    commands.spawn((
        Mesh3d(quad.clone()),
        MeshMaterial3d(sun_material.clone()),
        Transform::default(),
        Visibility::Hidden,
        NotShadowCaster,
        SunDisc,
    ));
    commands.spawn((
        Mesh3d(quad),
        MeshMaterial3d(moon_material.clone()),
        Transform::default(),
        Visibility::Hidden,
        NotShadowCaster,
        MoonDisc,
    ));

    commands.insert_resource(DiscState {
        sun_material,
        moon_material,
        sun_key: None,
        moon_key: None,
    });
}

/// Aim, scale, colour, and show / hide the sun and moon discs for the active sky
/// frame, and (re)request their sun / moon textures boosted.
#[expect(
    clippy::type_complexity,
    reason = "two Bevy queries whose disjointness filters keep the sun / moon discs distinct"
)]
pub(crate) fn drive_sun_moon_discs(
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
    environment: Res<EnvironmentState>,
    mut state: ResMut<DiscState>,
    mut materials: ResMut<Assets<SunDiscMaterial>>,
    mut textures: ResMut<TextureManager>,
    mut sun: Query<(&mut Transform, &mut Visibility), (With<SunDisc>, Without<MoonDisc>)>,
    mut moon: Query<(&mut Transform, &mut Visibility), (With<MoonDisc>, Without<SunDisc>)>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let camera_pos = camera.translation();
    let position = day_position(&environment.settings);
    let Some(sky) = environment
        .settings
        .blended_sky_settings(camera_pos.y, position)
    else {
        return;
    };

    // The frame's bodies, shared with `drive_sky` rather than recomputed here — it
    // used to be a verbatim copy. See `ResolvedSky`.
    let ResolvedSky {
        sun_dir,
        moon_dir,
        sun_up,
        moon_up,
        ..
    } = resolve_sky(&sky);

    // Aim each disc when its body is up, and show only the bodies above the
    // horizon (`getIsSunUp` / `getIsMoonUp`).
    if let Ok((mut transform, mut vis)) = sun.single_mut() {
        if sun_up {
            *transform = disc_transform(camera_pos, sun_dir, sky.sun_scale, SUN_DISK_RADIUS);
        }
        *vis = visible_if(sun_up);
    }
    if let Ok((mut transform, mut vis)) = moon.single_mut() {
        if moon_up {
            *transform = disc_transform(camera_pos, moon_dir, sky.moon_scale, MOON_DISK_RADIUS);
        }
        *vis = visible_if(moon_up);
    }

    // The sun disc is untinted (the reference `sunDiscF` ignores its bound diffuse
    // colour); the moon disc is scaled by the sky's moon brightness and faded near
    // the horizon by its up component (`moonF`).
    if let Some(mut material) = materials.get_mut(&state.sun_material) {
        material.params.up_component = sun_dir.y;
    }
    if let Some(mut material) = materials.get_mut(&state.moon_material) {
        material.params.brightness = sky.moon_brightness;
        material.params.up_component = moon_dir.y;
    }

    // Fetch the disc textures boosted (the sky frame's own, or the reference
    // built-ins) so they resolve ahead of ordinary faces.
    let sun_key = sky
        .sun_texture
        .unwrap_or_else(|| TextureKey::from(DEFAULT_SUN_ID));
    let moon_key = sky
        .moon_texture
        .unwrap_or_else(|| TextureKey::from(DEFAULT_MOON_ID));
    textures.request_boosted(sun_key, SKY_BOOST_PRIORITY);
    textures.request_boosted(moon_key, SKY_BOOST_PRIORITY);
    state.sun_key = Some(sun_key);
    state.moon_key = Some(moon_key);
}

/// Swap a decoded disc texture into the sun / moon material when its id resolves.
pub(crate) fn apply_disc_textures(
    mut decoded: MessageReader<TextureDecoded>,
    state: Res<DiscState>,
    manager: Res<TextureManager>,
    mut materials: ResMut<Assets<SunDiscMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for &TextureDecoded(id) in decoded.read() {
        let is_sun = state.sun_key == Some(id);
        let is_moon = state.moon_key == Some(id);
        if !is_sun && !is_moon {
            continue;
        }
        let Some(decoded) = manager.decoded(id) else {
            // The fetch/decode failed; the disc keeps its (transparent) placeholder.
            continue;
        };
        let handle = images.add(to_bevy_image(decoded));
        let target = if is_sun {
            &state.sun_material
        } else {
            &state.moon_material
        };
        if let Some(mut material) = materials.get_mut(target) {
            // Both texture slots share the id until the day cycle (P22.6) drives a
            // separate next-frame texture and the blend factor between them.
            material.diffuse = handle.clone();
            material.alt_diffuse = handle;
        }
    }
}

/// Startup: spawn the cloud dome (with its material, initially hidden until an
/// environment selects a sky frame) and register [`CloudState`].
pub(crate) fn setup_clouds(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CloudMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let placeholder = images.add(placeholder_image());
    let material = materials.add(CloudMaterial {
        params: default_cloud_params(),
        cloud_noise: placeholder.clone(),
        cloud_noise_next: placeholder,
    });
    commands.spawn((
        Mesh3d(meshes.add(build_cloud_dome_mesh())),
        MeshMaterial3d(material.clone()),
        Transform::default(),
        // The cloud layer never casts shadows (like the sky dome).
        NotShadowCaster,
        CloudDome,
    ));
    commands.insert_resource(CloudState {
        material,
        cloud_key: None,
        scroll: Vec2::ZERO,
        next_log_at: 0.0,
    });
}

/// Fold the current environment + camera altitude into the cloud material,
/// accumulate the cloud scroll, and (re)request the sky's cloud-noise texture
/// boosted.
pub(crate) fn drive_clouds(
    time: Res<Time>,
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
    environment: Res<EnvironmentState>,
    mut state: ResMut<CloudState>,
    mut materials: ResMut<Assets<CloudMaterial>>,
    mut textures: ResMut<TextureManager>,
) {
    let altitude = camera.single().map_or(0.0, |camera| camera.translation().y);
    let position = day_position(&environment.settings);
    let Some(sky) = environment
        .settings
        .blended_sky_settings(altitude, position)
    else {
        return;
    };

    // The frame's light, shared with `drive_sky` rather than recomputed here — it
    // used to be a verbatim copy. See `ResolvedSky`.
    let resolved = resolve_sky(&sky);

    // Accumulate the cloud scroll (`LLEnvironment::updateCloudScroll`): grow the
    // delta by `dt * rate / 100`, or reset it to zero when the rate is zero.
    let [rate_x, rate_y] = sky.cloud_scroll_rate;
    if rate_x == 0.0 && rate_y == 0.0 {
        state.scroll = Vec2::ZERO;
    } else {
        let dt = time.delta_secs();
        state.scroll.x += dt * rate_x / CLOUD_SCROLL_DIVISOR;
        state.scroll.y += dt * rate_y / CLOUD_SCROLL_DIVISOR;
    }

    if let Some(mut material) = materials.get_mut(&state.material) {
        material.params = cloud_params(
            &sky,
            resolved.lightnorm,
            resolved.sun_up_factor,
            resolved.glow_factor,
            state.scroll,
        );
    }

    // Fetch the sky's cloud-noise texture boosted (the sky frame's own, or the
    // reference built-in) so it resolves ahead of ordinary faces.
    let cloud_key = sky
        .cloud_texture
        .unwrap_or_else(|| TextureKey::from(DEFAULT_CLOUD_ID));
    textures.request_boosted(cloud_key, SKY_BOOST_PRIORITY);
    state.cloud_key = Some(cloud_key);

    // Opt-in cloud-param diagnostic (`SL_VIEWER_LOG_CLOUDS`): dump the EEP cloud
    // settings + the resolved cloud-noise texture id so a live aditi session can be
    // compared against Firestorm (R18 — the cloud distribution mismatch). Throttled
    // to ~2 s; purely a log, no rendering effect.
    if time.elapsed_secs() >= state.next_log_at && std::env::var("SL_VIEWER_LOG_CLOUDS").is_ok() {
        state.next_log_at = time.elapsed_secs() + 2.0;
        let pd1 = sky.cloud_pos_density1;
        let pd2 = sky.cloud_pos_density2;
        info!(
            "cloud params: texture={:?} region_specified={} scale={:.4} \
             pos_density1=({:.4},{:.4},{:.4}) pos_density2=({:.4},{:.4},{:.4}) \
             variance={:.4} scroll_rate=[{:.4},{:.4}] shadow={:.4} \
             color=({:.3},{:.3},{:.3})",
            cloud_key,
            sky.cloud_texture.is_some(),
            sky.cloud_scale,
            pd1.position_x(),
            pd1.position_y(),
            pd1.density(),
            pd2.position_x(),
            pd2.position_y(),
            pd2.density(),
            sky.cloud_variance,
            sky.cloud_scroll_rate[0],
            sky.cloud_scroll_rate[1],
            sky.cloud_shadow,
            sky.cloud_color.red(),
            sky.cloud_color.green(),
            sky.cloud_color.blue(),
        );
    }
}

/// Swap a decoded cloud-noise texture into the cloud material when its id resolves.
pub(crate) fn apply_cloud_textures(
    mut decoded: MessageReader<TextureDecoded>,
    state: Res<CloudState>,
    manager: Res<TextureManager>,
    mut materials: ResMut<Assets<CloudMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for &TextureDecoded(id) in decoded.read() {
        if state.cloud_key != Some(id) {
            continue;
        }
        let Some(decoded) = manager.decoded(id) else {
            // The fetch/decode failed; the layer keeps its (transparent) placeholder.
            if std::env::var("SL_VIEWER_LOG_CLOUDS").is_ok() {
                warn!("cloud texture {id:?} fetch/decode FAILED (using placeholder)");
            }
            continue;
        };
        if std::env::var("SL_VIEWER_LOG_CLOUDS").is_ok() {
            info!(
                "cloud texture {id:?} decoded ({}x{}, {} components)",
                decoded.width, decoded.height, decoded.components
            );
        }
        // The cloud shader tiles the noise: `cloud_scale` magnifies the UVs and the
        // `cloud_pos_density` / scroll offsets push them well outside `[0, 1]`, so the
        // texture must wrap. Bevy's default sampler is clamp-to-edge (which would smear
        // the black edge texel across the whole layer — the reference samples with
        // `GL_REPEAT`), so give the cloud image a repeating sampler.
        let mut image = to_bevy_image(decoded);
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            address_mode_w: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::linear()
        });
        let handle = images.add(image);
        if let Some(mut material) = materials.get_mut(&state.material) {
            // Both noise slots share the id until the day cycle (P22.6) drives a
            // separate next-frame texture and the blend factor between them.
            material.cloud_noise = handle.clone();
            material.cloud_noise_next = handle;
        }
    }
}

/// Startup: build the star-quad mesh, spawn the star field (with its material,
/// initially hidden until an environment selects a sky frame), and register
/// [`StarState`].
pub(crate) fn setup_stars(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StarMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let placeholder = images.add(placeholder_image());
    let material = materials.add(StarMaterial {
        params: StarParams {
            custom_alpha: 0.0,
            time: 0.0,
            reserved: Vec2::ZERO,
        },
        diffuse: placeholder,
    });
    commands.spawn((
        Mesh3d(meshes.add(build_star_mesh())),
        MeshMaterial3d(material.clone()),
        Transform::default(),
        Visibility::Hidden,
        // The star field never casts shadows (like the sky / cloud domes).
        NotShadowCaster,
        StarField,
    ));
    commands.insert_resource(StarState {
        material,
        star_key: None,
    });
}

/// Centre and slowly rotate the star field on the camera, fold the active sky
/// frame's `star_brightness` and the twinkle time into the material, show / hide
/// the field, and (re)request the sky's bloom texture boosted.
pub(crate) fn drive_stars(
    time: Res<Time>,
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
    environment: Res<EnvironmentState>,
    mut state: ResMut<StarState>,
    mut materials: ResMut<Assets<StarMaterial>>,
    mut textures: ResMut<TextureManager>,
    mut field: Query<(&mut Transform, &mut Visibility), With<StarField>>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let camera_pos = camera.translation();
    let position = day_position(&environment.settings);
    let Some(sky) = environment
        .settings
        .blended_sky_settings(camera_pos.y, position)
    else {
        return;
    };

    // The reference `custom_alpha` = `star_brightness / 500` (clamped); below the
    // `0.001` threshold the reference skips the pass, so hide the field.
    let custom_alpha = (sky.star_brightness / STAR_BRIGHTNESS_DIVISOR).min(1.0);
    let visible = custom_alpha >= STAR_ALPHA_THRESHOLD;
    let elapsed = time.elapsed_secs();

    if let Ok((mut transform, mut vis)) = field.single_mut() {
        // Keep the field centred on the camera and rotate it slowly about the up
        // axis (the reference `rotatef(gFrameTimeSeconds * 0.01, …)`, in degrees).
        *transform = Transform {
            translation: camera_pos,
            rotation: Quat::from_rotation_y((elapsed * STAR_ROTATION_RATE_DEG).to_radians()),
            scale: Vec3::ONE,
        };
        *vis = visible_if(visible);
    }

    if let Some(mut material) = materials.get_mut(&state.material) {
        material.params.custom_alpha = custom_alpha;
        material.params.time = elapsed * STAR_TIME_SCALE;
    }

    // Fetch the sky's bloom texture boosted (the sky frame's own, or the reference
    // built-in) so it resolves ahead of ordinary faces.
    let star_key = sky
        .bloom_texture
        .unwrap_or_else(|| TextureKey::from(IMG_BLOOM1));
    textures.request_boosted(star_key, SKY_BOOST_PRIORITY);
    state.star_key = Some(star_key);
}

/// Swap the decoded bloom texture into the star material when its id resolves.
pub(crate) fn apply_star_textures(
    mut decoded: MessageReader<TextureDecoded>,
    state: Res<StarState>,
    manager: Res<TextureManager>,
    mut materials: ResMut<Assets<StarMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for &TextureDecoded(id) in decoded.read() {
        if state.star_key != Some(id) {
            continue;
        }
        let Some(decoded) = manager.decoded(id) else {
            // The fetch/decode failed; the field keeps its (transparent) placeholder.
            continue;
        };
        let handle = images.add(to_bevy_image(decoded));
        if let Some(mut material) = materials.get_mut(&state.material) {
            material.diffuse = handle;
        }
    }
}

/// The reference `LLVOWLSky::calcPhi` stack-angle distribution: maps a normalised
/// stack parameter `t` (`0` at the zenith, `1` at the dome edge) to a polar angle
/// `φ ∈ [0, π/8]` measured from the zenith, biased toward the edge. This is why the
/// cloud dome is a shallow **overhead cap** (its edge sits ~22.5° from straight up),
/// so clouds concentrate overhead and never reach the horizon — the key to
/// avoiding the near-horizon smear (R18) the per-fragment full-sphere projection
/// produced.
fn calc_cloud_phi(t: f32) -> f32 {
    let mut x = t * t; // t²
    x = x * x; // t⁴
    x = 1.0 - x; // 1 − t⁴
    x = x * x; // (1 − t⁴)²
    x = 1.0 - x; // 1 − (1 − t⁴)²
    core::f32::consts::FRAC_PI_8 * x
}

/// Build the cloud-dome mesh: a faithful port of the reference `LLVOWLSky` sky-dome
/// tessellation used for clouds (`buildStripsBuffer`). A grid of
/// [`CLOUD_DOME_STACKS`]×[`CLOUD_DOME_SLICES`] vertices over the zenith cap
/// ([`calc_cloud_phi`]), each carrying the reference **baked** planar cloud
/// texcoord `((-z0 + 1) / 2, (-x0 + 1) / 2)` of its unit dome direction (Bevy Y-up:
/// `x0`/`z0` horizontal, `y0 = cos φ` up). `clouds.wgsl` samples the cloud texture
/// through this interpolated UV, so the projection matches the reference instead of
/// being derived per fragment across a full sphere.
pub(crate) fn build_cloud_dome_mesh() -> Mesh {
    let stride = CLOUD_DOME_SLICES.saturating_add(1);
    let vert_count = CLOUD_DOME_STACKS.saturating_add(1).saturating_mul(stride);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(vert_count);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(vert_count);
    let mut indices: Vec<u32> = Vec::new();

    // The camera sits this high inside the dome (`getCamHeight`); baking it into
    // the vertices (lowering the dome by `cam_height`) means the camera-centred
    // dome entity sees the `[0, π/8]` cap wrapped down over the whole sky, and the
    // vertex position is already camera-relative for the shader's lighting `rel_pos`.
    let cam_height = CLOUD_DOME_RADIUS * CLOUD_DOME_OFFSET;

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "small stack/slice counts, exactly representable as f32"
    )]
    let stacks_f = CLOUD_DOME_STACKS as f32;
    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "small stack/slice counts, exactly representable as f32"
    )]
    let slices_f = CLOUD_DOME_SLICES as f32;

    for i in 0..=CLOUD_DOME_STACKS {
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "small stack index, exactly representable as f32"
        )]
        let t = i as f32 / stacks_f;
        let phi = calc_cloud_phi(t);
        let (sin_phi, cos_phi) = (phi.sin(), phi.cos());
        for j in 0..=CLOUD_DOME_SLICES {
            #[expect(
                clippy::cast_precision_loss,
                clippy::as_conversions,
                reason = "small slice index, exactly representable as f32"
            )]
            let theta = std::f32::consts::TAU * (j as f32 / slices_f);
            let (sin_theta, cos_theta) = (theta.sin(), theta.cos());
            // Unit dome direction (Bevy Y-up: y0 is up, x0/z0 horizontal).
            let x0 = sin_phi * cos_theta;
            let y0 = cos_phi;
            let z0 = sin_phi * sin_theta;
            positions.push([
                x0 * CLOUD_DOME_RADIUS,
                y0 * CLOUD_DOME_RADIUS - cam_height,
                z0 * CLOUD_DOME_RADIUS,
            ]);
            // The reference baked planar texcoord (`buildStripsBuffer`):
            // `((-z0 + 1) / 2, (-x0 + 1) / 2)`, expressed as midpoints.
            uvs.push([f32::midpoint(-z0, 1.0), f32::midpoint(-x0, 1.0)]);
        }
    }

    for i in 0..CLOUD_DOME_STACKS {
        let row = i.saturating_mul(stride);
        let next_row = row.saturating_add(stride);
        for j in 0..CLOUD_DOME_SLICES {
            let a = u32::try_from(row.saturating_add(j)).unwrap_or(u32::MAX);
            let b = a.saturating_add(1);
            let c = u32::try_from(next_row.saturating_add(j)).unwrap_or(u32::MAX);
            let d = c.saturating_add(1);
            // Two triangles per grid cell; cloud material disables back-face
            // culling, so winding is immaterial.
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// Build the star-field mesh: [`STAR_COUNT`] small camera-facing quads scattered
/// over the upper hemisphere of a sphere of radius [`STAR_DOME_RADIUS`], each with
/// a per-star near-white colour (the reference `LLVOWLSky::initStars` /
/// `updateStarGeometry`). Deterministic (fixed-seed PRNG) so the field is stable
/// across runs.
pub(crate) fn build_star_mesh() -> Mesh {
    let mut rng = StarRng::new(STAR_RNG_SEED);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(STAR_COUNT.saturating_mul(4));
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(STAR_COUNT.saturating_mul(4));
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(STAR_COUNT.saturating_mul(4));
    let mut indices: Vec<u32> = Vec::with_capacity(STAR_COUNT.saturating_mul(6));

    for i in 0..STAR_COUNT {
        // A random direction on the upper hemisphere (Bevy Y up): the reference
        // `initStars` picks `x,y ∈ [-0.5, 0.5)`, `z ∈ [0, 0.5)` (Second Life up),
        // which maps to Bevy `x,z ∈ [-0.5, 0.5)`, `y ∈ [0, 0.5)`.
        let x = rng.frand() - 0.5;
        let z = rng.frand() - 0.5;
        let y = rng.frand() * 0.5;
        let dir = Vec3::new(x, y, z).normalize_or(Vec3::Y);
        let centre = scale3(dir, STAR_DOME_RADIUS);

        // Quad basis (the reference `at % up` / `at % left`): a stable pair
        // orthogonal to the view direction. Seed with a different axis near the
        // zenith so the cross products stay well-conditioned.
        let seed = if dir.y.abs() > 0.99 { Vec3::X } else { Vec3::Y };
        let left = dir.cross(seed).normalize_or(Vec3::X);
        let up = dir.cross(left).normalize_or(Vec3::Z);
        // Per-star size (the reference `sc = 16 + frand * 20`, at its 15000 m dome),
        // scaled down to our nearer dome so the *angular* size matches the reference.
        let sc = (16.0 + rng.frand() * 20.0) * (STAR_DOME_RADIUS / REFERENCE_DOME_RADIUS);
        let left = scale3(left, sc);
        let up = scale3(up, sc);

        // The four quad corners (the reference winds `star`, `star+up`,
        // `star+left+up`, `star+left`).
        let c0 = centre;
        let c1 = add3(centre, up);
        let c2 = add3(add3(centre, left), up);
        let c3 = add3(centre, left);
        positions.push(c0.to_array());
        positions.push(c1.to_array());
        positions.push(c2.to_array());
        positions.push(c3.to_array());

        // Matching corner UVs (the reference `(1,0) (1,1) (0,1) (0,0)`).
        uvs.push([1.0, 0.0]);
        uvs.push([1.0, 1.0]);
        uvs.push([0.0, 1.0]);
        uvs.push([0.0, 0.0]);

        // Per-star colour: a near-white with a little red / blue variance (the
        // reference `0.75 + frand * 0.25` on red and blue, green `1.0`).
        let red = 0.75 + rng.frand() * 0.25;
        let blue = 0.75 + rng.frand() * 0.25;
        let color = [red, 1.0, blue, 1.0];
        colors.push(color);
        colors.push(color);
        colors.push(color);
        colors.push(color);

        // Two triangles per quad. The base index is `i * 4`, computed without a
        // panicking multiply so the workspace lints stay happy.
        let base = u32::try_from(i.saturating_mul(4)).unwrap_or(u32::MAX);
        indices.push(base);
        indices.push(base.saturating_add(1));
        indices.push(base.saturating_add(2));
        indices.push(base);
        indices.push(base.saturating_add(2));
        indices.push(base.saturating_add(3));
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

/// Component-wise `a + b`, avoiding the glam `+` operator (which trips the
/// workspace `arithmetic_side_effects` lint).
fn add3(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

/// Component-wise `a * s`, avoiding the glam `*` operator (as [`add3`]).
fn scale3(a: Vec3, s: f32) -> Vec3 {
    Vec3::new(a.x * s, a.y * s, a.z * s)
}

/// A tiny deterministic PRNG (SplitMix64) standing in for the reference viewer's
/// `ll_frand`, so the star field is reproducible across runs without pulling in an
/// RNG dependency.
struct StarRng(u64);

impl StarRng {
    /// Seed the generator.
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next 64-bit SplitMix64 output.
    const fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A pseudo-random `f32` in `[0, 1)` (the reference `ll_frand`), from the top
    /// 24 mantissa-worth of bits.
    fn frand(&mut self) -> f32 {
        let bits = self.next_u64() >> 40;
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "24-bit value, exactly representable as f32; scaled to [0, 1)"
        )]
        let value = bits as f32 / 16_777_216.0_f32;
        value
    }
}

/// Build the billboard transform for a heavenly-body disc: a camera-facing quad
/// at [`DISC_DISTANCE`] along `dir`, oriented and sized like the reference
/// `LLVOSky::updateHeavenlyBodyGeometry` (with its near-horizon enlargement).
pub(crate) fn disc_transform(
    camera_pos: Vec3,
    dir: Vec3,
    scale: f32,
    disk_radius: f32,
) -> Transform {
    // Component-wise so the workspace `arithmetic_side_effects` lint (which fires on
    // the glam vector operators) stays happy: `camera_pos + dir * DISC_DISTANCE`.
    let translation = Vec3::new(
        camera_pos.x + dir.x * DISC_DISTANCE,
        camera_pos.y + dir.y * DISC_DISTANCE,
        camera_pos.z + dir.z * DISC_DISTANCE,
    );

    // Billboard basis: `right = dir × up`, `up = right × dir` (the reference's
    // `hb_right` / `hb_up`, with Second Life up = Bevy `y`), and the quad's `+z`
    // normal facing back toward the camera (`-dir`). Seed with `z` near the zenith
    // so the cross products stay well-conditioned.
    let seed = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let right = dir.cross(seed).normalize();
    let up = right.cross(dir).normalize();
    let rotation = Quat::from_mat3(&Mat3::from_cols(
        right,
        up,
        Vec3::new(-dir.x, -dir.y, -dir.z),
    ));

    // Near-horizon enlargement (`enlargm_factor = 1 - dir.z`), then the reference
    // half-extent `scale * distance * factor * disk_radius`.
    let enlarge = 1.0 - dir.y;
    let horiz = 1.0 + enlarge * 0.3;
    let vert = 1.0 + enlarge * 0.2;
    let half = scale * DISC_DISTANCE * HEAVENLY_BODY_FACTOR * disk_radius;

    Transform {
        translation,
        rotation,
        scale: Vec3::new(2.0 * horiz * half, 2.0 * vert * half, 1.0),
    }
}

/// [`Visibility::Visible`] when `up`, else [`Visibility::Hidden`].
const fn visible_if(up: bool) -> Visibility {
    if up {
        Visibility::Visible
    } else {
        Visibility::Hidden
    }
}

/// Build the sky-shader uniform block from a sky frame plus the per-frame light
/// direction, day/night factor, and glow factor.
const fn sky_params(
    sky: &SkySettings,
    lightnorm: Vec3,
    sun_up_factor: f32,
    glow_factor: f32,
) -> SkyParams {
    let sunlight = Vec3::from_array(color_alpha_rgb(sky.sunlight_color));
    SkyParams {
        lightnorm,
        sun_up_factor,
        sunlight_color: sunlight,
        haze_horizon: sky.haze_horizon,
        // The reference shares the sunlight colour for moonlight.
        moonlight_color: sunlight,
        haze_density: sky.haze_density,
        ambient_color: Vec3::from_array(color_rgb(sky.ambient)),
        cloud_shadow: sky.cloud_shadow,
        blue_horizon: Vec3::from_array(color_rgb(sky.blue_horizon)),
        density_multiplier: sky.density_multiplier,
        blue_density: Vec3::from_array(color_rgb(sky.blue_density)),
        distance_multiplier: sky.distance_multiplier,
        glow: glow_vec(sky.glow),
        max_y: sky.max_y,
        sun_moon_glow_factor: glow_factor,
        moisture_level: sky.moisture_level,
        droplet_radius: sky.droplet_radius,
        ice_level: sky.ice_level,
    }
}

/// The sky uniforms for the built-in legacy default sky, used to seed the
/// material before an environment is selected.
pub(crate) fn default_sky_params() -> SkyParams {
    let sky = SkySettings::legacy_windlight_default("Default");
    sky_params(&sky, Vec3::Y, 1.0, 1.0)
}

/// Build the cloud-shader uniform block from a sky frame plus the per-frame light
/// direction, day/night factor, glow factor, and accumulated scroll offset. The
/// scroll is folded into `cloud_pos_density1` the way the reference
/// `LLSettingsVOSky::applySpecial` does (the x offset negated).
pub(crate) fn cloud_params(
    sky: &SkySettings,
    lightnorm: Vec3,
    sun_up_factor: f32,
    glow_factor: f32,
    scroll: Vec2,
) -> CloudParams {
    let sunlight = Vec3::from_array(color_alpha_rgb(sky.sunlight_color));
    let pd1 = sky.cloud_pos_density1;
    let pd2 = sky.cloud_pos_density2;
    CloudParams {
        lightnorm,
        sun_up_factor,
        sunlight_color: sunlight,
        haze_horizon: sky.haze_horizon,
        // The reference shares the sunlight colour for moonlight.
        moonlight_color: sunlight,
        haze_density: sky.haze_density,
        ambient_color: Vec3::from_array(color_rgb(sky.ambient)),
        cloud_shadow: sky.cloud_shadow,
        blue_horizon: Vec3::from_array(color_rgb(sky.blue_horizon)),
        density_multiplier: sky.density_multiplier,
        blue_density: Vec3::from_array(color_rgb(sky.blue_density)),
        max_y: sky.max_y,
        glow: glow_vec(sky.glow),
        sun_moon_glow_factor: glow_factor,
        cloud_color: Vec3::from_array(color_rgb(sky.cloud_color)),
        cloud_scale: sky.cloud_scale,
        cloud_pos_density1: Vec3::new(
            pd1.position_x() - scroll.x,
            pd1.position_y() + scroll.y,
            pd1.density(),
        ),
        cloud_variance: sky.cloud_variance,
        cloud_pos_density2: Vec3::new(pd2.position_x(), pd2.position_y(), pd2.density()),
        blend_factor: 0.0,
    }
}

/// The cloud uniforms for the built-in legacy default sky, used to seed the
/// material before an environment is selected.
pub(crate) fn default_cloud_params() -> CloudParams {
    let sky = SkySettings::legacy_windlight_default("Default");
    cloud_params(&sky, Vec3::Y, 1.0, 1.0, Vec2::ZERO)
}

/// The scene lighting derived from a sky frame — the reference
/// `LLSettingsSky::calculateLightSettings`. The atmosphere attenuates the sun /
/// moon diffuse by altitude and Beer's-law transmittance; the ambient is the
/// sky's own ambient colour.
struct LightSettings {
    /// The sun's atmospheric diffuse colour (the scene light by day).
    sun_diffuse: [f32; 3],
    /// The moon's atmospheric diffuse colour (the scene light by night).
    moon_diffuse: [f32; 3],
    /// The sky's total ambient colour.
    total_ambient: [f32; 3],
}

/// Port of `LLSettingsSky::calculateLightSettings`. `light_up` is the up
/// component of the active light direction (the reference's `lightnorm.z`), and
/// `moon_up` selects the moon-brightness factor. The colour arithmetic stays in
/// per-component `f32` (the workspace `arithmetic_side_effects` lint fires on the
/// glam vector operators).
fn calculate_light_settings(sky: &SkySettings, light_up: f32, moon_up: bool) -> LightSettings {
    let sunlight = color_alpha_rgb(sky.sunlight_color);
    let ambient = color_rgb(sky.ambient);
    let blue_density = color_rgb(sky.blue_density);
    let haze_density = sky.haze_density;
    let density_multiplier = sky.density_multiplier;
    let max_y = sky.max_y;

    // Attenuation (per channel) and Beer's-law transmittance over `max_y`.
    let light_atten = [
        (blue_density[0] + haze_density * 0.25) * density_multiplier * max_y,
        (blue_density[1] + haze_density * 0.25) * density_multiplier * max_y,
        (blue_density[2] + haze_density * 0.25) * density_multiplier * max_y,
    ];
    let transmittance = [
        (-(blue_density[0] + haze_density) * density_multiplier * max_y).exp(),
        (-(blue_density[1] + haze_density) * density_multiplier * max_y).exp(),
        (-(blue_density[2] + haze_density) * density_multiplier * max_y).exp(),
    ];

    // Altitude term: reciprocal of the light's up component (clamped away from 0),
    // so a low sun is attenuated far more than one overhead.
    let mut lighty = light_up.abs();
    if lighty >= LIGHT_UP_LIMIT {
        lighty = 1.0 / lighty;
    }
    lighty = lighty.max(LIGHT_UP_LIMIT);

    let sun_diffuse = [
        sunlight[0] * (-light_atten[0] * lighty).exp() * transmittance[0],
        sunlight[1] * (-light_atten[1] * lighty).exp() * transmittance[1],
        sunlight[2] * (-light_atten[2] * lighty).exp() * transmittance[2],
    ];

    // Moon shares the sunlight colour, scaled by moon brightness.
    let moon_brightness = if moon_up { sky.moon_brightness } else { 0.001 };
    let moon_diffuse = [
        sunlight[0] * (-light_atten[0] * lighty).exp() * transmittance[0] * moon_brightness,
        sunlight[1] * (-light_atten[1] * lighty).exp() * transmittance[1] * moon_brightness,
        sunlight[2] * (-light_atten[2] * lighty).exp() * transmittance[2] * moon_brightness,
    ];

    LightSettings {
        sun_diffuse,
        moon_diffuse,
        total_ambient: ambient,
    }
}

/// The normalised day-cycle position (`0.0..=1.0`) for the current region time,
/// the reference `LLEnvironment::convert_time_to_position`: `fmod(now +
/// day_offset, day_length) / day_length` over the Unix clock.
///
/// The debug override `SL_VIEWER_SKY_DAY_POSITION` (a `0.0..=1.0` float) pins the
/// position instead, so the offline screenshot harness can inspect any point in
/// the day (e.g. midday) regardless of the wall clock.
pub(crate) fn day_position(settings: &sl_client_bevy::EnvironmentSettings) -> f32 {
    if let Ok(value) = std::env::var("SL_VIEWER_SKY_DAY_POSITION")
        && let Ok(position) = value.parse::<f32>()
    {
        return position.clamp(0.0, 1.0);
    }
    // The wrap must be in f64 (the Unix clock overflows f32's integer precision);
    // the result is a normalised fraction in `0.0..1.0`, so narrowing to f32 loses
    // only sub-epsilon precision.
    let day_length = f64::from(settings.day_length.max(1));
    let day_offset = f64::from(settings.day_offset);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64());
    let position = (now + day_offset).rem_euclid(day_length) / day_length;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "a normalised 0.0..1.0 day fraction; the wrap needs f64 but the result fits f32"
    )]
    let fraction = position as f32;
    fraction
}

/// A Second Life [`SlColor`] as a linear RGB triple.
const fn color_rgb(color: SlColor) -> [f32; 3] {
    [color.red(), color.green(), color.blue()]
}

/// A Second Life [`ColorAlpha`] as a linear RGB triple (dropping alpha).
const fn color_alpha_rgb(color: ColorAlpha) -> [f32; 3] {
    [color.red(), color.green(), color.blue()]
}

/// The glow shaping vector as a Bevy [`Vec3`] (`size`, unused middle, `focus`).
const fn glow_vec(glow: Glow) -> Vec3 {
    Vec3::new(glow.size(), glow.reserved(), glow.focus())
}

/// A 1×1 transparent-black placeholder [`Image`] for an overlay texture still in
/// flight.
pub(crate) fn placeholder_image() -> Image {
    Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0, 0, 0, 0],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::{SHADOW_MAP_SIZE, snap_shadow_direction};
    use bevy::math::Vec3;
    use pretty_assertions::assert_eq;

    /// The snapped direction stays a unit vector (a valid light orientation).
    #[test]
    fn snapped_direction_is_unit_length() {
        for dir in [
            Vec3::new(0.1736, 0.9848, 0.0),
            Vec3::new(-0.452, 0.892, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.577, 0.577, 0.577),
        ] {
            let snapped = snap_shadow_direction(dir.normalize());
            assert!(
                (snapped.length() - 1.0).abs() < 1.0e-5,
                "snapped {snapped:?} should be unit length"
            );
        }
    }

    /// Two directions closer than the snap step round to the **same** snapped
    /// direction, so the shadow-caster orientation is bit-stable across the frames
    /// whose true direction drifts within one cell (the R20 shimmer fix).
    #[test]
    fn nearby_directions_snap_together() {
        let step = 1.0 / SHADOW_MAP_SIZE;
        let base = Vec3::new(-0.452, 0.892, 0.0).normalize();
        // A drift a tenth of a step should never cross a cell boundary from the
        // cell centre, so it snaps identically.
        let centre = Vec3::new(
            (base.x / step).round() * step,
            (base.y / step).round() * step,
            (base.z / step).round() * step,
        )
        .normalize();
        let nudged = (centre + Vec3::splat(0.1 * step)).normalize();
        assert_eq!(snap_shadow_direction(centre), snap_shadow_direction(nudged));
    }

    /// The snapped direction never departs the input by more than about one grid
    /// step per component, so the shadow moves at most ~one texel per step.
    #[test]
    fn snap_stays_close_to_input() {
        let step = 1.0 / SHADOW_MAP_SIZE;
        let dir = Vec3::new(0.2, 0.95, -0.24).normalize();
        let snapped = snap_shadow_direction(dir);
        // Bounded by the rounding (half a step per component) plus the small
        // re-normalisation drift.
        assert!(
            (snapped - dir).length() < 2.0 * step,
            "snapped {snapped:?} drifted too far from {dir:?}"
        );
    }
}
