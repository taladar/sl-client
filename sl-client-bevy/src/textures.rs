//! Bevy integration for the decoding [`TextureStore`](sl_texture::TextureStore):
//! a bridge from the store's decoded RGBA8 output to Bevy's [`Image`], and a
//! blocking-HTTP [`TextureFetcher`](sl_texture::TextureFetcher) so a Bevy app
//! (which has no async runtime of its own) can build and drive a store.
//!
//! Because the store's `get`/`request` are `async`, a Bevy app drives them by
//! `block_on`-ing on a task/thread (the crate already fetches on `std` threads);
//! the store's decode still runs off-thread on its own `rayon` pool.

use arc_swap::ArcSwapOption;
use async_trait::async_trait;
use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bytes::Bytes;
use reqwest::StatusCode as ReqwestStatusCode;
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_proto::TextureKey;
use sl_texture::{AssetFetcher, DecodedImage, FetchChunk, FetchError};
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

/// A [`TextureFetcher`](sl_texture::TextureFetcher) over blocking `reqwest`, for
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

    /// Performs the blocking range request, returning the chunk.
    fn fetch_blocking(
        &self,
        id: TextureKey,
        start: usize,
        end: usize,
    ) -> Result<FetchChunk, FetchError> {
        let cap = self.cap_url.load_full().ok_or_else(|| {
            FetchError::Transport("GetTexture capability not available".to_owned())
        })?;
        let url = format!("{cap}/?texture_id={id}");
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
impl AssetFetcher<TextureKey> for BevyTextureFetcher {
    async fn fetch_range(
        &self,
        id: TextureKey,
        start: usize,
        end: usize,
    ) -> Result<FetchChunk, FetchError> {
        // The blocking request runs on whatever thread `block_on`s this future
        // (a Bevy task/thread dedicated to the fetch), which is the intended use.
        self.fetch_blocking(id, start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::to_bevy_image;
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sl_proto::DiscardLevel;
    use sl_texture::DecodedImage;

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
