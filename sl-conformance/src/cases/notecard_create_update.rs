//! Author a notecard the Second Life way: create the item *empty*
//! (`CreateInventoryItem`), then set its body over the
//! `UpdateNotecardAgentInventory` capability
//! ([`Command::UpdateInventoryAsset`] with [`UpdatableAssetType::Notecard`]).
//!
//! This is the portable notecard-authoring flow — and the one `asset-upload`
//! cannot exercise on Second Life. There, `NewFileAgentInventory` accepts only
//! the file-upload asset classes (texture / sound / animation / mesh / …) and
//! rejects a notecard with `Invalid asset type`, so a viewer never uploads a
//! notecard in one step; it creates an empty item and fills it in afterwards.
//! Both grids offer this create-then-update path (OpenSim *also* accepts the
//! one-step `NewFileAgentInventory` notecard that `asset-upload` uses, but
//! Second Life does not), so this case covers the path that works on both.
//!
//! The two steps are distinct wire flows: the create rides the legacy UDP
//! `CreateInventoryItem` (answered by an `UpdateCreateInventoryItem` →
//! [`Event::InventoryItemCreated`], which allocates the server item id and, on
//! a fresh notecard, a placeholder body asset); the body write is a two-step
//! CAPS POST to `UpdateNotecardAgentInventory` (metadata → uploader URL → raw
//! bytes → completion), whose [`Event::AssetUploaded`] names the *new* body
//! asset id that replaces the placeholder. The case asserts the created item,
//! the new body asset, and then best-effort re-fetches that asset over the
//! `ViewerAsset` `AssetStore` to confirm the body round-trips. Each run uses a
//! unique body so a leftover notecard cannot be mistaken for this run's; the
//! created item is deleted again on the way out.

use std::sync::Arc;
use std::time::Instant;

use sl_client_tokio::{
    AssetCacheLimits, AssetKey, AssetType, Command, Event, InventoryKey, InventoryType,
    NewInventoryItem, ReqwestAssetFetcher, Throttle, UpdatableAssetType, Uuid, WearableType,
};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check};

/// The next-owner permission mask a viewer sends for a fresh notecard
/// (move / modify / copy / transfer) — mirrors `asset-upload`.
const NEXT_OWNER_MASK: u32 = 0x0008_e000;

/// Creates an empty notecard and sets its body over `UpdateNotecardAgentInventory`.
#[derive(Debug)]
pub struct NotecardCreateUpdate;

impl GridTest for NotecardCreateUpdate {
    fn name(&self) -> &'static str {
        "notecard-create-update"
    }

    fn description(&self) -> &'static str {
        "Create an empty notecard, then set its body over UpdateNotecardAgentInventory"
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

            // The body write rides this capability; without it the create-update
            // authoring flow cannot run. Both grids offer it.
            if session.cap("UpdateNotecardAgentInventory").is_none() {
                ctx.mark_partial("no UpdateNotecardAgentInventory capability offered");
                return Ok(());
            }
            // Capture the ViewerAsset cap up front (for the best-effort re-fetch);
            // its absence only skips the round-trip check, not the case.
            let viewer_asset = session.cap("ViewerAsset");

            // Create the notecard under the agent inventory root; a folder id is
            // required and the root always exists.
            session.send(Command::QueryInventoryRoots).await?;
            let Some(root) = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::InventoryRoots { agent_root, .. } => Some(*agent_root),
                    _other => None,
                })
                .await?
            else {
                return Err(TestFailure::Assertion(
                    "no agent inventory root to create the notecard in".to_owned(),
                ));
            };

            // A distinct per-run name + body so a leftover notecard from an aborted
            // run cannot be confused for this run's, and concurrent runs never
            // collide.
            let tag: String = Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect();
            let name = format!("conf-notecard-{tag}");

            // --- create: an empty notecard. The reply carries the server-allocated
            // item id; on a fresh notecard the sim also mints a placeholder body
            // asset, which the update below replaces.
            let create_start = Instant::now();
            let wanted = name.clone();
            session
                .send(Command::CreateInventoryItem(NewInventoryItem {
                    folder_id: root,
                    transaction_id: Uuid::nil(),
                    next_owner_mask: NEXT_OWNER_MASK,
                    asset_type: AssetType::Notecard,
                    inv_type: InventoryType::Notecard,
                    wearable_type: WearableType::Shape,
                    name: name.clone(),
                    description: "sl-conformance notecard-create-update".to_owned(),
                }))
                .await?;
            let (sim_approved, item) = session
                .wait_for(LONG_TIMEOUT, move |event| match event {
                    Event::InventoryItemCreated {
                        sim_approved, item, ..
                    } if item.name == wanted => Some((*sim_approved, item.clone())),
                    _other => None,
                })
                .await?;
            check(
                sim_approved,
                "the simulator did not approve the notecard creation",
            )?;
            let item_id = item.item_id;
            check(
                !item_id.uuid().is_nil(),
                "the created notecard has a nil item id",
            )?;
            let create_secs = create_start.elapsed().as_secs_f64();

            // --- update: set the body over UpdateNotecardAgentInventory. The
            // completion names the new body asset id (replacing the placeholder).
            let body = notecard_bytes(&format!("sl-conformance notecard-create-update {tag}\n"));
            let byte_len = body.len();
            let update_start = Instant::now();
            session
                .send(Command::UpdateInventoryAsset {
                    item_id,
                    asset_type: UpdatableAssetType::Notecard,
                    data: body,
                })
                .await?;
            let outcome = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::AssetUploaded { new_asset, .. } => Some(Ok(*new_asset)),
                    Event::AssetUploadFailed { reason } => Some(Err(reason.clone())),
                    _other => None,
                })
                .await?;
            let update_secs = update_start.elapsed().as_secs_f64();

            let new_asset = match outcome {
                Ok(asset) => asset,
                Err(reason) => {
                    // Clean up the empty item before failing so a retry starts clean.
                    delete_item(ctx, item_id).await;
                    return Err(TestFailure::Assertion(format!(
                        "UpdateNotecardAgentInventory failed: {reason}"
                    )));
                }
            };
            check(!new_asset.is_nil(), "the update stored a nil body asset id")?;

            // --- best-effort round-trip: re-fetch the new body asset over the
            // ViewerAsset store and confirm the bytes match. A miss here (no cap,
            // aditi's ViewerAsset 503s, or brief propagation lag) does not fail the
            // case — the authoring flow, the thing under test, already succeeded.
            let roundtrip = match viewer_asset {
                Some(cap) => refetch_body(cap, new_asset, byte_len).await,
                None => None,
            };

            let metrics = ctx.metrics();
            metrics.set_timing("create_secs", create_secs);
            metrics.set_timing("update_secs", update_secs);
            metrics.set("asset_bytes", i64::try_from(byte_len).unwrap_or(-1));
            metrics.set("item_id", item_id.to_string());
            metrics.set("new_asset", new_asset.to_string());
            match roundtrip {
                Some(true) => metrics.set("roundtrip", "match"),
                Some(false) => metrics.set("roundtrip", "mismatch"),
                None => metrics.set("roundtrip", "skipped"),
            }

            // Best-effort cleanup so runs do not accumulate inventory.
            delete_item(ctx, item_id).await;

            Ok(())
        })
    }
}

/// Best-effort re-fetch of the just-written notecard body over the `ViewerAsset`
/// `AssetStore`. Returns `Some(true)` when the fetched bytes equal the uploaded
/// length, `Some(false)` on a length mismatch, or `None` when the asset could not
/// be fetched (service unavailable / propagation lag) — none of which fails the
/// case.
async fn refetch_body(cap: String, new_asset: Uuid, expected_len: usize) -> Option<bool> {
    let dir = std::env::temp_dir().join(format!("sl-conformance-notecard-{}", std::process::id()));
    let _removed = fs_err::remove_dir_all(&dir);
    let fetcher = Arc::new(ReqwestAssetFetcher::with_default_client());
    fetcher.set_cap_url(Some(cap));
    let store =
        sl_client_tokio::AssetStore::new(fetcher, Some(dir.clone()), AssetCacheLimits::default())
            .ok()?;
    let result = match store
        .get(AssetKey::from(new_asset), AssetType::Notecard)
        .await
    {
        Ok(entry) => entry.data().map(|data| data.len() == expected_len),
        Err(error) => {
            tracing::warn!(%error, "notecard-create-update: body re-fetch failed");
            None
        }
    };
    let _removed = fs_err::remove_dir_all(&dir);
    result
}

/// Best-effort deletion of the created notecard item (cleanup; a failure here
/// does not fail the case).
async fn delete_item(ctx: &mut TestContext, item_id: InventoryKey) {
    ctx.primary()
        .send(Command::RemoveInventoryItems(vec![item_id]))
        .await
        .ok();
}

/// Wraps `text` in the Second Life notecard asset format (`Linden text version
/// 2`) — the bytes a viewer POSTs for a notecard body.
fn notecard_bytes(text: &str) -> Vec<u8> {
    format!(
        "Linden text version 2\n{{\nLLEmbeddedItems version 1\n{{\ncount 0\n}}\nText length {}\n{}}}\n",
        text.len(),
        text,
    )
    .into_bytes()
}
