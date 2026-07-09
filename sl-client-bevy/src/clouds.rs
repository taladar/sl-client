//! A custom Bevy material for the Second Life / OpenSim **cloud layer**: a port of
//! the reference viewer's deferred cloud shaders (`LLVOClouds` /
//! `LLDrawPoolWLSky::renderSkyCloudsDeferred`, `class1/deferred/cloudsV.glsl` +
//! `cloudsF.glsl`). It shades a large inward-facing dome — the same camera-centred
//! sphere the sky uses — with the scrolling cloud noise layer.
//!
//! The material carries one [`CloudParams`] uniform block (the sky frame's
//! atmospheric inputs plus the cloud position / density / scale / variance /
//! colour) and the current and next cloud noise textures
//! (`cloud_noise` / `cloud_noise_next`, blended during a day-cycle transition —
//! the blend factor stays `0.0` until the day cycle drives it in a later phase).
//! The accompanying `clouds.wgsl` evaluates the cloud lighting and multi-octave
//! noise per fragment, exactly as the reference does per vertex + per fragment,
//! and outputs an alpha-blended cloud colour.
//!
//! This module is gated behind the `bevy_pbr` feature: the headless client needs
//! no renderer, so the PBR/render stack is pulled in only by the windowed viewer.
//! Register [`CloudMaterialPlugin`] to load the shader and the material.

use bevy::app::{App, Plugin};
use bevy::asset::{Asset, Handle, load_internal_asset, uuid_handle};
use bevy::image::Image;
use bevy::math::Vec3;
use bevy::mesh::{Mesh, MeshVertexBufferLayoutRef};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::AlphaMode;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::{Shader, ShaderRef};

/// The internal handle the cloud shader (`clouds.wgsl`) is loaded under, so the
/// material can reference it without an on-disk asset path.
const CLOUD_SHADER_HANDLE: Handle<Shader> = uuid_handle!("6b1e9d43-2c07-4a8f-9e5d-7f3a1c6b204d");

/// The per-frame inputs for the cloud layer: the sky frame's atmospheric inputs
/// (the same `LLSettingsSky` values `cloudsV.glsl` binds, so the cloud lighting
/// matches the sky) plus the cloud position / density / scale / variance / colour
/// and the day-cycle blend factor.
///
/// Laid out as `vec3` + trailing scalar pairs so the std140 uniform layout matches
/// the `clouds.wgsl` `CloudParams` (`ShaderType`) exactly.
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct CloudParams {
    /// Sun (or, at night, moon) direction, Bevy Y-up, clamped like the reference
    /// `LLEnvironment::getClampedLightNorm`.
    pub lightnorm: Vec3,
    /// `1.0` when the sun is up, `0.0` when only the moon is (day vs night light).
    pub sun_up_factor: f32,
    /// The sunlight colour, RGB.
    pub sunlight_color: Vec3,
    /// The haze horizon factor.
    pub haze_horizon: f32,
    /// The moonlight colour, RGB (the reference shares the sunlight colour).
    pub moonlight_color: Vec3,
    /// The haze density.
    pub haze_density: f32,
    /// The ambient light colour, RGB.
    pub ambient_color: Vec3,
    /// The cloud shadow / coverage (drives the cloud density and dims sunlight).
    pub cloud_shadow: f32,
    /// The horizon blue colour, RGB.
    pub blue_horizon: Vec3,
    /// The atmospheric density multiplier.
    pub density_multiplier: f32,
    /// The blue-density colour, RGB.
    pub blue_density: Vec3,
    /// The maximum sky dome altitude (the cloud layer altitude).
    pub max_y: f32,
    /// The sun / moon glow shaping vector (`size`, unused middle, `focus`).
    pub glow: Vec3,
    /// The sun / moon glow factor: full by day, a small moon fraction by night.
    pub sun_moon_glow_factor: f32,
    /// The cloud colour, RGB.
    pub cloud_color: Vec3,
    /// The cloud scale (the reference `cloud_scale`; `< 0.001` discards the layer).
    pub cloud_scale: f32,
    /// The cloud layer 1 position (X, Y — already offset by the accumulated scroll)
    /// and density (Z).
    pub cloud_pos_density1: Vec3,
    /// The cloud variance (the noise disturbance magnitude).
    pub cloud_variance: f32,
    /// The cloud layer 2 detail position (X, Y) and density (Z).
    pub cloud_pos_density2: Vec3,
    /// Blend factor between the current (`cloud_noise`) and next
    /// (`cloud_noise_next`) noise textures during a day-cycle transition. `0.0`
    /// until the day cycle drives it, so only `cloud_noise` is used for now.
    pub blend_factor: f32,
}

/// The cloud-layer material: one [`CloudParams`] uniform block plus the current and
/// next cloud noise textures, shaded by `clouds.wgsl`.
///
/// The noise textures may start as placeholders; the viewer fetches the sky's
/// referenced cloud texture **boosted** and swaps it in once decoded.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct CloudMaterial {
    /// The per-frame cloud inputs.
    #[uniform(0)]
    pub params: CloudParams,
    /// The current cloud noise texture (`cloud_noise_texture`).
    #[texture(1)]
    #[sampler(2)]
    pub cloud_noise: Handle<Image>,
    /// The next cloud noise texture (`cloud_noise_texture_next`), blended toward
    /// during a day-cycle transition.
    #[texture(3)]
    #[sampler(4)]
    pub cloud_noise_next: Handle<Image>,
}

impl Material for CloudMaterial {
    /// Use the bundled cloud shader for the vertex stage.
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(CLOUD_SHADER_HANDLE)
    }

    /// Use the bundled cloud shader for the fragment stage.
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(CLOUD_SHADER_HANDLE)
    }

    /// The cloud layer is drawn as an alpha-blended dome over the (opaque) sky
    /// dome.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    /// Pin the vertex buffer layout to the position attribute (the shader derives
    /// the cloud texcoords per fragment from the dome position, reading no UV or
    /// normal) and disable back-face culling so the inward-facing dome is visible.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// The plugin that registers the [`CloudMaterial`] and loads its shader. Add it to
/// a Bevy [`App`] (after `DefaultPlugins`) to render the cloud layer.
#[derive(Debug, Default)]
pub struct CloudMaterialPlugin;

impl Plugin for CloudMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, CLOUD_SHADER_HANDLE, "clouds.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<CloudMaterial>::default());
    }
}
