//! Upload a new asset over the modern CAPS `NewFileAgentInventory` uploader and
//! confirm the two-step flow both stores the asset *and* creates the inventory
//! item — the outcome the modern viewer relies on.
//!
//! The uploader is a two-step CAPS POST: the LLSD metadata (destination folder,
//! asset/inventory class, permissions, expected cost) goes to the
//! `NewFileAgentInventory` capability, which answers with an `uploader` URL; the
//! raw asset bytes go there, and the completion carries the new asset UUID plus
//! the new inventory-item UUID. Both grids offer the capability and the modern
//! viewer uploads exclusively over it, so the legacy UDP `AssetUploadRequest`
//! path is not exercised (it was dropped in favour of this CAPS-only flow,
//! mirroring `asset-fetch-http`).
//!
//! A notecard is uploaded because it needs no client-side encoding — the bytes
//! are the `Linden text version 2` container a viewer POSTs verbatim — and it is
//! free on OpenSim, which accepts a notecard through this capability. Each run
//! uses a unique body so a leftover item cannot be mistaken for this run's; the
//! created item is deleted again on the way out.
//!
//! **Grid divergence:** OpenSim serves notecard creation through
//! `NewFileAgentInventory`, so the full store-asset-and-create-item flow runs
//! there. Second Life does **not** — its `NewFileAgentInventory` accepts only the
//! file-upload asset classes (texture, sound, animation, mesh, …) and answers a
//! notecard with `Invalid asset type`; on SL a notecard is instead created empty
//! (`CreateInventoryItem`) and its body set with `UpdateNotecardAgentInventory`.
//! So on Second Life the case records `partial` with the server's reason — the
//! client correctly formed and POSTed the request; the grid declined the asset
//! class — mirroring `asset-fetch-http`'s aditi handling.

use std::time::Instant;

use sl_client_tokio::{AssetType, Command, Event, InventoryKey, InventoryType, Throttle, Uuid};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check, is_aditi};

/// Uploads a notecard over the `NewFileAgentInventory` capability.
#[derive(Debug)]
pub struct AssetUpload;

impl GridTest for AssetUpload {
    fn name(&self) -> &'static str {
        "asset-upload"
    }

    fn description(&self) -> &'static str {
        "Upload an asset over the NewFileAgentInventory CAPS uploader"
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

            // The two-step CAPS uploader both grids offer; without it the modern
            // upload cannot run (the legacy UDP path was dropped).
            if session.cap("NewFileAgentInventory").is_none() {
                ctx.mark_partial("no NewFileAgentInventory capability offered");
                return Ok(());
            }

            // Upload into the agent inventory root; a folder id is required in the
            // metadata and the root always exists.
            session.send(Command::QueryInventoryRoots).await?;
            let Some(root) = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::InventoryRoots { agent_root, .. } => Some(*agent_root),
                    _other => None,
                })
                .await?
            else {
                return Err(TestFailure::Assertion(
                    "no agent inventory root to upload into".to_owned(),
                ));
            };

            // A distinct per-run body so a leftover notecard from an aborted run
            // cannot be confused for this run's, and concurrent runs never collide.
            let tag: String = Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect();
            let name = format!("conf-upload-{tag}");
            let body = notecard_bytes(&format!("sl-conformance asset-upload {tag}\n"));
            let byte_len = body.len();

            let start = Instant::now();
            session
                .send(Command::UploadAsset {
                    folder_id: root,
                    asset_type: AssetType::Notecard,
                    inventory_type: InventoryType::Notecard,
                    name: name.clone(),
                    description: "sl-conformance asset-upload".to_owned(),
                    // The next-owner mask a viewer sends for a fresh notecard
                    // (move / modify / copy / transfer).
                    next_owner_mask: 0x0008_e000,
                    group_mask: 0,
                    everyone_mask: 0,
                    expected_upload_cost: 0,
                    data: body,
                })
                .await?;

            // The two-step uploader completes as `AssetUploaded` (stored asset +
            // created item) or `AssetUploadFailed` (a grid/permission error).
            let outcome = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::AssetUploaded {
                        new_asset,
                        new_inventory_item,
                    } => Some(Ok((*new_asset, *new_inventory_item))),
                    Event::AssetUploadFailed { reason } => Some(Err(reason.clone())),
                    _other => None,
                })
                .await?;
            let upload_secs = start.elapsed().as_secs_f64();

            let (new_asset, new_item) = match outcome {
                Ok(pair) => pair,
                Err(reason) => {
                    // Second Life declines a notecard through this capability (it
                    // uploads only file-based asset classes; a notecard is created
                    // empty and updated instead). That is a grid-behaviour
                    // difference, not a client fault — the request was formed and
                    // POSTed correctly — so record it partial with the server's
                    // reason, as `asset-fetch-http` does for aditi's `ViewerAsset`.
                    if is_aditi(grid) {
                        ctx.mark_partial(&format!("grid declined the notecard upload — {reason}"));
                        return Ok(());
                    }
                    return Err(TestFailure::Assertion(format!(
                        "NewFileAgentInventory upload failed: {reason}"
                    )));
                }
            };
            check(!new_asset.is_nil(), "upload stored a nil asset id")?;
            let new_item = new_item.filter(|item| !item.is_nil()).ok_or_else(|| {
                TestFailure::Assertion("upload created no inventory item".to_owned())
            })?;

            let metrics = ctx.metrics();
            metrics.set_timing("upload_secs", upload_secs);
            metrics.set("asset_bytes", i64::try_from(byte_len).unwrap_or(-1));
            metrics.set("new_asset", new_asset.to_string());
            metrics.set("new_item", new_item.to_string());

            // Best-effort cleanup: delete the created notecard so runs do not
            // accumulate inventory. A failure here does not fail the case — the
            // upload (the thing under test) already succeeded.
            let session = ctx.primary();
            session
                .send(Command::RemoveInventoryItems(vec![InventoryKey::from(
                    new_item,
                )]))
                .await
                .ok();

            Ok(())
        })
    }
}

/// Wraps `text` in the Second Life notecard asset format (`Linden text version
/// 2`) — the bytes a viewer POSTs for a notecard upload.
fn notecard_bytes(text: &str) -> Vec<u8> {
    format!(
        "Linden text version 2\n{{\nLLEmbeddedItems version 1\n{{\ncount 0\n}}\nText length {}\n{}}}\n",
        text.len(),
        text,
    )
    .into_bytes()
}
