//! Prove the inventory disk cache lets a relogin skip refetching folders whose
//! version is unchanged: crawl the agent tree once (loading every folder), let
//! the runtime persist the cache on logout, then log the same avatar back in and
//! observe the folders come back **already loaded** from the cache — no refetch —
//! because the login skeleton reports the same versions the cache holds.
//!
//! Where `inventory-tree-crawl` proves the tree can be fetched, this proves it
//! need not be fetched *again*. The runtime (`Client::set_inventory_cache_config`)
//! loads `<agent-uuid>.inv.llsd.gz` before the login skeleton and reconciles it:
//! a cached folder whose version equals the skeleton's keeps its loaded contents
//! ([`FolderState::Loaded`]); a mismatch is invalidated and requeued. The case
//! drives the cache directory through the harness (a cleared per-case dir, so the
//! first login is cold) and a mid-run `Session::disconnect`/`Session::relogin`
//! cycle (the same support `offline-msg-fetch` introduced), then asserts the
//! version-matching skip directly from the held model via
//! [`Command::QueryInventoryFolder`]: the agent root's child folders are
//! `Unknown` before the crawl (cold) and `Loaded` at the **same version** after
//! the relogin (warm) — the refetch skipped.
//!
//! The cache load/merge is grid-agnostic (it keys on the skeleton the login
//! returns, which both grids send), so this is a single `[both]` path; only the
//! per-folder crawl underneath it picks CAPS vs UDP per region, exactly as
//! `inventory-tree-crawl` documents.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, Event, FolderInfo, FolderState, InventoryCursor, InventoryFolderKey, Throttle,
};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::check;

/// How long to wait for the region to become active.
const REGION_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for the inventory root query to answer.
const ROOTS_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for any one folder's contents (a crawl fetch) to arrive.
const FOLDER_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for a local folder-page query to answer (no wire round-trip;
/// the runtime synthesises it from the held model).
const PAGE_TIMEOUT: Duration = Duration::from_secs(30);

/// Upper bound on the number of folders the crawl will fetch, so a malformed tree
/// cannot loop unboundedly. A stock agent inventory holds a few dozen folders.
const MAX_FOLDERS: usize = 5_000;

/// A single page large enough to hold a stock agent root's immediate children in
/// one query (a stock root has on the order of a couple dozen system folders).
const PAGE_LIMIT: usize = 10_000;

/// Crawls the agent inventory tree, persists the disk cache across a relogin, and
/// asserts the version-matching folders come back loaded from the cache rather
/// than being refetched.
#[derive(Debug)]
pub struct InventoryCacheSkip;

impl GridTest for InventoryCacheSkip {
    fn name(&self) -> &'static str {
        "inventory-cache-skip"
    }

    fn description(&self) -> &'static str {
        "Relogin skips refetching folders whose cached version still matches"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn inventory_cache(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            // The agent root, the entry point for both the crawl and the
            // before/after child-state observations.
            let agent_root = query_agent_root(session).await?;

            // --- Cold: the cache directory was cleared before login, so the root
            // and its children are skeleton-only (`Unknown`) until fetched. Record
            // each child folder's version now, to confirm later that the cache
            // skip kept the *same* version (the gate the skip turns on).
            let cold_children = query_child_folders(session, agent_root).await?;
            let cold_versions: HashMap<InventoryFolderKey, i32> = cold_children
                .iter()
                .map(|folder| (folder.folder_id, folder.version))
                .collect();
            let cold_loaded = cold_children
                .iter()
                .filter(|folder| matches!(folder.state, FolderState::Loaded { .. }))
                .count();
            check(
                !cold_children.is_empty(),
                "agent root reported no child folders — cannot observe a cache skip",
            )?;
            check(
                cold_loaded == 0,
                "a child folder was already loaded before any fetch — the cache was not cold",
            )?;

            // Crawl the whole tree so every folder is fetched (`Loaded`) and thus
            // written to the disk cache on logout.
            let crawl_start = Instant::now();
            let folders_cached = crawl_tree(session, agent_root).await?;
            let crawl_secs = crawl_start.elapsed().as_secs_f64();

            // --- Persist + reload: disconnect writes the cache on logout; relogin
            // reads it back before the login skeleton and reconciles it, so
            // version-matching folders stay loaded instead of being refetched.
            let relogin_start = Instant::now();
            session.disconnect().await?;
            session.relogin().await?;
            session.wait_for_region(REGION_TIMEOUT).await?;
            let relogin_secs = relogin_start.elapsed().as_secs_f64();

            // --- Warm: re-query the agent root's children. The cache skip should
            // have left them `Loaded` at the same version, with no refetch issued
            // this session.
            let warm_root = query_agent_root(session).await?;
            check(
                warm_root == agent_root,
                "agent root id changed across relogin — cannot compare cache state",
            )?;
            let warm_children = query_child_folders(session, warm_root).await?;

            let mut warm_loaded = 0_usize;
            let mut version_matched = 0_usize;
            for folder in &warm_children {
                if let FolderState::Loaded { version } = folder.state {
                    warm_loaded = warm_loaded.saturating_add(1);
                    if cold_versions.get(&folder.folder_id) == Some(&version) {
                        version_matched = version_matched.saturating_add(1);
                    }
                }
            }

            let metrics = ctx.metrics();
            metrics.set_timing("inventory_crawl_secs", crawl_secs);
            metrics.set_timing("relogin_secs", relogin_secs);
            metrics.set(
                "child_folders_count",
                i64::try_from(cold_children.len()).unwrap_or(-1),
            );
            metrics.set(
                "cold_loaded_children_count",
                i64::try_from(cold_loaded).unwrap_or(-1),
            );
            metrics.set(
                "warm_loaded_children_count",
                i64::try_from(warm_loaded).unwrap_or(-1),
            );
            metrics.set(
                "version_matched_children_count",
                i64::try_from(version_matched).unwrap_or(-1),
            );
            metrics.set(
                "folders_cached_count",
                i64::try_from(folders_cached).unwrap_or(-1),
            );

            // The skip is proven when warm children that the cold pass saw
            // `Unknown` come back `Loaded` at the *same* version — the cache
            // supplied them without a refetch, and the skeleton confirmed the
            // version still matched. Require it for every child folder the cold
            // pass recorded a version for.
            check(
                warm_loaded >= 1,
                "no child folder came back loaded after relogin — the cache skip did not happen",
            )?;
            check(
                version_matched == warm_loaded,
                "a warm-loaded child folder did not match its cold version — \
                 the skip was not version-gated",
            )?;
            check(
                warm_loaded == cold_children.len(),
                "some child folders were not restored loaded from the cache after relogin",
            )?;
            Ok(())
        })
    }
}

/// Query the agent inventory root, the entry point for the crawl and the
/// child-state observations.
async fn query_agent_root(
    session: &mut crate::context::Session,
) -> Result<InventoryFolderKey, TestFailure> {
    session.send(Command::QueryInventoryRoots).await?;
    session
        .wait_for(ROOTS_TIMEOUT, |event| match event {
            Event::InventoryRoots { agent_root, .. } => *agent_root,
            _ => None,
        })
        .await
}

/// Page `folder`'s immediate child folders out of the held model (a local query,
/// no wire round-trip), returning their [`FolderInfo`] (id, version, fetch state).
/// A stock root fits in one [`PAGE_LIMIT`] page.
async fn query_child_folders(
    session: &mut crate::context::Session,
    folder: InventoryFolderKey,
) -> Result<Vec<FolderInfo>, TestFailure> {
    let mut all = Vec::new();
    let mut before: Option<InventoryCursor> = None;
    loop {
        session
            .send(Command::QueryInventoryFolder {
                folder,
                before,
                limit: PAGE_LIMIT,
            })
            .await?;
        let (folders, prev) = session
            .wait_for(PAGE_TIMEOUT, |event| match event {
                Event::InventoryFolderPage {
                    folder: queried,
                    folders,
                    prev,
                    ..
                } if *queried == folder => Some((folders.to_vec(), *prev)),
                _ => None,
            })
            .await?;
        all.extend(folders);
        match prev {
            Some(cursor) => before = Some(cursor),
            None => break,
        }
    }
    Ok(all)
}

/// Breadth-first crawl from `root`, fetching every folder's contents over
/// [`Command::RequestFolderContents`] and following its sub-folders, so each
/// folder is left [`FolderState::Loaded`] (and thus cacheable). Returns the number
/// of folders fetched. Mirrors `inventory-tree-crawl`'s descent.
async fn crawl_tree(
    session: &mut crate::context::Session,
    root: InventoryFolderKey,
) -> Result<usize, TestFailure> {
    let mut visited: HashSet<InventoryFolderKey> = HashSet::new();
    let mut queue: VecDeque<InventoryFolderKey> = VecDeque::new();
    visited.insert(root);
    queue.push_back(root);
    while let Some(folder) = queue.pop_front() {
        if visited.len() > MAX_FOLDERS {
            return Err(TestFailure::Assertion(format!(
                "inventory crawl exceeded {MAX_FOLDERS} folders — aborting"
            )));
        }
        session.send(Command::RequestFolderContents(folder)).await?;
        let subfolders = session
            .wait_for(FOLDER_TIMEOUT, |event| match event {
                Event::InventoryDescendents {
                    folder_id, folders, ..
                } if *folder_id == folder => Some(folders.clone()),
                _ => None,
            })
            .await?;
        for sub in &subfolders {
            if visited.insert(sub.folder_id) {
                queue.push_back(sub.folder_id);
            }
        }
    }
    Ok(visited.len())
}
