//! The primary gives an inventory item to the secondary over IM; the secondary
//! accepts it, the primary observes the acceptance, and the item is confirmed in
//! the recipient's inventory.
//!
//! Where `inventory-item-ops` proves a single avatar can create, copy, move, and
//! link its *own* items, this proves the cross-avatar hand-off: a give is an
//! `ImprovedInstantMessage` with the `IM_INVENTORY_OFFERED` dialog
//! ([`ImDialog::InventoryOffered`]) whose binary bucket carries the offered
//! asset's type byte and id, routed by the grid's IM service to the named
//! recipient (not broadcast like local chat). The recipient decodes the offer
//! ([`inventory_offer`](sl_client_tokio::InstantMessage::inventory_offer)) and
//! replies with `IM_INVENTORY_ACCEPTED`
//! ([`Command::AcceptInventoryOffer`]); the grid relays that acceptance back to
//! the giver.
//!
//! Sequence (primary = giver, secondary = recipient):
//!
//! 1. The primary creates a transferable notecard in its own inventory.
//! 2. The primary [`Command::GiveInventory`]s it to the secondary with a fresh
//!    correlation transaction id.
//! 3. The secondary — a separate session — observes the matching
//!    [`Event::InstantMessageReceived`] with [`ImDialog::InventoryOffered`],
//!    attributed to the primary, and decodes the
//!    [`InventoryOffer`](sl_client_tokio::InventoryOffer).
//! 4. The secondary [`Command::AcceptInventoryOffer`]s, filing it into its
//!    Notecards folder.
//! 5. The primary observes the matching [`Event::InstantMessageReceived`] with
//!    [`ImDialog::InventoryAccepted`] attributed to the secondary — the grid's
//!    confirmation that the give round-tripped.
//! 6. The case re-fetches the recipient's Notecards folder over
//!    [`Command::RequestFolderContents`] and asserts the offered item's copy is
//!    present — never trusting the optimistic local cache.
//!
//! **OpenSim semantics** (`InventoryTransferModule` / `Scene.GiveInventoryItem`):
//! the give handler files a *copy* of the item into the recipient's inventory at
//! offer time (in the recipient's default folder for the asset type — the
//! Notecards folder for a notecard), then rewrites the offer IM's binary bucket
//! to carry the *new copy's* id before forwarding it to the recipient. So the
//! decoded offer's `item_id` is the copy in the recipient's inventory, not the
//! giver's original. The recipient's `IM_INVENTORY_ACCEPTED` is forwarded back to
//! the giver verbatim (the destination folder it carries is honoured only on the
//! task-inventory accept path; for an agent give the copy is already filed), so
//! the acceptance is observable on the giver side and the run is a full
//! round-trip on OpenSim (unlike `calling-card`, whose accept handler is a
//! no-op). The giver's original keeps its Copy permission, so the grid does not
//! delete it (a give of a copyable item leaves the original behind).
//!
//! `2av`. `[opensim]` only; the Aditi variant is deferred to Phase Z pending a
//! second Aditi avatar. The flow is plain LLUDP `ImprovedInstantMessage` in both
//! directions, identical on both grids.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    AssetType, Command, Event, FolderType, ImDialog, InventoryFolderKey, InventoryItem,
    InventoryItemOrFolderKey, InventoryKey, InventoryType, NewInventoryItem, Throttle,
    TransactionId, Uuid, WearableType,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{check_eq, secs_metric, send_then_wait};

/// How long to wait for the region to become active.
const REGION_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for the inventory root query to answer.
const ROOTS_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for any one folder's contents to arrive.
const FOLDER_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for the create reply that allocates the new item id.
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a verification poll keeps re-fetching before giving up.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait between verification re-fetches.
const VERIFY_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// The next-owner permissions a created item is given: copy | modify | transfer
/// (`PERM_COPY | PERM_MODIFY | PERM_TRANSFER`). The Transfer bit is what makes the
/// item giveable; OpenSim also forces the creator's own current permissions to
/// full, so the original survives the give (a copyable item is not deleted).
const NEXT_OWNER_FULL: u32 = 0x0008_2000 | 0x0004_0000 | 0x0002_0000;

/// Query the agent's inventory root folder.
async fn agent_root(session: &mut Session) -> Result<InventoryFolderKey, TestFailure> {
    send_then_wait(
        session,
        Command::QueryInventoryRoots,
        ROOTS_TIMEOUT,
        |event| match event {
            Event::InventoryRoots { agent_root, .. } => *agent_root,
            _ => None,
        },
    )
    .await
}

/// The key of the system folder of `folder_type` directly under `root`, matched
/// by its preferred-type byte.
async fn find_system_folder(
    session: &mut Session,
    root: InventoryFolderKey,
    folder_type: FolderType,
) -> Result<InventoryFolderKey, TestFailure> {
    let wanted = folder_type.to_code();
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
        .find(|entry| entry.folder_type == wanted)
        .map(|entry| entry.folder_id)
        .ok_or_else(|| {
            TestFailure::Assertion(format!(
                "no {folder_type:?} system folder under the inventory root"
            ))
        })
}

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

/// Gives an inventory item from the primary to the secondary, confirming the
/// offer, the acceptance round-trip, and the item's arrival in the recipient's
/// inventory.
#[derive(Debug)]
pub struct GiveInventory;

impl GridTest for GiveInventory {
    fn name(&self) -> &'static str {
        "give-inventory"
    }

    fn description(&self) -> &'static str {
        "Primary gives an item to the secondary; secondary accepts; the item arrives (UDP)"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Both avatars must be logged in and active before an offer can be
            // routed between them.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;

            // Capture both agent ids: the secondary's while it is borrowed, then
            // release the borrow before reborrowing the primary. The offer is
            // attributed to the primary and addressed to the secondary.
            let secondary_id = secondary.agent_id().ok_or_else(|| {
                TestFailure::Assertion("secondary login did not report an agent id".to_owned())
            })?;
            let primary_id = ctx.primary().agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login did not report an agent id".to_owned())
            })?;

            // Distinct per-run name so a leftover item from an aborted run cannot
            // be mistaken for this run's, and so concurrent runs do not collide.
            let tag: String = Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect();
            let item_name = format!("conf-give-{tag}");

            // --- Primary: create a transferable notecard in its own Notecards
            // folder. The reply carries the server-allocated item id; re-fetch
            // confirms it landed.
            let primary = ctx.primary();
            primary
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;
            let primary_root = agent_root(primary).await?;
            let primary_notecards =
                find_system_folder(primary, primary_root, FolderType::Notecard).await?;

            let wanted = item_name.clone();
            let original = send_then_wait(
                primary,
                Command::CreateInventoryItem(NewInventoryItem {
                    folder_id: primary_notecards,
                    transaction_id: Uuid::nil(),
                    next_owner_mask: NEXT_OWNER_FULL,
                    asset_type: AssetType::Notecard,
                    inv_type: InventoryType::Notecard,
                    wearable_type: WearableType::Shape,
                    name: item_name.clone(),
                    description: "sl-conformance give-inventory".to_owned(),
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
                primary,
                primary_notecards,
                |items| items.iter().any(|entry| entry.item_id == original),
                "created item did not appear under the giver's Notecards folder",
            )
            .await?;

            // --- Primary gives the item to the secondary with a fresh correlation
            // id; time the delivery.
            let transaction = TransactionId::from(Uuid::new_v4());
            let given_at = Instant::now();
            primary
                .send(Command::GiveInventory {
                    to_agent_id: secondary_id,
                    item_id: original,
                    asset_type: AssetType::Notecard,
                    item_name: item_name.clone(),
                    transaction_id: transaction,
                })
                .await?;

            // --- Secondary observes the inventory offer from the primary and
            // decodes it. Filtering on the offering agent, the dialog, and the
            // exact item name ignores any unrelated background offer.
            let wanted_name = item_name.clone();
            let offer_match_name = item_name.clone();
            let offer = ctx
                .secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for(REPLY_TIMEOUT, move |event| match event {
                    Event::InstantMessageReceived(im)
                        if im.from_agent_id == primary_id
                            && im.dialog == ImDialog::InventoryOffered
                            && im.message == offer_match_name =>
                    {
                        im.inventory_offer()
                    }
                    _ => None,
                })
                .await?;
            let offer_rtt = given_at.elapsed();

            // The offer is attributed to the primary and describes a single
            // notecard item (not a folder). OpenSim rewrites the bucket to carry
            // the recipient's copy id, so this is the id to verify against the
            // recipient's inventory.
            check_eq("offer from_agent_id", &offer.from_agent_id, &primary_id)?;
            check_eq("offer asset_type", &offer.asset_type, &AssetType::Notecard)?;
            check_eq("offer from_task", &offer.from_task, &false)?;
            let copy_id: InventoryKey = match offer.item_id {
                InventoryItemOrFolderKey::Item(id) => id,
                InventoryItemOrFolderKey::Folder(_) => {
                    return Err(TestFailure::Assertion(
                        "inventory offer carried a folder, expected a single item".to_owned(),
                    ));
                }
            };

            // --- Secondary accepts, filing the item into its Notecards folder, and
            // confirms the item physically landed there (never trusting the
            // optimistic cache).
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            let secondary_root = agent_root(secondary).await?;
            let secondary_notecards =
                find_system_folder(secondary, secondary_root, FolderType::Notecard).await?;
            let accepted_at = Instant::now();
            secondary
                .send(Command::AcceptInventoryOffer {
                    offer,
                    folder_id: secondary_notecards,
                })
                .await?;

            // --- Primary observes the acceptance the grid relays back. This is the
            // round-trip confirmation that OpenSim's calling-card accept lacks.
            ctx.primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::InstantMessageReceived(im)
                        if im.from_agent_id == secondary_id
                            && im.dialog == ImDialog::InventoryAccepted =>
                    {
                        Some(())
                    }
                    _ => None,
                })
                .await?;
            let accept_rtt = accepted_at.elapsed();

            // --- Verify the offered item's copy is in the recipient's Notecards
            // folder (OpenSim files it there at offer time), matched on both the
            // copy id and the name.
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            poll_items(
                secondary,
                secondary_notecards,
                |items| {
                    items
                        .iter()
                        .any(|entry| entry.item_id == copy_id && entry.name == wanted_name)
                },
                "the given item did not appear in the recipient's Notecards folder",
            )
            .await?;

            // Clean up so back-to-back runs start clean: the recipient deletes the
            // received copy, the giver deletes its original. Item deletion is not
            // Trash-gated, so neither needs the move-to-Trash dance.
            secondary
                .send(Command::RemoveInventoryItems(vec![copy_id]))
                .await?;
            ctx.primary()
                .send(Command::RemoveInventoryItems(vec![original]))
                .await?;

            let metrics = ctx.metrics();
            metrics.set("path", "udp");
            metrics.set_timing(&secs_metric("offer_rtt"), offer_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("accept_rtt"), accept_rtt.as_secs_f64());
            Ok(())
        })
    }
}
