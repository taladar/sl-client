//! Drive an inventory **item** through the four structural operations a viewer
//! performs on items — create, copy, move, link — and confirm each against the
//! grid's authoritative folder listing.
//!
//! Where `ais3-folder-lifecycle` proves the write side for *folders*, this proves
//! it for the *items* inside them. All four operations ride the legacy UDP
//! inventory messages on both grids (the reference viewer still creates, copies,
//! moves, and links items over UDP even where AIS3 exists, which carries folder
//! mutations and item *metadata* edits): `CreateInventoryItem`,
//! `CopyInventoryItem`, `MoveInventoryItem`, `LinkInventoryItem`.
//!
//! Create, copy, and link each draw a direct reply that allocates the new item's
//! server id — `CreateInventoryItem`/`LinkInventoryItem` reply with an
//! `UpdateCreateInventoryItem` ([`Event::InventoryItemCreated`]), and a copy
//! replies the same way on OpenSim (its `CopyInventoryItem` routes through the
//! same create path) or as a `BulkUpdateInventory` ([`Event::InventoryBulkUpdate`])
//! elsewhere, so the copy predicate accepts either. The case captures the new id
//! from that reply, then — never trusting the optimistic local cache — re-fetches
//! the affected folder over [`Command::RequestFolderContents`] and asserts against
//! the grid's authoritative [`Event::InventoryDescendents`] item list, polling to
//! absorb OpenSim's fire-and-forget descendents worker.
//!
//! The lifecycle creates a `src` and a `dst` folder under the agent root, then:
//! **create** a notecard in `src`; **copy** it to a second name in `src` (proving
//! the original survives — a copy, not a move); **move** the original to `dst`
//! (asserted on both edges — present under `dst`, gone from `src`); **link** to
//! the moved original, filing the link in `src` (proving the link's target still
//! lives in `dst` — a pointer, not a relocation). The created items and both
//! folders are deleted at the end so re-runs start clean.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    AssetType, Command, Event, FolderType, InventoryFolderKey, InventoryItem,
    InventoryItemOrFolderKey, InventoryKey, InventoryType, NewInventoryItem, NewInventoryLink,
    Throttle, Uuid, WearableType,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::registry::{GridTest, TestFuture};
use crate::support::{check, send_then_wait};

/// How long to wait for the region to become active.
const REGION_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for the inventory root query to answer.
const ROOTS_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for any one folder's contents to arrive.
const FOLDER_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for a create/copy/link reply that allocates the new item id.
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a verification poll keeps re-fetching before giving up.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait between verification re-fetches.
const VERIFY_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// The next-owner permissions a created/linked item is given: copy | modify |
/// transfer (`PERM_COPY | PERM_MODIFY | PERM_TRANSFER`). Only the value carried to
/// a future owner; OpenSim forces the creator's own current permissions to full
/// regardless, so the freshly created item is always copyable (the copy step
/// depends on that).
const NEXT_OWNER_FULL: u32 = 0x0008_2000 | 0x0004_0000 | 0x0002_0000;

/// `AT_LINK` — the asset type marking an inventory link that points at a single
/// item (vs `AT_LINK_FOLDER` = 25 for a folder link).
const AT_LINK: i32 = 24;

/// Fetch a folder's immediate items by issuing a fresh
/// [`Command::RequestFolderContents`] and returning the grid's authoritative
/// reply (rather than the optimistic local cache).
async fn fetch_items(
    session: &mut Session,
    parent: InventoryFolderKey,
) -> Result<Vec<InventoryItem>, TestFailure> {
    send_then_wait(
        session,
        Command::RequestFolderContents(parent),
        FOLDER_TIMEOUT,
        |event| match event {
            Event::InventoryDescendents {
                folder_id, items, ..
            } if *folder_id == parent => Some(items.clone()),
            _ => None,
        },
    )
    .await
}

/// Re-fetch `parent` until its item list satisfies `predicate`, or fail with
/// `description` once [`VERIFY_TIMEOUT`] elapses. Absorbs the brief lag from
/// OpenSim's fire-and-forget descendents worker.
async fn poll_items<P>(
    session: &mut Session,
    parent: InventoryFolderKey,
    mut predicate: P,
    description: &str,
) -> Result<(), TestFailure>
where
    P: FnMut(&[InventoryItem]) -> bool,
{
    let start = Instant::now();
    loop {
        let items = fetch_items(session, parent).await?;
        if predicate(&items) {
            return Ok(());
        }
        if start.elapsed() >= VERIFY_TIMEOUT {
            return Err(TestFailure::Assertion(format!(
                "inventory never reached expected state: {description}"
            )));
        }
        tokio::time::sleep(VERIFY_POLL_INTERVAL).await;
    }
}

/// Create a UDP folder under `parent` (the same fire-and-forget
/// `CreateInventoryFolder` `ais3-folder-lifecycle` uses for its scaffolding), and
/// poll until the grid reflects it. Returns the new folder's (client-minted) key.
async fn make_folder(
    session: &mut Session,
    parent: InventoryFolderKey,
    name: &str,
) -> Result<InventoryFolderKey, TestFailure> {
    let folder_id = InventoryFolderKey::from(Uuid::new_v4());
    session
        .send(Command::CreateInventoryFolder {
            folder_id,
            parent_id: parent,
            folder_type: FolderType::None,
            name: name.to_owned(),
        })
        .await?;
    // Confirm via the folder's appearance under its parent.
    let wanted = folder_id;
    poll_subfolder(session, parent, wanted, "new working folder did not appear").await?;
    Ok(folder_id)
}

/// Re-fetch `parent`'s sub-folders until `folder` appears (used only by
/// [`make_folder`]; the item operations assert against items, not folders).
async fn poll_subfolder(
    session: &mut Session,
    parent: InventoryFolderKey,
    folder: InventoryFolderKey,
    description: &str,
) -> Result<(), TestFailure> {
    let start = Instant::now();
    loop {
        let folders = send_then_wait(
            session,
            Command::RequestFolderContents(parent),
            FOLDER_TIMEOUT,
            |event| match event {
                Event::InventoryDescendents {
                    folder_id, folders, ..
                } if *folder_id == parent => Some(folders.clone()),
                _ => None,
            },
        )
        .await?;
        if folders.iter().any(|entry| entry.folder_id == folder) {
            return Ok(());
        }
        if start.elapsed() >= VERIFY_TIMEOUT {
            return Err(TestFailure::Assertion(format!(
                "inventory never reached expected state: {description}"
            )));
        }
        tokio::time::sleep(VERIFY_POLL_INTERVAL).await;
    }
}

/// Whether `items` holds an item with this key.
fn has_item(items: &[InventoryItem], item: InventoryKey) -> bool {
    items.iter().any(|entry| entry.item_id == item)
}

/// The key of the Trash folder among `folders`, matched by its `FT_TRASH`
/// preferred type. OpenSim only deletes a folder once it lives under Trash (the
/// `onlyIfTrash` guard — see `ais3-folder-lifecycle`), so cleanup re-parents the
/// working folders there before removing them.
async fn find_trash(
    session: &mut Session,
    root: InventoryFolderKey,
) -> Result<InventoryFolderKey, TestFailure> {
    let trash = FolderType::Trash.to_code();
    let folders = send_then_wait(
        session,
        Command::RequestFolderContents(root),
        FOLDER_TIMEOUT,
        |event| match event {
            Event::InventoryDescendents {
                folder_id, folders, ..
            } if *folder_id == root => Some(folders.clone()),
            _ => None,
        },
    )
    .await?;
    folders
        .iter()
        .find(|entry| entry.folder_type == trash)
        .map(|entry| entry.folder_id)
        .ok_or_else(|| {
            TestFailure::Assertion("no Trash folder under the inventory root".to_owned())
        })
}

/// Exercises the create → copy → move → link item operations, confirming each
/// against the grid's authoritative folder listing.
#[derive(Debug)]
pub struct InventoryItemOps;

impl GridTest for InventoryItemOps {
    fn name(&self) -> &'static str {
        "inventory-item-ops"
    }

    fn description(&self) -> &'static str {
        "Create, copy, move, and link an inventory item (UDP)"
    }

    fn grids(&self) -> &'static [crate::grid::Grid] {
        &[crate::grid::Grid::Opensim, crate::grid::Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let agent = ctx
                .primary()
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            // The agent inventory root the working folders are created under.
            session.send(Command::QueryInventoryRoots).await?;
            let root = session
                .wait_for(ROOTS_TIMEOUT, |event| match event {
                    Event::InventoryRoots { agent_root, .. } => *agent_root,
                    _ => None,
                })
                .await?;

            let trash = find_trash(session, root).await?;

            // Distinct per-run names so a leftover item/folder from an aborted run
            // cannot be mistaken for this run's, and so concurrent runs do not
            // collide.
            let tag: String = Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect();
            let item_name = format!("conf-item-{tag}");
            let copy_name = format!("conf-copy-{tag}");
            let link_name = format!("conf-link-{tag}");
            let src_name = format!("conf-src-{tag}");
            let dst_name = format!("conf-dst-{tag}");

            let src = make_folder(session, root, &src_name).await?;
            let dst = make_folder(session, root, &dst_name).await?;

            let lifecycle_start = Instant::now();

            // --- create: a fresh notecard in `src`. The reply carries the
            // server-allocated item id; re-fetch confirms it lands in `src`.
            let create_start = Instant::now();
            let wanted = item_name.clone();
            let original = send_then_wait(
                session,
                Command::CreateInventoryItem(NewInventoryItem {
                    folder_id: src,
                    transaction_id: Uuid::nil(),
                    next_owner_mask: NEXT_OWNER_FULL,
                    asset_type: AssetType::Notecard,
                    inv_type: InventoryType::Notecard,
                    wearable_type: WearableType::Shape,
                    name: item_name.clone(),
                    description: "sl-conformance inventory-item-ops".to_owned(),
                }),
                REPLY_TIMEOUT,
                move |event| match event {
                    Event::InventoryItemCreated { item, .. } if item.name == wanted => {
                        Some(item.item_id)
                    }
                    _ => None,
                },
            )
            .await?;
            poll_items(
                session,
                src,
                |items| has_item(items, original),
                "created item did not appear under the source folder",
            )
            .await?;
            let create_secs = create_start.elapsed().as_secs_f64();

            // --- copy: duplicate the original into `src` under a new name. OpenSim
            // routes a copy through the create path (an `UpdateCreateInventoryItem`
            // reply); other grids may answer with a `BulkUpdateInventory`, so accept
            // either. The original must survive (a copy, not a move).
            let copy_start = Instant::now();
            let wanted = copy_name.clone();
            let copy = send_then_wait(
                session,
                Command::CopyInventoryItem {
                    old_agent_id: agent,
                    old_item_id: original,
                    new_folder_id: src,
                    new_name: copy_name.clone(),
                },
                REPLY_TIMEOUT,
                move |event| match event {
                    Event::InventoryItemCreated { item, .. } if item.name == wanted => {
                        Some(item.item_id)
                    }
                    Event::InventoryBulkUpdate { items, .. } => items
                        .iter()
                        .find(|entry| entry.name == wanted)
                        .map(|entry| entry.item_id),
                    _ => None,
                },
            )
            .await?;
            check(
                copy != original,
                "the copy was allocated the same id as the original",
            )?;
            poll_items(
                session,
                src,
                |items| has_item(items, original) && has_item(items, copy),
                "source folder did not hold both the original and its copy after the copy",
            )
            .await?;
            let copy_secs = copy_start.elapsed().as_secs_f64();

            // --- move: re-parent the original from `src` into `dst`. Asserted on
            // both edges — present under `dst`, gone from `src`. (The copy stays in
            // `src`, untouched.)
            let move_start = Instant::now();
            session
                .send(Command::MoveInventoryItem {
                    item_id: original,
                    folder_id: dst,
                    new_name: String::new(),
                })
                .await?;
            poll_items(
                session,
                dst,
                |items| has_item(items, original),
                "moved item did not appear under the destination folder",
            )
            .await?;
            poll_items(
                session,
                src,
                |items| !has_item(items, original) && has_item(items, copy),
                "source folder did not lose the original (or lost the copy) after the move",
            )
            .await?;
            let move_secs = move_start.elapsed().as_secs_f64();

            // --- link: a lightweight pointer to the moved original, filed in `src`.
            // Its target keeps living in `dst` (a link is not a relocation), which
            // the post-link assertion confirms.
            let link_start = Instant::now();
            let wanted = link_name.clone();
            let link = send_then_wait(
                session,
                Command::LinkInventoryItem(NewInventoryLink {
                    folder_id: src,
                    linked_id: InventoryItemOrFolderKey::Item(original),
                    link_type: AssetType::Other(AT_LINK),
                    inv_type: InventoryType::Notecard,
                    name: link_name.clone(),
                    description: "sl-conformance inventory-item-ops link".to_owned(),
                }),
                REPLY_TIMEOUT,
                move |event| match event {
                    Event::InventoryItemCreated { item, .. } if item.name == wanted => {
                        Some(item.item_id)
                    }
                    _ => None,
                },
            )
            .await?;
            poll_items(
                session,
                src,
                |items| has_item(items, link),
                "link did not appear under the source folder",
            )
            .await?;
            poll_items(
                session,
                dst,
                |items| has_item(items, original),
                "link's target vanished from the destination (a link must not move its target)",
            )
            .await?;
            let link_secs = link_start.elapsed().as_secs_f64();

            let lifecycle_secs = lifecycle_start.elapsed().as_secs_f64();

            // Clean up so back-to-back runs start from a clean inventory: delete the
            // three items (item deletion is not Trash-gated), then send the now-empty
            // working folders to Trash and remove them (folder removal is — see
            // `find_trash`).
            session
                .send(Command::RemoveInventoryItems(vec![original, copy, link]))
                .await?;
            for (folder, label) in [(src, "source"), (dst, "destination")] {
                session
                    .send(Command::MoveInventoryFolder {
                        folder_id: folder,
                        parent_id: trash,
                    })
                    .await?;
                poll_subfolder(
                    session,
                    trash,
                    folder,
                    &format!("{label} folder did not reach Trash during cleanup"),
                )
                .await?;
                session
                    .send(Command::RemoveInventoryFolders(vec![folder]))
                    .await?;
            }

            let metrics = ctx.metrics();
            metrics.set("path", "udp");
            metrics.set_timing("create_secs", create_secs);
            metrics.set_timing("copy_secs", copy_secs);
            metrics.set_timing("move_secs", move_secs);
            metrics.set_timing("link_secs", link_secs);
            metrics.set_timing("lifecycle_secs", lifecycle_secs);

            Ok(())
        })
    }
}
