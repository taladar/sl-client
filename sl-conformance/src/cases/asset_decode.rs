//! Fetch a known texture over the HTTP capability and decode it, timing the
//! round-trip and recording the codec and byte length.

use std::time::Instant;

use sl_client_tokio::{Command, DiscardLevel, Event, Throttle};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, fixtures};

/// Fetches the default plywood texture and records its decode timing and size.
#[derive(Debug)]
pub struct AssetDecode;

impl GridTest for AssetDecode {
    fn name(&self) -> &'static str {
        "asset-decode"
    }

    fn description(&self) -> &'static str {
        "Fetch and decode the default plywood texture"
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

            let start = Instant::now();
            session
                .send(Command::FetchTexture {
                    texture_id,
                    discard_level: DiscardLevel::FULL,
                })
                .await?;
            let (codec, bytes) = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::TextureReceived(texture) if texture.id == texture_id => {
                        Some((format!("{:?}", texture.codec), texture.data.len()))
                    }
                    _ => None,
                })
                .await?;
            let elapsed = start.elapsed().as_secs_f64();

            let byte_count = i64::try_from(bytes).unwrap_or(-1);
            let metrics = ctx.metrics();
            metrics.set_timing("texture_fetch_secs", elapsed);
            metrics.set("texture_bytes", byte_count);
            metrics.set("texture_codec", codec);
            Ok(())
        })
    }
}
