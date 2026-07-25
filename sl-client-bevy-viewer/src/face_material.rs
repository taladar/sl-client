//! The custom Bevy material every Second Life / OpenSim **prim, mesh, sculpt,
//! tree, grass, rigged-attachment, avatar-BoM and media face** renders through:
//! an [`ExtendedMaterial<StandardMaterial, SlFaceExt>`] (aliased [`FaceMaterial`]).
//!
//! Bevy's [`StandardMaterial`] carries a single `uv_transform` for all texture
//! maps and has no Blinn-Phong specular workflow, so it cannot express two things
//! Second Life faces need: **per-map UV transforms** (PBR base-colour / normal /
//! metallic-roughness / emissive each carry their own `KHR_texture_transform`, and
//! a legacy `LLMaterial` normal / specular map each carry their own
//! offset/repeat/rotation) and the **legacy specular map + specular colour +
//! glossiness + environment** highlight. [`SlFaceExt`] adds exactly those as an
//! extension: extra uniform + specular-map bindings (indices 100+, clear of every
//! `StandardMaterial` binding) plus a fragment shader that re-samples the base
//! material's maps at their own UVs and, for a legacy face, adds a Blinn-Phong
//! specular lobe on top of the reused `StandardMaterial` PBR lighting.
//!
//! The extension is **inert** for a plain diffuse / avatar / not-yet-transformed
//! PBR face ([`SlFaceExt::inert`]): `mode = 0`, no re-sample flags, identity
//! transforms — so it renders bit-identically to a bare `StandardMaterial`. This
//! lets every face carry one stable [`FaceMaterial`] handle that the whole face
//! pipeline mutates in place (writing `.base` for the `StandardMaterial` fields it
//! always set, and `.extension` for the per-map transforms / legacy specular),
//! rather than swapping material types when a face flips between PBR and legacy.
//!
//! Modelled on [`crate`](sl_client_bevy)'s `WaterMaterial` (the repo's custom
//! `AsBindGroup` + `load_internal_asset!` + `MaterialPlugin` template). Register
//! [`SlFaceMaterialPlugin`] to load the shader and the material.

use bevy::asset::{Asset, Handle, load_internal_asset, uuid_handle};
use bevy::image::Image;
use bevy::math::{Affine2, Vec2, Vec4};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin, StandardMaterial};
use bevy::prelude::{App, Plugin};
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::{Shader, ShaderRef};

/// The internal handle the face shader (`face_material.wgsl`) is loaded under, so
/// the material references it without an on-disk asset path (the repo compiles
/// every shader in via `load_internal_asset!` rather than the `assets/` dir).
const FACE_SHADER_HANDLE: Handle<Shader> = uuid_handle!("6b1f0a92-4c3d-4e18-9f27-2a5d7c84e061");

/// The material every SL prim / mesh / rigged / avatar-BoM / media face renders
/// through: a [`StandardMaterial`] extended with [`SlFaceExt`]. The `.base` half
/// holds the `StandardMaterial` fields the face pipeline already sets (tint,
/// diffuse texture, alpha mode, the four PBR maps, factors); the `.extension` half
/// holds the per-map UV transforms and the legacy specular workflow.
pub(crate) type FaceMaterial = ExtendedMaterial<StandardMaterial, SlFaceExt>;

/// Legacy Blinn-Phong specular mode: the extension adds a specular highlight over
/// the reused PBR lighting.
#[expect(
    dead_code,
    reason = "wired up in the legacy-specular shader phase (Phase 2)"
)]
pub(crate) const SL_FACE_MODE_LEGACY: u32 = 1;
/// PBR / plain-diffuse mode: no added highlight (the base material is the whole
/// surface); per-map UV transforms may still apply via [`SlFaceParams::map_flags`].
pub(crate) const SL_FACE_MODE_PBR: u32 = 0;

/// [`SlFaceParams::map_flags`] bit: re-sample the normal map at [`uv_normal`](SlFaceParams::uv_normal).
#[expect(
    dead_code,
    reason = "wired up in the per-map-transform shader phase (Phase 1)"
)]
pub(crate) const MAP_FLAG_NORMAL: u32 = 1 << 0;
/// [`SlFaceParams::map_flags`] bit: re-sample the metallic-roughness (ORM) map at
/// [`uv_mr`](SlFaceParams::uv_mr).
#[expect(
    dead_code,
    reason = "wired up in the per-map-transform shader phase (Phase 1)"
)]
pub(crate) const MAP_FLAG_MR: u32 = 1 << 1;
/// [`SlFaceParams::map_flags`] bit: re-sample the emissive map at [`uv_emissive`](SlFaceParams::uv_emissive).
#[expect(
    dead_code,
    reason = "wired up in the per-map-transform shader phase (Phase 1)"
)]
pub(crate) const MAP_FLAG_EMISSIVE: u32 = 1 << 2;
/// [`SlFaceParams::map_flags`] bit: sample the legacy specular map (extension slot)
/// at [`uv_spec`](SlFaceParams::uv_spec).
#[expect(
    dead_code,
    reason = "wired up in the legacy-specular shader phase (Phase 2)"
)]
pub(crate) const MAP_FLAG_SPEC: u32 = 1 << 3;

/// The extension's uniform block: the per-map UV transforms (as a packed 2×2
/// matrix + translation each) and the legacy specular workflow scalars.
///
/// Only `f32`/`u32`/`Vec2`/`Vec4` are used (no `Vec3`), so the `encase` std140
/// layout the `ShaderType` derive produces matches the `face_material.wgsl`
/// `SlFaceParams` field-for-field without hand-inserted padding.
#[derive(Clone, Copy, Debug, ShaderType)]
pub(crate) struct SlFaceParams {
    /// Normal-map UV transform: the 2×2 linear part `(m00, m01, m10, m11)`.
    pub(crate) uv_normal_mat: Vec4,
    /// Metallic-roughness UV transform: the 2×2 linear part.
    pub(crate) uv_mr_mat: Vec4,
    /// Emissive UV transform: the 2×2 linear part.
    pub(crate) uv_emissive_mat: Vec4,
    /// Legacy specular-map UV transform: the 2×2 linear part.
    pub(crate) uv_spec_mat: Vec4,
    /// The legacy specular highlight tint (RGB) and, in `.w`, unused padding to a
    /// `Vec4`; the glossiness lives in [`glossiness`](Self::glossiness).
    pub(crate) specular_color: Vec4,
    /// The four maps' translations packed two per 16-byte slot:
    /// `(normal.x, normal.y, mr.x, mr.y)`.
    pub(crate) uv_translations_a: Vec4,
    /// `(emissive.x, emissive.y, spec.x, spec.y)`.
    pub(crate) uv_translations_b: Vec4,
    /// Render mode ([`SL_FACE_MODE_PBR`] / [`SL_FACE_MODE_LEGACY`]).
    pub(crate) mode: u32,
    /// Which maps to re-sample at their own UV (the `MAP_FLAG_*` bitset).
    pub(crate) map_flags: u32,
    /// Legacy glossiness `0..=1` (`specular_exponent / 255`), scaled per-texel by
    /// the normal-map alpha in the shader.
    pub(crate) glossiness: f32,
    /// Legacy environment-reflection intensity `0..=1` (`environment_intensity / 255`).
    pub(crate) env_intensity: f32,
}

impl SlFaceParams {
    /// The inert params: PBR mode, no re-sampling, identity transforms, no legacy
    /// specular — an extension that changes nothing.
    pub(crate) const fn inert() -> Self {
        let identity_mat = Vec4::new(1.0, 0.0, 0.0, 1.0);
        Self {
            uv_normal_mat: identity_mat,
            uv_mr_mat: identity_mat,
            uv_emissive_mat: identity_mat,
            uv_spec_mat: identity_mat,
            specular_color: Vec4::ONE,
            uv_translations_a: Vec4::ZERO,
            uv_translations_b: Vec4::ZERO,
            mode: SL_FACE_MODE_PBR,
            map_flags: 0,
            glossiness: 0.0,
            env_intensity: 0.0,
        }
    }

    /// Write one map slot's UV transform: the 2×2 linear part into `mat` and the
    /// translation into the two lanes of a packed translation `Vec4`.
    #[expect(
        dead_code,
        reason = "wired up in the per-map-transform shader phase (Phase 1)"
    )]
    pub(crate) fn set_transform(mat: &mut Vec4, translation: &mut Vec2, affine: Affine2) {
        let matrix = affine.matrix2;
        *mat = Vec4::new(
            matrix.x_axis.x,
            matrix.x_axis.y,
            matrix.y_axis.x,
            matrix.y_axis.y,
        );
        *translation = affine.translation;
    }
}

/// The face material's extension: the per-map UV transforms + legacy specular
/// scalars ([`SlFaceParams`]) and the legacy specular map (its own binding, since
/// [`StandardMaterial`] has no specular-map slot). Bindings start at 100 to stay
/// clear of every `StandardMaterial` binding (0–12, plus its feature-gated ones).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub(crate) struct SlFaceExt {
    /// The per-map transforms and legacy specular scalars.
    #[uniform(100)]
    pub(crate) params: SlFaceParams,
    /// The legacy `LLMaterial` specular map (RGB specular colour), or a default
    /// (fallback white) handle when the face carries none. Sampled only when
    /// [`MAP_FLAG_SPEC`] is set.
    #[texture(101)]
    #[sampler(102)]
    pub(crate) specular_map: Handle<Image>,
}

impl SlFaceExt {
    /// The inert extension — renders identically to a bare [`StandardMaterial`].
    pub(crate) fn inert() -> Self {
        Self {
            params: SlFaceParams::inert(),
            specular_map: Handle::default(),
        }
    }
}

impl Default for SlFaceExt {
    /// The inert extension (so `FaceMaterial::default()` is a plain default
    /// `StandardMaterial` with a do-nothing extension).
    fn default() -> Self {
        Self::inert()
    }
}

impl MaterialExtension for SlFaceExt {
    /// Shade with the bundled face shader (re-samples per-map UVs + adds the legacy
    /// specular lobe); the vertex stage stays the base `StandardMaterial` mesh one.
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(FACE_SHADER_HANDLE)
    }
}

/// Wrap a composed [`StandardMaterial`] in an inert [`FaceMaterial`] — the single
/// place face construction turns a `StandardMaterial` into the face material type,
/// so the pipeline keeps building `StandardMaterial`s and this adds the do-nothing
/// extension.
pub(crate) fn inert_face_material(base: StandardMaterial) -> FaceMaterial {
    FaceMaterial {
        base,
        extension: SlFaceExt::inert(),
    }
}

/// Loads the face shader and registers the [`FaceMaterial`]. Add once to the
/// [`App`] (after `DefaultPlugins`), like the sky / water material plugins.
#[derive(Debug, Default)]
pub(crate) struct SlFaceMaterialPlugin;

impl Plugin for SlFaceMaterialPlugin {
    /// Compile `face_material.wgsl` under [`FACE_SHADER_HANDLE`] and add the
    /// [`MaterialPlugin`] for the extended face material.
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            FACE_SHADER_HANDLE,
            "face_material.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins(MaterialPlugin::<FaceMaterial>::default());
    }
}
