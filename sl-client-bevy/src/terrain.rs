//! A custom Bevy material for Second Life / OpenSim **terrain texture
//! splatting**: it blends a region's four ground ("detail") textures by a
//! per-vertex four-component weight and lights the result from the sky the way
//! the reference's deferred pass lights its legacy terrain
//! ([`crate::sky_lighting`]).
//!
//! The per-vertex weights are computed on the CPU by the Bevy-free `sl-terrain`
//! crate (elevation bilinear interpolation plus a Perlin transition band) and
//! carried on the terrain mesh in the [`ATTRIBUTE_TERRAIN_WEIGHTS`] vertex
//! attribute; the GPU side — sampling the four textures and blending them by the
//! interpolated weight — lives in the accompanying `terrain.wgsl` shader.
//!
//! This module is gated behind the `bevy_pbr` feature: the headless client
//! needs no renderer, so the PBR/render stack is pulled in only by the windowed
//! viewer. Register [`TerrainMaterialPlugin`] to load the shader and the
//! material, build a [`TerrainMaterial`] with the region's four detail textures,
//! and attach it to a mesh carrying [`ATTRIBUTE_TERRAIN_WEIGHTS`].

use bevy::app::{App, Plugin};
use bevy::asset::{Asset, Handle, load_internal_asset, uuid_handle};
use bevy::image::Image;
use bevy::math::Vec2;
use bevy::mesh::{Mesh, MeshVertexAttribute, MeshVertexBufferLayoutRef, VertexFormat};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::{Shader, ShaderRef};

/// The mesh vertex attribute carrying a terrain vertex's four detail-texture
/// blend weights (one per detail texture, as produced by
/// [`sl_terrain::TerrainComposition::blend_weights`](https://docs.rs/sl-terrain)).
/// The terrain mesh must carry this attribute for [`TerrainMaterial`] to blend
/// its four textures; it is matched to the shader's `@location(3)` in
/// [`TerrainMaterial::specialize`].
pub const ATTRIBUTE_TERRAIN_WEIGHTS: MeshVertexAttribute = MeshVertexAttribute::new(
    "TerrainWeights",
    0x5c_7e_44_49_8a_00,
    VertexFormat::Float32x4,
);

/// The internal handle the terrain shader (`terrain.wgsl`) is loaded under, so
/// the material can reference it without an on-disk asset path.
const TERRAIN_SHADER_HANDLE: Handle<Shader> = uuid_handle!("5c7e4449-8a00-4c37-9b1e-2f0d3a6b7c88");

/// A terrain texture-splat material: the region's four ground ("detail")
/// textures, blended per fragment by the interpolated
/// [`ATTRIBUTE_TERRAIN_WEIGHTS`] weight.
///
/// A missing (not-yet-fetched) detail texture can be left as a placeholder
/// handle; the viewer swaps in the decoded texture and replaces the material
/// once it arrives.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `TerrainMaterial`, where the name reads clearly"
)]
pub struct TerrainMaterial {
    /// The lowest-elevation ground texture (`TerrainDetail0`).
    #[texture(0)]
    #[sampler(1)]
    pub detail0: Handle<Image>,
    /// The second ground texture (`TerrainDetail1`).
    #[texture(2)]
    #[sampler(3)]
    pub detail1: Handle<Image>,
    /// The third ground texture (`TerrainDetail2`).
    #[texture(4)]
    #[sampler(5)]
    pub detail2: Handle<Image>,
    /// The highest-elevation ground texture (`TerrainDetail3`).
    #[texture(6)]
    #[sampler(7)]
    pub detail3: Handle<Image>,
    /// The shared sky-lighting texture the ground is lit by — always
    /// [`SKY_LIGHTING_IMAGE`](crate::SKY_LIGHTING_IMAGE), bound by handle so a new
    /// sky reaches every region's material without rewriting any of them. See
    /// [`crate::sky_lighting`].
    #[texture(8)]
    #[sampler(9)]
    pub sky_lighting: Handle<Image>,
    /// How the parcel-ownership tint below is mapped and how strongly it is
    /// applied. Its [`strength`](TerrainOwnership::strength) is the on/off
    /// switch, so toggling the overlay rewrites one uniform rather than every
    /// region's ownership map.
    #[uniform(10)]
    pub ownership: TerrainOwnership,
    /// The region's parcel-ownership map: one texel per 4 m parcel-overlay
    /// square, carrying that square's ownership-class colour and the overlay's
    /// alpha (the reference's `LLViewerParcelOverlay` texture). Left as a 1×1
    /// transparent placeholder where a region has no decoded overlay yet.
    #[texture(11)]
    #[sampler(12)]
    pub ownership_map: Handle<Image>,
}

/// How a [`TerrainMaterial`]'s parcel-ownership tint is mapped onto the ground
/// and how strongly it shows.
///
/// The terrain mesh's UV is region-local metres over a fixed detail tile, and
/// the ownership map spans the whole region, so the two differ by a per-region
/// scale — which is why this is a material uniform rather than a constant.
#[derive(Clone, Copy, Debug, ShaderType)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `TerrainOwnership`, where the name reads \
              clearly — the same reason `TerrainMaterial` beside it carries this"
)]
pub struct TerrainOwnership {
    /// Multiply the mesh's detail UV by this to get the ownership map's UV:
    /// `detail tile metres / region width metres`.
    pub uv_scale: f32,
    /// How strongly the tint is blended over the lit ground, `0.0` for off —
    /// the `ShowParcelOwners` switch.
    pub strength: f32,
    /// Padding to the uniform's 16-byte stride. Never read by the shader.
    pub padding: Vec2,
}

impl Default for TerrainOwnership {
    /// The tint off, mapped for a standard 256 m region — the state a material
    /// built before its region's overlay has arrived should be in.
    fn default() -> Self {
        Self {
            uv_scale: DETAIL_TILE_METRES / DEFAULT_REGION_WIDTH_METRES,
            strength: 0.0,
            padding: Vec2::ZERO,
        }
    }
}

/// The world span, in metres, over which a ground detail texture repeats once —
/// the scale the terrain mesh's UV is built at (`local metres / this`), and so
/// the numerator of [`TerrainOwnership::uv_scale`].
///
/// Detail textures tile far more finely than the whole region, so the mesh's
/// UVs wrap every few metres rather than stretching one texture across 256 m.
/// It lives here, beside the material and the vertex attribute it pairs with,
/// because the ownership uniform is computed from it and the mesh builder is a
/// layer above.
pub const DETAIL_TILE_METRES: f32 = 8.0;

/// The region width, in metres, a standard (non-var) region has.
pub const DEFAULT_REGION_WIDTH_METRES: f32 = 256.0;

impl Material for TerrainMaterial {
    /// Use the bundled terrain shader for the vertex stage (it carries the blend
    /// weights through to the fragment stage).
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(TERRAIN_SHADER_HANDLE)
    }

    /// Use the bundled terrain shader for the fragment stage (it blends the four
    /// detail textures by the interpolated weights).
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(TERRAIN_SHADER_HANDLE)
    }

    /// Pin the vertex buffer layout to position / normal / UV0 plus the custom
    /// [`ATTRIBUTE_TERRAIN_WEIGHTS`] weight attribute, matching the shader's
    /// vertex `@location`s.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
            ATTRIBUTE_TERRAIN_WEIGHTS.at_shader_location(3),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        Ok(())
    }
}

/// The plugin that registers the [`TerrainMaterial`] and loads its shader. Add
/// it to a Bevy [`App`] (after `DefaultPlugins`) to render terrain with the
/// four-way texture splat.
#[derive(Debug, Default)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `TerrainMaterialPlugin`, where the name reads clearly"
)]
pub struct TerrainMaterialPlugin;

impl Plugin for TerrainMaterialPlugin {
    fn build(&self, app: &mut App) {
        crate::sky_lighting::load_sky_lighting(app);
        load_internal_asset!(
            app,
            TERRAIN_SHADER_HANDLE,
            "terrain.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
    }
}
