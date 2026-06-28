//! Fetch the agent's inventory root folder over UDP, timing the round-trip.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event, Throttle};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};

/// How long to wait for the region to become active.
const REGION_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for the inventory root query to answer.
const ROOTS_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for the folder contents to arrive.
const CONTENTS_TIMEOUT: Duration = Duration::from_secs(60);

/// Queries the inventory roots and fetches the agent root folder's contents,
/// recording the fetch time and the folder/item counts.
#[derive(Debug)]
pub struct InventoryFetch;

impl GridTest for InventoryFetch {
    fn name(&self) -> &'static str {
        "inventory-fetch"
    }

    fn description(&self) -> &'static str {
        "Fetch the agent inventory root folder contents over UDP"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            session.send(Command::QueryInventoryRoots).await?;
            let agent_root = session
                .wait_for(ROOTS_TIMEOUT, |event| match event {
                    Event::InventoryRoots { agent_root, .. } => *agent_root,
                    _ => None,
                })
                .await?;

            let start = Instant::now();
            session
                .send(Command::RequestFolderContents(agent_root))
                .await?;
            let (folders, items) = session
                .wait_for(CONTENTS_TIMEOUT, |event| match event {
                    Event::InventoryDescendents {
                        folder_id,
                        folders,
                        items,
                        ..
                    } if *folder_id == agent_root => Some((folders.len(), items.len())),
                    _ => None,
                })
                .await?;
            let elapsed = start.elapsed().as_secs_f64();

            let folders_count = i64::try_from(folders).unwrap_or(-1);
            let items_count = i64::try_from(items).unwrap_or(-1);
            let metrics = ctx.metrics();
            metrics.set_timing("inventory_fetch_secs", elapsed);
            metrics.set("root_folders", folders_count);
            metrics.set("root_items", items_count);
            if folders_count < 0 || items_count < 0 {
                return Err(TestFailure::Assertion(
                    "inventory count exceeded i64".to_owned(),
                ));
            }
            Ok(())
        })
    }
}
