//! Rez a small stand of Linden trees for viewer rendering tests
//! (VIEWER_ROADMAP P26.2): logs in, rezzes a labelled row of a few `LLVOTree`
//! species (each a `PCODE_NEW_TREE` object whose `state` byte selects the
//! species), and logs out leaving them in the region so a viewer can be pointed
//! at them and screenshotted.
//!
//! Trees are sized by the magnitude of their scale vector (the reference
//! viewer's `radius = scale.length() * 0.05`), so they are rezzed at a large
//! uniform scale to stand a few metres tall.
//!
//! Run against the local OpenSim as a build-capable avatar (e.g. the estate
//! owner). Configure via the same environment variables as `rez_sample_prims`
//! (`SL_LOGIN_URI`, `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`); `SL_START` picks the
//! start location (default `last`).
//!
//! The trees are left in the world deliberately — delete them from a viewer when
//! they are no longer needed.

use std::collections::HashSet;
use std::time::Duration;

use sl_client_tokio::{
    Client, Command, DisconnectReason, Event, LoginParams, LoginRequest, PrimShape, Throttle,
    Vector, pcode,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// The region-local Y the row of trees runs along.
const ROW_Y: f32 = 112.0;

/// The region-local Z (ground-ish) the trees are planted at.
const ROW_Z: f32 = 22.0;

/// The X of the first tree; successive trees step by [`STEP_X`].
const START_X: f32 = 116.0;

/// The spacing between adjacent trees, in metres.
const STEP_X: f32 = 6.0;

/// The uniform scale each tree is rezzed at (its vector length drives the tree
/// size). OpenSim's vegetation module multiplies a rezzed tree's scale by ~8
/// (`VegetationModule.AdaptTree`), so a small rez scale still yields a
/// several-metre tree.
const TREE_SCALE: f32 = 0.5;

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

/// One tree to rez: its species byte and human-readable name.
struct Tree {
    /// The `LLVOTree` species byte (the object `state`).
    species: u8,
    /// A human-readable species name, logged as it is rezzed.
    label: &'static str,
}

/// The stand of trees: a spread of species (a couple of pines, an oak, a palm,
/// a cypress, and a eucalyptus).
fn trees() -> Vec<Tree> {
    vec![
        Tree {
            species: 0,
            label: "Pine 1",
        },
        Tree {
            species: 1,
            label: "Oak",
        },
        Tree {
            species: 3,
            label: "Palm 1",
        },
        Tree {
            species: 7,
            label: "Cypress 1",
        },
        Tree {
            species: 14,
            label: "Eucalyptus",
        },
    ]
}

/// A tree's rez shape at `position`: a [`PrimShape::cube`] retyped to a
/// `PCODE_NEW_TREE` carrying the species in its `state` byte, at [`TREE_SCALE`].
const fn tree_shape(species: u8, position: Vector) -> PrimShape {
    let mut shape = PrimShape::cube(position);
    shape.pcode = pcode::NEW_TREE;
    shape.state = species;
    shape.scale = Vector {
        x: TREE_SCALE,
        y: TREE_SCALE,
        z: TREE_SCALE,
    };
    shape
}

/// Whether `position` is within half a metre of `target` on each axis — used to
/// match a streamed `ObjectUpdate` back to the tree we rezzed.
fn near(position: &Vector, target: &Vector) -> bool {
    (position.x - target.x).abs() < 0.5
        && (position.y - target.y).abs() < 0.5
        && (position.z - target.z).abs() < 0.5
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let login_uri: url::Url = env_or("SL_LOGIN_URI", "http://127.0.0.1:9000/").parse()?;
    let first = std::env::var("SL_FIRST")?;
    let last = std::env::var("SL_LAST")?;
    let password = std::env::var("SL_PASSWORD")?;
    let start = env_or("SL_START", "last").parse::<sl_client_tokio::StartLocation>()?;
    let channel = env_or("SL_CHANNEL", "sl-client-rez-sample-trees");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));

    info!("logging in as {first} {last}...");
    let request = LoginRequest::new(first, last, password, start, channel, version);
    let client = Client::connect(LoginParams { login_uri, request }).await?;
    info!("login succeeded; running session");

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
    let (command_tx, command_rx) = mpsc::channel::<Command>(64);
    let (diag_tx, _diag_rx) = mpsc::channel(16);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

    let trees = trees();
    let positions: Vec<Vector> = (0..trees.len())
        .map(|index| {
            let step = u16::try_from(index).map_or(0.0, f32::from);
            Vector {
                x: START_X + STEP_X * step,
                y: ROW_Y,
                z: ROW_Z,
            }
        })
        .collect();
    let mut seen: HashSet<usize> = HashSet::new();

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::RegionHandshakeComplete => {
                info!("region ready; rezzing {} trees", trees.len());
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_1000()))
                    .await
                    .ok();
                for (tree, position) in trees.iter().zip(&positions) {
                    info!("rezzing {} (species {})", tree.label, tree.species);
                    command_tx
                        .send(Command::RezObject {
                            shape: tree_shape(tree.species, position.clone()),
                            group_id: None,
                        })
                        .await
                        .ok();
                }
            }
            Event::ObjectAdded(object) => {
                if object.pcode != pcode::NEW_TREE && object.pcode != pcode::TREE {
                    continue;
                }
                let Some(index) = positions
                    .iter()
                    .position(|target| near(&object.motion.position, target))
                else {
                    continue;
                };
                if !seen.insert(index) {
                    continue;
                }
                let label = trees.get(index).map_or("?", |tree| tree.label);
                info!(
                    "rezzed tree {label} (species {}) as local id {}",
                    object.state,
                    object.scoped_id()
                );
                if seen.len() == trees.len() {
                    let command_tx = command_tx.clone();
                    tokio::spawn(async move {
                        sleep(Duration::from_secs(5)).await;
                        info!("all trees rezzed; logging out");
                        command_tx.send(Command::Logout).await.ok();
                    });
                }
            }
            Event::LoggedOut => {
                info!("logged out — {} trees left in the region", seen.len());
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
