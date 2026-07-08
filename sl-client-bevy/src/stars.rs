//! A custom Bevy material for the Second Life / OpenSim **star field**: a port of
//! the reference viewer's deferred star shaders (`LLDrawPoolWLSky::
//! renderStarsDeferred`, `class1/deferred/starsV.glsl` + `starsF.glsl`, drawing
//! `LLVOWLSky::drawStars`). The star field is a sphere of small camera-facing
//! quads, each textured with the sky's bloom texture and twinkling over time.
//!
//! Unlike the sky / cloud domes (a single inward sphere evaluated per fragment),
//! the star field is real quad geometry — the viewer builds a mesh of 1000 star
//! quads (the reference `getStarsNumVerts`), each carrying a per-star colour, and
//! this material shades it.
//!
//! The material carries one [`StarParams`] uniform block (the reference
//! `custom_alpha` = `star_brightness / 500`, and the twinkle animation `time`)
//! plus the bloom texture. The accompanying `stars.wgsl` samples the bloom, tints
//! it by the per-star vertex colour, scales the alpha by `custom_alpha` and the
//! per-star twinkle, and is drawn additively (the reference
//! `BT_ADD_WITH_ALPHA`) so the stars add their light over the (dark, at night)
//! sky.
//!
//! **Single bloom texture, no A/B blend.** The reference binds both the current
//! and next bloom textures (`getBloomTex` / `getBloomTexNext`), but the deferred
//! `starsF.glsl` samples `diffuseMap` twice and `mix`es by `blend_factor` — i.e.
//! it samples the *same* map for both, so the day-cycle blend is a no-op for
//! stars (only the current bloom matters). This material therefore carries one
//! texture, unlike the cloud / disc materials whose shaders genuinely sample two.
//!
//! This module is gated behind the `bevy_pbr` feature: the headless client needs
//! no renderer, so the PBR/render stack is pulled in only by the windowed viewer.
//! Register [`StarMaterialPlugin`] to load the shader and the material.

use bevy::app::{App, Plugin};
use bevy::asset::{Asset, Handle, load_internal_asset, uuid_handle};
use bevy::image::Image;
use bevy::mesh::{Mesh, MeshVertexBufferLayoutRef};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::{AlphaMode, Vec2};
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::{Shader, ShaderRef};

/// The internal handle the star shader (`stars.wgsl`) is loaded under, so the
/// material can reference it without an on-disk asset path.
const STAR_SHADER_HANDLE: Handle<Shader> = uuid_handle!("4a7e2c93-6f1b-4d8a-b2e5-3c9f0a7d1e64");

/// The per-frame inputs for the star field: the reference viewer's star uniforms
/// (`custom_alpha` and the twinkle `time`).
///
/// Two live scalars plus a [`Vec2`] pad, filling a single 16-byte std140 uniform
/// slot so the layout matches the `stars.wgsl` `StarParams` exactly.
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct StarParams {
    /// The star-field opacity, the reference `custom_alpha` = `star_brightness /
    /// 500` (clamped to `1.0`). The viewer hides the field entirely below the
    /// reference `0.001` threshold, so the field only renders when this is
    /// meaningful.
    pub custom_alpha: f32,
    /// The twinkle animation time, the reference `sStarTime` = elapsed seconds ×
    /// `0.5`. `stars.wgsl` scrolls the per-star twinkle sawtooth by this.
    pub time: f32,
    /// Padding so the block fills a 16-byte std140 slot (matches `stars.wgsl`).
    pub reserved: Vec2,
}

/// The star-field material: one [`StarParams`] uniform block plus the bloom
/// texture, shaded by `stars.wgsl`.
///
/// The bloom texture may start as a placeholder; the viewer fetches the sky's
/// referenced bloom texture (or the built-in default) **boosted** and swaps it in
/// once decoded.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct StarMaterial {
    /// The per-frame star inputs.
    #[uniform(0)]
    pub params: StarParams,
    /// The bloom / star texture (`diffuseMap`).
    #[texture(1)]
    #[sampler(2)]
    pub diffuse: Handle<Image>,
}

impl Material for StarMaterial {
    /// Use the bundled star shader for the vertex stage.
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(STAR_SHADER_HANDLE)
    }

    /// Use the bundled star shader for the fragment stage.
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(STAR_SHADER_HANDLE)
    }

    /// The stars are drawn additively over the (opaque) sky dome — the reference
    /// `BT_ADD_WITH_ALPHA`, so the black areas of the bloom texture add nothing
    /// and only the bright star texels light up the sky.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }

    /// Pin the vertex buffer layout to the position + UV + per-star colour
    /// attributes (the shader reads no normal) and disable back-face culling, so
    /// each star quad is visible whichever way it faces.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(2),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// The plugin that registers the [`StarMaterial`] and loads its shader. Add it to
/// a Bevy [`App`] (after `DefaultPlugins`) to render the star field.
#[derive(Debug, Default)]
pub struct StarMaterialPlugin;

impl Plugin for StarMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, STAR_SHADER_HANDLE, "stars.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<StarMaterial>::default());
    }
}
