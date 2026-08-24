//! P27.4 — the legacy per-face bump / shiny / glow / fullbright surface flags,
//! mapped onto Bevy [`StandardMaterial`] approximations.
//!
//! A `TextureEntry` face carries four legacy surface effects packed into its
//! `bump_shiny_fullbright` byte plus the separate `glow` scalar (the pre-PBR
//! per-face material controls, distinct from the P27.1 GLTF materials and the
//! P27.3 `LLMaterial` normal/specular maps):
//!
//! * **Fullbright** — the face is unlit (shown at full texture brightness,
//!   ignoring scene lighting). Maps exactly onto [`StandardMaterial::unlit`].
//! * **Glow** (0..1) — the face emits into the reference viewer's glow buffer and
//!   blooms. Handled by the faithful glow pass ([`crate::glow`]): the glow scalar is
//!   carried on the face material's [`SlFaceExt`](crate::face_material::SlFaceExt)
//!   extension and written into the scene alpha (the per-face glow mask), which
//!   [`crate::glow`] extracts, blurs, and adds back — the port of the reference
//!   `RenderGlow` pipeline. So `apply_surface_flags` does **not** touch it (it is
//!   no longer approximated as `emissive`). The glow is uniform across the face
//!   rather than following the texture, a documented approximation.
//! * **Shiny** (none / low / medium / high) — an environment-reflection specular
//!   sheen. The reference packs it as an environment intensity
//!   (`SHININESS_TO_ALPHA` = `[0, .25, .5, .75]`) that a cube-map shiny pass
//!   reflects. This viewer has no reflection probe (a metallic surface would read
//!   black), so shiny is approximated with an *analytic-light* highlight instead:
//!   raise [`StandardMaterial::reflectance`] and lower
//!   [`StandardMaterial::perceptual_roughness`] with the shiny level, so the sun /
//!   moon directional light throws a progressively sharper, brighter specular
//!   highlight. Metallic is left at zero.
//! * **Bump** (brightness / darkness / one of the standard emboss textures) — a
//!   per-face surface relief. This module generates a tangent-space **normal
//!   map** from a source texture's luminance (Sobel height field) and drops it
//!   into [`StandardMaterial::normal_map_texture`] so the sun lights the relief.
//!   The source depends on the bump code, matching the reference: **brightness**
//!   / **darkness** derive from the face's own diffuse texture (darkness inverts
//!   the height field), while the **standard emboss** codes (≥ 3 — woodgrain,
//!   bark, bricks, …) fetch their fixed Linden bump texture
//!   (`STANDARD_BUMP_TEXTURES`, the reference's `std_bump.ini`) through the
//!   shared texture manager and derive the normal from that.
//!
//! The scalar effects (fullbright / glow / shiny) are written straight onto each
//! face's material as it is built, by `apply_surface_flags` called from
//! `face_material`. Bump needs the decoded
//! diffuse pixels, so it runs as a small fetch/generate pipeline like the P27.3
//! normal-map path: [`register_bump_faces`] parks each newly-spawned bumped face
//! on its diffuse texture id, and [`apply_bump_normals`] generates and assigns the
//! normal map once that texture decodes.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sl_client_bevy::{DecodedTexture, Priority, TextureFace, TextureKey, Uuid};

use crate::face_material::FaceMaterial;
use crate::materials::ObjectRenderMaterials;
use crate::objects::{FaceTextureDebug, PrimFaceEntity};
use crate::textures::{TextureApplyBudget, TextureManager};
use crate::world_api::TERRAIN_BOOST_PRIORITY;

/// The reference viewer's `SHININESS_TO_ALPHA` table (`llface.cpp`): the
/// environment-reflection intensity for each of the four shiny levels (none / low
/// / medium / high). Drives the analytic-light highlight approximation below.
const SHININESS_TO_ENV: [f32; 4] = [0.0, 0.25, 0.5, 0.75];

/// The `perceptual_roughness` a non-shiny face keeps (the diffuse default set in
/// `face_material`); the smoothest (high-shiny)
/// face drops to [`SHINY_ROUGHNESS_MIN`].
const SHINY_ROUGHNESS_MAX: f32 = 0.9;

/// The `perceptual_roughness` of the highest shiny level — smooth enough for a
/// crisp specular highlight without a mirror-hard edge.
const SHINY_ROUGHNESS_MIN: f32 = 0.15;

/// The `reflectance` bump applied at full environment intensity (added to Bevy's
/// dielectric default of `0.5`, reaching `1.0` at high shiny), so a shiny face
/// throws a brighter highlight.
const SHINY_REFLECTANCE_GAIN: f32 = 0.5;

/// The per-texel height scale of the generated bump normal map: larger values
/// tilt the surface normals more, deepening the relief.
const BUMP_NORMAL_STRENGTH: f32 = 2.0;

/// The fetch priority a bump face's source texture is (re)requested at, so its
/// normal map can be generated promptly (a modest boost, like a material map).
const BUMP_TEXTURE_PRIORITY: Priority = TERRAIN_BOOST_PRIORITY;

/// The `bump_shiny_fullbright` low-bits code for the brightness emboss bump (a
/// normal generated from the face's diffuse luminance, upright).
const BE_BRIGHTNESS: u8 = 1;

/// The code for the darkness emboss bump (as brightness, but the height field is
/// inverted — dark pixels read as raised).
const BE_DARKNESS: u8 = 2;

/// The first standard-emboss bump code (the reference's `BE_STANDARD_0`); codes
/// from here up index `STANDARD_BUMP_TEXTURES`.
const BE_STANDARD_0: u8 = 3;

/// The fixed Linden standard-emboss bump textures, in bump-code order starting at
/// [`BE_STANDARD_0`] (`3` = woodgrain … `17` = weave). Straight from the reference
/// viewer's `app_settings/std_bump.ini`; each is a greyscale height/emboss pattern
/// the normal map is generated from. A face with one of these bump codes shows its
/// own diffuse as base colour but takes its relief from the matching texture here.
const STANDARD_BUMP_TEXTURES: [Uuid; 15] = [
    Uuid::from_u128(0x058c_75c0_a0d5_f2f8_43f3_e969_9a89_c2fc), // woodgrain
    Uuid::from_u128(0x6c9f_a78a_1c69_2168_325b_3e03_ffa3_48ce), // bark
    Uuid::from_u128(0xb8ee_d5f0_64b7_6e12_b67f_43fa_8e77_3440), // bricks
    Uuid::from_u128(0x9dea_b416_9c63_78d6_d558_9a15_6f12_044c), // checker
    Uuid::from_u128(0xdb9d_39ec_a896_c287_1ced_6456_6217_021e), // concrete
    Uuid::from_u128(0xf2d7_b6f6_4200_1e9a_fd5b_9645_9e95_0f94), // crustytile
    Uuid::from_u128(0xd925_8671_868f_7511_c321_7bae_f9e9_48a4), // cutstone
    Uuid::from_u128(0xd21e_44ca_ff1c_a96e_b2ef_c075_3426_b7d9), // discs
    Uuid::from_u128(0x4726_f13e_bd07_f2fb_feb0_bfa2_ac58_ab61), // gravel
    Uuid::from_u128(0xe569_711a_27c2_aad4_9246_0c91_0239_a179), // petridish
    Uuid::from_u128(0x073c_9723_540c_5449_cdd4_0e87_fdc1_59e3), // siding
    Uuid::from_u128(0xae87_4d1a_93ef_54fb_5fd3_eb0c_b156_afc0), // stonetile
    Uuid::from_u128(0x92e6_6e00_f56f_598a_7997_048a_a64c_de18), // stucco
    Uuid::from_u128(0x83b7_7fc6_10b4_63ec_4de7_f406_29f2_38c5), // suction
    Uuid::from_u128(0x7351_98cf_6ea0_2550_e222_21d3_c6a3_41ae), // weave
];

/// The standard-emboss bump texture for a bump code, or `None` for a code below
/// [`BE_STANDARD_0`] or past the end of the table (an unknown/reserved code).
fn standard_bump_texture(code: u8) -> Option<TextureKey> {
    let index = usize::from(code.checked_sub(BE_STANDARD_0)?);
    STANDARD_BUMP_TEXTURES
        .get(index)
        .copied()
        .map(TextureKey::from)
}

/// The reference environment intensity for a shiny level (`0` = none .. `3` =
/// high), clamped into the `SHININESS_TO_ENV` table's range.
fn shiny_env(shiny: u8) -> f32 {
    SHININESS_TO_ENV
        .get(usize::from(shiny.min(3)))
        .copied()
        .unwrap_or(0.0)
}

/// The `perceptual_roughness` for a shiny level (`0` = none .. `3` = high): a
/// linear ramp from the diffuse default down to the smoothest level, so a shinier
/// face reflects a sharper, brighter analytic highlight. Level `0` returns the
/// untouched default.
fn roughness_from_shiny(shiny: u8) -> f32 {
    let env = shiny_env(shiny);
    // env in [0, 0.75]; scale it onto the [MAX, MIN] roughness range.
    let span = SHINY_ROUGHNESS_MAX - SHINY_ROUGHNESS_MIN;
    SHINY_ROUGHNESS_MAX - env * (span / 0.75)
}

/// The `reflectance` for a shiny level: Bevy's `0.5` dielectric default plus a
/// gain scaled by the level's environment intensity (reaching `1.0` at high
/// shiny). Level `0` returns the untouched default.
fn reflectance_from_shiny(shiny: u8) -> f32 {
    let env = shiny_env(shiny);
    0.5 + env * (SHINY_REFLECTANCE_GAIN / 0.75)
}

/// Apply a face's legacy scalar surface flags — fullbright and shiny — onto the
/// [`StandardMaterial`] being built for it. Bump is handled separately (it needs the
/// decoded diffuse) by [`register_bump_faces`] / [`apply_bump_normals`], and **glow**
/// is handled by the faithful glow pass ([`crate::glow`]): the face's glow scalar is
/// carried on the [`SlFaceExt`](crate::face_material::SlFaceExt) extension and
/// written into the scene alpha (the glow mask) by `face_material.wgsl`, so it is not
/// set here. A face with none of these flags set is left untouched.
pub(crate) fn apply_surface_flags(material: &mut StandardMaterial, face: &TextureFace) {
    if face.fullbright() {
        material.unlit = true;
    }
    let shiny = face.shininess();
    if shiny > 0 {
        material.perceptual_roughness = roughness_from_shiny(shiny);
        material.reflectance = reflectance_from_shiny(shiny);
        // Left dielectric (metallic 0): with no reflection probe a metallic face
        // would read black, so shiny is an analytic-light highlight, not a mirror.
    }
}

/// The generated bump-normal-map pipeline: a cache of normal maps by
/// `(diffuse texture id, inverted?)` and the face materials waiting on a texture
/// to decode so their normal map can be generated.
#[derive(Debug, Resource, Default)]
pub struct BumpManager {
    /// Normal maps generated from each diffuse texture, keyed by the texture id and
    /// whether the height field was inverted (the darkness bump code), so a texture
    /// shared by many bumped faces is turned into a normal map once per polarity.
    normals: HashMap<(TextureKey, bool), Handle<Image>>,
    /// Face materials parked on a diffuse texture id, each with whether its bump
    /// code inverts the height field, applied once the texture decodes.
    pending: HashMap<TextureKey, Vec<(Handle<FaceMaterial>, bool)>>,
}

impl BumpManager {
    /// The generated normal-map [`Image`] for `id` at the requested polarity,
    /// building it from the decoded diffuse on first use and caching it.
    fn normal_image(
        &mut self,
        images: &mut Assets<Image>,
        id: TextureKey,
        invert: bool,
        decoded: &Arc<DecodedTexture>,
    ) -> Handle<Image> {
        if let Some(handle) = self.normals.get(&(id, invert)) {
            return handle.clone();
        }
        let handle = images.add(generate_normal_map(decoded, invert));
        let _inserted = self.normals.insert((id, invert), handle.clone());
        handle
    }
}

/// Register each newly-spawned face carrying a legacy bump code with the
/// [`BumpManager`], parking its material on the texture the normal is derived from
/// (its own diffuse for brightness / darkness, the fixed standard-emboss texture
/// for codes ≥ 3) and requesting that texture so the normal map is generated once
/// it decodes. Skips a face that:
///
/// * has no bump code (the common case),
/// * has no source texture to derive relief from (nil diffuse for brightness /
///   darkness, or an unknown standard code),
/// * carries a legacy `LLMaterial` id (P27.3 supplies that face's real normal
///   map — bump is superseded), or
/// * has a PBR GLTF material (P27.1, which supersedes the legacy surface flags as
///   in the reference viewer).
pub fn register_bump_faces(
    mut manager: ResMut<BumpManager>,
    mut textures: ResMut<TextureManager>,
    new_faces: Query<
        (
            &MeshMaterial3d<FaceMaterial>,
            &PrimFaceEntity,
            &FaceTextureDebug,
            &ChildOf,
        ),
        Added<PrimFaceEntity>,
    >,
    pbr_holders: Query<&ObjectRenderMaterials>,
) {
    for (material, face, FaceTextureDebug(texture_face), child_of) in &new_faces {
        let bump = texture_face.bumpmap();
        if bump == 0 {
            continue;
        }
        // A legacy `LLMaterial` on this face supplies its own normal map (P27.3).
        if texture_face.material_id.is_some_and(|id| !id.is_nil()) {
            continue;
        }
        // A PBR GLTF material on this face supersedes the legacy surface flags.
        let face_index = face.face_id.as_usize();
        if let Ok(holder) = pbr_holders.get(child_of.parent())
            && holder
                .faces
                .iter()
                .any(|(index, _id)| usize::from(*index) == face_index)
        {
            continue;
        }
        // Resolve the texture the normal is derived from and whether the height
        // field inverts: brightness → the diffuse (upright), darkness → the
        // diffuse (inverted), a standard code → its fixed emboss texture (upright).
        let (source, invert) = match bump {
            BE_BRIGHTNESS => (texture_face.texture_id, false),
            BE_DARKNESS => (texture_face.texture_id, true),
            code => match standard_bump_texture(code) {
                Some(id) => (id, false),
                None => continue,
            },
        };
        if source.uuid().is_nil() {
            continue;
        }
        manager
            .pending
            .entry(source)
            .or_default()
            .push((material.0.clone(), invert));
        // Request the source texture: the diffuse is normally already in flight
        // (from `face_material`), but a standard-emboss bump texture is fetched
        // only here — boosted so it decodes at full resolution.
        textures.request_boosted(source, BUMP_TEXTURE_PRIORITY);
    }
}

/// Generate and drop a bump normal map into each face material parked on a diffuse
/// texture that has decoded: build the normal map from the texture's luminance
/// (once per texture/polarity) and set it as the material's normal map. Drains
/// parked faces for any texture that has decoded (freshly or already cached), so it
/// needs no decode message.
pub fn apply_bump_normals(
    mut manager: ResMut<BumpManager>,
    textures: Res<TextureManager>,
    mut budget: ResMut<TextureApplyBudget>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    let ready: Vec<TextureKey> = manager
        .pending
        .keys()
        .filter(|id| textures.decoded(**id).is_some())
        .copied()
        .collect();
    for id in ready {
        let Some(decoded) = textures.decoded(id).map(Arc::clone) else {
            continue;
        };
        let parked = manager.pending.remove(&id).unwrap_or_default();
        debug!(
            "generating bump normal map for {} face(s) from texture {}",
            parked.len(),
            id.uuid()
        );
        // Overflow past this frame's shared image budget re-parks for a later frame,
        // so a cache-warm burst of generated normal maps does not upload every map at
        // once (the serial `extract_render_asset<GpuImage>` spike).
        let mut deferred: Vec<(Handle<FaceMaterial>, bool)> = Vec::new();
        for (handle, invert) in parked {
            // A generated normal map already uploaded for this (id, polarity) is free
            // to reuse; only a first-use build spends image budget. Defer the uncached
            // faces once the budget is spent — cached ones still apply for free.
            let cached = manager.normals.contains_key(&(id, invert));
            if !cached && !budget.take_image() {
                deferred.push((handle, invert));
                continue;
            }
            let image = manager.normal_image(&mut images, id, invert, &decoded);
            if let Some(mut material) = materials.get_mut(&handle) {
                material.base.normal_map_texture = Some(image);
            }
        }
        if !deferred.is_empty() {
            manager.pending.entry(id).or_default().extend(deferred);
        }
    }
}

/// Build a tangent-space normal map [`Image`] from a decoded diffuse texture,
/// treating pixel luminance as a height field: the per-texel luminance gradient
/// (central differences, wrapping at the edges to match the repeating face
/// sampler) tilts each surface normal. `invert` negates the height (the darkness
/// bump code — dark pixels read as raised). The map is linear (`Rgba8Unorm`) and
/// tiles with the same repeating sampler as the diffuse.
pub(crate) fn generate_normal_map(decoded: &Arc<DecodedTexture>, invert: bool) -> Image {
    let width = decoded.width.max(1);
    let height = decoded.height.max(1);
    let w = usize::try_from(width).unwrap_or(1).max(1);
    let h = usize::try_from(height).unwrap_or(1).max(1);

    // Luminance height field (0..1), one sample per RGBA8 texel. `as_chunks`
    // drops any trailing partial texel; a short slice yields fewer samples, which
    // the wrapping fetch below treats as zero height.
    let lum: Vec<f32> = decoded
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&[r, g, b, _]| {
            (0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b)) / 255.0
        })
        .collect();
    // The luminance sample at wrapping coordinates (the face tiles its texture).
    let height_at = |x: usize, y: usize| -> f32 {
        let index = y.saturating_mul(w).saturating_add(x);
        lum.get(index).copied().unwrap_or(0.0)
    };

    let sign = if invert { -1.0 } else { 1.0 };
    let mut data: Vec<u8> = Vec::with_capacity(w.saturating_mul(h).saturating_mul(4));
    for y in 0..h {
        for x in 0..w {
            // Wrapping neighbours.
            let left = height_at(wrap_prev(x, w), y);
            let right = height_at(wrap_next(x, w), y);
            let up = height_at(x, wrap_prev(y, h));
            let down = height_at(x, wrap_next(y, h));
            let dx = (right - left) * sign * BUMP_NORMAL_STRENGTH;
            let dy = (down - up) * sign * BUMP_NORMAL_STRENGTH;
            // Surface normal of the height field: (-dh/dx, -dh/dy, 1), normalised.
            let inv_len = 1.0 / (dx * dx + dy * dy + 1.0).sqrt();
            let nx = -dx * inv_len;
            let ny = -dy * inv_len;
            let nz = inv_len;
            data.push(encode_normal_component(nx));
            data.push(encode_normal_component(ny));
            data.push(encode_normal_component(nz));
            data.push(255);
        }
    }

    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// The index of the previous element in a wrapping row/column of `n` (the last
/// element wraps to the first).
fn wrap_prev(i: usize, n: usize) -> usize {
    i.checked_sub(1).unwrap_or_else(|| n.saturating_sub(1))
}

/// The index of the next element in a wrapping row/column of `n` (past the end
/// wraps to the first).
const fn wrap_next(i: usize, n: usize) -> usize {
    let next = i.saturating_add(1);
    if next >= n { 0 } else { next }
}

/// Encode a normal component in `[-1, 1]` into an unsigned byte in `[0, 255]`
/// (the `n * 0.5 + 0.5` tangent-space normal-map convention).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to 0.0..=255.0 before truncation, so it fits u8"
)]
fn encode_normal_component(value: f32) -> u8 {
    ((value * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn standard_bump_lookup_covers_the_emboss_codes() {
        // No standard texture below BE_STANDARD_0 (those are diffuse-derived).
        assert!(standard_bump_texture(BE_BRIGHTNESS).is_none());
        assert!(standard_bump_texture(BE_DARKNESS).is_none());
        // The first standard code (3) is woodgrain; the last (17) is weave.
        assert_eq!(
            standard_bump_texture(BE_STANDARD_0),
            STANDARD_BUMP_TEXTURES
                .first()
                .copied()
                .map(TextureKey::from)
        );
        assert_eq!(
            standard_bump_texture(17),
            STANDARD_BUMP_TEXTURES.last().copied().map(TextureKey::from)
        );
        // A code past the table (reserved) resolves to nothing.
        assert!(standard_bump_texture(18).is_none());
        assert!(standard_bump_texture(31).is_none());
    }

    #[test]
    fn shiny_ramps_from_default_to_smooth() {
        // None keeps the diffuse default; each level is smoother than the last.
        assert!((roughness_from_shiny(0) - SHINY_ROUGHNESS_MAX).abs() < 1e-6);
        assert!((roughness_from_shiny(3) - SHINY_ROUGHNESS_MIN).abs() < 1e-6);
        assert!(roughness_from_shiny(1) > roughness_from_shiny(2));
        assert!(roughness_from_shiny(2) > roughness_from_shiny(3));
    }

    #[test]
    fn shiny_raises_reflectance() {
        // None keeps Bevy's dielectric default; high reaches the full gain.
        assert!((reflectance_from_shiny(0) - 0.5).abs() < 1e-6);
        assert!((reflectance_from_shiny(3) - (0.5 + SHINY_REFLECTANCE_GAIN)).abs() < 1e-6);
        assert!(reflectance_from_shiny(2) > reflectance_from_shiny(1));
    }

    #[test]
    fn surface_flags_set_fullbright_and_shiny() {
        let mut material = StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: SHINY_ROUGHNESS_MAX,
            ..default()
        };
        let mut face = TextureFace::new(TextureKey::from(Uuid::nil()));
        // Fullbright + high shiny both fold in. (Glow is no longer set here — it
        // rides the face-material extension into the glow-mask alpha, `crate::glow`.)
        face.bump_shiny_fullbright = 0b1110_0000; // shiny high (bits 6-7) + fullbright (bit 5)
        apply_surface_flags(&mut material, &face);
        assert!(material.unlit);
        assert!((material.perceptual_roughness - SHINY_ROUGHNESS_MIN).abs() < 1e-6);
    }

    #[test]
    fn normal_component_encoding_is_centred() {
        // A flat (z-up) normal encodes the up axis at full and the lateral axes at
        // the 0.5 midpoint (128).
        assert_eq!(encode_normal_component(1.0), 255);
        assert_eq!(encode_normal_component(0.0), 128);
        assert_eq!(encode_normal_component(-1.0), 0);
    }

    #[test]
    fn flat_texture_generates_flat_normals() {
        // A uniform-luminance texture has no gradient, so every normal points
        // straight up (0.5, 0.5, 1.0) → (128, 128, 255).
        let decoded = Arc::new(DecodedTexture::new(
            2,
            2,
            3,
            sl_client_bevy::DiscardLevel::FULL,
            bytes::Bytes::from(vec![200_u8; 2 * 2 * 4]),
            None,
        ));
        let image = generate_normal_map(&decoded, false);
        let data = image.data.unwrap_or_default();
        assert_eq!(data.get(..4), Some([128, 128, 255, 255].as_slice()));
    }
}
