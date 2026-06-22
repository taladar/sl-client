//! Logs in to a Second Life / OpenSim grid and exercises the inventory
//! **mutation** surface (ROADMAP #30): it learns the inventory root from the
//! login skeleton, then (unless `SL_READONLY=1`) runs a full create → update →
//! move → delete cycle over UDP, plus the `CreateInventoryCategory` capability
//! (a confirmed folder create served by both OpenSim and Second Life).
//!
//! The legacy UDP `CreateInventoryFolder` has no reply, so the folder is cached
//! optimistically; `CreateInventoryItem` is answered by
//! `UpdateCreateInventoryItem` ([`Event::InventoryItemCreated`]); a copy/give is
//! answered by `BulkUpdateInventory` ([`Event::InventoryBulkUpdate`]). AIS3
//! (`InventoryAPIv3`) is Second-Life only — stock OpenSim serves no such cap, so
//! its commands no-op here.
//!
//! Configure via environment variables:
//!   `SL_LOGIN_URI`  (default `http://127.0.0.1:9000/`)
//!   `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`  (required)
//!   `SL_START`      (default `last`)
//!   `SL_CHANNEL`    (default `sl-client-tokio-inventory`)
//!   `SL_VERSION`    (default this crate's version)
//!   `SL_HOLD_SECS`  (default `25`)
//!   `SL_READONLY`   (set to `1` to only list the root folder)

use std::time::Duration;

use sl_client_tokio::{
    Client, Command, DisconnectReason, Error, Event, InventoryFolderKey, InventoryKey, LoginParams,
    LoginRequest, NewInventoryItem, Throttle, Uuid,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let login_uri = env_or("SL_LOGIN_URI", "http://127.0.0.1:9000/");
    let first = std::env::var("SL_FIRST")?;
    let last = std::env::var("SL_LAST")?;
    let password = std::env::var("SL_PASSWORD")?;
    let start = env_or("SL_START", "last").parse::<sl_client_tokio::StartLocation>()?;
    let channel = env_or("SL_CHANNEL", "sl-client-tokio-inventory");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));
    let hold_secs: u64 = env_or("SL_HOLD_SECS", "25").parse()?;
    let readonly = env_or("SL_READONLY", "0") == "1";

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
    let agent_id = client.agent_id().ok_or("no agent id after login")?;
    info!("login succeeded; agent {agent_id}");

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
    let (command_tx, command_rx) = mpsc::channel::<Command>(16);
    let (diag_tx, _diag_rx) = mpsc::channel(16);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

    // The inventory root (learned from the login skeleton: the folder with a nil
    // parent), and a fresh folder/item we create then clean up.
    let mut root: Option<InventoryFolderKey> = None;
    let test_folder = InventoryFolderKey::from(Uuid::new_v4());
    let mut created_item: Option<InventoryKey> = None;
    let mut started = false;

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::InventorySkeleton(folders) => {
                info!("inventory skeleton: {} folders", folders.len());
                root = folders
                    .iter()
                    .find(|folder| folder.parent_id.uuid().is_nil())
                    .map(|folder| folder.folder_id)
                    .or(root);
            }
            Event::RegionHandshakeComplete | Event::RegionChanged { .. } if !started => {
                started = true;
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_1000()))
                    .await
                    .ok();
                let Some(root) = root else {
                    warn!("no inventory root in the login skeleton; nothing to do");
                    continue;
                };
                info!("inventory root is {root}; listing it");
                command_tx
                    .send(Command::RequestFolderContents(root))
                    .await
                    .ok();

                if !readonly {
                    info!("creating folder {test_folder} under root (UDP)");
                    command_tx
                        .send(Command::CreateInventoryFolder {
                            folder_id: test_folder,
                            parent_id: root,
                            folder_type: -1,
                            name: "sl-client #30 test".to_owned(),
                        })
                        .await
                        .ok();
                    // A confirmed folder create via the CreateInventoryCategory cap.
                    command_tx
                        .send(Command::CreateInventoryCategory {
                            parent_id: root,
                            folder_type: -1,
                            name: "sl-client #30 cap folder".to_owned(),
                        })
                        .await
                        .ok();
                    // Create a notecard item in the new folder; the reply
                    // (UpdateCreateInventoryItem) drives the rest of the cycle.
                    info!("creating an item in the test folder");
                    command_tx
                        .send(Command::CreateInventoryItem(NewInventoryItem {
                            folder_id: test_folder,
                            transaction_id: Uuid::nil(),
                            next_owner_mask: 0x0008_e000,
                            asset_type: 7, // notecard
                            inv_type: 7,
                            wearable_type: 0,
                            name: "sl-client #30 note".to_owned(),
                            description: "created then deleted".to_owned(),
                        }))
                        .await
                        .ok();
                }

                let command_tx = command_tx.clone();
                tokio::spawn(async move {
                    sleep(Duration::from_secs(hold_secs)).await;
                    command_tx.send(Command::Logout).await.ok();
                });
            }
            Event::InventoryDescendents {
                folder_id,
                folders,
                items,
                ..
            } => {
                info!(
                    "folder {folder_id}: {} sub-folders, {} items",
                    folders.len(),
                    items.len()
                );
            }
            Event::InventoryItemCreated { item, .. } => {
                info!("item created: {} ({})", item.name, item.item_id);
                // Only drive the edit cycle off the *first* creation; the copy
                // below also arrives as an `UpdateCreateInventoryItem` on OpenSim
                // (Second Life answers a copy with `BulkUpdateInventory`), so
                // guarding here avoids an endless copy loop.
                let first = created_item.is_none();
                created_item = Some(item.item_id);
                if !readonly && first {
                    // Rename the new item.
                    let mut renamed = item.clone();
                    "sl-client #30 note (renamed)".clone_into(&mut renamed.name);
                    command_tx
                        .send(Command::UpdateInventoryItem {
                            item: Box::new(renamed),
                            transaction_id: Uuid::nil(),
                        })
                        .await
                        .ok();
                    // Copy it: the reply is a `BulkUpdateInventory` carrying the
                    // async callback id (#44), surfaced as `item_callbacks` below.
                    command_tx
                        .send(Command::CopyInventoryItem {
                            old_agent_id: agent_id,
                            old_item_id: item.item_id,
                            new_folder_id: test_folder,
                            new_name: "sl-client #44 copy".to_owned(),
                        })
                        .await
                        .ok();
                    // Then delete the item and its folder (purging the copy too).
                    command_tx
                        .send(Command::RemoveInventoryItems(vec![item.item_id]))
                        .await
                        .ok();
                    command_tx
                        .send(Command::RemoveInventoryFolders(vec![test_folder]))
                        .await
                        .ok();
                    info!("renamed and copied, then removed the item and folder");
                }
            }
            Event::InventoryBulkUpdate {
                folders,
                items,
                item_callbacks,
                ..
            } => {
                info!(
                    "bulk inventory update: {} folder(s), {} item(s), {} callback(s)",
                    folders.len(),
                    items.len(),
                    item_callbacks.len()
                );
                for folder in &folders {
                    info!("  folder {} \"{}\"", folder.folder_id, folder.name);
                }
                for (item_id, callback_id) in &item_callbacks {
                    info!("  item {item_id} correlates to callback {callback_id}");
                }
            }
            Event::LoggedOut => {
                info!("logged out cleanly; last created item was {created_item:?}");
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
