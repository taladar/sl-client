//! Rez a small stand of Linden grass for viewer rendering tests
//! (VIEWER_ROADMAP P26.3): logs in, rezzes a labelled row of a few `LLVOGrass`
//! species (each a `PCODE_GRASS` object whose `state` byte selects the species),
//! and logs out leaving them in the region so a viewer can be pointed at them and
//! screenshotted.
//!
//! A grass clump spreads its blades over an area set by the object's X/Y scale
//! (the reference viewer's `x = exp_x * mScale`), so the patches are rezzed at a
//! few metres' scale to cover ground rather than sit as a single tuft.
//!
//! Run against the local OpenSim as a build-capable avatar (e.g. the estate
//! owner). Configure via the same environment variables as `rez_sample_trees`
//! (`SL_LOGIN_URI`, `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`); `SL_START` picks the
//! start location (default `last`).
//!
//! The grass is left in the world deliberately — delete it from a viewer when it
//! is no longer needed.

use std::collections::HashSet;
use std::time::Duration;

use sl_client_tokio::{
    Client, Command, DisconnectReason, Event, LoginParams, LoginRequest, PrimShape, Throttle,
    Vector, pcode,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// The region-local Y the row of grass runs along (a couple of metres off the
/// tree row rezzed by `rez_sample_trees`).
const ROW_Y: f32 = 106.0;

/// The region-local Z (ground-ish) the grass is planted at.
const ROW_Z: f32 = 22.0;

/// The X of the first clump; successive clumps step by [`STEP_X`].
const START_X: f32 = 116.0;

/// The spacing between adjacent clumps, in metres.
const STEP_X: f32 = 4.0;

/// The uniform scale each grass clump is rezzed at (its X/Y spread the blades over
/// an area). A few metres so the clump covers ground rather than sitting as a
/// single tuft.
const GRASS_SCALE: f32 = 4.0;

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

/// One grass clump to rez: its species byte and human-readable name.
struct Grass {
    /// The `LLVOGrass` species byte (the object `state`).
    species: u8,
    /// A human-readable species name, logged as it is rezzed.
    label: &'static str,
}

/// The stand of grass: one clump of each defined species (grass.xml has 6).
fn grasses() -> Vec<Grass> {
    vec![
        Grass {
            species: 0,
            label: "Grass 0",
        },
        Grass {
            species: 1,
            label: "Grass 1",
        },
        Grass {
            species: 2,
            label: "Grass 2",
        },
        Grass {
            species: 3,
            label: "Grass 3",
        },
        Grass {
            species: 4,
            label: "Grass 4",
        },
        Grass {
            species: 5,
            label: "undergrowth_1",
        },
    ]
}

/// A grass clump's rez shape at `position`: a [`PrimShape::cube`] retyped to a
/// `PCODE_GRASS` carrying the species in its `state` byte, at [`GRASS_SCALE`].
const fn grass_shape(species: u8, position: Vector) -> PrimShape {
    let mut shape = PrimShape::cube(position);
    shape.pcode = pcode::GRASS;
    shape.state = species;
    shape.scale = Vector {
        x: GRASS_SCALE,
        y: GRASS_SCALE,
        z: GRASS_SCALE,
    };
    shape
}

/// Whether `position` is within half a metre of `target` on each axis — used to
/// match a streamed `ObjectUpdate` back to the clump we rezzed.
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
    let channel = env_or("SL_CHANNEL", "sl-client-rez-sample-grass");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));

    info!("logging in as {first} {last}...");
    let request = LoginRequest::new(first, last, password, start, channel, version);
    let client = Client::connect(LoginParams { login_uri, request }).await?;
    info!("login succeeded; running session");

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
    let (command_tx, command_rx) = mpsc::channel::<Command>(64);
    let (diag_tx, _diag_rx) = mpsc::channel(16);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

    let grasses = grasses();
    let positions: Vec<Vector> = (0..grasses.len())
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
                info!("region ready; rezzing {} grass clumps", grasses.len());
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_1000()))
                    .await
                    .ok();
                for (grass, position) in grasses.iter().zip(&positions) {
                    info!("rezzing {} (species {})", grass.label, grass.species);
                    command_tx
                        .send(Command::RezObject {
                            shape: grass_shape(grass.species, position.clone()),
                            group_id: None,
                        })
                        .await
                        .ok();
                }
            }
            Event::ObjectAdded(object) => {
                if object.pcode != pcode::GRASS {
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
                let label = grasses.get(index).map_or("?", |grass| grass.label);
                info!(
                    "rezzed grass {label} (species {}) as local id {}",
                    object.state,
                    object.scoped_id()
                );
                if seen.len() == grasses.len() {
                    let command_tx = command_tx.clone();
                    tokio::spawn(async move {
                        sleep(Duration::from_secs(5)).await;
                        info!("all grass rezzed; logging out");
                        command_tx.send(Command::Logout).await.ok();
                    });
                }
            }
            Event::LoggedOut => {
                info!(
                    "logged out — {} grass clumps left in the region",
                    seen.len()
                );
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
