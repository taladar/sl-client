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
//! for refraction and reflection probes for reflection. This port has all three:
//! Bevy's `view_transmission_texture` is the screen copy, the [`scene_depth`]
//! field is the depth buffer that copy was taken with (which is what lets the
//! refraction reject a sample lying *in front of* the surface), and the reflection
//! comes off the view's reflection probe, falling back to a sky tint when none is
//! bound.
//!
//! [`scene_depth`]: WaterMaterial::scene_depth
//!
//! Per the reference `LLDrawPoolWater::render`, the water **colour / waves /
//! fresnel are region-wide**
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
/// direction and sky-reflection tint. The wave-scroll clock and the camera
/// position are **not** CPU-driven: `water.wgsl` reads Bevy's `globals.time`
/// and the view's `world_position` directly, so running waves and a moving
/// camera never dirty the material.
///
/// Laid out as `vec3` + trailing scalar pairs (and a `vec2` + `vec2` pair) so the
/// std140 uniform layout matches the `water.wgsl` `WaterParams` (`ShaderType`)
/// exactly: a `vec3` occupies 12 bytes with 16-byte alignment, and the following
/// scalar fills the 4-byte remainder of that 16-byte slot.
#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `WaterParams`, where the name reads clearly"
)]
pub struct WaterParams {
    /// The direction toward the sun (or, at night, the moon) in Bevy Y-up space.
    pub light_dir: Vec3,
    /// The fresnel scale (`fresnelScale`): how strongly grazing angles reflect.
    pub fresnel_scale: f32,
    /// The normal-map (wavelet) scale (`normScale`), X/Y horizontal, Z up.
    pub normal_scale: Vec3,
    /// The fresnel offset (`fresnelOffset`): the base reflectivity looking straight
    /// down.
    pub fresnel_offset: f32,
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
    /// How far the wave normal displaces the refraction sample in screen space
    /// (`refScale`): the reference binds the water frame's `scaleAbove` when the eye
    /// is above the surface and `scaleBelow` when it is under
    /// (`lldrawpoolwater.cpp:299`), so the eye state is resolved before this is
    /// filled.
    pub ref_scale: f32,
    /// The **authored** (sRGB) water fog colour (`waterFogColor`), for the surface's
    /// own underside: seen from below, the surface shows the world *above* the
    /// water, which the haze pass leaves alone (it fogs only what is under the
    /// surface), so the water column between the eye and the surface has to be
    /// applied here — `underWaterF.glsl`'s
    /// `fb = applyWaterFogViewLinearNoClip(vary_position, fb)`.
    pub water_fog_color: Vec3,
    /// The water fog density for the current eye state
    /// (`getModifiedWaterFogDensity`), resolved on the CPU like
    /// [`ref_scale`](Self::ref_scale) above — there is one water material, so a
    /// per-frame write costs nothing.
    pub water_fog_density: f32,
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
    /// The screen-space **water-exclusion mask** (`exclusionTex`): a single-channel
    /// image, `1` where water should render and `0` where a water-exclusion surface
    /// (an "invisiprim" successor) punches a hole in the sea. The viewer renders the
    /// exclusion faces into this target each frame; the fragment shader discards the
    /// water where the mask reads `0`. A `1×1` white placeholder (water everywhere)
    /// until the viewer wires the real mask in, so the sea is unaffected until then.
    /// Mirrors the reference viewer's `LLDrawPoolWaterExclusion` mask sampled by
    /// `class3/environment/waterF.glsl`.
    #[texture(5)]
    #[sampler(6)]
    pub exclusion_mask: Handle<Image>,
    /// The **opaque scene depth** behind the water (the reference's `depthMap`): a
    /// copy of the view's depth buffer taken at the same moment as the screen copy
    /// the refraction samples, so the shader can tell whether the texel a wave
    /// displaces it onto is actually *behind* the surface.
    ///
    /// Without it a water fragment beside an avatar's silhouette samples a texel
    /// from *inside* it and paints the avatar's colour onto the sea — the
    /// screen-space-refraction fringe of
    /// `viewer-water-refraction-smears-avatar-silhouette`. The reference rejects it
    /// (`class3/environment/waterF.glsl`: `if (pos.z < refPos.z - 0.05) distort2 =
    /// distort;`) and falls back to the undistorted one.
    ///
    /// Multisampled `Depth32Float`, matching the main view's 4× depth texture, and
    /// read with `textureLoad` (no sampler): a depth buffer has no meaningful
    /// filtered sample. A `1×1` placeholder until the viewer's copy pass wires the
    /// real one in — and since a material bind group is shared by every view, the
    /// shader reads the depth only when it measures the same as the view being
    /// shaded, so the placeholder (and a probe capture's foreign view) rejects
    /// nothing and the sea looks exactly as it did before the depth existed.
    #[texture(7, sample_type = "depth", multisampled = true)]
    pub scene_depth: Handle<Image>,
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

    /// The water surface is **opaque**, as the reference's is
    /// (`LLDrawPoolWater::renderPostDeferred` opens with `LLGLDisable
    /// blend(GL_BLEND)`): what you see through the sea is a *sample of the screen
    /// behind it*, displaced by the wave normal, not the scene blended through a
    /// translucent plane. See `reads_view_transmission_texture` below, which is
    /// what puts the surface in the phase where that sample exists.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Opaque
    }

    /// Read the screen copy Bevy takes for screen-space transmission — our
    /// refraction source, and the reason the surface can be opaque.
    ///
    /// This moves the water out of `Transparent3d` and into `Transmissive3d`, whose
    /// pass copies the main texture and then draws the phase
    /// (`bevy_pbr::transmission`), which is exactly the shape of the reference's own
    /// water pass: it copies the deferred colour buffer at `lldrawpoolwater.cpp:116`
    /// and samples it as `screenTex`. It also means the surface is pipelined as
    /// opaque — no blending, depth written — which is the reference's state too.
    ///
    /// The copy is taken after the opaque and alpha-mask passes and before
    /// `Transparent3d`, so translucent content *below* the surface has to be drawn
    /// before it or it would not be in the sample at all; the viewer's
    /// `transparency` module draws it in a pre-water pass for that reason.
    fn reads_view_transmission_texture(&self) -> bool {
        true
    }

    /// Pin the vertex buffer layout to the position attribute (the shader derives
    /// the wave texcoords, view vector, and fresnel per fragment from the world
    /// position, reading no UV or normal), disable back-face culling so the surface
    /// is visible from below (an avatar underwater still sees the surface), and keep
    /// the surface's **depth write**.
    ///
    /// The depth write matches the reference (`LLDrawPoolWater` renders with
    /// `LLGLDepthTest(GL_TRUE, GL_TRUE)`) and is what gives per-pixel occlusion of
    /// above-water translucency that dips behind the surface — a fountain's spray, a
    /// boat wake — rather than the whole-plane back-to-front sort that painted it
    /// out. It is stated here rather than left to the transmissive pipeline's default
    /// because it is load-bearing, and because it is only correct in concert with the
    /// draw order the viewer's `transparency` module keeps: below-water translucency
    /// first, then the water, then above-water translucency.
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
        // Write depth so the water surface occludes above-water translucency behind
        // it per pixel; the pre/post-water draw ordering (see the doc above) keeps it
        // from hiding below-water translucency.
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(true);
        }
        // No `preserve_glow_mask_alpha` here, unlike the viewer's alpha-blended
        // materials: that helper rewrites a blend component, and an opaque pipeline
        // has no blending to rewrite. The shader writes the glow mask itself — zero,
        // since water does not glow.
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
        // The surface fogs its own underside, so it imports the shared module.
        crate::water_fog::load_water_fog_shader(app);
        load_internal_asset!(app, WATER_SHADER_HANDLE, "water.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default());
    }
}
