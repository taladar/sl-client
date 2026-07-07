//! Bevy integration for the decoding [`TextureStore`](sl_texture::TextureStore):
//! a bridge from the store's decoded RGBA8 output to Bevy's [`Image`], and a
//! blocking-HTTP [`TextureFetcher`] so a Bevy app
//! (which has no async runtime of its own) can build and drive a store.
//!
//! Because the store's `get`/`request` are `async`, a Bevy app drives them by
//! `block_on`-ing on a task/thread (the crate already fetches on `std` threads);
//! the store's decode still runs off-thread on its own `rayon` pool.

use arc_swap::ArcSwapOption;
use async_trait::async_trait;
use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::math::{Affine2, Mat2, Vec2};
use bytes::Bytes;
use reqwest::StatusCode as ReqwestStatusCode;
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_proto::{TextureFace, TextureKey};
use sl_texture::{DecodedImage, FetchChunk, FetchError, RemoteTextureSource, TextureFetcher};
use wgpu_types::{Extent3d, TextureDimension, TextureFormat};

/// Converts a decoded RGBA8 texture into a Bevy [`Image`] (`Rgba8UnormSrgb`),
/// ready to insert into `Assets<Image>` and use as a rendered texture.
#[must_use]
pub fn to_bevy_image(decoded: &DecodedImage) -> Image {
    Image::new(
        Extent3d {
            width: decoded.width,
            height: decoded.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        decoded.pixels.to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// The per-face texture-placement transform of a [`TextureFace`] as a Bevy
/// [`Affine2`], ready to drop into a `StandardMaterial`'s `uv_transform` (which
/// the PBR shader applies to `ATTRIBUTE_UV_0` before sampling).
///
/// This is a faithful port of the reference viewer's `xform`
/// (`indra/newview/llface.cpp`), which maps each texture coordinate about the
/// **centre of the face** `(0.5, 0.5)`: recentre, rotate by the face rotation,
/// scale by the repeats (`scale_s` / `scale_t`), then offset (`offset_s` /
/// `offset_t`) and un-recentre. Repeats above one tile the texture; a rotation
/// spins it about the face centre; an offset slides it. The identity face
/// (unit repeats, zero offset/rotation) yields [`Affine2::IDENTITY`].
///
/// The transform is expressed directly as the affine that reproduces `xform`:
///
/// ```text
/// s' = (ms·cos)·s + (ms·sin)·t + (offset_s + 0.5 − 0.5·ms·(cos + sin))
/// t' = (−mt·sin)·s + (mt·cos)·t + (offset_t + 0.5 + 0.5·mt·(sin − cos))
/// ```
///
/// where `ms = scale_s`, `mt = scale_t`, and `cos` / `sin` are of
/// [`rotation`](TextureFace::rotation).
#[must_use]
pub fn texture_face_uv_transform(face: &TextureFace) -> Affine2 {
    let (sin, cos) = face.rotation.sin_cos();
    let (ms, mt) = (face.scale_s, face.scale_t);
    // Columns of the linear part (glam `Mat2` is column-major): the `s` column
    // is the response to the input `s`, the `t` column to the input `t`.
    let matrix2 = Mat2::from_cols(
        Vec2::new(ms * cos, -mt * sin),
        Vec2::new(ms * sin, mt * cos),
    );
    let translation = Vec2::new(
        face.offset_s + 0.5 - 0.5 * ms * (cos + sin),
        face.offset_t + 0.5 + 0.5 * mt * (sin - cos),
    );
    Affine2 {
        matrix2,
        translation,
    }
}

/// The **planar** texture coordinate of a vertex under `TEX_GEN_PLANAR` — a
/// faithful port of the reference viewer's `LLFace::planarProjection`
/// (`indra/newview/llface.cpp`).
///
/// A planar face does not use the volume's stored UVs; instead each vertex's
/// texture coordinate is projected from its `position` (in the object's local
/// Second Life space, first scaled by the object `scale`) onto the plane the
/// face `normal` defines. The projection axis is chosen from the normal so the
/// texture keeps a fixed world scale across the prim's faces. The returned
/// coordinate is in Second Life's bottom-up texture space — the same space as
/// the stored profile UVs — so the caller applies the identical downstream
/// handling (the `1 − v` flip and the per-face [`texture_face_uv_transform`],
/// which the reference viewer likewise applies *after* the projection).
#[must_use]
pub fn planar_texgen_uv(position: [f32; 3], normal: [f32; 3], scale: [f32; 3]) -> [f32; 2] {
    // The vertex position in object-relative units (Firestorm's `vec.mul(scale)`).
    let vec = [
        position[0] * scale[0],
        position[1] * scale[1],
        position[2] * scale[2],
    ];
    // Pick a binormal aligned to whichever axis the face least faces, so a
    // roughly axis-aligned face projects without degenerating.
    let d = normal[0];
    let binormal = if d >= 0.5 || d <= -0.5 {
        if d < 0.0 {
            [0.0, -1.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        }
    } else if normal[1] > 0.0 {
        [-1.0, 0.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    // tangent = binormal × normal.
    let tangent = [
        binormal[1] * normal[2] - binormal[2] * normal[1],
        binormal[2] * normal[0] - binormal[0] * normal[2],
        binormal[0] * normal[1] - binormal[1] * normal[0],
    ];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let u = 1.0 + (dot(binormal, vec) * 2.0 - 0.5);
    let v = -(dot(tangent, vec) * 2.0 - 0.5);
    [u, v]
}

/// A [`TextureFetcher`] over blocking `reqwest`, for
/// a Bevy app with no async runtime. It fetches `GetTexture` codestream byte
/// ranges; the capability URL is held in an [`ArcSwapOption`] so it can be
/// refreshed on a region change.
#[derive(Debug)]
pub struct BevyTextureFetcher {
    /// The shared blocking HTTP client.
    http: ReqwestBlockingClient,
    /// The current `GetTexture` capability URL, or `None` before caps arrive.
    cap_url: ArcSwapOption<String>,
}

impl BevyTextureFetcher {
    /// A fetcher with a freshly built blocking client and no capability URL yet.
    #[must_use]
    pub fn new() -> Self {
        let http = ReqwestBlockingClient::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_error| ReqwestBlockingClient::new());
        Self {
            http,
            cap_url: ArcSwapOption::empty(),
        }
    }

    /// Updates (or clears) the `GetTexture` capability URL.
    pub fn set_cap_url(&self, url: Option<String>) {
        self.cap_url.store(url.map(std::sync::Arc::new));
    }

    /// The URL a fetch of `id` from `source` targets: for a default texture the
    /// `GetTexture` capability queried by UUID, for a server bake the appearance-
    /// service URL supplied with the source (`FTT_SERVER_BAKE`).
    fn source_url(
        &self,
        id: TextureKey,
        source: &RemoteTextureSource,
    ) -> Result<String, FetchError> {
        match source {
            RemoteTextureSource::Default => {
                let cap = self.cap_url.load_full().ok_or_else(|| {
                    FetchError::Transport("GetTexture capability not available".to_owned())
                })?;
                Ok(format!("{cap}/?texture_id={id}"))
            }
            RemoteTextureSource::ServerBake { url } => Ok(url.clone()),
        }
    }

    /// Performs the blocking range request, returning the chunk.
    fn fetch_blocking(
        &self,
        id: TextureKey,
        source: &RemoteTextureSource,
        start: usize,
        end: usize,
    ) -> Result<FetchChunk, FetchError> {
        let url = self.source_url(id, source)?;
        let response = self
            .http
            .get(&url)
            .header("Accept", "image/x-j2c")
            .header("Range", format!("bytes={start}-{}", end.saturating_sub(1)))
            .send()
            .map_err(|error| FetchError::Transport(error.to_string()))?;
        let status = response.status();
        if status == ReqwestStatusCode::NOT_FOUND {
            return Err(FetchError::NotFound);
        }
        if status == ReqwestStatusCode::RANGE_NOT_SATISFIABLE {
            return Ok(FetchChunk {
                bytes: Bytes::new(),
                whole: false,
            });
        }
        let whole = status == ReqwestStatusCode::OK;
        if !status.is_success() {
            return Err(FetchError::Transport(format!("unexpected status {status}")));
        }
        let bytes = response
            .bytes()
            .map_err(|error| FetchError::Transport(error.to_string()))?;
        Ok(FetchChunk { bytes, whole })
    }
}

impl Default for BevyTextureFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TextureFetcher for BevyTextureFetcher {
    async fn fetch_range(
        &self,
        id: TextureKey,
        source: &RemoteTextureSource,
        start: usize,
        end: usize,
    ) -> Result<FetchChunk, FetchError> {
        // The blocking request runs on whatever thread `block_on`s this future
        // (a Bevy task/thread dedicated to the fetch), which is the intended use.
        self.fetch_blocking(id, source, start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::{planar_texgen_uv, texture_face_uv_transform, to_bevy_image};
    use bevy::math::{Affine2, Vec2};
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sl_proto::{DiscardLevel, TextureFace, TextureKey};
    use sl_texture::DecodedImage;
    use uuid::Uuid;

    /// The planar projection of a `+Z`-facing (cap) vertex matches the reference
    /// viewer's `planarProjection`: the position is scaled by the object size,
    /// then `u = 1 + (x·2 − 0.5)`, `v = −(−y·2 − 0.5)`.
    #[test]
    fn planar_texgen_projects_a_z_face() {
        // vec = (0.25, 0.1, 0.5) ⊙ (10, 10, 0.14) = (2.5, 1.0, 0.07).
        // binormal = (1,0,0), tangent = (0,-1,0):
        //   u = 1 + (2.5·2 − 0.5) = 5.5, v = −((−1.0)·2 − 0.5) = 2.5.
        let uv = planar_texgen_uv([0.25, 0.1, 0.5], [0.0, 0.0, 1.0], [10.0, 10.0, 0.14]);
        assert!((uv[0] - 5.5).abs() < 1.0e-5, "u was {}", uv[0]);
        assert!((uv[1] - 2.5).abs() < 1.0e-5, "v was {}", uv[1]);
    }

    /// A strongly `+X`-facing vertex picks the `(0,1,0)` binormal branch
    /// (`|normal.x| ≥ 0.5`), so `tangent = (0,0,-1)`.
    #[test]
    fn planar_texgen_projects_an_x_face() {
        // vec = (0.5, 0.2, 0.3) ⊙ (2,2,2) = (1.0, 0.4, 0.6).
        //   u = 1 + (0.4·2 − 0.5) = 1.3, v = −((−0.6)·2 − 0.5) = 1.7.
        let uv = planar_texgen_uv([0.5, 0.2, 0.3], [1.0, 0.0, 0.0], [2.0, 2.0, 2.0]);
        assert!((uv[0] - 1.3).abs() < 1.0e-5, "u was {}", uv[0]);
        assert!((uv[1] - 1.7).abs() < 1.0e-5, "v was {}", uv[1]);
    }

    /// A neutral (identity) face maps every UV to itself.
    #[test]
    fn identity_face_is_the_identity_transform() {
        let face = TextureFace::new(TextureKey::from(Uuid::nil()));
        let transform = texture_face_uv_transform(&face);
        assert!(transform.abs_diff_eq(Affine2::IDENTITY, 1.0e-6));
    }

    /// Doubling the repeats tiles the texture twice, centred on the face: the
    /// centre `(0.5, 0.5)` stays put while the corners spread out.
    #[test]
    fn repeats_tile_about_the_face_centre() {
        let mut face = TextureFace::new(TextureKey::from(Uuid::nil()));
        face.scale_s = 2.0;
        face.scale_t = 2.0;
        let transform = texture_face_uv_transform(&face);
        // The centre is the fixed point of a pure scale about the centre.
        assert!(
            transform
                .transform_point2(Vec2::new(0.5, 0.5))
                .abs_diff_eq(Vec2::new(0.5, 0.5), 1.0e-6)
        );
        // A corner maps out to twice the distance from the centre.
        assert!(
            transform
                .transform_point2(Vec2::new(1.0, 1.0))
                .abs_diff_eq(Vec2::new(1.5, 1.5), 1.0e-6)
        );
    }

    /// A pure offset slides every UV by the same amount.
    #[test]
    fn offset_translates_every_uv() {
        let mut face = TextureFace::new(TextureKey::from(Uuid::nil()));
        face.offset_s = 0.25;
        face.offset_t = -0.1;
        let transform = texture_face_uv_transform(&face);
        assert!(
            transform
                .transform_point2(Vec2::new(0.3, 0.7))
                .abs_diff_eq(Vec2::new(0.55, 0.6), 1.0e-6)
        );
    }

    /// A quarter-turn rotation spins the texture about the face centre, leaving
    /// the centre fixed and swapping the axes at a corner.
    #[test]
    fn rotation_spins_about_the_face_centre() {
        let mut face = TextureFace::new(TextureKey::from(Uuid::nil()));
        face.rotation = core::f32::consts::FRAC_PI_2;
        let transform = texture_face_uv_transform(&face);
        assert!(
            transform
                .transform_point2(Vec2::new(0.5, 0.5))
                .abs_diff_eq(Vec2::new(0.5, 0.5), 1.0e-6)
        );
        // Firestorm's `xform` at 90°: s' = t (about the centre), t' = -s.
        // A point 0.5 above the centre (0.5, 1.0) rotates to 0.5 right (1.0, 0.5).
        assert!(
            transform
                .transform_point2(Vec2::new(0.5, 1.0))
                .abs_diff_eq(Vec2::new(1.0, 0.5), 1.0e-6)
        );
    }

    #[test]
    fn converts_rgba_to_a_bevy_image() {
        let decoded = DecodedImage {
            width: 2,
            height: 2,
            components: 4,
            discard_level: DiscardLevel::FULL,
            pixels: Bytes::from(vec![0x7F_u8; 2 * 2 * 4]),
        };
        let image = to_bevy_image(&decoded);
        assert_eq!(image.width(), 2);
        assert_eq!(image.height(), 2);
    }
}
