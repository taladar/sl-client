//! Fetch inventory **items by id** — the per-item fetch
//! ([`Command::FetchInventoryItems`]) — and check what comes back against what a
//! folder fetch says about the same items.
//!
//! The library asks each item of the tree it is filed in: `FetchInventory2` for
//! the agent's own items, `FetchLib2` for the Library's, or the legacy UDP
//! `FetchInventory` where a region grants neither. Every road surfaces as an
//! [`Event::InventoryBulkUpdate`] with a nil transaction id.
//!
//! The case walks each tree breadth-first over
//! [`Command::RequestFolderContents`] until it has a few items from it, then
//! asks for all of them, plus one id no grid holds, in a single command. Every
//! agent item must come back, matching its folder listing field by field, and
//! the unknown id must not.
//!
//! What a Library item gets differs by grid. Second Life grants `FetchLib2`,
//! so its Library items must come back like the agent's. OpenSim grants no
//! `FetchLib2`, so they go out as UDP `FetchInventory`, and OpenSim answers
//! that out of its inventory service, which does not hold the Library (it is
//! served from memory by the Library module): nothing comes back, as nothing
//! does for the reference viewer. On OpenSim the case asserts that silence for
//! five seconds, so a grid that starts answering is noticed.
//!
//! It is read-only: nothing is created, so there is nothing to clean up.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event, InventoryFolderKey, InventoryItem, InventoryKey, Throttle};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::check;

/// How long to wait for the region to become active.
const REGION_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for the inventory root query to answer.
const ROOTS_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for any one folder's contents to arrive.
const FOLDER_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for every fetched item to come back.
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);

/// How long the case listens for an answer OpenSim is measured not to give
/// (a Library item over UDP) after the agent items have come back.
const SILENCE_WINDOW: Duration = Duration::from_secs(5);

/// How many items each tree contributes to the fetch.
const ITEMS_PER_TREE: usize = 3;

/// How many folders the search for items in one tree may fetch before it gives
/// up, so an empty or pathological tree cannot keep it walking.
const MAX_FOLDERS: usize = 200;

/// An id no grid holds, asked for alongside the real items: the grid leaves it
/// out of its answer rather than inventing an entry.
const UNKNOWN_ITEM: u128 = 0x5ead_0000_0000_4000_8000_0000_0000_f00d;

/// Fetches items of both inventory trees by id and checks each against its
/// folder listing.
#[derive(Debug)]
pub struct InventoryItemFetch;

impl GridTest for InventoryItemFetch {
    fn name(&self) -> &'static str {
        "inventory-item-fetch"
    }

    fn description(&self) -> &'static str {
        "Fetch inventory items by id (FetchInventory2 / FetchLib2 / UDP) and match their listing"
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
            let agent_root = agent_root.ok_or_else(|| {
                TestFailure::Assertion("the login named no agent inventory root".to_owned())
            })?;

            let agent_items = sample_items(session, agent_root).await?;
            let library_items = match library_root {
                Some(root) => sample_items(session, root).await?,
                None => Vec::new(),
            };
            check(
                !agent_items.is_empty(),
                "found no item in the agent's inventory to fetch",
            )?;

            // OpenSim offers no road for a Library item (see the module docs).
            let library_answered = ctx.grid() != Grid::Opensim;
            let listed: HashMap<InventoryKey, InventoryItem> = agent_items
                .iter()
                .chain(library_items.iter().filter(|_| library_answered))
                .map(|item| (item.item_id, item.clone()))
                .collect();
            let unknown = InventoryKey::from(sl_client_tokio::Uuid::from_u128(UNKNOWN_ITEM));
            let mut unanswered: HashSet<InventoryKey> = HashSet::from([unknown]);
            if !library_answered {
                unanswered.extend(library_items.iter().map(|item| item.item_id));
            }
            let mut asked: Vec<InventoryKey> = agent_items
                .iter()
                .chain(&library_items)
                .map(|item| item.item_id)
                .collect();
            asked.push(unknown);

            let session = ctx.primary();
            let start = Instant::now();
            session.send(Command::FetchInventoryItems(asked)).await?;
            let fetched = collect_fetched(session, &listed, &unanswered).await?;
            let elapsed = start.elapsed().as_secs_f64();
            if !library_answered && !library_items.is_empty() {
                expect_silence(session, &unanswered).await?;
            }

            for (key, listing) in &listed {
                let answer = fetched
                    .get(key)
                    .ok_or_else(|| TestFailure::Assertion(format!("item {key} never came back")))?;
                compare(listing, answer)?;
            }

            let metrics = ctx.metrics();
            metrics.set_timing("item_fetch_secs", elapsed);
            metrics.set(
                "agent_items_fetched",
                i64::try_from(agent_items.len()).unwrap_or(-1),
            );
            metrics.set(
                "library_items_fetched",
                i64::try_from(library_items.len()).unwrap_or(-1),
            );
            metrics.set("library_items_answered", i64::from(library_answered));
            if library_items.is_empty() {
                ctx.mark_partial("grid provided no Library item to fetch");
            }
            Ok(())
        })
    }
}

/// Walks the tree under `root` breadth-first until it holds
/// [`ITEMS_PER_TREE`] items (or the tree runs out), returning each as its
/// folder listing states it. Links are skipped: a link's listing and its
/// per-item answer may legitimately resolve differently.
async fn sample_items(
    session: &mut Session,
    root: InventoryFolderKey,
) -> Result<Vec<InventoryItem>, TestFailure> {
    let mut visited: HashSet<InventoryFolderKey> = HashSet::from([root]);
    let mut queue: VecDeque<InventoryFolderKey> = VecDeque::from([root]);
    let mut found = Vec::new();
    while let Some(folder) = queue.pop_front() {
        if found.len() >= ITEMS_PER_TREE || visited.len() > MAX_FOLDERS {
            break;
        }
        session.send(Command::RequestFolderContents(folder)).await?;
        let (subfolders, items) = session
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
        found.extend(
            items
                .into_iter()
                .filter(|item| !is_link(item))
                .take(ITEMS_PER_TREE.saturating_sub(found.len())),
        );
        for sub in subfolders {
            if visited.insert(sub.folder_id) {
                queue.push_back(sub.folder_id);
            }
        }
    }
    Ok(found)
}

/// Whether `item` is an inventory link (`AT_LINK` 24 / `AT_LINK_FOLDER` 25).
const fn is_link(item: &InventoryItem) -> bool {
    matches!(item.item_type, 24 | 25)
}

/// Whether `event` is a per-item fetch answer (a nil-transaction bulk update)
/// naming any item in `listed` or `unanswered`.
fn fetch_answer<'e>(
    event: &'e Event,
    listed: &HashMap<InventoryKey, InventoryItem>,
    unanswered: &HashSet<InventoryKey>,
) -> Option<&'e [InventoryItem]> {
    match event {
        Event::InventoryBulkUpdate {
            transaction_id,
            items,
            ..
        } if transaction_id.is_nil()
            && items.iter().any(|item| {
                listed.contains_key(&item.item_id) || unanswered.contains(&item.item_id)
            }) =>
        {
            Some(items)
        }
        _ => None,
    }
}

/// Fails if any of `items` is one the grid is expected not to answer for.
fn check_unanswered(
    items: &[InventoryItem],
    unanswered: &HashSet<InventoryKey>,
) -> Result<(), TestFailure> {
    match items.iter().find(|item| unanswered.contains(&item.item_id)) {
        Some(item) => Err(TestFailure::Assertion(format!(
            "the grid answered for item {} ({}), which it is expected not to",
            item.item_id, item.name
        ))),
        None => Ok(()),
    }
}

/// Gathers the per-item fetch's answers until every item in `listed` has come
/// back. An answer naming an item in `unanswered` fails the case.
async fn collect_fetched(
    session: &mut Session,
    listed: &HashMap<InventoryKey, InventoryItem>,
    unanswered: &HashSet<InventoryKey>,
) -> Result<HashMap<InventoryKey, InventoryItem>, TestFailure> {
    let now = Instant::now();
    let deadline = now.checked_add(FETCH_TIMEOUT).unwrap_or(now);
    let mut fetched: HashMap<InventoryKey, InventoryItem> = HashMap::new();
    while fetched.len() < listed.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let items = session
            .wait_for(remaining, |event| {
                fetch_answer(event, listed, unanswered).map(<[InventoryItem]>::to_vec)
            })
            .await
            .map_err(|error| {
                TestFailure::Assertion(format!(
                    "{} of {} items came back before the wait ended: {error}",
                    fetched.len(),
                    listed.len()
                ))
            })?;
        check_unanswered(&items, unanswered)?;
        fetched.extend(
            items
                .into_iter()
                .filter(|item| listed.contains_key(&item.item_id))
                .map(|item| (item.item_id, item)),
        );
    }
    Ok(fetched)
}

/// Listens for [`SILENCE_WINDOW`] and fails if an answer names any item in
/// `unanswered`.
async fn expect_silence(
    session: &mut Session,
    unanswered: &HashSet<InventoryKey>,
) -> Result<(), TestFailure> {
    let none = HashMap::new();
    match session
        .wait_for(SILENCE_WINDOW, |event| {
            fetch_answer(event, &none, unanswered).map(<[InventoryItem]>::to_vec)
        })
        .await
    {
        Ok(items) => check_unanswered(&items, unanswered),
        Err(TestFailure::Timeout(_)) => Ok(()),
        Err(error) => Err(error),
    }
}

/// Fails unless the per-item answer agrees with the folder listing on every
/// field both roads carry. The previous owner is left out: the UDP reply does
/// not carry it.
fn compare(listing: &InventoryItem, answer: &InventoryItem) -> Result<(), TestFailure> {
    let key = listing.item_id;
    let fields: [(&str, bool); 14] = [
        ("folder", listing.folder_id == answer.folder_id),
        ("name", listing.name == answer.name),
        ("description", listing.description == answer.description),
        ("asset id", listing.asset_id == answer.asset_id),
        ("asset type", listing.item_type == answer.item_type),
        ("inventory type", listing.inv_type == answer.inv_type),
        ("flags", listing.flags == answer.flags),
        ("sale type", listing.sale_type == answer.sale_type),
        ("sale price", listing.sale_price == answer.sale_price),
        (
            "creation date",
            listing.creation_date == answer.creation_date,
        ),
        ("owner", listing.owner == answer.owner),
        ("group", listing.group == answer.group),
        ("creator", listing.creator_id == answer.creator_id),
        ("permissions", listing.permissions == answer.permissions),
    ];
    let differing: Vec<&str> = fields
        .iter()
        .filter(|(_, same)| !same)
        .map(|(name, _)| *name)
        .collect();
    check(
        differing.is_empty(),
        &format!(
            "item {key} ({}) differs from its folder listing in: {}\n  listing: {listing:?}\n  fetched: {answer:?}",
            listing.name,
            differing.join(", ")
        ),
    )
}
