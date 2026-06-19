//! Logs in to a Second Life / OpenSim grid and fetches a texture and (optionally)
//! a generic asset by UUID over both transports (ROADMAP #19): the legacy UDP
//! image / transfer path and the modern HTTP `GetTexture`/`GetAsset` capability.
//! Reports the codec and byte length of whatever comes back.
//!
//! Configure via environment variables:
//!   `SL_LOGIN_URI`  (default `http://127.0.0.1:9000/`)
//!   `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`  (required)
//!   `SL_START`      (default `last`)
//!   `SL_CHANNEL`    (default `sl-client-tokio-asset`)
//!   `SL_VERSION`    (default this crate's version)
//!   `SL_HOLD_SECS`  (default `20`)
//!   `SL_TEXTURE_ID` (default the standard "plywood" texture, present in OpenSim)
//!   `SL_ASSET_ID`   (optional: a generic asset UUID to fetch as well)
//!   `SL_ASSET_TYPE` (default `sound`; one of sound/animation/landmark/notecard/
//!                    gesture/clothing/bodypart/object/callingcard/mesh)

use std::time::Duration;

use sl_client_tokio::{
    AssetType, Client, Command, DisconnectReason, Error, Event, LoginParams, LoginRequest,
    Throttle, Uuid,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

/// Maps an `SL_ASSET_TYPE` string to an [`AssetType`] (defaulting to sound).
fn asset_type_from(name: &str) -> AssetType {
    match name.to_ascii_lowercase().as_str() {
        "animation" => AssetType::Animation,
        "landmark" => AssetType::Landmark,
        "notecard" => AssetType::Notecard,
        "gesture" => AssetType::Gesture,
        "clothing" => AssetType::Clothing,
        "bodypart" => AssetType::Bodypart,
        "object" => AssetType::Object,
        "callingcard" => AssetType::CallingCard,
        "mesh" => AssetType::Mesh,
        _ => AssetType::Sound,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let login_uri = env_or("SL_LOGIN_URI", "http://127.0.0.1:9000/");
    let first = std::env::var("SL_FIRST")?;
    let last = std::env::var("SL_LAST")?;
    let password = std::env::var("SL_PASSWORD")?;
    let start = env_or("SL_START", "last");
    let channel = env_or("SL_CHANNEL", "sl-client-tokio-asset");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));
    let hold_secs: u64 = env_or("SL_HOLD_SECS", "20").parse()?;
    // The standard SL/OpenSim "plywood" default texture, present in a stock grid.
    let texture_id: Uuid = env_or("SL_TEXTURE_ID", "89556747-24cb-43ed-920b-47caed15465f")
        .parse()
        .map_err(|_ignored| "SL_TEXTURE_ID is not a valid UUID")?;
    let asset = match std::env::var("SL_ASSET_ID") {
        Ok(value) => {
            let id: Uuid = value
                .parse()
                .map_err(|_ignored| "SL_ASSET_ID is not a valid UUID")?;
            Some((id, asset_type_from(&env_or("SL_ASSET_TYPE", "sound"))))
        }
        Err(_unset) => None,
    };

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
    let (diag_tx, _diag_rx) = mpsc::channel(16);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

    let mut requested = false;
    while let Some(event) = event_rx.recv().await {
        match event {
            Event::RegionHandshakeComplete | Event::RegionChanged { .. } if !requested => {
                requested = true;
                info!("region active; advertising throttle then fetching assets");
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_1000()))
                    .await
                    .ok();
                // Texture: both the HTTP GetTexture cap and the legacy UDP path.
                command_tx
                    .send(Command::FetchTexture {
                        texture_id,
                        discard_level: 0,
                    })
                    .await
                    .ok();
                // Also fetch a coarse LOD: this exercises the HTTP `Range`
                // path, transferring only the level-of-detail prefix.
                command_tx
                    .send(Command::FetchTexture {
                        texture_id,
                        discard_level: 3,
                    })
                    .await
                    .ok();
                command_tx
                    .send(Command::RequestTexture {
                        texture_id,
                        discard_level: 0,
                        priority: 1.0e6,
                    })
                    .await
                    .ok();
                if let Some((asset_id, asset_type)) = asset {
                    command_tx
                        .send(Command::FetchAsset {
                            asset_id,
                            asset_type,
                            byte_range: None,
                        })
                        .await
                        .ok();
                    command_tx
                        .send(Command::RequestAsset {
                            asset_id,
                            asset_type,
                            priority: 1.0,
                        })
                        .await
                        .ok();
                }
                let command_tx = command_tx.clone();
                tokio::spawn(async move {
                    sleep(Duration::from_secs(hold_secs)).await;
                    command_tx.send(Command::Logout).await.ok();
                });
            }
            Event::TextureReceived(texture) => {
                info!(
                    "TextureReceived {} ({:?}, {} bytes)",
                    texture.id,
                    texture.codec,
                    texture.data.len()
                );
            }
            Event::TextureNotFound(id) => warn!("TextureNotFound {id}"),
            Event::AssetTransferStarted {
                asset_id,
                asset_type,
                size,
            } => {
                info!("AssetTransferStarted {asset_id} ({asset_type:?}, declared {size} bytes)");
            }
            Event::AssetReceived(asset) => {
                info!(
                    "AssetReceived {} ({:?}, {} bytes)",
                    asset.id,
                    asset.asset_type,
                    asset.data.len()
                );
            }
            Event::AssetTransferFailed {
                asset_id, status, ..
            } => warn!("AssetTransferFailed {asset_id}: {status:?}"),
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
