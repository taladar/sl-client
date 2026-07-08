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
//! The day-cycle keyframe interpolation over region time (animating the frame
//! chosen here) is a later phase; P22.2 renders the statically selected frame.

use std::time::{SystemTime, UNIX_EPOCH};

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sl_client_bevy::{
    Color as SlColor, ColorAlpha, Glow, SkyMaterial, SkyParams, SkySettings, TextureKey, Uuid,
    to_bevy_image,
};

use crate::camera::FlyCamera;
use crate::coords::sl_to_bevy_object_rotation;
use crate::environment::EnvironmentState;
use crate::render_priority::SKY_BOOST_PRIORITY;
use crate::textures::{TextureDecoded, TextureManager};

/// The radius of the sky dome, in metres. Large enough that the whole region sits
/// well inside it (so scene geometry always depth-tests in front of the sky) yet
/// comfortably within the camera's far plane (4096 m) so the dome is never
/// clipped away.
const SKY_DOME_RADIUS: f32 = 3000.0;

/// The scene directional light's illuminance (lux). Held constant; the sky's
/// computed sun / moon diffuse colour carries the day↔night brightness change
/// (a night moon diffuse is a fraction of the daytime sun diffuse), so the light
/// dims naturally as the colour darkens without re-scaling the illuminance.
const SCENE_LIGHT_ILLUMINANCE: f32 = 10_000.0;

/// Maps the sky's ambient colour luminance to the Bevy ambient-light brightness
/// (lux). The reference default ambient (`0.25` grey) lands at a soft fill.
const AMBIENT_BRIGHTNESS_SCALE: f32 = 400.0;

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
        .active_sky_settings(0.0, day_position(&environment.settings));
    let params = sky.map_or_else(default_sky_params, |sky| sky_params(sky, Vec3::Y, 1.0, 1.0));
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
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-0.4, -1.0, -0.3), Vec3::Y),
        SceneSun,
    ));
    commands.insert_resource(SkyState {
        material,
        rainbow_key: None,
        halo_key: None,
    });
}

/// Keep the sky dome centred on the camera each frame, so the atmosphere always
/// surrounds the viewpoint (the reference renders the dome camera-relative).
pub(crate) fn center_sky_on_camera(
    camera: Query<&GlobalTransform, With<FlyCamera>>,
    mut dome: Query<&mut Transform, With<SkyDome>>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let Ok(mut transform) = dome.single_mut() else {
        return;
    };
    transform.translation = camera.translation();
}

/// Fold the current environment + camera altitude into the sky material, the
/// directional light, and the ambient light, and (re)request the sky's rainbow /
/// halo overlay textures boosted.
pub(crate) fn drive_sky(
    camera: Query<&GlobalTransform, With<FlyCamera>>,
    environment: Res<EnvironmentState>,
    mut state: ResMut<SkyState>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut textures: ResMut<TextureManager>,
    mut sun: Query<(&mut Transform, &mut DirectionalLight), With<SceneSun>>,
    mut ambient: ResMut<GlobalAmbientLight>,
) {
    let altitude = camera.single().map_or(0.0, |camera| camera.translation().y);
    let position = day_position(&environment.settings);
    let Some(sky) = environment.settings.active_sky_settings(altitude, position) else {
        return;
    };

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

    if let Some(mut material) = materials.get_mut(&state.material) {
        material.params = sky_params(sky, lightnorm, sun_up_factor, glow_factor);
    }

    // Scene lighting from the sky (`calculateLightSettings`).
    let lighting = calculate_light_settings(sky, light_dir.y, moon_up);
    let diffuse = if sun_up {
        lighting.sun_diffuse
    } else if moon_up {
        lighting.moon_diffuse
    } else {
        [1.0, 1.0, 1.0]
    };
    if let Ok((mut transform, mut light)) = sun.single_mut() {
        // The light travels *toward* its forward axis, i.e. away from the body, so
        // its forward is the negated light direction. Pick a safe up when the body
        // is near the zenith (forward near-parallel to +Y).
        let forward = Vec3::new(-light_dir.x, -light_dir.y, -light_dir.z);
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
    let amb = lighting.total_ambient;
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
fn default_sky_params() -> SkyParams {
    let sky = SkySettings::legacy_windlight_default("Default");
    sky_params(&sky, Vec3::Y, 1.0, 1.0)
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
fn day_position(settings: &sl_client_bevy::EnvironmentSettings) -> f32 {
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
fn placeholder_image() -> Image {
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
