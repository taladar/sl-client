//! Crawl the agent's whole inventory tree beyond the root: starting from the
//! agent root folder, fetch each folder's contents and follow every sub-folder
//! it reports, recording the folder/item totals and the deepest level reached.
//!
//! Where `inventory-fetch` proves the single root folder answers, this proves the
//! recursive descent: the crawl walks past the root into its sub-folders (and
//! their sub-folders), so the totals span the full reachable tree rather than one
//! level. The harness drives the descent explicitly over
//! [`Command::RequestFolderContents`] / [`Event::InventoryDescendents`] (the same
//! per-folder fetch the client's automatic background crawl issues, here pumped
//! by the test so completion is deterministic), which the library routes to the
//! modern CAPS `FetchInventoryDescendents2` where the region advertises it
//! (Second Life) and the legacy UDP `FetchInventoryDescendents` where it does not
//! (OpenSim), so the same case runs against both grids.

use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event, InventoryFolderKey, Throttle};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::check;

/// How long to wait for the region to become active.
const REGION_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for the inventory root query to answer.
const ROOTS_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for any one folder's contents to arrive.
const FOLDER_TIMEOUT: Duration = Duration::from_secs(60);

/// Upper bound on the number of folders the crawl will fetch, so a malformed or
/// pathological tree cannot loop unboundedly. A stock agent inventory holds a few
/// dozen folders; this leaves ample headroom while still terminating.
const MAX_FOLDERS: usize = 5_000;

/// Crawls the agent inventory tree breadth-first from the root, fetching every
/// folder's contents and following its sub-folders, recording the folder/item
/// totals and the maximum depth reached.
#[derive(Debug)]
pub struct InventoryTreeCrawl;

impl GridTest for InventoryTreeCrawl {
    fn name(&self) -> &'static str {
        "inventory-tree-crawl"
    }

    fn description(&self) -> &'static str {
        "Crawl the full agent inventory tree beyond the root (CAPS or UDP)"
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

            // Breadth-first crawl from the root: `visited` is every folder we have
            // scheduled (so each is fetched once even if reachable by more than one
            // path), `items` dedups items by id across the whole tree, and the
            // queue carries each pending folder with its depth so the deepest level
            // reached can be recorded as evidence the descent went beyond the root.
            let mut visited: HashSet<InventoryFolderKey> = HashSet::new();
            let mut items: HashSet<sl_client_tokio::InventoryKey> = HashSet::new();
            let mut queue: VecDeque<(InventoryFolderKey, u32)> = VecDeque::new();
            visited.insert(agent_root);
            queue.push_back((agent_root, 0));

            let mut max_depth = 0_u32;
            let start = Instant::now();
            while let Some((folder, depth)) = queue.pop_front() {
                if visited.len() > MAX_FOLDERS {
                    return Err(TestFailure::Assertion(format!(
                        "inventory crawl exceeded {MAX_FOLDERS} folders — aborting"
                    )));
                }
                session.send(Command::RequestFolderContents(folder)).await?;
                let (subfolders, folder_items) = session
                    .wait_for(FOLDER_TIMEOUT, |event| match event {
                        Event::InventoryDescendents {
                            folder_id,
                            folders,
                            items,
                            ..
                        } if *folder_id == folder => Some((folders.clone(), items.clone())),
                        _ => None,
                    })
                    .await?;
                max_depth = max_depth.max(depth);
                for item in &folder_items {
                    items.insert(item.item_id);
                }
                for sub in &subfolders {
                    if visited.insert(sub.folder_id) {
                        queue.push_back((sub.folder_id, depth.saturating_add(1)));
                    }
                }
            }
            let elapsed = start.elapsed().as_secs_f64();

            let folders_total = i64::try_from(visited.len()).unwrap_or(-1);
            let items_total = i64::try_from(items.len()).unwrap_or(-1);
            let depth = i64::from(max_depth);
            let metrics = ctx.metrics();
            metrics.set_timing("inventory_crawl_secs", elapsed);
            metrics.set("folders_total", folders_total);
            metrics.set("items_total", items_total);
            metrics.set("max_depth", depth);

            check(
                folders_total >= 0 && items_total >= 0,
                "inventory count exceeded i64",
            )?;
            // The whole point of the crawl is to reach past the root: a stock
            // inventory's root holds the standard system sub-folders, so a crawl
            // that descended must have found at least one and reached depth ≥ 1.
            check(
                folders_total > 1,
                "crawl found only the root folder — descent did not go beyond it",
            )?;
            check(
                max_depth >= 1,
                "crawl never reached a sub-folder (max depth 0)",
            )?;
            Ok(())
        })
    }
}
