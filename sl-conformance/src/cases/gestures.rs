//! Activate a gesture, then deactivate it — the gesture-activation round trip.
//!
//! A viewer marks a gesture "active" so the simulator preloads it and its
//! trigger word/key fires it, and "inactive" to stop that. The two wire
//! messages are fire-and-forget "tell the database" packets with no reply:
//! `ActivateGestures` (item id + asset id) and `DeactivateGestures` (item id).
//! The observable effect is that the simulator flips a bit in the gesture's
//! **inventory item flags** — OpenSim's `GesturesModule` sets `Flags |= 1` on
//! activate and clears it on deactivate — so re-fetching the folder over
//! [`Command::RequestFolderContents`] and inspecting the item's flags is the
//! proof the toggle took effect. (On Second Life the active set is surfaced to
//! the viewer only in the login response's `gestures` array, not in an in-session
//! query, so the flag round-trip is asserted only on OpenSim; see below.)
//!
//! The case is self-contained on either grid: it creates its own throwaway
//! gesture item ([`Command::CreateInventoryItem`] with [`AssetType::Gesture`]),
//! so no pre-existing gesture inventory is needed, then:
//!
//! 1. **Activate** it ([`Command::ActivateGestures`] pairing the item id with its
//!    asset id) and, on OpenSim, poll the containing folder until the item's
//!    flags carry the active bit.
//! 2. **Deactivate** it ([`Command::DeactivateGestures`] by item id) and, on
//!    OpenSim, poll until the active bit is cleared again.
//!
//! The created gesture item is deleted on the way out so re-runs start clean.
//!
//! `1av`, `[both]`. **OpenSim** exercises the full observable round trip (the
//! inventory flag flips on and off). **Second Life** (aditi) drives the same wire
//! exchange but modern SL does not reflect gesture-active state in the in-session
//! inventory item flags — the authoritative active set reaches a viewer only via
//! the login response's `gestures` list — so there the flag is best-effort: if
//! the grid happens to reflect it the case records that, otherwise it marks
//! partial rather than failing.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    AssetType, Command, Event, GestureActivation, InventoryFolderKey, InventoryItem, InventoryKey,
    InventoryType, NewInventoryItem, Throttle, Uuid, WearableType,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    LONG_TIMEOUT, REGION_TIMEOUT, REPLY_TIMEOUT, check, is_opensim, send_then_wait,
};

/// The inventory item flag bit a simulator sets on a gesture while it is active
/// (`GesturesModule`: `item.Flags |= 1`). Bit 0; cleared on deactivate.
const GESTURE_ACTIVE_FLAG: u32 = 1;

/// The next-owner permission mask a viewer sends for a fresh item (move / modify
/// / copy / transfer) — matches the notecard/asset-upload cases.
const NEXT_OWNER_MASK: u32 = 0x0008_e000;

/// How long a verification poll keeps re-fetching the folder before giving up.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait between verification re-fetches (absorbs OpenSim's
/// fire-and-forget descendents worker).
const VERIFY_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Activates a throwaway gesture and deactivates it again, verifying each step by
/// the simulator's inventory-item flag on OpenSim.
#[derive(Debug)]
pub struct Gestures;

impl GridTest for Gestures {
    fn name(&self) -> &'static str {
        "gestures"
    }

    fn description(&self) -> &'static str {
        "Activate a gesture, then deactivate it"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            // Create the throwaway gesture under the agent inventory root; a folder
            // id is required and the root always exists.
            session.send(Command::QueryInventoryRoots).await?;
            let root = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::InventoryRoots { agent_root, .. } => *agent_root,
                    _ => None,
                })
                .await?;

            // A distinct per-run name so a leftover gesture from an aborted run
            // cannot be mistaken for this run's, and concurrent runs never collide.
            let tag: String = Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect();
            let name = format!("conf-gesture-{tag}");

            // --- create: an empty gesture item. The reply carries the
            // server-allocated item id (and a placeholder asset id).
            let create_start = Instant::now();
            let wanted = name.clone();
            let created = send_then_wait(
                session,
                Command::CreateInventoryItem(NewInventoryItem {
                    folder_id: root,
                    transaction_id: Uuid::nil(),
                    next_owner_mask: NEXT_OWNER_MASK,
                    asset_type: AssetType::Gesture,
                    inv_type: InventoryType::Gesture,
                    wearable_type: WearableType::Shape,
                    name: name.clone(),
                    description: "sl-conformance gestures".to_owned(),
                }),
                REPLY_TIMEOUT,
                move |event| match event {
                    Event::InventoryItemCreated {
                        sim_approved, item, ..
                    } if item.name == wanted => Some((*sim_approved, item.clone())),
                    _ => None,
                },
            )
            .await?;
            let (sim_approved, item) = created;
            check(
                sim_approved,
                "the simulator did not approve the gesture creation",
            )?;
            let item_id = item.item_id;
            check(
                !item_id.uuid().is_nil(),
                "the created gesture has a nil item id",
            )?;
            let create_secs = create_start.elapsed().as_secs_f64();

            // --- activate: mark the gesture active. Fire-and-forget; the effect is
            // the inventory flag flip.
            let activate_start = Instant::now();
            session
                .send(Command::ActivateGestures {
                    gestures: vec![GestureActivation {
                        item_id,
                        asset_id: item.asset_id,
                    }],
                })
                .await?;
            let activated = poll_flag(session, root, item_id, |flags| {
                flags & GESTURE_ACTIVE_FLAG != 0
            })
            .await?;
            let activate_secs = activate_start.elapsed().as_secs_f64();

            // --- deactivate: mark it inactive again; the active bit clears.
            let deactivate_start = Instant::now();
            session
                .send(Command::DeactivateGestures {
                    item_ids: vec![item_id],
                })
                .await?;
            let deactivated = if activated {
                // Only meaningful to watch the bit clear if it was ever observed set.
                poll_flag(session, root, item_id, |flags| {
                    flags & GESTURE_ACTIVE_FLAG == 0
                })
                .await?
            } else {
                false
            };
            let deactivate_secs = deactivate_start.elapsed().as_secs_f64();

            // Best-effort cleanup so runs do not accumulate inventory.
            session
                .send(Command::RemoveInventoryItems(vec![item_id]))
                .await
                .ok();

            // OpenSim must exercise the full observable round trip; Second Life
            // does not reflect the active state in the in-session inventory flag
            // (the active set is only in the login response's gestures array), so
            // there the flag observation is best-effort.
            if is_opensim(grid) {
                check(
                    activated,
                    "the gesture's inventory flag never carried the active bit after ActivateGestures",
                )?;
                check(
                    deactivated,
                    "the gesture's inventory flag never cleared the active bit after DeactivateGestures",
                )?;
            } else if !activated {
                ctx.mark_partial(
                    "aditi did not reflect the gesture-active state in the in-session inventory \
                     flag (the active set is surfaced only in the login response's gestures array); \
                     the ActivateGestures/DeactivateGestures wire exchange was still driven",
                );
            }

            let metrics = ctx.metrics();
            metrics.set("item_id", item_id.to_string());
            metrics.set("active_flag_observed", activated);
            metrics.set("inactive_flag_observed", deactivated);
            metrics.set_timing("create_secs", create_secs);
            metrics.set_timing("activate_secs", activate_secs);
            metrics.set_timing("deactivate_secs", deactivate_secs);
            Ok(())
        })
    }
}

/// Re-fetch `folder` over [`Command::RequestFolderContents`] until `item`'s flags
/// satisfy `accept`, returning `true` once they do. Returns `false` if the item
/// never reaches that state within [`VERIFY_TIMEOUT`] — a soft result the caller
/// turns into a hard assertion (OpenSim) or a partial (aditi).
async fn poll_flag(
    session: &mut Session,
    folder: InventoryFolderKey,
    item: InventoryKey,
    mut accept: impl FnMut(u32) -> bool,
) -> Result<bool, TestFailure> {
    let start = Instant::now();
    loop {
        let items = fetch_items(session, folder).await?;
        if let Some(entry) = items.iter().find(|entry| entry.item_id == item)
            && accept(entry.flags)
        {
            return Ok(true);
        }
        if start.elapsed() >= VERIFY_TIMEOUT {
            return Ok(false);
        }
        tokio::time::sleep(VERIFY_POLL_INTERVAL).await;
    }
}

/// Fetch a folder's immediate items by issuing a fresh
/// [`Command::RequestFolderContents`] and returning the grid's authoritative
/// reply (rather than the optimistic local cache).
async fn fetch_items(
    session: &mut Session,
    folder: InventoryFolderKey,
) -> Result<Vec<InventoryItem>, TestFailure> {
    send_then_wait(
        session,
        Command::RequestFolderContents(folder),
        REPLY_TIMEOUT,
        |event| match event {
            Event::InventoryDescendents {
                folder_id, items, ..
            } if *folder_id == folder => Some(items.clone()),
            _ => None,
        },
    )
    .await
}
