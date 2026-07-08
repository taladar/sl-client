//! A custom Bevy material for the Second Life / OpenSim **atmospheric sky dome**:
//! a faithful port of the reference viewer's deferred sky shaders
//! (`LLVOSky` / `LLVOWLSky`, `class1/deferred/skyV.glsl` + `skyF.glsl`) — the
//! legacy two-colour exponential atmosphere (blue / haze scattering with an
//! anti-solar glow) plus the rainbow / halo overlays.
//!
//! The material carries one [`SkyParams`] uniform block (the `LLSettingsSky`
//! values the reference binds as sky-shader uniforms — filled per frame from the
//! region's EEP settings and the computed sun / moon direction) plus the rainbow
//! and halo textures. The accompanying `sky.wgsl` evaluates the atmosphere per
//! fragment on a large inward-facing dome the viewer keeps centred on the camera.
//!
//! This module is gated behind the `bevy_pbr` feature: the headless client needs
//! no renderer, so the PBR/render stack is pulled in only by the windowed viewer.
//! Register [`SkyMaterialPlugin`] to load the shader and the material.

use bevy::app::{App, Plugin};
use bevy::asset::{Asset, Handle, load_internal_asset, uuid_handle};
use bevy::image::Image;
use bevy::math::Vec3;
use bevy::mesh::{Mesh, MeshVertexBufferLayoutRef};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::{Shader, ShaderRef};

/// The internal handle the sky shader (`sky.wgsl`) is loaded under, so the
/// material can reference it without an on-disk asset path.
const SKY_SHADER_HANDLE: Handle<Shader> = uuid_handle!("7b3e1d9a-4c22-4f8e-b0a1-6d2c9e5f4a70");

/// The atmospheric inputs for one sky frame: the `LLSettingsSky` values the
/// reference viewer binds as sky-shader uniforms, plus the per-frame sun / moon
/// light direction and day/night factor.
///
/// The field order (each `Vec3` paired with a trailing scalar) matches the
/// `sky.wgsl` `SkyParams` layout so the std140 uniform packing lines up: a
/// `vec3` occupies 12 bytes with 16-byte alignment, and the following scalar
/// fills the 4-byte remainder of that 16-byte slot.
#[derive(Clone, Copy, Debug, ShaderType)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `SkyParams`, where the name reads clearly"
)]
pub struct SkyParams {
    /// Sun (or, at night, moon) direction in Bevy Y-up space, clamped like the
    /// reference `LLEnvironment::getClampedLightNorm`.
    pub lightnorm: Vec3,
    /// `1.0` when the sun is up, `0.0` when only the moon is (selects the day vs
    /// night light colour in the shader).
    pub sun_up_factor: f32,
    /// The sky's sunlight colour (`sunlight_color`).
    pub sunlight_color: Vec3,
    /// The legacy haze-horizon scalar.
    pub haze_horizon: f32,
    /// The moonlight colour (the reference shares the sunlight colour).
    pub moonlight_color: Vec3,
    /// The legacy haze-density scalar.
    pub haze_density: f32,
    /// The sky's ambient colour.
    pub ambient_color: Vec3,
    /// The cloud-shadow fraction (dims direct sunlight, lifts ambient).
    pub cloud_shadow: f32,
    /// The legacy blue-horizon colour.
    pub blue_horizon: Vec3,
    /// The atmospheric density multiplier.
    pub density_multiplier: f32,
    /// The legacy blue-density colour.
    pub blue_density: Vec3,
    /// The atmospheric distance multiplier.
    pub distance_multiplier: f32,
    /// The glow shaping vector (`glow.x` = size/spread, `glow.z` = focus, a
    /// negative exponent). The middle component is unused by the shader.
    pub glow: Vec3,
    /// The altitude clamp (`max_y`) the sky ray is normalised against.
    pub max_y: f32,
    /// The sun/moon glow factor (`getSunMoonGlowFactor`): `1.0` by day, a small
    /// moon-brightness fraction by night, `0.0` when neither body is up.
    pub sun_moon_glow_factor: f32,
    /// The atmosphere's moisture level (scales the rainbow overlay).
    pub moisture_level: f32,
    /// The rain-droplet radius (selects the rainbow band).
    pub droplet_radius: f32,
    /// The atmosphere's ice level (scales the halo overlay).
    pub ice_level: f32,
}

/// The atmospheric sky-dome material: one [`SkyParams`] uniform block plus the
/// rainbow and halo overlay textures, shaded by `sky.wgsl`.
///
/// The rainbow / halo textures may start as placeholders; the viewer fetches the
/// sky's referenced textures **boosted** and swaps them in once decoded.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `SkyMaterial`, where the name reads clearly"
)]
pub struct SkyMaterial {
    /// The per-frame atmospheric inputs.
    #[uniform(0)]
    pub params: SkyParams,
    /// The rainbow overlay texture (`rainbow_map`).
    #[texture(1)]
    #[sampler(2)]
    pub rainbow: Handle<Image>,
    /// The 22-degree ice-halo overlay texture (`halo_map`).
    #[texture(3)]
    #[sampler(4)]
    pub halo: Handle<Image>,
}

impl Material for SkyMaterial {
    /// Use the bundled sky shader for the vertex stage (it passes the
    /// camera-relative dome position through to the fragment stage).
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(SKY_SHADER_HANDLE)
    }

    /// Use the bundled sky shader for the fragment stage (it evaluates the
    /// atmosphere per fragment).
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SKY_SHADER_HANDLE)
    }

    /// Pin the vertex buffer layout to just the position attribute (the shader
    /// reads no normal / UV) and disable back-face culling, so the camera *inside*
    /// the dome sees its inward-facing surface.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout
            .0
            .get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// The plugin that registers the [`SkyMaterial`] and loads its shader. Add it to
/// a Bevy [`App`] (after `DefaultPlugins`) to render the atmospheric sky dome.
#[derive(Debug, Default)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `SkyMaterialPlugin`, where the name reads clearly"
)]
pub struct SkyMaterialPlugin;

impl Plugin for SkyMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SKY_SHADER_HANDLE, "sky.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<SkyMaterial>::default());
    }
}
