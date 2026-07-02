//! Query the world map three ways: region blocks, overlay items, and the
//! image-tile layer.
//!
//! A viewer paints the world map from three separate UDP round-trips, and this
//! case drives all three against the current region:
//!
//! 1. **Map blocks** — [`Command::RequestMapBlocks`] over a small grid-coordinate
//!    rectangle around the agent's own region. Each in-range region arrives as an
//!    [`Event::MapBlock`] ([`MapRegionInfo`]: name, grid coordinates, maturity,
//!    map-tile id). The case drains the blocks to a quiet gap and asserts the
//!    agent's own region is among them.
//! 2. **Map items** — [`Command::RequestMapItems`] for [`MapItemType::AgentLocations`]
//!    (the map's "green dots") targeting the current region (`region_handle`
//!    [`RegionHandle(0)`](RegionHandle)). The reply is one [`Event::MapItems`]
//!    echoing the requested item type; the case asserts it carries at least one
//!    item (OpenSim always sends a placeholder dot for a lightly-populated region)
//!    and that the echoed type matches.
//! 3. **Map layer** — [`Command::RequestMapLayer`]. The reply is one
//!    [`Event::MapLayers`] describing the image tiles covering the grid; the case
//!    asserts at least one layer.
//!
//! The one cross-grid invariant is that all three replies decode and that the
//! agent's own region shows up in the block reply — the map must at minimum know
//! about the region the agent is standing in. Records each round-trip's latency
//! plus the block / item / layer counts and the resolved region name.
//!
//! `1av`, `[both]`. No new client code — the whole command/event surface
//! (`request_map_blocks`, `request_map_items`, `request_map_layer`) already
//! existed; `sl-survey` uses the same `RequestMapBlocks` path to enumerate
//! regions. On OpenSim's standalone grid the block reply is the single local
//! Default Region, the item reply the one green-dot placeholder, and the layer
//! reply OpenSim's built-in whole-grid tile. Second Life answers the same UDP
//! requests (the aditi run is deferred with the batch; SL may additionally serve
//! the layer/tile over a CAPS path, but the UDP replies still arrive).

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event, GridCoordinates, MapItemType, MapRegionInfo, RegionHandle};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, count_metric, secs_metric};

/// The quiet gap (no further `MapBlockReply`) that marks the block reply fully
/// drained. OpenSim's world-map worker batches regions in groups of ten with a
/// ~50 ms sleep between batches, so this stays comfortably above that cadence;
/// on the single-region local grid one batch is all that arrives.
const BLOCK_DRAIN_QUIET: Duration = Duration::from_secs(2);

/// How many grid cells to pad the block-request rectangle by on each side of the
/// agent's own region. A margin of one exercises the multi-cell rectangle path
/// (and picks up any neighbours) while staying tightly around the region.
const BLOCK_MARGIN: u32 = 1;

/// Requests world-map blocks, overlay items, and the image layer, recording the
/// three round-trip latencies and reply counts.
#[derive(Debug)]
pub struct MapBlocksItems;

impl GridTest for MapBlocksItems {
    fn name(&self) -> &'static str {
        "map-blocks-items"
    }

    fn description(&self) -> &'static str {
        "Request world-map blocks, overlay items, and the image layer"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // The agent's own region anchors every query: its grid coordinates
            // scope the block rectangle and prove the block reply is real.
            let region_handle = session.region_handle().ok_or_else(|| {
                TestFailure::Assertion("login reported no region handle".to_owned())
            })?;
            let here = GridCoordinates::from(region_handle);

            // 1. Map blocks over a small rectangle around the current region.
            let blocks_start = Instant::now();
            session
                .send(Command::RequestMapBlocks {
                    min_x: here.x().saturating_sub(BLOCK_MARGIN),
                    max_x: here.x().saturating_add(BLOCK_MARGIN),
                    min_y: here.y().saturating_sub(BLOCK_MARGIN),
                    max_y: here.y().saturating_add(BLOCK_MARGIN),
                })
                .await?;
            let blocks = drain_map_blocks(session, BLOCK_DRAIN_QUIET).await?;
            let blocks_secs = blocks_start.elapsed().as_secs_f64();
            let own = blocks.iter().find(|block| block.grid_coordinates == here);
            check(
                own.is_some(),
                "the map block reply did not include the agent's own region",
            )?;
            let region_name = own
                .and_then(|block| block.name.as_ref())
                .map(ToString::to_string)
                .unwrap_or_default();

            // 2. Map items — the "green dots" for the current region.
            let items_start = Instant::now();
            session
                .send(Command::RequestMapItems {
                    item_type: MapItemType::AgentLocations,
                    region_handle: RegionHandle(0),
                })
                .await?;
            let (reply_type, items) = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::MapItems { item_type, items } => Some((*item_type, items.clone())),
                    _ => None,
                })
                .await?;
            let items_secs = items_start.elapsed().as_secs_f64();
            check(
                reply_type == MapItemType::AgentLocations,
                "the map item reply echoed a different item type than requested",
            )?;
            check(
                !items.is_empty(),
                "the agent-locations map item reply carried no items",
            )?;

            // 3. Map layer — the whole-grid image tile(s).
            let layer_start = Instant::now();
            session.send(Command::RequestMapLayer).await?;
            let layers = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::MapLayers { layers } => Some(layers.clone()),
                    _ => None,
                })
                .await?;
            let layer_secs = layer_start.elapsed().as_secs_f64();
            check(
                !layers.is_empty(),
                "the map layer reply carried no image-tile layers",
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("map_blocks"), blocks_secs);
            metrics.set_timing(&secs_metric("map_items"), items_secs);
            metrics.set_timing(&secs_metric("map_layer"), layer_secs);
            metrics.set(
                &count_metric("blocks"),
                i64::try_from(blocks.len()).unwrap_or(-1),
            );
            metrics.set(
                &count_metric("items"),
                i64::try_from(items.len()).unwrap_or(-1),
            );
            metrics.set(
                &count_metric("layers"),
                i64::try_from(layers.len()).unwrap_or(-1),
            );
            metrics.set("region_name", region_name);
            metrics.set("grid_x", i64::from(here.x()));
            metrics.set("grid_y", i64::from(here.y()));
            Ok(())
        })
    }
}

/// Drains the [`Event::MapBlock`] entries a `MapBlockReply` yields until none
/// arrives for `quiet`, returning every region reported.
///
/// The first block is awaited with the full [`REPLY_TIMEOUT`] (OpenSim queues the
/// request onto a worker thread); once one is in hand, further ones follow closely
/// or not at all.
///
/// # Errors
///
/// Propagates a [`Session::wait_for`] disconnect, or times out if not even the
/// first block arrives.
async fn drain_map_blocks(
    session: &mut Session,
    quiet: Duration,
) -> Result<Vec<MapRegionInfo>, TestFailure> {
    let mut blocks = Vec::new();
    loop {
        let timeout = if blocks.is_empty() {
            REPLY_TIMEOUT
        } else {
            quiet
        };
        match session
            .wait_for(timeout, |event| match event {
                Event::MapBlock(region) => Some((**region).clone()),
                _ => None,
            })
            .await
        {
            Ok(region) => blocks.push(region),
            Err(TestFailure::Timeout(_)) => return Ok(blocks),
            Err(other) => return Err(other),
        }
    }
}
