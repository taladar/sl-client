//! Atmospheric sky rendering (P22.2): render the Second Life sky dome from the
//! region's Extended-Environment (EEP) settings and drive the scene's sun / moon
//! light from the same sky.
//!
//! The heavy lifting is a faithful port of the reference viewer's deferred sky
//! shaders ([`SkyMaterial`] / `sky.wgsl`, `LLVOSky` / `class1/deferred/skyV.glsl`
//! + `skyF.glsl`). This module drives that material:
//!
//! - `setup_sky` spawns a large inward-facing dome carrying the sky material,
//!   plus the scene's single directional light (the sun / moon);
//! - `center_sky_on_camera` keeps the dome centred on the camera every frame so
//!   the atmosphere always surrounds the viewpoint;
//! - `drive_sky` selects the active `SkySettings` for the camera's altitude
//!   (the reference `LLEnvironment::calculateSkyTrackForAltitude`), computes the
//!   sun / moon direction and the scene light + ambient the way
//!   `LLSettingsSky::calculateLightSettings` does, and folds them into the sky
//!   material, the directional light, and the ambient light. It also fetches the
//!   sky's rainbow / halo textures **boosted** through the shared texture manager;
//! - `apply_sky_textures` swaps each decoded sky texture into the material.
//!
//! On top of the dome it renders the **sun and moon discs** (P22.3), textured
//! billboards at the computed sun / moon directions (the reference
//! `LLDrawPoolWLSky::renderHeavenlyBodies` / `sunDiscV/F.glsl` + `moonV/F.glsl`):
//!
//! - `setup_sun_moon_discs` spawns the two billboard entities (a shared unit
//!   quad + a [`SunDiscMaterial`] each) and registers `DiscState`;
//! - `drive_sun_moon_discs` aims, scales, colours, and shows / hides each disc
//!   for the active sky frame, and fetches its sun / moon textures **boosted**;
//! - `apply_disc_textures` swaps each decoded disc texture into its material.
//!
//! It also renders the **star field** (P22.5), a sphere of small camera-facing
//! quads that fade in at night with the sky frame's `star_brightness` (the
//! reference `LLDrawPoolWLSky::renderStarsDeferred` / `LLVOWLSky::drawStars`):
//!
//! - `setup_stars` builds the 1000-star quad mesh and spawns it with a
//!   [`StarMaterial`] (initially hidden) and registers `StarState`;
//! - `drive_stars` centres and slowly rotates the field on the camera, folds
//!   `star_brightness` and the twinkle time into the material, shows / hides the
//!   field for the active sky frame, and fetches its bloom texture **boosted**;
//! - `apply_star_textures` swaps the decoded bloom texture into the material.
//!
//! Every frame the sky, discs, clouds, and stars pull the **blended**
//! `SkySettings` for the current region time
//! (`EnvironmentSettings::blended_sky_settings`) — the smooth interpolation
//! between the two day-cycle keyframes bounding the moment (P22.6), so the
//! atmosphere and the sun / moon animate continuously through the day rather
//! than snapping between keyframes.
//!
//! The moment they pull it for is `day_position`, which quantises the day to
//! `DAY_POSITION_STEPS` sampling cells. That is what every write-on-change
//! guard in the scene rests on: those guards are float equality on values
//! derived from the sky frame, so a position that advanced with the wall clock
//! made each of them miss on every single frame — including
//! `drive_terrain_lighting`, which then re-prepared every region's terrain
//! material forever. The cell is finer than the shadow-caster direction snap,
//! so nothing visible steps that was not already stepping.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{CascadeShadowConfig, CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sl_client_bevy::{
    CloudMaterial, CloudParams, Color as SlColor, ColorAlpha, DecodedTexture, Glow, SkyMaterial,
    SkyParams, SkySettings, StarMaterial, StarParams, SunDiscMaterial, SunDiscParams, TextureKey,
    Uuid, to_bevy_image,
};

use crate::coords::sl_to_bevy_object_rotation;
use crate::environment::EnvironmentState;
use crate::probe_layers::{environment_render_layers, mirror_sun_render_layers};
use crate::textures::{TextureDecoded, TextureManager};
use crate::transparency::SkyBackdrop;
use crate::world_api::{DecodedTextures, SKY_BOOST_PRIORITY, ViewerCamera, WorldPhase};

/// The sky stack's own scheduling: the dome, the sun / moon discs, the cloud
/// layer and the star field, spawned at `Startup` and driven every frame.
///
/// Everything that must see the finished viewpoint orders itself against
/// [`WorldPhase::CameraPositioned`] rather than naming the system that writes
/// it — which is what lets the sky live below the camera that drives it, and
/// lets the dome / disc / cloud / star markers and their state resources stay
/// private to this crate.
#[derive(Debug, Default)]
pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        // The ambient before any sky resolves, stated rather than left at Bevy's
        // default 80 nits: this crate's lighting model is that the reflection probe
        // supplies the ambient (`probes::probe_ambient_scale`), so a world with no
        // sky yet — between login and the first `EnvironmentSettings`, or in a
        // region whose environment never arrives — must not flash a flat fill the
        // sky would then take away. `drive_sky` owns the value from its first frame.
        app.insert_resource(GlobalAmbientLight {
            brightness: 0.0,
            ..default()
        })
        .add_systems(
            Startup,
            (setup_sky, setup_sun_moon_discs, setup_clouds, setup_stars),
        )
        .add_systems(
            Update,
            (
                // Keep the dome centred on the camera, then fold the region
                // environment + camera altitude into the sky material, the sun /
                // moon directional light, and the ambient light, and swap each
                // decoded sky overlay texture into the material.
                center_sky_on_camera.after(WorldPhase::CameraPositioned),
                drive_sky.after(WorldPhase::CameraPositioned),
                apply_sky_textures,
                // Sun / moon discs (P22.3): aim and colour the billboards from the
                // same active sky frame, then swap each decoded disc texture in.
                drive_sun_moon_discs.after(WorldPhase::CameraPositioned),
                apply_disc_textures,
                // Cloud layer (P22.4): fold the same active sky frame into the
                // cloud material, accumulate the scroll, and swap in the noise.
                drive_clouds.after(WorldPhase::CameraPositioned),
                apply_cloud_textures,
                // Star field (P22.5): centre / rotate the field on the camera,
                // fade it in with `star_brightness`, and swap in the bloom.
                drive_stars.after(WorldPhase::CameraPositioned),
                apply_star_textures,
            ),
        );
    }
}

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

/// The [`GlobalAmbientLight`] a sky's total ambient asks for: its luminance sets the
/// fill strength, its (normalised) hue the tint, and `probe_scale`
/// ([`crate::probes::probe_ambient_scale`]) says how much of that flat fill survives
/// once the reflection probe is supplying image-based ambient of its own.
///
/// The probe suppression belongs *here*, in the absolute value the sky writes, and
/// not in a later system that multiplies the resource down: an attenuation applied
/// to whatever the resource holds compounds every frame the sky does not rewrite it
/// (`drive_sky` early-returns whenever no sky frame resolves), and it also makes the
/// caller's write-on-change guard compare the sky's value against a scaled one, so
/// the guard misses and the resource is dirty every frame. Neither showed while the
/// scale sat at its idempotent `0.0` default.
fn sky_ambient_light(ambient: [f32; 3], probe_scale: f32) -> (Color, f32) {
    let luminance = 0.2126 * ambient[0] + 0.7152 * ambient[1] + 0.0722 * ambient[2];
    let peak = ambient[0].max(ambient[1]).max(ambient[2]).max(1.0e-4);
    let color = Color::linear_rgb(ambient[0] / peak, ambient[1] / peak, ambient[2] / peak);
    (color, luminance * AMBIENT_BRIGHTNESS_SCALE * probe_scale)
}

/// Read the `SL_VIEWER_SHADOW_CASCADES` experiment env: how many sun shadow
/// cascades to build (clamped `1..=4`; `None` when unset, so the stored
/// `RenderShadowCascades` preference drives it instead — the env, when set,
/// **wins** over the preference like the tonemap / glow overrides). The
/// per-frame shadow-caster cull (`check_dir_light_mesh_visibility`, ungated)
/// and the shadow-map render both scale with the cascade count × caster count,
/// so cutting cascades isolates how much the shadow *view count* costs — an
/// entity/view lever distinct from the sun-movement churn one.
///
/// Resolved once per process: the environment is fixed at launch, and this is read
/// from a per-frame preference-apply system.
#[must_use]
pub fn shadow_cascade_count() -> Option<usize> {
    static COUNT: OnceLock<Option<usize>> = OnceLock::new();
    *COUNT.get_or_init(|| {
        std::env::var("SL_VIEWER_SHADOW_CASCADES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .map(|count| count.clamp(1, 4))
    })
}

/// Cascaded-shadow-map coverage for the scene sun / moon (P24.1). Tuned to a
/// Second Life region's scale (256 m): the last cascade reaches to a region's
/// diagonal (~362 m) so an avatar's shadow, nearby prims, and terrain relief all
/// receive the sun, while the first (near) cascade is kept tight so avatar-close
/// detail gets most of the shadow-map resolution. The reference
/// `LLPipeline::renderShadow` uses four split sun cascades likewise.
pub(crate) fn shadow_cascades() -> CascadeShadowConfig {
    shadow_cascades_for(shadow_cascade_count().unwrap_or(4))
}

/// `shadow_cascades` with an explicit cascade count (clamped `1..=4`): the
/// builder body shared by the spawn-time default above and the
/// `RenderShadowCascades` preference applier, which rebuilds the sun's
/// [`CascadeShadowConfig`] when the stored count changes.
#[must_use]
pub fn shadow_cascades_for(count: usize) -> CascadeShadowConfig {
    CascadeShadowConfigBuilder {
        num_cascades: count.clamp(1, 4),
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

/// How often [`drive_clouds`] re-anchors the GPU-integrated cloud scroll while a
/// rate is active. `clouds.wgsl` unwinds its hourly-wrapping `globals.time`
/// across **one** wrap (the face_material pattern), so the anchor must be
/// refreshed well within an hour; half the wrap period leaves ample margin, at
/// one material re-prepare per half hour.
const CLOUD_SCROLL_REANCHOR_SECS: f32 = 1800.0;

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

/// The quantisation step for the star-field rotation, in degrees: the [`drive_stars`]
/// Transform write rounds the [`STAR_ROTATION_RATE_DEG`] drift down to this grid
/// (one step every ~5 s) so the Transform settles between steps instead of being
/// marked changed every frame; 0.05° of star-field yaw is imperceptible.
/// (The reference twinkle-time scale — `sStarTime = getElapsedSeconds() * 0.5` —
/// now lives in `stars.wgsl` as `STAR_TIME_SCALE`, applied to `globals.time`.)
const STAR_ROTATION_STEP_DEG: f32 = 0.05;

/// The seed for the deterministic star-placement PRNG, so the star field is
/// identical across runs (the reference seeds from the global `ll_frand`).
const STAR_RNG_SEED: u64 = 0x5142_4152_5354_4152;

/// Marks the sky-dome entity so [`center_sky_on_camera`] can follow the camera.
#[derive(Debug, Component)]
pub(crate) struct SkyDome;

/// Marks the scene's sun / moon directional light so `drive_sky` can aim and
/// colour it from the sky.
#[derive(Debug, Component)]
pub struct SceneSun;

/// Marks the **shadow-free mirror sun** — a second directional light that copies
/// [`SceneSun`]'s direction and colour every frame but casts **no** shadows and
/// sits on the reflection-probe render layers only
/// ([`mirror_sun_render_layers`]).
///
/// It exists so reflection-probe capture cameras (which render the probe layers,
/// not the main layer) are still lit by the sun without Bevy building — and, each
/// capture cycle, re-specializing — a full set of sun shadow cascades for every
/// capture camera, which was the periodic frame-stall this split removes
/// (viewer-perf-pipeline-specialization-stalls). The real [`SceneSun`] stays on
/// the main layer with shadows on, so the main view is unchanged.
#[derive(Debug, Component)]
pub(crate) struct SceneSunMirror;

/// The viewer's sky-render state: the shared sky material and the decoded /
/// requested rainbow / halo overlay textures.
#[derive(Debug, Resource)]
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
#[derive(Debug, Component)]
pub(crate) struct SunDisc;

/// Marks the moon-disc billboard entity so [`drive_sun_moon_discs`] can aim it.
#[derive(Debug, Component)]
pub(crate) struct MoonDisc;

/// The viewer's sun / moon disc state: the two disc materials and the disc
/// textures currently requested for them.
#[derive(Debug, Resource)]
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
#[derive(Debug, Component)]
pub(crate) struct CloudDome;

/// The viewer's cloud-layer state: the cloud material, the requested cloud-noise
/// texture, and the accumulated scroll offset.
#[derive(Debug, Resource)]
pub(crate) struct CloudState {
    /// The single cloud-dome material, updated each frame by [`drive_clouds`].
    material: Handle<CloudMaterial>,
    /// The texture id currently requested for the cloud noise (the active sky
    /// frame's, or the built-in [`DEFAULT_CLOUD_ID`]).
    cloud_key: Option<TextureKey>,
    /// The current scroll rate, in offset units per second (the sky frame's
    /// `cloud_scroll_rate` over the reference divisor) — the value uploaded as
    /// `CloudParams::scroll_rate`. The scroll itself (the reference
    /// `LLEnvironment::mCloudScrollDelta`) is integrated **GPU-side** from
    /// `globals.time`, so a steadily drifting layer never dirties the material;
    /// the CPU only re-anchors on a rate change (or before the shader clock's
    /// hourly wrap could double-wrap).
    scroll_rate: Vec2,
    /// The accumulated scroll offset at the anchor (uploaded as
    /// `CloudParams::scroll_base`). Persists across sky-frame changes, like the
    /// reference; reset to zero when the rate goes to zero (also the reference).
    scroll_base: Vec2,
    /// `Time::elapsed_secs` at the anchor, for the accumulated-offset fold and
    /// the periodic re-anchor.
    scroll_anchor_elapsed: f32,
    /// `Time::elapsed_wrapped` at the anchor — the shader-clock (`globals.time`)
    /// value uploaded as `CloudParams::scroll_ref_time`.
    scroll_ref_time: f32,
    /// The next time (`Time::elapsed_secs`) the opt-in cloud-param debug log
    /// (`SL_VIEWER_LOG_CLOUDS`) may fire, throttling it to a readable cadence.
    next_log_at: f32,
}

/// Marks the star-field entity so [`drive_stars`] can centre / rotate it.
#[derive(Debug, Component)]
pub(crate) struct StarField;

/// The viewer's star-field state: the star material and the bloom texture
/// currently requested for it.
#[derive(Debug, Resource)]
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
        environment_render_layers(),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: SCENE_LIGHT_ILLUMINANCE,
            // P24.1: cast cascaded shadow maps from the sun / moon. Disabled by the
            // `SL_VIEWER_SUN_SHADOWS=0` experiment env to measure the total
            // per-frame cost of the directional-shadow subsystem (the ungated
            // caster cull plus the shadow-map render).
            shadow_maps_enabled: sun_shadows_enabled(),
            ..default()
        },
        // Cascades tuned to region scale so shadows cover an avatar plus nearby
        // prims and terrain (`drive_sky` keeps the direction on the active body).
        shadow_cascades(),
        Transform::default().looking_to(Vec3::new(-0.4, -1.0, -0.3), Vec3::Y),
        SceneSun,
    ));
    // The shadow-free mirror sun: lights reflection-probe captures without
    // building shadow cascades for their cameras (see [`SceneSunMirror`]). It
    // carries no `CascadeShadowConfig` and `shadow_maps_enabled = false`, and
    // renders only on the probe layers, so it never touches the main view.
    commands.spawn((
        DirectionalLight {
            illuminance: SCENE_LIGHT_ILLUMINANCE,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-0.4, -1.0, -0.3), Vec3::Y),
        mirror_sun_render_layers(),
        SceneSunMirror,
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
        // Only write when the camera actually moved: an unconditional write
        // marks the dome `Transform` changed every frame and re-propagates /
        // re-extracts both domes even with a parked camera.
        if transform.translation != translation {
            transform.translation = translation;
        }
    }
}

/// Whether the `SL_VIEWER_LOG_SKY_HDR` diagnostic is on: the sky's "fake HDR"
/// scale, the sun's altitude, and the decoded sun-disc texture are logged as they
/// change. Resolved once per process — the gate is tested from three per-frame
/// sites, and the environment is fixed at launch.
fn log_sky_hdr() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("SL_VIEWER_LOG_SKY_HDR").is_some())
}

/// Whether the `SL_VIEWER_LOG_CLOUDS` diagnostic is on: the EEP cloud settings and
/// the resolved cloud-noise texture are logged (throttled) for an A/B against
/// Firestorm. Resolved once per process — the gate is tested from three per-frame
/// sites, and the environment is fixed at launch.
fn log_clouds() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("SL_VIEWER_LOG_CLOUDS").is_some())
}

/// Whether the sun casts shadows (`SL_VIEWER_SUN_SHADOWS`, default on): set to
/// `0` to disable `shadow_maps_enabled` on [`SceneSun`] at spawn, so an A/B
/// (frame time via Tracy or the status bar) measures the total per-frame cost of
/// the directional-shadow subsystem. That cost is the more decisive number than the
/// sun-churn slice, because the shadow-caster cull runs every frame regardless
/// of sun movement.
///
/// Resolved once per process: the environment is fixed at launch, and this is read
/// from a per-frame preference-apply system.
#[must_use]
pub fn sun_shadows_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        !matches!(
            std::env::var("SL_VIEWER_SUN_SHADOWS").ok().as_deref(),
            Some("0")
        )
    })
}

/// Fold the current environment + camera altitude into the sky material, the
/// directional light, and the ambient light, and (re)request the sky's rainbow /
/// halo overlay textures boosted.
///
/// Every write is guarded on an actual value change (the texture_anim
/// compare-then-`get_mut` idiom): an unguarded sun-`Transform` write dirties
/// [`SceneSun`] via `Mut` change detection even when R20's texel-snap left the
/// direction unchanged, which recomputes the four shadow cascades and re-culls
/// every outdoor caster each frame; the material / ambient / exposure writes
/// likewise re-prepared or re-flagged their targets per frame. Under a static
/// environment and camera this system now writes nothing; under a live day cycle
/// it writes once per day-cycle sampling step ([`DAY_POSITION_STEPS`]) rather
/// than once per frame, because every one of those guards is float equality on a
/// value derived from the sampled day position.
///
/// That holds for the ambient because the write is the *whole* value the frame
/// asks for, reflection-probe suppression included ([`sky_ambient_light`]). A
/// second system scaling [`GlobalAmbientLight`] afterwards would break the guard
/// here — the sky would compare its own value against a scaled one and rewrite
/// every frame — as well as decaying the resource whenever this system
/// early-returns.
#[expect(
    clippy::type_complexity,
    reason = "one query over both directional lights (shadow sun + shadow-free mirror)"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the sky \
              material, both suns, the ambient light, and the exposure inputs"
)]
pub(crate) fn drive_sky(
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
    environment: Res<EnvironmentState>,
    mut state: ResMut<SkyState>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut textures: ResMut<TextureManager>,
    // Both the shadow-casting [`SceneSun`] and the shadow-free
    // [`SceneSunMirror`]; both take the texel-snapped direction, so both settle
    // between snap steps (the mirror only lights reflection-probe captures,
    // where a sub-texel angular step is invisible).
    mut suns: Query<
        (&mut Transform, &mut DirectionalLight),
        Or<(With<SceneSun>, With<SceneSunMirror>)>,
    >,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut exposure_range: ResMut<crate::exposure::ExposureRange>,
) {
    let altitude = camera.single().map_or(0.0, |camera| camera.translation().y);
    let position = day_position(&environment.settings);
    let Some(sky) = environment
        .settings
        .blended_sky_settings(altitude, position)
    else {
        return;
    };

    // Publish the active sky's dynamic-exposure inputs (the `sky_hdr_scale`
    // counterweight) so the exposure pass tracks the exact frame drawn here rather
    // than re-deriving the altitude-blended sky; `refresh_exposure` resolves them
    // into a range with the live settings. `can_auto_adjust` is the reference's
    // `mCanAutoAdjust` — true for a legacy sky, which our decode collapses to
    // `reflection_probe_ambiance == 0` (an EEP sky that explicitly authors an ambiance
    // of exactly 0 is declaring "no HDR", so treating it as legacy is behaviourally
    // identical: both stay inert).
    let new_range = crate::exposure::ExposureRange {
        reflection_probe_ambiance: sky.reflection_probe_ambiance,
        gamma: sky.gamma,
        can_auto_adjust: sky.reflection_probe_ambiance == 0.0,
    };
    if *exposure_range != new_range {
        *exposure_range = new_range;
    }

    // Every derivation this system used to do inline. See `ResolvedSky`.
    let resolved = resolve_sky(&sky);
    let light_dir = resolved.light_dir;
    let diffuse = resolved.diffuse;

    // The texture_anim idiom: read-only compare, `get_mut` (and so a material
    // re-prepare) only when the resolved params actually changed.
    if materials
        .get(&state.material)
        .is_some_and(|material| material.params != resolved.params)
        && let Some(mut material) = materials.get_mut(&state.material)
    {
        material.params = resolved.params;
    }

    // The light travels *toward* its forward axis, i.e. away from the body, so
    // its forward is the negated light direction. Both suns snap the direction
    // to a texel-equivalent angular grid first (R20): the real-time day cycle
    // rotates the sun a hair every frame, which rotates the cascaded shadow
    // map's light-space texel grid and makes the ground shadows shimmer — Bevy
    // texel-snaps the cascade origin, but a per-frame-rotating light defeats
    // it. Snapping holds the direction bit-stable between steps, which also
    // lets the `set_if_neq` below skip the write (and the shadow-cascade
    // rebuild it would trigger) entirely between steps. The shadow-free mirror
    // (probe-capture lighting only) takes the same snapped direction so it
    // settles too — a sub-texel step is invisible in a probe. The visible sun
    // disc, sky, and light colour still use the un-snapped direction. Pick a
    // safe up when the body is near the zenith (forward near-parallel to +Y).
    let dir = snap_shadow_direction(light_dir);
    let forward = Vec3::new(-dir.x, -dir.y, -dir.z);
    let up = if forward.dot(Vec3::Y).abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let sun_transform = Transform::default().looking_to(forward, up);
    let sun_color = Color::linear_rgb(
        diffuse[0].clamp(0.0, 1.0),
        diffuse[1].clamp(0.0, 1.0),
        diffuse[2].clamp(0.0, 1.0),
    );
    for (mut transform, mut light) in &mut suns {
        transform.set_if_neq(sun_transform);
        if light.color != sun_color {
            light.color = sun_color;
        }
    }

    // Ambient from the sky's total ambient, already carrying the reflection probe's
    // share of it — see `sky_ambient_light`.
    let (ambient_color, ambient_brightness) =
        sky_ambient_light(resolved.ambient, crate::probes::probe_ambient_scale());
    if ambient.color != ambient_color
        || ambient.brightness.to_bits() != ambient_brightness.to_bits()
    {
        ambient.color = ambient_color;
        ambient.brightness = ambient_brightness;
    }

    // Fetch the sky's referenced rainbow / halo textures boosted (the sky frame's
    // own, or the reference built-ins) so they resolve ahead of ordinary faces.
    // Only on a key change: the boost request is persistent in the store, and an
    // unconditional re-request every frame marks both `TextureManager` and
    // `SkyState` changed with identical values.
    let rainbow_key = Some(
        sky.rainbow_texture
            .unwrap_or_else(|| TextureKey::from(IMG_RAINBOW)),
    );
    let halo_key = Some(
        sky.halo_texture
            .unwrap_or_else(|| TextureKey::from(IMG_HALO)),
    );
    if state.rainbow_key != rainbow_key {
        if let Some(key) = rainbow_key {
            textures.request_boosted(key, SKY_BOOST_PRIORITY);
        }
        state.rainbow_key = rainbow_key;
    }
    if state.halo_key != halo_key {
        if let Some(key) = halo_key {
            textures.request_boosted(key, SKY_BOOST_PRIORITY);
        }
        state.halo_key = halo_key;
    }
}

/// Swap a decoded sky texture into the material when its rainbow / halo id
/// resolves.
pub(crate) fn apply_sky_textures(
    mut decoded: MessageReader<TextureDecoded>,
    state: Res<SkyState>,
    store: Res<DecodedTextures>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for &TextureDecoded(id) in decoded.read() {
        let is_rainbow = state.rainbow_key == Some(id);
        let is_halo = state.halo_key == Some(id);
        if !is_rainbow && !is_halo {
            continue;
        }
        let Some(decoded) = store.get(id) else {
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
            // Seeded at the legacy no-op; `drive_sun_moon_discs` sets the active
            // sky frame's scale each frame.
            sky_hdr_scale: 1.0,
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
            sky_hdr_scale: 1.0,
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
        // A sky backdrop, so the discs sort behind world-anchored transparent
        // overlays (name tags and the like) instead of by their 2000 m distance.
        SkyBackdrop::HeavenlyBody,
        environment_render_layers(),
    ));
    commands.spawn((
        Mesh3d(quad),
        MeshMaterial3d(moon_material.clone()),
        Transform::default(),
        Visibility::Hidden,
        NotShadowCaster,
        MoonDisc,
        SkyBackdrop::HeavenlyBody,
        environment_render_layers(),
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
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries, plus two `Local` \
              accumulators for the env-gated on-change sky/sun diagnostic"
)]
pub(crate) fn drive_sun_moon_discs(
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
    environment: Res<EnvironmentState>,
    mut state: ResMut<DiscState>,
    mut materials: ResMut<Assets<SunDiscMaterial>>,
    mut textures: ResMut<TextureManager>,
    mut sun: Query<(&mut Transform, &mut Visibility), (With<SunDisc>, Without<MoonDisc>)>,
    mut moon: Query<(&mut Transform, &mut Visibility), (With<MoonDisc>, Without<SunDisc>)>,
    mut last_logged_hdr: Local<Option<f32>>,
    mut last_logged_sun_y: Local<Option<f32>>,
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
    // horizon (`getIsSunUp` / `getIsMoonUp`). `set_if_neq` throughout: with a
    // parked camera and a fixed sky nothing here changes, and an unconditional
    // write would re-extract both discs every frame.
    if let Ok((mut transform, mut vis)) = sun.single_mut() {
        if sun_up {
            transform.set_if_neq(disc_transform(
                camera_pos,
                sun_dir,
                sky.sun_scale,
                SUN_DISK_RADIUS,
            ));
        }
        vis.set_if_neq(visible_if(sun_up));
    }
    if let Ok((mut transform, mut vis)) = moon.single_mut() {
        if moon_up {
            transform.set_if_neq(disc_transform(
                camera_pos,
                moon_dir,
                sky.moon_scale,
                MOON_DISK_RADIUS,
            ));
        }
        vis.set_if_neq(visible_if(moon_up));
    }

    // The sun disc is untinted (the reference `sunDiscF` ignores its bound diffuse
    // colour); the moon disc is scaled by the sky's moon brightness and faded near
    // the horizon by its up component (`moonF`). Both are scaled by the sky's
    // "fake HDR" factor so they sit in the same range as the sky dome behind them
    // (1.0 for a legacy sky; > 1.0 for an EEP probe-ambiance sky, so the disc
    // blows out instead of rendering a flat grey).
    let hdr_scale = resolved_sky_hdr_scale(&sky);
    // On-change diagnostic (env-gated, matching `SL_VIEWER_LOG_CLOUDS`): confirm
    // whether the active sky is on the EEP "fake HDR" path — a non-zero
    // `reflection_probe_ambiance` gives `sky_hdr_scale > 1.0`. A legacy sky logs
    // `1.0` (a no-op).
    if log_sky_hdr() && *last_logged_hdr != Some(hdr_scale) {
        *last_logged_hdr = Some(hdr_scale);
        info!(
            "sky hdr: reflection_probe_ambiance={:.4} gamma={:.4} sky_hdr_scale={:.4}",
            sky.reflection_probe_ambiance, sky.gamma, hdr_scale
        );
    }
    // Log the sun's altitude (Bevy y = up component) when it moves — for
    // comparing where a World ▸ Environment selection puts the sun (a Day Cycle
    // frame sampled from the region's own cycle vs an authored Legacy / Modern
    // preset). `sun_up` says whether the disc is drawn at all.
    if log_sky_hdr() && last_logged_sun_y.is_none_or(|prev| (prev - sun_dir.y).abs() > 0.02) {
        *last_logged_sun_y = Some(sun_dir.y);
        info!(
            "sun position: selection={:?} up_component={:.3} (sun_up={sun_up}) sun_dir=({:.3},{:.3},{:.3})",
            environment.fixed(),
            sun_dir.y,
            sun_dir.x,
            sun_dir.y,
            sun_dir.z
        );
    }
    // Compare-then-`get_mut` (the texture_anim idiom): a disc material is only
    // re-prepared when its params actually changed.
    let sun_params = materials.get(&state.sun_material).map(|material| {
        let mut params = material.params;
        params.up_component = sun_dir.y;
        params.sky_hdr_scale = hdr_scale;
        (params, params != material.params)
    });
    if let Some((params, true)) = sun_params
        && let Some(mut material) = materials.get_mut(&state.sun_material)
    {
        material.params = params;
    }
    let moon_params = materials.get(&state.moon_material).map(|material| {
        let mut params = material.params;
        params.brightness = sky.moon_brightness;
        params.up_component = moon_dir.y;
        params.sky_hdr_scale = hdr_scale;
        (params, params != material.params)
    });
    if let Some((params, true)) = moon_params
        && let Some(mut material) = materials.get_mut(&state.moon_material)
    {
        material.params = params;
    }

    // Fetch the disc textures boosted (the sky frame's own, or the reference
    // built-ins) so they resolve ahead of ordinary faces. Only on a key change:
    // the boost request is persistent in the store, and re-requesting per frame
    // marks `TextureManager` and `DiscState` changed with identical values.
    let sun_key = sky
        .sun_texture
        .unwrap_or_else(|| TextureKey::from(DEFAULT_SUN_ID));
    let moon_key = sky
        .moon_texture
        .unwrap_or_else(|| TextureKey::from(DEFAULT_MOON_ID));
    if state.sun_key != Some(sun_key) {
        textures.request_boosted(sun_key, SKY_BOOST_PRIORITY);
        state.sun_key = Some(sun_key);
    }
    if state.moon_key != Some(moon_key) {
        textures.request_boosted(moon_key, SKY_BOOST_PRIORITY);
        state.moon_key = Some(moon_key);
    }
}

/// Swap a decoded disc texture into the sun / moon material when its id resolves.
pub(crate) fn apply_disc_textures(
    mut decoded: MessageReader<TextureDecoded>,
    state: Res<DiscState>,
    store: Res<DecodedTextures>,
    mut materials: ResMut<Assets<SunDiscMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for &TextureDecoded(id) in decoded.read() {
        let is_sun = state.sun_key == Some(id);
        let is_moon = state.moon_key == Some(id);
        if !is_sun && !is_moon {
            continue;
        }
        let Some(decoded) = store.get(id) else {
            // The fetch/decode failed; the disc keeps its (transparent) placeholder.
            continue;
        };
        // Diagnostic (env-gated) for the grey sun-disc investigation
        // (viewer-sun-disc-grey-aditi-hdr-scale): dump the decoded disc's
        // dimensions, source component count (3 = no alpha ⇒ opaque, 4 = RGBA),
        // and a few RGBA samples (centre / mid-radius / corner). The reference sun
        // texture is a soft, low-alpha glow that lets the bright near-sun haze show
        // through; if ours decodes as a hard opaque disc it reads as a grey hole.
        if log_sky_hdr() {
            let (w, h) = (decoded.width, decoded.height);
            // Integer texel fetch (no `as` casts, per the workspace lints): clamp
            // the requested texel into range and index the RGBA8 buffer.
            let texel = |x: u32, y: u32| -> [u8; 4] {
                let x = x.min(w.saturating_sub(1));
                let y = y.min(h.saturating_sub(1));
                let (Ok(x), Ok(y), Ok(w)) =
                    (usize::try_from(x), usize::try_from(y), usize::try_from(w))
                else {
                    return [0, 0, 0, 0];
                };
                let off = y.saturating_mul(w).saturating_add(x).saturating_mul(4);
                decoded
                    .pixels
                    .get(off..off.saturating_add(4))
                    .and_then(|s| <[u8; 4]>::try_from(s).ok())
                    .unwrap_or([0, 0, 0, 0])
            };
            // `checked_div` (not bare `/`) to satisfy the workspace
            // `arithmetic_side_effects` lint: centre, ~0.85-radius, and a corner.
            let half_w = w.checked_div(2).unwrap_or(0);
            let half_h = h.checked_div(2).unwrap_or(0);
            let mid_h = h.saturating_mul(17).checked_div(20).unwrap_or(0);
            let edge_w = w.checked_div(50).unwrap_or(0);
            let edge_h = h.checked_div(50).unwrap_or(0);
            info!(
                "disc texture {}: {w}x{h} components={} centre={:?} mid={:?} corner={:?}",
                if is_sun { "SUN" } else { "MOON" },
                decoded.components,
                texel(half_w, half_h),
                texel(half_w, mid_h),
                texel(edge_w, edge_h),
            );
        }
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
        // A sky backdrop: the dome is centred on the camera, so Bevy's distance sort
        // would make it the *nearest* transparent object and draw it over every
        // world-anchored overlay in front of it (`viewer-nametags-occluded-by-clouds`).
        SkyBackdrop::Clouds,
        environment_render_layers(),
    ));
    commands.insert_resource(CloudState {
        material,
        cloud_key: None,
        scroll_rate: Vec2::ZERO,
        scroll_base: Vec2::ZERO,
        scroll_anchor_elapsed: 0.0,
        scroll_ref_time: 0.0,
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

    // The cloud scroll (`LLEnvironment::updateCloudScroll`) is integrated
    // GPU-side from `globals.time`; the CPU only maintains the anchor. Re-anchor
    // when the rate changes (fold the offset accumulated so far into the base so
    // the layer does not jump — or reset to zero when the rate goes to zero,
    // like the reference) and periodically, so the shader's single-wrap unwind
    // of its hourly-wrapping clock always suffices.
    let [rate_x, rate_y] = sky.cloud_scroll_rate;
    let rate = if rate_x == 0.0 && rate_y == 0.0 {
        Vec2::ZERO
    } else {
        // Per-component `f32` arithmetic (the glam vector operators trip the
        // workspace `arithmetic_side_effects` lint).
        Vec2::new(rate_x / CLOUD_SCROLL_DIVISOR, rate_y / CLOUD_SCROLL_DIVISOR)
    };
    let elapsed = time.elapsed_secs();
    let overdue = elapsed - state.scroll_anchor_elapsed > CLOUD_SCROLL_REANCHOR_SECS;
    if rate != state.scroll_rate || (overdue && rate != Vec2::ZERO) {
        let dt_anchor = elapsed - state.scroll_anchor_elapsed;
        let accumulated = Vec2::new(
            state.scroll_base.x + state.scroll_rate.x * dt_anchor,
            state.scroll_base.y + state.scroll_rate.y * dt_anchor,
        );
        state.scroll_base = if rate == Vec2::ZERO {
            Vec2::ZERO
        } else {
            accumulated
        };
        state.scroll_rate = rate;
        state.scroll_anchor_elapsed = elapsed;
        state.scroll_ref_time = time.elapsed_wrapped().as_secs_f32();
    }

    // Compare-then-`get_mut` (the texture_anim idiom): with a static sky and a
    // stable anchor the params are identical every frame, so a steadily
    // scrolling cloud layer re-prepares nothing — and with a live day cycle they
    // are identical between day-cycle sampling steps (`DAY_POSITION_STEPS`),
    // which is the only reason this float-equality compare ever holds on a grid.
    let params = cloud_params(
        &sky,
        resolved.lightnorm,
        resolved.sun_up_factor,
        resolved.glow_factor,
        state.scroll_ref_time,
        state.scroll_rate,
        state.scroll_base,
    );
    if materials
        .get(&state.material)
        .is_some_and(|material| material.params != params)
        && let Some(mut material) = materials.get_mut(&state.material)
    {
        material.params = params;
    }

    // Fetch the sky's cloud-noise texture boosted (the sky frame's own, or the
    // reference built-in) so it resolves ahead of ordinary faces. Only on a key
    // change — the boost request is persistent in the store.
    let cloud_key = sky
        .cloud_texture
        .unwrap_or_else(|| TextureKey::from(DEFAULT_CLOUD_ID));
    if state.cloud_key != Some(cloud_key) {
        textures.request_boosted(cloud_key, SKY_BOOST_PRIORITY);
        state.cloud_key = Some(cloud_key);
    }

    // Opt-in cloud-param diagnostic (`SL_VIEWER_LOG_CLOUDS`): dump the EEP cloud
    // settings + the resolved cloud-noise texture id so a live aditi session can be
    // compared against Firestorm (R18 — the cloud distribution mismatch). Throttled
    // to ~2 s; purely a log, no rendering effect.
    if time.elapsed_secs() >= state.next_log_at && log_clouds() {
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
    store: Res<DecodedTextures>,
    mut materials: ResMut<Assets<CloudMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for &TextureDecoded(id) in decoded.read() {
        if state.cloud_key != Some(id) {
            continue;
        }
        let Some(decoded) = store.get(id) else {
            // The fetch/decode failed; the layer keeps its (transparent) placeholder.
            if log_clouds() {
                warn!("cloud texture {id:?} fetch/decode FAILED (using placeholder)");
            }
            continue;
        };
        if log_clouds() {
            info!(
                "cloud texture {id:?} decoded ({}x{}, {} components)",
                decoded.width, decoded.height, decoded.components
            );
        }
        let handle = images.add(cloud_noise_image(decoded));
        if let Some(mut material) = materials.get_mut(&state.material) {
            // Both noise slots share the id until the day cycle (P22.6) drives a
            // separate next-frame texture and the blend factor between them.
            material.cloud_noise = handle.clone();
            material.cloud_noise_next = handle;
        }
    }
}

/// Upload a decoded cloud-noise texture: **linear**, and tiling (R18).
///
/// Both halves are load-bearing. The noise is *data*, not colour: `clouds.wgsl`
/// ports `cloudsF.glsl`, whose density term is `cloudNoise(uv).x - 0.5` on the
/// raw byte values — the reference binds the noise as a plain `GL_RGBA8`
/// texture (`llvosky.cpp` even calls `setExplicitFormat(GL_RGBA8, GL_RGBA)`)
/// and its shader has no `srgb_to_linear`. Uploading through `to_bevy_image`
/// (which is `Rgba8UnormSrgb`-only, the same trap the normal-map uploaders
/// document) had the GPU sRGB-decode every sample, pushing a mid-gray byte 128
/// down to 0.216: with the default cloud texture only ~9% of texels cleared the
/// `alpha1 > 0` density threshold instead of ~46%, and the survivors clustered
/// in a few isolated blobs — the "clouds in one quadrant, rest empty" defect.
///
/// The sampler must repeat because `cloud_scale` magnifies the UVs and the
/// scroll offsets push them well outside `[0, 1]` (the reference samples with
/// `GL_REPEAT`); Bevy's default clamp-to-edge would smear the edge texel across
/// the whole layer.
fn cloud_noise_image(decoded: &DecodedTexture) -> Image {
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
            reserved: Vec3::ZERO,
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
        // A sky backdrop, for the same reason as the cloud dome: `drive_stars` keeps
        // the field centred on the camera.
        SkyBackdrop::Stars,
        environment_render_layers(),
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
        vis.set_if_neq(visible_if(visible));
        // Keep the field centred on the camera and rotate it slowly about the up
        // axis (the reference `rotatef(gFrameTimeSeconds * 0.01, …)`, in degrees).
        // Only while visible, and with the angle quantised to coarse steps: at
        // 0.01°/s the continuous rotation moved sub-texel per frame but still
        // marked the Transform changed every frame; a 0.05° step (one write per
        // ~5 s) is far below what the eye can pick out on a star field while the
        // Transform settles between steps. (The twinkle animates GPU-side from
        // `globals.time`, so a still Transform does not freeze the stars.)
        if visible {
            let angle_deg = (elapsed * STAR_ROTATION_RATE_DEG / STAR_ROTATION_STEP_DEG).floor()
                * STAR_ROTATION_STEP_DEG;
            transform.set_if_neq(Transform {
                translation: camera_pos,
                rotation: Quat::from_rotation_y(angle_deg.to_radians()),
                scale: Vec3::ONE,
            });
        }
    }

    // Compare-then-`get_mut`: the material is only re-prepared when the fold's
    // brightness actually changed (the twinkle no longer lives in the params).
    if materials
        .get(&state.material)
        .is_some_and(|material| material.params.custom_alpha.to_bits() != custom_alpha.to_bits())
        && let Some(mut material) = materials.get_mut(&state.material)
    {
        material.params.custom_alpha = custom_alpha;
    }

    // Fetch the sky's bloom texture boosted (the sky frame's own, or the reference
    // built-in) so it resolves ahead of ordinary faces. Only on a key change —
    // the boost request is persistent in the store.
    let star_key = sky
        .bloom_texture
        .unwrap_or_else(|| TextureKey::from(IMG_BLOOM1));
    if state.star_key != Some(star_key) {
        textures.request_boosted(star_key, SKY_BOOST_PRIORITY);
        state.star_key = Some(star_key);
    }
}

/// Swap the decoded bloom texture into the star material when its id resolves.
pub(crate) fn apply_star_textures(
    mut decoded: MessageReader<TextureDecoded>,
    state: Res<StarState>,
    store: Res<DecodedTextures>,
    mut materials: ResMut<Assets<StarMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for &TextureDecoded(id) in decoded.read() {
        if state.star_key != Some(id) {
            continue;
        }
        let Some(decoded) = store.get(id) else {
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

/// Whether to linearise the sky / cloud colour before the tone mapper: on by
/// default (the reference behaviour), off when `SL_VIEWER_SKY_LINEARIZE=0` — an
/// A/B knob to isolate the linearisation's effect, including on what the
/// reflection-probe / environment-map capture reads of the sky.
///
/// Resolved once per process (the environment is fixed at launch); it is read from
/// the per-frame sky / cloud params builds.
fn sky_linearize() -> f32 {
    static LINEARIZE: OnceLock<f32> = OnceLock::new();
    *LINEARIZE.get_or_init(|| {
        if std::env::var("SL_VIEWER_SKY_LINEARIZE").as_deref() == Ok("0") {
            0.0
        } else {
            1.0
        }
    })
}

/// The active sky "fake HDR" scale (`SKY_HDR_SCALE`) for a sky frame: the value
/// the reference computes from the frame ([`SkySettings::sky_hdr_scale`] —
/// `sqrt(gamma) * 2` for an EEP reflection-probe-ambiance sky, `1.0` for a legacy
/// / classic-mode sky), unless the `SL_VIEWER_SKY_HDR_SCALE` A/B knob forces a
/// value.
///
/// The override exists alongside `SL_VIEWER_SKY_LINEARIZE` so the EEP blow-out
/// path can be exercised on *any* grid: the default aditi and OpenSim regions
/// serve legacy skies (scale `1.0`, a no-op), so without the override there is no
/// on-grid way to see the sun disc, clouds, and sky expand into HDR. A value `< 0`
/// is clamped to `0`.
fn resolved_sky_hdr_scale(sky: &SkySettings) -> f32 {
    sky_hdr_scale_override().unwrap_or_else(|| sky.sky_hdr_scale())
}

/// The `SL_VIEWER_SKY_HDR_SCALE` override (clamped to `>= 0`), or `None` when unset
/// or unparsable — see [`resolved_sky_hdr_scale`]. Resolved once per process (the
/// environment is fixed at launch); the override is consulted from the per-frame
/// sky / cloud / star / disc params builds.
fn sky_hdr_scale_override() -> Option<f32> {
    static OVERRIDE: OnceLock<Option<f32>> = OnceLock::new();
    *OVERRIDE.get_or_init(|| {
        std::env::var("SL_VIEWER_SKY_HDR_SCALE")
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .map(|scale| scale.max(0.0))
    })
}

/// Build the sky-shader uniform block from a sky frame plus the per-frame light
/// direction, day/night factor, and glow factor.
fn sky_params(
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
        linearize: sky_linearize(),
        sky_hdr_scale: resolved_sky_hdr_scale(sky),
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
    scroll_ref_time: f32,
    scroll_rate: Vec2,
    scroll_base: Vec2,
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
        // The scroll is integrated GPU-side (`clouds.wgsl` `cloud_scroll`), so
        // the layer position uploads unscrolled.
        cloud_pos_density1: Vec3::new(pd1.position_x(), pd1.position_y(), pd1.density()),
        cloud_variance: sky.cloud_variance,
        cloud_pos_density2: Vec3::new(pd2.position_x(), pd2.position_y(), pd2.density()),
        blend_factor: 0.0,
        linearize: sky_linearize(),
        sky_hdr_scale: resolved_sky_hdr_scale(sky),
        scroll_ref_time,
        scroll_rate,
        scroll_base,
    }
}

/// The cloud uniforms for the built-in legacy default sky, used to seed the
/// material before an environment is selected.
pub(crate) fn default_cloud_params() -> CloudParams {
    let sky = SkySettings::legacy_windlight_default("Default");
    cloud_params(&sky, Vec3::Y, 1.0, 1.0, 0.0, Vec2::ZERO, Vec2::ZERO)
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

/// The number of sampling steps the day cycle is quantised to — the day-position
/// counterpart of [`snap_shadow_direction`]'s angular grid, and the reason every
/// write-on-change guard downstream of the sky actually holds on a live grid.
///
/// The environment is sampled at a *continuously* advancing day position, so
/// without a grid `blended_sky_settings` synthesises a slightly different frame
/// every frame, every value derived from it differs in its last bits, and every
/// float-equality guard below it — the sky / cloud / star / water material
/// compare-then-`get_mut`, and above all `drive_terrain_lighting`'s
/// `Assets::iter_mut` over *every* region's terrain material — fires on every
/// frame forever. Rounding the position down to a grid holds the sampled frame
/// **bit-identical** across the frames whose true position falls in one cell, so
/// those guards hold between steps and the scene settles.
///
/// The step is a fraction of the day rather than a fixed number of seconds
/// because what must stay imperceptible is how far the sun *moves* per step, and
/// that is `360° / steps` whatever the region's day length: a region running a
/// five-minute day rotates its sun 48× faster than Second Life's four-hour one,
/// and gets 48× more frequent samples out of the same grid. At 32768 steps one
/// step turns the sun by 0.011°, below the ~0.014° the shadow-caster direction is
/// already snapped to (`1 / SHADOW_MAP_SIZE` radians), so the day cycle steps no
/// more coarsely than the light direction the renderer already quantises — while
/// a four-hour day resamples every 0.44 s instead of every frame.
const DAY_POSITION_STEPS: f64 = 32768.0;

/// The normalised day-cycle position (`0.0..=1.0`) for the current region time,
/// the reference `LLEnvironment::convert_time_to_position`: `fmod(now +
/// day_offset, day_length) / day_length` over the Unix clock, quantised to
/// [`DAY_POSITION_STEPS`] steps per day so the sampled environment settles
/// between steps.
///
/// The debug override `SL_VIEWER_SKY_DAY_POSITION` (a `0.0..=1.0` float) pins the
/// position instead, so the offline screenshot harness can inspect any point in
/// the day (e.g. midday) regardless of the wall clock. A pinned position is
/// already stable, so it is honoured exactly rather than rounded to the grid.
pub(crate) fn day_position(settings: &sl_client_bevy::EnvironmentSettings) -> f32 {
    if let Some(position) = pinned_day_position() {
        return position;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64());
    quantised_day_position(now, settings.day_length, settings.day_offset)
}

/// The `SL_VIEWER_SKY_DAY_POSITION` override (clamped to `0.0..=1.0`), or `None`
/// when unset or unparsable — see [`day_position`]. Resolved once per process (the
/// environment is fixed at launch), because [`day_position`] is called from seven
/// per-frame sites: the sky, cloud, star, and disc drives plus terrain, water, and
/// the underwater fog.
pub(crate) fn pinned_day_position() -> Option<f32> {
    static PINNED: OnceLock<Option<f32>> = OnceLock::new();
    *PINNED.get_or_init(|| {
        std::env::var("SL_VIEWER_SKY_DAY_POSITION")
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .map(|position| position.clamp(0.0, 1.0))
    })
}

/// [`day_position`] without the clock and the debug override: the normalised
/// position of `now` (seconds since the Unix epoch) in a `day_length`-second day
/// phase-shifted by `day_offset`, rounded down to the [`DAY_POSITION_STEPS`]
/// grid.
pub(crate) fn quantised_day_position(now: f64, day_length: i32, day_offset: i32) -> f32 {
    // The wrap must be in f64 (the Unix clock overflows f32's integer precision);
    // the result is a normalised fraction in `0.0..1.0`, so narrowing to f32 loses
    // only sub-epsilon precision — and a grid step is a power of two, so the
    // quantised fraction is exact in both widths.
    let day_length = f64::from(day_length.max(1));
    let day_offset = f64::from(day_offset);
    let position = (now + day_offset).rem_euclid(day_length) / day_length;
    let stepped = (position * DAY_POSITION_STEPS).floor() / DAY_POSITION_STEPS;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "a normalised 0.0..1.0 day fraction; the wrap needs f64 but the result fits f32"
    )]
    let fraction = stepped as f32;
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
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use super::{
        AMBIENT_BRIGHTNESS_SCALE, CLOUD_DOME_RADIUS, DAY_POSITION_STEPS, SHADOW_MAP_SIZE,
        build_cloud_dome_mesh, quantised_day_position, sky_ambient_light, snap_shadow_direction,
    };
    use bevy::camera::primitives::MeshAabb as _;
    use bevy::math::Vec3;
    use pretty_assertions::{assert_eq, assert_ne};

    /// The cloud dome's **mesh centre** — the one point Bevy's [`Transparent3d`]
    /// distance sort looks at — sits essentially *at* the camera the dome is
    /// anchored to, because the reference's `getCamHeight` offset is baked into the
    /// vertices and the visible cap is only the `[0, π/8]` crown of a 15 km sphere.
    ///
    /// That is why the dome cannot be ordered by distance at all: a sort distance of
    /// ~0 makes it the *nearest* transparent object in the scene and it is drawn
    /// last, over every world-anchored overlay in front of it (the name-tag bug,
    /// `viewer-nametags-occluded-by-clouds`). The fix is the
    /// [`SkyBackdrop`](crate::transparency::SkyBackdrop) bucket, and this test pins
    /// the premise: if the dome geometry ever moved its centre out into the sky, the
    /// bucket would stop being load-bearing and this test should be revisited rather
    /// than silently drifting.
    #[test]
    fn the_cloud_dome_mesh_is_centred_on_the_camera() {
        let aabb = build_cloud_dome_mesh()
            .compute_aabb()
            .expect("the cloud dome mesh has vertex positions");
        let centre = Vec3::from(aabb.center).length();
        assert!(
            centre < CLOUD_DOME_RADIUS / 100.0,
            "the cloud dome's mesh centre is {centre} m from the camera, \
             more than 1% of its {CLOUD_DOME_RADIUS} m radius",
        );
    }

    /// A Second Life day, in seconds (the grid default, and the length the
    /// day-position tests reason in).
    const SL_DAY_SECS: i32 = 14400;

    /// One day-cycle sampling step of an [`SL_DAY_SECS`] day, in seconds. Exact
    /// in binary (both operands are powers of two times a whole number), so a
    /// time built from it lands exactly on a cell boundary.
    fn step_secs() -> f64 {
        f64::from(SL_DAY_SECS) / DAY_POSITION_STEPS
    }

    /// Every sample taken inside one grid cell returns the **same** position, so
    /// the sky frame blended from it — and every colour and scalar derived from
    /// that frame — is bit-identical across those frames and the write-on-change
    /// guards downstream hold.
    #[test]
    fn day_position_is_stable_within_a_step() {
        let step = step_secs();
        // A whole number of steps since the epoch, i.e. exactly a cell boundary.
        let start = 3_000_000.0 * step;
        let first = quantised_day_position(start, SL_DAY_SECS, 0);
        for fraction in [0.0, 0.1, 0.25, 0.5, 0.75, 0.99] {
            // Bit patterns: the claim is that the sampled position is *identical*,
            // which is what the float-equality guards downstream test (and what
            // `float_cmp` forbids asserting with `==`).
            assert_eq!(
                quantised_day_position(start + fraction * step, SL_DAY_SECS, 0).to_bits(),
                first.to_bits(),
                "a sample {fraction} of the way through a cell left it"
            );
        }
    }

    /// The position still advances: the next cell is a different position, and a
    /// step is exactly one part in [`DAY_POSITION_STEPS`] of the day.
    #[test]
    fn day_position_advances_one_step_per_cell() {
        let step = step_secs();
        let start = 3_000_000.0 * step;
        let first = quantised_day_position(start, SL_DAY_SECS, 0);
        let next = quantised_day_position(start + step, SL_DAY_SECS, 0);
        assert_ne!(
            first.to_bits(),
            next.to_bits(),
            "the day cycle must not freeze"
        );
        #[expect(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            reason = "the grid step is a power of two, exact in f32"
        )]
        let expected = (1.0 / DAY_POSITION_STEPS) as f32;
        assert!(
            (next - first - expected).abs() < f32::EPSILON,
            "one cell should advance the position by exactly one step, got {}",
            next - first
        );
    }

    /// A region running a *short* day gets proportionally more frequent samples
    /// out of the same grid — the step is a fraction of the day, not a fixed
    /// number of seconds, because what must stay imperceptible is how far the sun
    /// moves per step.
    #[test]
    fn day_position_step_scales_with_the_day_length() {
        // A five-minute day: 48x shorter than Second Life's, so its cells are
        // 48x shorter in wall-clock seconds.
        let short_day = 300;
        let step = f64::from(short_day) / DAY_POSITION_STEPS;
        let start = 3_000_000.0 * step;
        assert_eq!(
            quantised_day_position(start + 0.5 * step, short_day, 0).to_bits(),
            quantised_day_position(start, short_day, 0).to_bits(),
        );
        assert_ne!(
            quantised_day_position(start + step, short_day, 0).to_bits(),
            quantised_day_position(start, short_day, 0).to_bits(),
        );
    }

    /// The day-cycle grid is finer than the angular grid the shadow-caster
    /// direction is already snapped to, so quantising the day position adds no
    /// sun motion coarser than the renderer already quantises away.
    #[test]
    fn a_day_step_turns_the_sun_less_than_a_shadow_snap_step() {
        let day_step_deg = 360.0 / DAY_POSITION_STEPS;
        let shadow_step_deg = (1.0 / f64::from(SHADOW_MAP_SIZE)).to_degrees();
        assert!(
            day_step_deg < shadow_step_deg,
            "a day step turns the sun {day_step_deg}°, coarser than the \
             {shadow_step_deg}° shadow-direction snap"
        );
    }

    /// The day offset phase-shifts the position, and the position wraps at the
    /// end of the day rather than running past 1.0.
    #[test]
    fn day_position_wraps_and_honours_the_offset() {
        let quarter = SL_DAY_SECS / 4;
        let midnight = 0.0;
        assert_eq!(
            quantised_day_position(midnight, SL_DAY_SECS, 0).to_bits(),
            0.0_f32.to_bits()
        );
        assert_eq!(
            quantised_day_position(midnight, SL_DAY_SECS, quarter).to_bits(),
            0.25_f32.to_bits(),
            "a quarter-day offset should start the day a quarter in"
        );
        let position = quantised_day_position(f64::from(SL_DAY_SECS) * 3.5, SL_DAY_SECS, 0);
        assert!(
            (position - 0.5).abs() < f32::EPSILON,
            "three and a half days in is midday, got {position}"
        );
    }

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

    /// A daylight sky's total ambient — a warm off-white, the shape
    /// `resolve_sky` hands `drive_sky`.
    const DAYLIGHT_AMBIENT: [f32; 3] = [0.32, 0.30, 0.25];

    /// The probe scale is the share of the sky's *own* ambient that survives:
    /// `0.0` drops the flat fill entirely (the probe is then the single ambient
    /// source), `1.0` keeps all of it, and a half keeps exactly half.
    #[test]
    fn ambient_brightness_is_the_probes_share_of_the_sky_value() {
        let (_, full) = sky_ambient_light(DAYLIGHT_AMBIENT, 1.0);
        let (_, half) = sky_ambient_light(DAYLIGHT_AMBIENT, 0.5);
        let (_, none) = sky_ambient_light(DAYLIGHT_AMBIENT, 0.0);
        assert!(full > 0.0, "a lit sky should ask for some ambient");
        assert!(
            (half - 0.5 * full).abs() < f32::EPSILON * AMBIENT_BRIGHTNESS_SCALE,
            "half the share should be half the brightness, got {half} of {full}"
        );
        assert_eq!(none.to_bits(), 0.0_f32.to_bits());
    }

    /// The regression: the share is a factor of the value the frame asks for, so
    /// a frame's ambient is the **same** however many frames have gone before it.
    ///
    /// The multiplicative `PostUpdate` post-pass this replaced computed
    /// `brightness * scale` against the resource, so a steady sky decayed as
    /// `scale^frames` — 120 frames at `0.5` is a factor of `1e-37`, i.e. black —
    /// and, because the product never equalled what the sky asked for,
    /// `drive_sky`'s write-on-change guard missed on every frame as well. Both
    /// are asserted against here: bit-identical, and never the decayed value.
    #[test]
    fn ambient_brightness_does_not_decay_across_frames() {
        let scale = 0.5;
        let (_, first) = sky_ambient_light(DAYLIGHT_AMBIENT, scale);
        let mut decaying = first;
        for frame in 1..120_u32 {
            let (_, this) = sky_ambient_light(DAYLIGHT_AMBIENT, scale);
            // Bit patterns: what `drive_sky`'s guard compares, so this is the
            // guard holding, not merely the value being close.
            assert_eq!(
                this.to_bits(),
                first.to_bits(),
                "frame {frame} asked for a different ambient than frame 0"
            );
            decaying *= scale;
        }
        assert!(
            decaying < first * 1.0e-30,
            "the post-pass this replaced should have collapsed, got {decaying}"
        );
    }

    /// The share scales the fill strength only; the tint is the sky's normalised
    /// hue and is the same whatever the probe supplies — including at `0.0`,
    /// where the colour still has to be a valid one rather than whatever a
    /// zero-brightness light happens to hold.
    #[test]
    fn ambient_tint_is_independent_of_the_probes_share() {
        let (full, _) = sky_ambient_light(DAYLIGHT_AMBIENT, 1.0);
        for scale in [0.0, 0.25, 0.5] {
            let (tinted, _) = sky_ambient_light(DAYLIGHT_AMBIENT, scale);
            assert_eq!(tinted, full, "a share of {scale} changed the ambient hue");
        }
    }

    /// A black sky (a fully overcast midnight authors one) divides by its own
    /// peak, so the tint is guarded against a zero denominator rather than
    /// reaching `GlobalAmbientLight` as a NaN colour.
    #[test]
    fn a_black_ambient_stays_finite() {
        let (color, brightness) = sky_ambient_light([0.0, 0.0, 0.0], 1.0);
        assert_eq!(brightness.to_bits(), 0.0_f32.to_bits());
        let linear = color.to_linear();
        assert!(
            linear.red.is_finite() && linear.green.is_finite() && linear.blue.is_finite(),
            "a black sky produced a non-finite ambient tint: {color:?}"
        );
    }
}
