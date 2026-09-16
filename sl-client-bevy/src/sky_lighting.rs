//! The **sky lighting** every lit legacy surface shares — the reference viewer's
//! `sunlit` and `amblit` (`calcAtmosphericVars`), carried to the GPU in one small
//! texture, plus the shader module (`sky_lighting.wgsl`) that lights a surface with
//! them the way the reference's deferred `softenLight` does.
//!
//! **Why a texture.** The reference lights every non-PBR surface — prims, meshes,
//! sculpts, avatars, trees, terrain — in one deferred pass, from one set of sky
//! uniforms. This viewer lights each in its own material shader, and a material
//! uniform is per material: writing the sky into thousands of face materials would
//! re-prepare every one of their bind groups each time the day cycle steps (every
//! fraction of a second on a live cycle). So the values live in one texture that
//! every such material binds by the same handle, [`SKY_LIGHTING_IMAGE`]. Rewriting
//! the texture's texels in place, at the same size, reuses the GPU texture it is
//! already uploaded to, so no material notices anything but the new values.
//!
//! **What a surface does with them** is the shader module's business; see
//! `sky_lighting.wgsl`. What this side decides is only the three numbers a frame
//! resolves to, and [`SkyLightingMode::Fallback`] — the texture's contents before
//! any sky has been resolved, and for good in an app with no sky at all (the
//! gallery, a test scene), where a surface keeps the lighting it had before this
//! module existed.

use bevy::app::App;
use bevy::asset::{Assets, Handle, RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::image::Image;
use bevy::math::Vec3;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::shader::Shader;

/// The internal handle the shared sky-lighting shader module is loaded under.
const SKY_LIGHTING_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("8e3b2f61-4d7a-4c19-b0e5-7a2c9d41f836");

/// The one sky-lighting texture every lit legacy surface binds. See the module
/// docs for why this is a texture and not a uniform.
pub const SKY_LIGHTING_IMAGE: Handle<Image> = uuid_handle!("c41a7e92-3b58-4f0d-8e6a-15d2b9c7f403");

/// How a surface is to use the sky lighting — the reference's `classic_mode`, plus
/// the state of having no sky at all.
///
/// Stored in the texture as a float (`0`, `1`, `2`), and compared in
/// `sky_lighting.wgsl` against the same numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `SkyLightingMode`, where the name reads clearly"
)]
pub enum SkyLightingMode {
    /// No sky has been resolved. A surface lights itself as it did before this
    /// module: a face through the stock physically based lighting, the terrain
    /// through its flat sun-plus-ambient.
    #[default]
    Fallback,
    /// An EEP sky (`classic_mode == 0`): linear light, the reflection probes
    /// supply the ambient.
    Eep,
    /// A legacy WindLight sky (`classic_mode == 1`): the reference's gamma-space
    /// "classic" combine, and the sky's own ambient rather than the probes'.
    Classic,
}

impl SkyLightingMode {
    /// The value the shader compares against.
    const fn encoded(self) -> f32 {
        match self {
            Self::Fallback => 0.0,
            Self::Eep => 1.0,
            Self::Classic => 2.0,
        }
    }
}

/// The sky lighting one frame resolves to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyLighting {
    /// The reference `sunlit` as `calcAtmosphericVarsLinear` returns it: the
    /// active body's bound colour, attenuated by the atmosphere its light crosses,
    /// converted to linear for an EEP sky (a classic one's is left as it is) and
    /// scaled by `sky_sunlight_scale`. The shader applies what `softenLight` does
    /// to it afterwards.
    pub sunlit: Vec3,
    /// The reference `amblit` as `calcAtmosphericVars` returns it, **before** the
    /// per-fragment `ambientLighting` term and the EEP conversion to linear, both
    /// of which depend on the fragment and so are the shader's.
    pub amblit: Vec3,
    /// The sky's `reflection_probe_ambiance` — how much of an EEP surface's ambient
    /// comes from the reflection probes rather than from `amblit`.
    pub probe_ambiance: f32,
    /// Which of the reference's two lighting models, if either, applies.
    pub mode: SkyLightingMode,
}

impl SkyLighting {
    /// The lighting before any sky has been resolved. A face ignores the colours
    /// in this mode; the terrain lights itself with them as a plain sun and
    /// ambient, the flat light it always fell back to.
    pub const FALLBACK: Self = Self {
        sunlit: Vec3::splat(0.8),
        amblit: Vec3::splat(0.35),
        probe_ambiance: 0.0,
        mode: SkyLightingMode::Fallback,
    };

    /// The texture bytes: two `Rgba16Float` texels, `(sunlit, mode)` and
    /// `(amblit, probe_ambiance)`, little-endian.
    fn texels(&self) -> Vec<u8> {
        [
            self.sunlit.x,
            self.sunlit.y,
            self.sunlit.z,
            self.mode.encoded(),
            self.amblit.x,
            self.amblit.y,
            self.amblit.z,
            self.probe_ambiance,
        ]
        .into_iter()
        .flat_map(half_float_bytes)
        .collect()
    }
}

/// `value` as a little-endian IEEE 754 half float, for an `Rgba16Float` texel.
///
/// Built from the bits of the `f32` rather than through a numeric cast (which the
/// workspace denies). Only what sky lighting needs is represented: a negative or
/// NaN value writes `0`, a value past the half-float range writes its largest
/// finite value, and one too small for a normal half writes `0`. The mantissa is
/// truncated, an error below one part in a thousand.
fn half_float_bytes(value: f32) -> [u8; 2] {
    /// The largest finite half float, `65504`.
    const HALF_MAX: u32 = 0x7BFF;
    let value = value.max(0.0);
    let bits = value.to_bits();
    let biased = bits.checked_shr(23).unwrap_or(0) & 0xFF;
    let exponent = i32::try_from(biased).unwrap_or(0).saturating_sub(127);
    let half = if value == 0.0 || exponent < -14 {
        0
    } else if exponent > 15 {
        HALF_MAX
    } else {
        let half_exponent = u32::try_from(exponent.saturating_add(15)).unwrap_or(0);
        let half_mantissa = (bits & 0x7F_FFFF).checked_shr(13).unwrap_or(0);
        half_exponent.checked_shl(10).unwrap_or(0) | half_mantissa
    };
    [
        u8::try_from(half & 0xFF).unwrap_or(0),
        u8::try_from(half.checked_shr(8).unwrap_or(0) & 0xFF).unwrap_or(0),
    ]
}

/// A fresh sky-lighting texture holding `lighting`.
fn sky_lighting_image(lighting: &SkyLighting) -> Image {
    Image::new(
        Extent3d {
            width: 2,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        lighting.texels(),
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

/// Write `lighting` into the shared texture, **in place**, and only if it differs
/// from what the texture already holds.
///
/// In place, because a same-size write reuses the GPU texture every material has
/// already bound (a replaced image would be a new one); and only on a change,
/// because even an in-place write re-uploads it. A texture that is missing — an app
/// that never loaded the module — is created.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `write_sky_lighting`, where the name reads clearly"
)]
pub fn write_sky_lighting(images: &mut Assets<Image>, lighting: &SkyLighting) {
    let texels = lighting.texels();
    let current = images
        .get(&SKY_LIGHTING_IMAGE)
        .and_then(|image| image.data.as_deref());
    if current == Some(texels.as_slice()) {
        return;
    }
    if let Some(mut image) = images.get_mut(&SKY_LIGHTING_IMAGE) {
        image.data = Some(texels);
        return;
    }
    let _inserted = images.insert(&SKY_LIGHTING_IMAGE, sky_lighting_image(lighting));
}

/// Load the shared sky-lighting shader module and seed [`SKY_LIGHTING_IMAGE`] with
/// the [`SkyLighting::FALLBACK`] lighting.
///
/// Idempotent, like `load_water_fog_shader`: each plugin whose material binds the
/// texture calls it from `build`. An existing texture is left alone, so a second
/// plugin's call cannot reset a sky the first has already written. An app with no
/// image assets at all (no renderer) only gets the shader module.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `load_sky_lighting`, where the name reads clearly"
)]
pub fn load_sky_lighting(app: &mut App) {
    load_internal_asset!(
        app,
        SKY_LIGHTING_SHADER_HANDLE,
        "sky_lighting.wgsl",
        Shader::from_wgsl
    );
    if let Some(mut images) = app.world_mut().get_resource_mut::<Assets<Image>>()
        && !images.contains(&SKY_LIGHTING_IMAGE)
    {
        let _inserted = images.insert(
            &SKY_LIGHTING_IMAGE,
            sky_lighting_image(&SkyLighting::FALLBACK),
        );
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{SkyLighting, SkyLightingMode, half_float_bytes};

    /// Known half-float encodings, including the ones the texture actually carries
    /// (the mode numbers) and the edges the encoder clamps at.
    #[test]
    fn half_floats_encode_as_ieee_754() {
        assert_eq!(half_float_bytes(0.0), [0x00, 0x00]);
        assert_eq!(half_float_bytes(1.0), [0x00, 0x3C]);
        assert_eq!(half_float_bytes(2.0), [0x00, 0x40]);
        assert_eq!(half_float_bytes(0.5), [0x00, 0x38]);
        assert_eq!(half_float_bytes(1.5), [0x00, 0x3E]);
        assert_eq!(half_float_bytes(65504.0), [0xFF, 0x7B]);
        assert_eq!(half_float_bytes(1.0e9), [0xFF, 0x7B]);
        assert_eq!(half_float_bytes(-1.0), [0x00, 0x00]);
        assert_eq!(half_float_bytes(f32::NAN), [0x00, 0x00]);
    }

    /// The texel layout the shader reads: `(sunlit, mode)`, `(amblit, ambiance)`.
    #[test]
    fn texels_pack_sunlit_mode_then_amblit_ambiance() {
        let lighting = SkyLighting {
            sunlit: bevy::math::Vec3::new(1.0, 2.0, 0.5),
            amblit: bevy::math::Vec3::new(0.5, 1.0, 2.0),
            probe_ambiance: 1.5,
            mode: SkyLightingMode::Classic,
        };
        assert_eq!(
            lighting.texels(),
            vec![
                0x00, 0x3C, 0x00, 0x40, 0x00, 0x38, 0x00, 0x40, //
                0x00, 0x38, 0x00, 0x3C, 0x00, 0x40, 0x00, 0x3E,
            ]
        );
    }
}
