//! A custom Bevy material for the Second Life / OpenSim **sun and moon discs**:
//! a port of the reference viewer's deferred heavenly-body shaders
//! (`LLDrawPoolWLSky::renderHeavenlyBodies`, `class1/deferred/sunDiscV.glsl` +
//! `sunDiscF.glsl` and `moonV.glsl` + `moonF.glsl`). Each disc is a
//! camera-facing billboard textured with the sky frame's sun / moon disc.
//!
//! The material carries one [`SunDiscParams`] uniform block plus the current and
//! next disc textures (`diffuse` / `alt_diffuse`, blended during a day-cycle
//! transition — the blend factor stays `0.0` until the day cycle drives it in a
//! later phase). The accompanying `sun_disc.wgsl` samples the disc and applies
//! the moon's brightness, transparent-pixel discard, and near-horizon alpha fade.
//!
//! One [`SunDiscMaterial`] instance renders the sun (`moon_mode` `0.0`,
//! brightness `1.0`) and another the moon (`moon_mode` `1.0`, brightness = the
//! sky's moon brightness); the viewer aims, scales, and colours them per frame.
//!
//! This module is gated behind the `bevy_pbr` feature: the headless client needs
//! no renderer, so the PBR/render stack is pulled in only by the windowed viewer.
//! Register [`SunDiscMaterialPlugin`] to load the shader and the material.

use bevy::app::{App, Plugin};
use bevy::asset::{Asset, Handle, load_internal_asset, uuid_handle};
use bevy::image::Image;
use bevy::mesh::{Mesh, MeshVertexBufferLayoutRef};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::AlphaMode;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::{Shader, ShaderRef};

/// The internal handle the sun / moon disc shader (`sun_disc.wgsl`) is loaded
/// under, so the material can reference it without an on-disk asset path.
const SUN_DISC_SHADER_HANDLE: Handle<Shader> = uuid_handle!("2f9c6b41-8d05-4c7e-9a3b-1e6d4f2c8a95");

/// The per-frame inputs for one heavenly-body disc: the reference viewer's
/// heavenly-body uniforms (moon brightness, day-cycle blend factor) plus the flag
/// and up component that select the moon's near-horizon fade.
///
/// Four scalars packed into a single 16-byte std140 uniform slot, so the layout
/// matches the `sun_disc.wgsl` `SunDiscParams` exactly.
#[derive(Clone, Copy, Debug, ShaderType)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `SunDiscParams`, where the name reads clearly"
)]
pub struct SunDiscParams {
    /// Overall brightness multiplier: the sky's moon brightness for the moon,
    /// `1.0` for the sun.
    pub brightness: f32,
    /// Blend factor between the current (`diffuse`) and next (`alt_diffuse`) disc
    /// textures during a day-cycle transition. `0.0` until the day cycle drives
    /// it, so only `diffuse` is used for now.
    pub blend_factor: f32,
    /// `1.0` to apply the moon's transparent-pixel discard and near-horizon alpha
    /// fade, `0.0` for the sun.
    pub moon_mode: f32,
    /// The body's up component (Bevy `y`) for the moon's near-horizon alpha fade.
    pub up_component: f32,
}

/// The sun / moon disc material: one [`SunDiscParams`] uniform block plus the
/// current and next disc textures, shaded by `sun_disc.wgsl`.
///
/// The disc textures may start as placeholders; the viewer fetches the sky's
/// referenced sun / moon textures **boosted** and swaps them in once decoded.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `SunDiscMaterial`, where the name reads clearly"
)]
pub struct SunDiscMaterial {
    /// The per-frame disc inputs.
    #[uniform(0)]
    pub params: SunDiscParams,
    /// The current disc texture (`diffuseMap`).
    #[texture(1)]
    #[sampler(2)]
    pub diffuse: Handle<Image>,
    /// The next disc texture (`altDiffuseMap`), blended toward during a day-cycle
    /// transition.
    #[texture(3)]
    #[sampler(4)]
    pub alt_diffuse: Handle<Image>,
}

impl Material for SunDiscMaterial {
    /// Use the bundled disc shader for the vertex stage.
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(SUN_DISC_SHADER_HANDLE)
    }

    /// Use the bundled disc shader for the fragment stage.
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SUN_DISC_SHADER_HANDLE)
    }

    /// The disc is drawn as an alpha-blended billboard over the (opaque) sky dome.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    /// Pin the vertex buffer layout to the position + UV attributes (the shader
    /// reads no normal) and disable back-face culling, so the billboard is visible
    /// whichever way it faces.
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

/// The plugin that registers the [`SunDiscMaterial`] and loads its shader. Add it
/// to a Bevy [`App`] (after `DefaultPlugins`) to render the sun / moon discs.
#[derive(Debug, Default)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `SunDiscMaterialPlugin`, where the name reads clearly"
)]
pub struct SunDiscMaterialPlugin;

impl Plugin for SunDiscMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            SUN_DISC_SHADER_HANDLE,
            "sun_disc.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins(MaterialPlugin::<SunDiscMaterial>::default());
    }
}
