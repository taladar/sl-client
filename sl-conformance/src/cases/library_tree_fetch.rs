//! Crawl the read-only shared Library inventory tree from its root: starting at
//! the Library root folder, fetch each folder's contents and follow every
//! sub-folder it reports, recording the folder/item totals and the deepest level
//! reached.
//!
//! Where `inventory-tree-crawl` walks the agent's *own* tree, this walks the
//! grid-owned **Library** — a second, read-only inventory tree owned by a
//! distinct Library owner (not the agent), surfaced alongside the agent root by a
//! [`Command::QueryInventoryRoots`]. The crawl drives the same per-folder fetch
//! over [`Command::RequestFolderContents`] / [`Event::InventoryDescendents`], but
//! the library routes each Library-folder fetch to `FetchLibDescendents2` where
//! the region advertises it (Second Life) and the legacy UDP
//! `FetchInventoryDescendents` addressed to the Library owner where it does not
//! (OpenSim) — automatically, because every Library folder is filed under the
//! Library owner in the inventory model, so the same case runs against both
//! grids. The Library being a separate tree is asserted directly: its root is
//! distinct from the agent root.

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
/// pathological tree cannot loop unboundedly. A stock Library holds a few dozen
/// folders; this leaves ample headroom while still terminating.
const MAX_FOLDERS: usize = 5_000;

/// Crawls the read-only shared Library inventory tree breadth-first from its
/// root, fetching every folder's contents and following its sub-folders,
/// recording the folder/item totals and the maximum depth reached.
#[derive(Debug)]
pub struct LibraryTreeFetch;

impl GridTest for LibraryTreeFetch {
    fn name(&self) -> &'static str {
        "library-tree-fetch"
    }

    fn description(&self) -> &'static str {
        "Crawl the read-only shared Library inventory tree (CAPS or UDP)"
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
            let (agent_root, library_root) = session
                .wait_for(ROOTS_TIMEOUT, |event| match event {
                    Event::InventoryRoots {
                        agent_root,
                        library_root,
                    } => Some((*agent_root, *library_root)),
                    _ => None,
                })
                .await?;

            // The grid may not expose a Library at all; that is a legitimate
            // absence, not a failure, so record it as partial and stop.
            let Some(library_root) = library_root else {
                ctx.mark_partial("grid provided no Library inventory tree");
                return Ok(());
            };

            // The Library is a *separate* tree from the agent's own inventory:
            // its root must differ from the agent root (when both are present).
            if let Some(agent_root) = agent_root {
                check(
                    library_root != agent_root,
                    "Library root coincides with the agent root — not a separate tree",
                )?;
            }

            // Breadth-first crawl from the Library root: `visited` is every folder
            // we have scheduled (so each is fetched once even if reachable by more
            // than one path), `items` dedups items by id across the whole tree, and
            // the queue carries each pending folder with its depth so the deepest
            // level reached can be recorded as evidence the descent went beyond the
            // root.
            let mut visited: HashSet<InventoryFolderKey> = HashSet::new();
            let mut items: HashSet<sl_client_tokio::InventoryKey> = HashSet::new();
            let mut queue: VecDeque<(InventoryFolderKey, u32)> = VecDeque::new();
            visited.insert(library_root);
            queue.push_back((library_root, 0));

            let mut max_depth = 0_u32;
            let start = Instant::now();
            while let Some((folder, depth)) = queue.pop_front() {
                if visited.len() > MAX_FOLDERS {
                    return Err(TestFailure::Assertion(format!(
                        "Library crawl exceeded {MAX_FOLDERS} folders — aborting"
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
            metrics.set_timing("library_crawl_secs", elapsed);
            metrics.set("library_folders_total", folders_total);
            metrics.set("library_items_total", items_total);
            metrics.set("library_max_depth", depth);

            check(
                folders_total >= 0 && items_total >= 0,
                "Library count exceeded i64",
            )?;
            // A stock shared Library holds the standard system sub-folders
            // (Textures, Animations, …), so a crawl that descended must have found
            // at least one sub-folder and reached depth ≥ 1 — the proof the descent
            // went beyond the root.
            check(
                folders_total > 1,
                "crawl found only the Library root — descent did not go beyond it",
            )?;
            check(
                max_depth >= 1,
                "crawl never reached a Library sub-folder (max depth 0)",
            )?;
            Ok(())
        })
    }
}
