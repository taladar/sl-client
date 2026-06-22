//! Live test for object interaction & editing (ROADMAP #17): logs in, rezzes a
//! cube at a distinctive position, identifies the new prim by its position when
//! its `ObjectUpdate` streams back, then renames it, moves it, reads its
//! properties, and finally deletes it before logging out.
//!
//! Run against the local OpenSim as a build-capable avatar (e.g. the estate
//! owner). Configure via the same environment variables as
//! `tokio_login_hold_logout` (`SL_LOGIN_URI`, `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`).

use std::time::Duration;

use sl_client_tokio::{
    Client, Command, DeRezDestination, DisconnectReason, Event, LoginParams, LoginRequest,
    ObjectTransform, PrimShape, RegionLocalObjectId, SaleType, Throttle, Uuid, Vector, pcode,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// The inventory folder type of the agent's Trash folder (`FT_TRASH`).
const TRASH_FOLDER_TYPE: i8 = 14;

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

/// The distinctive region-local position to rez the test prim at, used to
/// recognise it among the region's other objects.
const REZ_POSITION: Vector = Vector {
    x: 137.5,
    y: 42.25,
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
    let start = env_or("SL_START", "last").parse::<sl_client_tokio::StartLocation>()?;
    let channel = env_or("SL_CHANNEL", "sl-client-rez-edit");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));

    info!("logging in as {first} {last}...");
    let request = LoginRequest::new(first, last, password, start, channel, version);
    let client = Client::connect(LoginParams { login_uri, request }).await?;
    info!("login succeeded; running session");

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
    let (command_tx, command_rx) = mpsc::channel::<Command>(16);
    let (diag_tx, _diag_rx) = mpsc::channel(16);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

    // The local id of our freshly-rezzed prim, once we recognise it.
    let mut target: Option<RegionLocalObjectId> = None;
    // The agent's Trash folder id, learned from the login inventory skeleton.
    let mut trash_folder: Option<Uuid> = None;
    let mut deleted = false;

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::InventorySkeleton(folders) => {
                trash_folder = folders
                    .iter()
                    .find(|f| f.folder_type == TRASH_FOLDER_TYPE)
                    .map(|f| f.folder_id);
                info!("trash folder: {trash_folder:?}");
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
                if target.is_none()
                    && object.pcode == pcode::PRIMITIVE
                    && near_rez(&object.motion.position)
                {
                    let local_id = object.local_id;
                    target = Some(local_id);
                    info!(
                        "rezzed prim recognised: local id {local_id} at {:?}",
                        object.motion.position
                    );
                    // Read its properties, then rename / move / mark-for-sale it.
                    command_tx
                        .send(Command::RequestObjectProperties {
                            local_ids: vec![local_id],
                        })
                        .await
                        .ok();
                    command_tx
                        .send(Command::SetObjectName {
                            local_id,
                            name: "sl-client #17 test prim".to_owned(),
                        })
                        .await
                        .ok();
                    command_tx
                        .send(Command::SetObjectForSale {
                            local_id,
                            sale_type: SaleType::Copy,
                            sale_price: 10,
                        })
                        .await
                        .ok();
                    command_tx
                        .send(Command::UpdateObject {
                            local_id,
                            transform: ObjectTransform {
                                position: Some(Vector {
                                    x: REZ_POSITION.x + 5.0,
                                    y: REZ_POSITION.y,
                                    z: REZ_POSITION.z,
                                }),
                                ..ObjectTransform::default()
                            },
                        })
                        .await
                        .ok();
                    // Give the edits a moment to round-trip, then delete it to
                    // trash (the portable DeRezObject path; ObjectDelete is the
                    // god-only force-delete that OpenSim ignores).
                    let command_tx = command_tx.clone();
                    let trash = trash_folder.unwrap_or_else(Uuid::nil);
                    tokio::spawn(async move {
                        sleep(Duration::from_secs(4)).await;
                        info!("deleting the test prim {local_id} to trash {trash}");
                        command_tx
                            .send(Command::DerezObjects {
                                local_ids: vec![local_id],
                                destination: DeRezDestination::Trash,
                                destination_id: trash,
                                transaction_id: Uuid::from_u128(0x051C_17DE_1E7E),
                                group_id: Uuid::nil(),
                            })
                            .await
                            .ok();
                        sleep(Duration::from_secs(3)).await;
                        command_tx.send(Command::Logout).await.ok();
                    });
                }
            }
            Event::ObjectUpdated(object) => {
                if Some(object.local_id) == target {
                    info!(
                        "test prim updated: name={:?} pos={:?}",
                        object.properties.as_ref().map(|p| &p.name),
                        object.motion.position
                    );
                }
            }
            Event::ObjectProperties(props) => {
                info!(
                    "object properties: name={:?} desc={:?} sale_type={} price={}",
                    props.name, props.description, props.sale_type, props.sale_price
                );
                info!(
                    "  recovered: item_id={} folder_id={} from_task_id={} inv_serial={} \
                     aggregate_perms={:#x}/{:#x}/{:#x} texture_ids={:?}",
                    props.item_id,
                    props.folder_id,
                    props.from_task_id,
                    props.inventory_serial,
                    props.aggregate_perms,
                    props.aggregate_perm_textures,
                    props.aggregate_perm_textures_owner,
                    props.texture_ids
                );
            }
            Event::ObjectRemoved { local_id, .. } => {
                if Some(local_id) == target {
                    info!("test prim {local_id} removed — delete confirmed");
                    deleted = true;
                }
            }
            Event::LoggedOut => {
                info!("logged out cleanly (prim deleted: {deleted})");
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
