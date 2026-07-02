//! Fetch a known texture over the HTTP `GetTexture` capability, decode its J2C
//! codestream header, and exercise the level-of-detail (`discard_level`) prefix
//! path that uses HTTP `Range` requests.
//!
//! This extends `asset-decode` (which only times a full-resolution fetch): it
//! parses the JPEG-2000 codestream header to recover the real image geometry
//! (width, height, components, wavelet decomposition levels) and then re-fetches
//! the same texture at a coarse discard level, asserting the LOD prefix is no
//! larger than the full codestream — the observable effect of the `Range`-based
//! partial fetch in `sl-client-tokio`'s `fetch_texture_http`.

use std::time::Instant;

use sl_client_tokio::{Command, Event, ImageCodec, Throttle, j2c};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check, fixtures};

/// A coarse discard level to force the LOD-prefix / HTTP `Range` path. Level 3
/// halves each dimension three times, so on any non-trivial texture the prefix
/// is well below the full codestream.
const COARSE_DISCARD_LEVEL: u8 = 3;

/// Fetches the default plywood texture at full and coarse LOD over HTTP,
/// decoding the J2C header and comparing prefix sizes.
#[derive(Debug)]
pub struct TextureFetchHttp;

impl GridTest for TextureFetchHttp {
    fn name(&self) -> &'static str {
        "texture-fetch-http"
    }

    fn description(&self) -> &'static str {
        "Fetch a texture over the GetTexture cap, decode its J2C header, and exercise the LOD prefix"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let texture_id = fixtures::plywood_texture()?;

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            // Full-resolution fetch (discard 0): the whole codestream. Textures
            // can be large, so the predicate decodes the header and measures the
            // length in place rather than cloning the payload out of the event.
            let full_start = Instant::now();
            session
                .send(Command::FetchTexture {
                    texture_id,
                    discard_level: 0,
                })
                .await?;
            let (full_codec, full_len, header) = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::TextureReceived(texture) if texture.id == texture_id => Some((
                        texture.codec,
                        texture.data.len(),
                        j2c::parse_header(&texture.data),
                    )),
                    _ => None,
                })
                .await?;
            let full_secs = full_start.elapsed().as_secs_f64();

            check(
                full_codec == ImageCodec::J2c,
                &format!("full fetch codec: expected J2c, got {full_codec:?}"),
            )?;
            check(full_len > 0, "full fetch returned no bytes")?;

            // The decoded JPEG-2000 codestream header recovers image geometry.
            let header = header.ok_or_else(|| {
                crate::context::TestFailure::Assertion(
                    "full fetch did not parse as a J2C codestream header".to_owned(),
                )
            })?;
            check(header.width > 0, "decoded width is zero")?;
            check(header.height > 0, "decoded height is zero")?;
            check(
                header.components >= 1 && header.components <= 4,
                &format!("decoded components out of range: {}", header.components),
            )?;

            // Coarse LOD fetch: forces the header-probe + Range-prefix path.
            let coarse_start = Instant::now();
            session
                .send(Command::FetchTexture {
                    texture_id,
                    discard_level: COARSE_DISCARD_LEVEL,
                })
                .await?;
            let coarse_len = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::TextureReceived(texture) if texture.id == texture_id => {
                        Some(texture.data.len())
                    }
                    _ => None,
                })
                .await?;
            let coarse_secs = coarse_start.elapsed().as_secs_f64();

            check(coarse_len > 0, "coarse fetch returned no bytes")?;
            check(
                coarse_len <= full_len,
                &format!(
                    "coarse LOD prefix ({coarse_len} bytes) exceeds full codestream ({full_len} bytes)"
                ),
            )?;

            let full_bytes = i64::try_from(full_len).unwrap_or(-1);
            let coarse_bytes = i64::try_from(coarse_len).unwrap_or(-1);
            let metrics = ctx.metrics();
            metrics.set_timing("texture_full_fetch_secs", full_secs);
            metrics.set_timing("texture_coarse_fetch_secs", coarse_secs);
            metrics.set("texture_full_bytes", full_bytes);
            metrics.set("texture_coarse_bytes", coarse_bytes);
            metrics.set("texture_width", header.width);
            metrics.set("texture_height", header.height);
            metrics.set("texture_components", i64::from(header.components));
            metrics.set(
                "texture_decomposition_levels",
                i64::from(header.decomposition_levels.unwrap_or(0)),
            );
            metrics.set("texture_codec", format!("{full_codec:?}"));
            Ok(())
        })
    }
}
