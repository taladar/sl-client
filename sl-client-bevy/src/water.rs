//! A custom Bevy material for the Second Life / OpenSim **water surface**: a port
//! of the reference viewer's water shaders (`LLDrawPoolWater`,
//! `class1/environment/waterV.glsl` + `class3/environment/waterF.glsl`). It shades
//! a flat horizontal plane at the region water height with scrolling wave normals,
//! a fresnel-blended sky reflection, the water-fog deep-water tint, and a sun
//! specular highlight.
//!
//! The material carries one [`WaterParams`] uniform block (the region's EEP
//! `LLSettingsWater` values the reference binds as water-shader uniforms, plus the
//! per-frame sun direction, camera position, sky-reflection tint, and wave-scroll
//! time) and the current and next wave normal maps (`normal_map` /
//! `normal_map_next`, blended during a day-cycle transition — the blend factor
//! stays `0.0` until the day cycle drives it, like the cloud / disc materials).
//! The accompanying `water.wgsl` evaluates the waves, fresnel, reflection, and
//! specular per fragment.
//!
//! The reference is a deferred shader reading the screen colour / depth buffers
//! for refraction and reflection probes for reflection; the headless viewer has
//! neither, so the port covers exactly the P23.1 scope (fresnel, reflection tint,
//! scrolling wave normals) and approximates refraction with the fog-tinted
//! deep-water colour and reflection with a sky tint. Per the reference
//! `LLDrawPoolWater::render`, the water **colour / waves / fresnel are region-wide**
//! (a single `getCurrentWater()` binds the whole water pass); only the water
//! **height** varies per region, which the viewer handles by placing each region's
//! plane at its own height.
//!
//! This module is gated behind the `bevy_pbr` feature: the headless client needs
//! no renderer, so the PBR/render stack is pulled in only by the windowed viewer.
//! Register [`WaterMaterialPlugin`] to load the shader and the material.

use bevy::app::{App, Plugin};
use bevy::asset::{Asset, Handle, load_internal_asset, uuid_handle};
use bevy::image::Image;
use bevy::math::{Vec2, Vec3};
use bevy::mesh::{Mesh, MeshVertexBufferLayoutRef};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::AlphaMode;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::{Shader, ShaderRef};

/// The internal handle the water shader (`water.wgsl`) is loaded under, so the
/// material can reference it without an on-disk asset path.
const WATER_SHADER_HANDLE: Handle<Shader> = uuid_handle!("2f8d6c14-9b3a-4e57-8c0d-1a6f4b29e753");

/// The per-frame inputs for the water surface: the region's EEP `LLSettingsWater`
/// values the reference binds as water-shader uniforms, plus the per-frame sun
/// direction, camera position (for the view vector), sky-reflection tint, and
/// wave-scroll time.
///
/// Laid out as `vec3` + trailing scalar pairs (and a `vec2` + `vec2` pair) so the
/// std140 uniform layout matches the `water.wgsl` `WaterParams` (`ShaderType`)
/// exactly: a `vec3` occupies 12 bytes with 16-byte alignment, and the following
/// scalar fills the 4-byte remainder of that 16-byte slot.
#[derive(Clone, Copy, Debug, ShaderType)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `WaterParams`, where the name reads clearly"
)]
pub struct WaterParams {
    /// The direction toward the sun (or, at night, the moon) in Bevy Y-up space.
    pub light_dir: Vec3,
    /// Accumulated seconds, scrolling the wave texcoords (`waterV.glsl` `time`).
    pub time: f32,
    /// The camera world position, for the per-fragment view vector.
    pub camera_position: Vec3,
    /// The fresnel scale (`fresnelScale`): how strongly grazing angles reflect.
    pub fresnel_scale: f32,
    /// The normal-map (wavelet) scale (`normScale`), X/Y horizontal, Z up.
    pub normal_scale: Vec3,
    /// The fresnel offset (`fresnelOffset`): the base reflectivity looking straight
    /// down.
    pub fresnel_offset: f32,
    /// The water fog colour (`waterFogColor`) — the deep-water tint seen looking
    /// into the water.
    pub water_fog_color: Vec3,
    /// The water fog density (`waterFogDensity`).
    pub water_fog_density: f32,
    /// The sky's sunlight colour, tinting the sun specular highlight.
    pub sunlight_color: Vec3,
    /// The reflection blur multiplier (`blurMultiplier`) — the surface roughness,
    /// which broadens the specular highlight.
    pub blur_multiplier: f32,
    /// The sky-reflection tint (the atmosphere colour the surface mirrors at
    /// grazing angles), supplied per frame from the sky settings.
    pub reflection_color: Vec3,
    /// The A/B normal-map blend factor during a day-cycle transition. `0.0` until
    /// the day cycle drives it, so only `normal_map` is used for now.
    pub blend_factor: f32,
    /// Wave-layer 1 scroll direction (`waveDir1`).
    pub wave1_dir: Vec2,
    /// Wave-layer 2 scroll direction (`waveDir2`).
    pub wave2_dir: Vec2,
}

/// The water-surface material: one [`WaterParams`] uniform block plus the current
/// and next wave normal maps, shaded by `water.wgsl`.
///
/// The normal maps may start as placeholders (a flat +Z normal); the viewer
/// fetches the water's referenced normal texture **boosted** and swaps it in once
/// decoded.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `WaterMaterial`, where the name reads clearly"
)]
pub struct WaterMaterial {
    /// The per-frame water inputs.
    #[uniform(0)]
    pub params: WaterParams,
    /// The current wave normal map (`bumpMap`).
    #[texture(1)]
    #[sampler(2)]
    pub normal_map: Handle<Image>,
    /// The next wave normal map (`bumpMap2`), blended toward during a day-cycle
    /// transition.
    #[texture(3)]
    #[sampler(4)]
    pub normal_map_next: Handle<Image>,
}

impl Material for WaterMaterial {
    /// Use the bundled water shader for the vertex stage.
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(WATER_SHADER_HANDLE)
    }

    /// Use the bundled water shader for the fragment stage.
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(WATER_SHADER_HANDLE)
    }

    /// The water surface is alpha-blended so shallow water reveals the ground
    /// beneath it and the sea composites over the terrain / sea floor.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    /// Pin the vertex buffer layout to the position attribute (the shader derives
    /// the wave texcoords, view vector, and fresnel per fragment from the world
    /// position, reading no UV or normal) and disable back-face culling so the
    /// surface is visible from below (an avatar underwater still sees the surface).
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

/// The plugin that registers the [`WaterMaterial`] and loads its shader. Add it to
/// a Bevy [`App`] (after `DefaultPlugins`) to render the water surface.
#[derive(Debug, Default)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `WaterMaterialPlugin`, where the name reads clearly"
)]
pub struct WaterMaterialPlugin;

impl Plugin for WaterMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, WATER_SHADER_HANDLE, "water.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default());
    }
}
