//! Logs in to a Second Life / OpenSim grid and uploads an asset (ROADMAP #23)
//! over both paths: the legacy UDP `AssetUploadRequest` (which stores the asset
//! with no inventory item) and the modern `NewFileAgentInventory` capability
//! (which also creates an inventory item). Uploads a small notecard by default.
//!
//! For a mesh, this only uploads the **fully-formed mesh asset bytes** — it does
//! not run the viewer's model-import pipeline (LOD / physics-shape / cost
//! generation), matching the roadmap scope.
//!
//! Configure via environment variables:
//!   `SL_LOGIN_URI`  (default `http://127.0.0.1:9000/`)
//!   `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`  (required)
//!   `SL_START`      (default `last`)
//!   `SL_CHANNEL`    (default `sl-client-tokio-upload`)
//!   `SL_VERSION`    (default this crate's version)
//!   `SL_HOLD_SECS`  (default `25`)
//!   `SL_NOTE_TEXT`  (default a short greeting) — the notecard body to upload

use std::time::Duration;

use sl_client_tokio::{
    AssetType, Client, Command, DisconnectReason, Error, Event, InventoryType, LoginParams,
    LoginRequest, Throttle, Uuid,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

/// Wraps `text` in the Second Life notecard asset format (`Linden text version
/// 2`), the bytes a viewer uploads for a notecard.
fn notecard_bytes(text: &str) -> Vec<u8> {
    format!(
        "Linden text version 2\n{{\nLLEmbeddedItems version 1\n{{\ncount 0\n}}\nText length {}\n{}}}\n",
        text.len(),
        text,
    )
    .into_bytes()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let login_uri = env_or("SL_LOGIN_URI", "http://127.0.0.1:9000/");
    let first = std::env::var("SL_FIRST")?;
    let last = std::env::var("SL_LAST")?;
    let password = std::env::var("SL_PASSWORD")?;
    let start = env_or("SL_START", "last");
    let channel = env_or("SL_CHANNEL", "sl-client-tokio-upload");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));
    let hold_secs: u64 = env_or("SL_HOLD_SECS", "25").parse()?;
    let note_text = env_or("SL_NOTE_TEXT", "Uploaded by sl-client ROADMAP #23.\n");
    let asset_data = notecard_bytes(&note_text);

    info!("logging in...");
    let request = LoginRequest::new(first, last, password, start, channel, version);
    let params = LoginParams { login_uri, request };
    let client = match Client::connect(params).await {
        Ok(client) => client,
        Err(Error::MfaChallenge(_)) => {
            return Err("this probe does not support interactive MFA".into());
        }
        Err(other) => return Err(other.into()),
    };
    info!("login succeeded; running session");

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
    let (command_tx, command_rx) = mpsc::channel::<Command>(8);
    let run = tokio::spawn(client.run(event_tx, command_rx));

    // The destination folder for the CAPS upload, learned from the inventory
    // skeleton (the root "My Inventory" folder, whose parent is nil).
    let mut root_folder: Option<Uuid> = None;
    let mut requested = false;
    while let Some(event) = event_rx.recv().await {
        match event {
            Event::InventorySkeleton(folders) => {
                root_folder = folders
                    .iter()
                    .find(|folder| folder.parent_id.is_nil())
                    .map(|folder| folder.folder_id)
                    .or(root_folder);
            }
            Event::RegionHandshakeComplete | Event::RegionChanged { .. } if !requested => {
                requested = true;
                info!("region active; advertising throttle then uploading");
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_1000()))
                    .await
                    .ok();
                // Legacy UDP path: stores the asset, no inventory item.
                command_tx
                    .send(Command::UploadAssetUdp {
                        asset_type: AssetType::Notecard,
                        data: asset_data.clone(),
                        temp_file: false,
                        store_local: false,
                    })
                    .await
                    .ok();
                // Modern CAPS path: stores the asset and creates an inventory item.
                match root_folder {
                    Some(folder_id) => {
                        command_tx
                            .send(Command::UploadAsset {
                                folder_id,
                                asset_type: AssetType::Notecard,
                                inventory_type: InventoryType::Notecard,
                                name: "sl-client upload #23".to_owned(),
                                description: "uploaded over NewFileAgentInventory".to_owned(),
                                next_owner_mask: 0x0008_e000,
                                group_mask: 0,
                                everyone_mask: 0,
                                expected_upload_cost: 0,
                                data: asset_data.clone(),
                            })
                            .await
                            .ok();
                    }
                    None => warn!("no inventory root learned; skipping CAPS upload"),
                }
                let command_tx = command_tx.clone();
                tokio::spawn(async move {
                    sleep(Duration::from_secs(hold_secs)).await;
                    command_tx.send(Command::Logout).await.ok();
                });
            }
            Event::AssetUploadComplete {
                asset_id,
                asset_type,
                success,
            } => info!("AssetUploadComplete (UDP) {asset_id} ({asset_type:?}) success={success}"),
            Event::AssetUploaded {
                new_asset,
                new_inventory_item,
            } => info!("AssetUploaded (CAPS) asset={new_asset} item={new_inventory_item:?}"),
            Event::AssetUploadFailed { reason } => warn!("AssetUploadFailed: {reason}"),
            Event::LoggedOut => {
                info!("logged out cleanly");
                break;
            }
            Event::Disconnected(reason) => {
                match reason {
                    DisconnectReason::Timeout => warn!("disconnected: inactivity timeout"),
                    other => warn!("disconnected: {other:?}"),
                }
                break;
            }
            _ => {}
        }
    }

    run.await??;
    Ok(())
}
