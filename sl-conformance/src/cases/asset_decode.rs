//! Fetch a known texture over the HTTP capability and decode it, timing the
//! round-trip and recording the codec and byte length.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event, TextureKey, Throttle, Uuid};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};

/// How long to wait for the region to become active.
const REGION_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for the texture to arrive and decode.
const TEXTURE_TIMEOUT: Duration = Duration::from_secs(60);

/// The standard SL/OpenSim "plywood" default texture, present on a stock grid.
const PLYWOOD_TEXTURE: &str = "89556747-24cb-43ed-920b-47caed15465f";

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
            let texture_uuid: Uuid = PLYWOOD_TEXTURE
                .parse()
                .map_err(|_invalid| TestFailure::Assertion("bad texture uuid".to_owned()))?;
            let texture_id = TextureKey::from(texture_uuid);

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            let start = Instant::now();
            session
                .send(Command::FetchTexture {
                    texture_id,
                    discard_level: 0,
                })
                .await?;
            let (codec, bytes) = session
                .wait_for(TEXTURE_TIMEOUT, |event| match event {
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
