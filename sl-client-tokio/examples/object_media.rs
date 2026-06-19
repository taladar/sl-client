//! Live test for media-on-a-prim (ROADMAP #24): logs in, rezzes a cube, sets
//! per-face media on it over the `ObjectMedia` UPDATE capability, then fetches
//! it back with an `ObjectMedia` GET and prints the decoded per-face media —
//! exercising the full set → fetch → decode round-trip. Cleans up the prim and
//! logs out.
//!
//! Run against the local OpenSim (whose `MoapModule` serves the `ObjectMedia`
//! and `ObjectMediaNavigate` capabilities) as a build-capable avatar (e.g. the
//! estate owner). Configure via the same environment variables as
//! `tokio_login_hold_logout` (`SL_LOGIN_URI`, `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`).

use std::time::Duration;

use sl_client_tokio::{
    Client, Command, DeRezDestination, DisconnectReason, Event, LoginParams, LoginRequest,
    MediaEntry, PrimShape, Throttle, Uuid, Vector, pcode,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// The inventory folder type of the agent's Trash folder (`FT_TRASH`).
const TRASH_FOLDER_TYPE: i8 = 14;

/// The number of faces on a cube; the media array carries one slot per face.
const CUBE_FACES: usize = 6;

/// The media URL we set on face 0 and expect to read back.
const MEDIA_URL: &str = "https://example.com/sl-client-24-stream";

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

/// The region-local position to rez the test prim at, used to recognise it
/// among the region's other objects.
const REZ_POSITION: Vector = Vector {
    x: 141.0,
    y: 48.5,
    z: 41.0,
};

/// Whether `position` is within half a metre of [`REZ_POSITION`] on each axis.
fn near_rez(position: &Vector) -> bool {
    (position.x - REZ_POSITION.x).abs() < 0.5
        && (position.y - REZ_POSITION.y).abs() < 0.5
        && (position.z - REZ_POSITION.z).abs() < 0.5
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let login_uri = env_or("SL_LOGIN_URI", "http://127.0.0.1:9000/");
    let first = std::env::var("SL_FIRST")?;
    let last = std::env::var("SL_LAST")?;
    let password = std::env::var("SL_PASSWORD")?;
    let start = env_or("SL_START", "last");
    let channel = env_or("SL_CHANNEL", "sl-client-object-media");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));

    info!("logging in as {first} {last}...");
    let request = LoginRequest::new(first, last, password, start, channel, version);
    let client = Client::connect(LoginParams { login_uri, request }).await?;
    info!("login succeeded; running session");

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
    let (command_tx, command_rx) = mpsc::channel::<Command>(16);
    let (diag_tx, _diag_rx) = mpsc::channel(16);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

    // The local id + full id of our freshly-rezzed prim, once we recognise it.
    let mut target_local: Option<u32> = None;
    let mut target_full: Option<Uuid> = None;
    let mut trash_folder: Option<Uuid> = None;
    let mut saw_media = false;

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::InventorySkeleton(folders) => {
                trash_folder = folders
                    .iter()
                    .find(|f| f.folder_type == TRASH_FOLDER_TYPE)
                    .map(|f| f.folder_id);
            }
            Event::RegionHandshakeComplete => {
                info!("region handshake complete; opening throttle and rezzing a cube");
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_1000()))
                    .await
                    .ok();
                command_tx
                    .send(Command::RezObject {
                        shape: PrimShape::cube(REZ_POSITION),
                        group_id: Uuid::nil(),
                    })
                    .await
                    .ok();
            }
            Event::ObjectAdded(object) => {
                if target_local.is_none()
                    && object.pcode == pcode::PRIMITIVE
                    && near_rez(&object.motion.position)
                {
                    target_local = Some(object.local_id);
                    target_full = Some(object.full_id);
                    info!(
                        "rezzed prim recognised: local id {} full id {}",
                        object.local_id, object.full_id
                    );

                    // Put media on face 0 (no media on the other faces), then —
                    // after a moment for the UPDATE to apply — fetch it back.
                    let mut faces: Vec<Option<MediaEntry>> = vec![None; CUBE_FACES];
                    if let Some(slot) = faces.first_mut() {
                        *slot = Some(MediaEntry {
                            current_url: MEDIA_URL.to_owned(),
                            home_url: MEDIA_URL.to_owned(),
                            auto_play: true,
                            auto_scale: true,
                            width_pixels: 1024,
                            height_pixels: 512,
                            ..MediaEntry::default()
                        });
                    }
                    let object_id = object.full_id;
                    command_tx
                        .send(Command::SetObjectMedia { object_id, faces })
                        .await
                        .ok();

                    let command_tx = command_tx.clone();
                    let trash = trash_folder.unwrap_or_else(Uuid::nil);
                    tokio::spawn(async move {
                        sleep(Duration::from_secs(2)).await;
                        info!("fetching object media for {object_id}");
                        command_tx
                            .send(Command::RequestObjectMedia { object_id })
                            .await
                            .ok();
                        sleep(Duration::from_secs(3)).await;
                        if let Some(local_id) = target_local {
                            info!("deleting the test prim {local_id} to trash {trash}");
                            command_tx
                                .send(Command::DerezObjects {
                                    local_ids: vec![local_id],
                                    destination: DeRezDestination::Trash,
                                    destination_id: trash,
                                    transaction_id: Uuid::from_u128(0x0024_3DE7),
                                    group_id: Uuid::nil(),
                                })
                                .await
                                .ok();
                        }
                        sleep(Duration::from_secs(2)).await;
                        command_tx.send(Command::Logout).await.ok();
                    });
                }
            }
            Event::ObjectMedia {
                object_id,
                version,
                faces,
            } => {
                if Some(object_id) == target_full {
                    saw_media = true;
                    let with_media = faces.iter().filter(|f| f.is_some()).count();
                    info!(
                        "object media for {object_id} (version {version}): {} faces, {with_media} with media",
                        faces.len()
                    );
                    if let Some(Some(face0)) = faces.first() {
                        info!(
                            "face 0 media: current_url={:?} auto_play={} {}x{}",
                            face0.current_url,
                            face0.auto_play,
                            face0.width_pixels,
                            face0.height_pixels
                        );
                    }
                }
            }
            Event::LoggedOut => {
                info!("logged out cleanly (object media observed: {saw_media})");
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
