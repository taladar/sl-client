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
use bevy::math::{Affine2, Vec4};
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
pub(crate) const SL_FACE_MODE_LEGACY: u32 = 1;
/// PBR / plain-diffuse mode: no added highlight (the base material is the whole
/// surface); per-map UV transforms may still apply via [`SlFaceParams::map_flags`].
pub(crate) const SL_FACE_MODE_PBR: u32 = 0;

/// [`SlFaceParams::map_flags`] bit: re-sample the normal map at [`uv_normal`](SlFaceParams::uv_normal).
pub(crate) const MAP_FLAG_NORMAL: u32 = 1 << 0;
/// [`SlFaceParams::map_flags`] bit: re-sample the metallic-roughness (ORM) map at
/// [`uv_mr`](SlFaceParams::uv_mr).
pub(crate) const MAP_FLAG_MR: u32 = 1 << 1;
/// [`SlFaceParams::map_flags`] bit: re-sample the emissive map at [`uv_emissive`](SlFaceParams::uv_emissive).
pub(crate) const MAP_FLAG_EMISSIVE: u32 = 1 << 2;
/// [`SlFaceParams::map_flags`] bit: sample the legacy specular map (extension slot)
/// at [`uv_spec`](SlFaceParams::uv_spec).
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
    /// GPU-driven texture animation (P28.2), timing params: `(rate, start, length,
    /// start_time)`. The shader derives the frame from `globals.time - start_time`,
    /// so the material's UV is animated **on the GPU** and this data is written
    /// **once** (on start / re-parameterisation) rather than every frame — avoiding
    /// a per-frame material re-prepare storm. Inert (`anim_mode == 0`) leaves it
    /// unused.
    pub(crate) anim_params: Vec4,
    /// Texture-animation fall-back placement — the face's **static** texture-entry
    /// `(rotation, offset_s, offset_t, scale_s)`, used for whichever placement
    /// components the animation does not drive (the port of the reference viewer's
    /// per-face fall-back). `scale_t` lives in [`anim_grid`](Self::anim_grid).
    pub(crate) anim_static: Vec4,
    /// Texture-animation flip-book grid + `scale_t`: `(size_x, size_y, scale_t,
    /// unused)`. A non-zero `size_x`/`size_y` pages a `size_x × size_y` sprite grid.
    pub(crate) anim_grid: Vec4,
    /// Render mode ([`SL_FACE_MODE_PBR`] / [`SL_FACE_MODE_LEGACY`]).
    pub(crate) mode: u32,
    /// Which maps to re-sample at their own UV (the `MAP_FLAG_*` bitset).
    pub(crate) map_flags: u32,
    /// Texture-animation mode bits (`texture_anim_mode::*`); `0` = no animation. The
    /// `ON` bit gates the GPU animation path in the shader.
    pub(crate) anim_mode: u32,
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
            anim_params: Vec4::ZERO,
            anim_static: Vec4::ZERO,
            anim_grid: Vec4::ZERO,
            mode: SL_FACE_MODE_PBR,
            map_flags: 0,
            anim_mode: 0,
            glossiness: 0.0,
            env_intensity: 0.0,
        }
    }

    /// Pack an [`Affine2`]'s 2×2 linear part into a `Vec4` (`col0.xy, col1.xy`), the
    /// shader's `mat2x2` layout.
    fn matrix_of(affine: Affine2) -> Vec4 {
        let matrix = affine.matrix2;
        Vec4::new(
            matrix.x_axis.x,
            matrix.x_axis.y,
            matrix.y_axis.x,
            matrix.y_axis.y,
        )
    }

    /// Set the PBR per-map UV transforms (normal / metallic-roughness / emissive),
    /// each already composed onto the face's diffuse placement, packing their
    /// linear parts and translations into the uniform. The specular translation
    /// (`uv_translations_b.zw`, legacy) is left untouched.
    pub(crate) fn set_pbr_transforms(&mut self, normal: Affine2, mr: Affine2, emissive: Affine2) {
        self.uv_normal_mat = Self::matrix_of(normal);
        self.uv_mr_mat = Self::matrix_of(mr);
        self.uv_emissive_mat = Self::matrix_of(emissive);
        self.uv_translations_a = Vec4::new(
            normal.translation.x,
            normal.translation.y,
            mr.translation.x,
            mr.translation.y,
        );
        self.uv_translations_b = Vec4::new(
            emissive.translation.x,
            emissive.translation.y,
            self.uv_translations_b.z,
            self.uv_translations_b.w,
        );
    }

    /// Set the **legacy Blinn-Phong** specular workflow: switch to
    /// [`SL_FACE_MODE_LEGACY`] (the shader adds the analytic normalized Blinn-Phong
    /// lobe over the matte base), store the specular highlight tint, glossiness
    /// (`specular_exponent / 255`) and environment intensity
    /// (`environment_intensity / 255`), and pack the normal- and specular-map UV
    /// transforms (each built from the map's own offset / repeat / rotation and
    /// applied to the raw face UV, independent of the diffuse placement — the
    /// reference viewer's per-channel `xform`). The normal transform reuses the
    /// [`uv_normal_mat`](Self::uv_normal_mat) slot the PBR path also uses (a legacy
    /// face is never a PBR face), and the specular transform its own
    /// [`uv_spec_mat`](Self::uv_spec_mat) slot. The `map_flags` re-sample bits are
    /// set later, as each map uploads.
    pub(crate) fn set_legacy(
        &mut self,
        specular_color: [f32; 3],
        glossiness: f32,
        env_intensity: f32,
        normal: Affine2,
        specular: Affine2,
    ) {
        self.mode = SL_FACE_MODE_LEGACY;
        let [r, g, b] = specular_color;
        self.specular_color = Vec4::new(r, g, b, 1.0);
        self.glossiness = glossiness;
        self.env_intensity = env_intensity;
        self.uv_normal_mat = Self::matrix_of(normal);
        self.uv_spec_mat = Self::matrix_of(specular);
        self.uv_translations_a = Vec4::new(
            normal.translation.x,
            normal.translation.y,
            self.uv_translations_a.z,
            self.uv_translations_a.w,
        );
        self.uv_translations_b = Vec4::new(
            self.uv_translations_b.x,
            self.uv_translations_b.y,
            specular.translation.x,
            specular.translation.y,
        );
    }
}

impl Default for SlFaceParams {
    /// The inert params (the fallback value in the bindless data array).
    fn default() -> Self {
        Self::inert()
    }
}

/// The face material's extension: the per-map UV transforms + legacy specular
/// scalars ([`SlFaceParams`]) and the extension's maps (each its own binding,
/// since `StandardMaterial` has one shared UV transform).
///
/// **Bindless.** The extension is declared bindless (mirroring the official
/// `extended_material_bindless` example) so `ExtendedMaterial<StandardMaterial,
/// SlFaceExt>` stays bindless — without it, forcing the whole material
/// non-bindless loses Bevy's cross-material draw-call batching and a busy scene
/// (thousands of distinct face materials) drops to a crawl. Following the example:
/// bindless index-table slots start at **50** and the extension's own bindings at
/// **100**, both clear of every `StandardMaterial` slot/binding. `#[data(50, …)]`
/// packs the whole extension into the [`SlFaceParams`] data array (via the
/// [`From`] below); the four maps take slots 51–58.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[data(50, SlFaceParams, binding_array(101))]
#[bindless(index_table(range(50..59), binding(100)))]
pub(crate) struct SlFaceExt {
    /// The per-map transforms and legacy specular scalars (packed into the bindless
    /// data array at slot 50 via [`From<&SlFaceExt>`](SlFaceParams)).
    pub(crate) params: SlFaceParams,
    /// The legacy `LLMaterial` specular map (RGB specular colour), or a default
    /// (fallback white) handle when the face carries none. Sampled only when
    /// [`MAP_FLAG_SPEC`] is set.
    #[texture(51)]
    #[sampler(52)]
    pub(crate) specular_map: Handle<Image>,
    /// The PBR normal map, sampled at [`uv_normal`](SlFaceParams::uv_normal) when
    /// [`MAP_FLAG_NORMAL`] is set. The PBR maps live in the extension (not the base
    /// `StandardMaterial`) so they can be sampled at their own per-map UV transform;
    /// the base then carries only the base-colour texture and the scalar factors,
    /// which the extension multiplies its samples by.
    #[texture(53)]
    #[sampler(54)]
    pub(crate) normal_map: Handle<Image>,
    /// The PBR metallic-roughness (ORM) map, sampled at [`uv_mr`](SlFaceParams::uv_mr)
    /// when [`MAP_FLAG_MR`] is set (green = roughness, blue = metallic, red =
    /// occlusion).
    #[texture(55)]
    #[sampler(56)]
    pub(crate) metallic_roughness_map: Handle<Image>,
    /// The PBR emissive map, sampled at [`uv_emissive`](SlFaceParams::uv_emissive)
    /// when [`MAP_FLAG_EMISSIVE`] is set.
    #[texture(57)]
    #[sampler(58)]
    pub(crate) emissive_map: Handle<Image>,
}

impl From<&SlFaceExt> for SlFaceParams {
    /// The GPU data for the bindless `#[data(50, …)]` array: just the extension's
    /// [`SlFaceParams`] (the maps are bound separately by index).
    fn from(extension: &SlFaceExt) -> Self {
        extension.params
    }
}

impl SlFaceExt {
    /// The inert extension — renders identically to a bare [`StandardMaterial`].
    pub(crate) fn inert() -> Self {
        Self {
            params: SlFaceParams::inert(),
            specular_map: Handle::default(),
            normal_map: Handle::default(),
            metallic_roughness_map: Handle::default(),
            emissive_map: Handle::default(),
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
