//! Populate a region with one prim of each volume type for viewer rendering
//! tests (VIEWER_ROADMAP R4): logs in, rezzes a labelled row of a box, cylinder,
//! prism, sphere, torus, tube, and ring — each textured with the default
//! plywood texture at a 2×2 tiling — plus two extra boxes demonstrating the
//! per-face `TextureEntry` placement (a 45° texture rotation and a texture
//! offset). The prims persist in the region after logout, so a viewer can be
//! pointed at them and screenshotted.
//!
//! A 2×2 tiling is the clean visual signal for the texture-placement fix: with
//! the `TextureEntry` repeat applied each face shows the plywood grain four
//! times, and without it the grain is stretched once across the face.
//!
//! Run against the local OpenSim as a build-capable avatar (e.g. the estate
//! owner). Configure via the same environment variables as
//! `tokio_login_hold_logout` (`SL_LOGIN_URI`, `SL_FIRST`, `SL_LAST`,
//! `SL_PASSWORD`); `SL_START` picks the start location (default `last`).
//!
//! The prims are left in the world deliberately — delete them from a viewer, or
//! re-run after a region reset, when they are no longer needed.

use std::collections::HashSet;
use std::time::Duration;

use sl_client_tokio::{
    Client, Command, DisconnectReason, Event, LoginParams, LoginRequest, PrimShape, TextureEntry,
    TextureFace, TextureKey, Throttle, Uuid, Vector, pcode,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// The default OpenSim "plywood" diffuse texture — a fine wood grain whose
/// tiling is easy to read, present on every grid.
const PLYWOOD: Uuid = Uuid::from_u128(0x8955_6747_24cb_43ed_920b_47ca_ed15_465f);

/// The height (region-local Z) the row of test prims is rezzed at — a couple of
/// metres up so they clear the terrain and sit at eye level.
const ROW_Z: f32 = 26.0;

/// The region-local Y the row runs along.
const ROW_Y: f32 = 128.0;

/// The X of the first prim; successive prims step by [`STEP_X`].
const START_X: f32 = 120.0;

/// The spacing between adjacent prims in the row, in metres.
const STEP_X: f32 = 1.5;

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

/// How a rezzed prim's texture should be placed, exercised through
/// [`Command::SetObjectImage`].
#[derive(Clone, Copy)]
enum Placement {
    /// A uniform `n × n` tiling of the plywood texture.
    Tile(f32),
    /// A `1 ×` plywood rotated by `radians` about the face centre.
    Rotate(f32),
    /// A `1 ×` plywood slid by `(offset_s, offset_t)`.
    Offset(f32, f32),
}

/// One prim to rez: its label, its shape at the given position, and how its
/// plywood texture is placed.
struct Sample {
    /// A human-readable name for the volume type, logged as it is rezzed.
    label: &'static str,
    /// The prim's shape (path/profile/hole and rez position).
    shape: PrimShape,
    /// How the plywood texture is placed on the prim's faces.
    placement: Placement,
}

/// Mutates a [`PrimShape::cube`] into a given volume type by overriding its
/// path/profile bytes (and, for the swept-circle types, the hole size).
const fn shape_at(position: Vector, path_curve: u8, profile_curve: u8, hole: bool) -> PrimShape {
    let mut shape = PrimShape::cube(position);
    shape.path_curve = path_curve;
    shape.profile_curve = profile_curve;
    if hole {
        // path_scale_y 150 → (200 - 150) * 0.01 = 0.5 m top size, opening the
        // torus/tube/ring hole so the sweep reads as a ring rather than a blob.
        shape.path_scale_y = 150;
    }
    shape
}

/// The full row of samples: the seven volume types tiled 2×2, then two extra
/// boxes showing a rotated and an offset texture.
fn samples() -> Vec<Sample> {
    // (label, path_curve, profile_curve, hole, placement)
    let specs: [(&str, u8, u8, bool, Placement); 9] = [
        ("box", 0x10, 0x01, false, Placement::Tile(2.0)),
        ("cylinder", 0x10, 0x00, false, Placement::Tile(2.0)),
        ("prism", 0x10, 0x03, false, Placement::Tile(2.0)),
        ("sphere", 0x20, 0x05, false, Placement::Tile(2.0)),
        ("torus", 0x20, 0x00, true, Placement::Tile(2.0)),
        ("tube", 0x20, 0x01, true, Placement::Tile(2.0)),
        ("ring", 0x20, 0x03, true, Placement::Tile(2.0)),
        (
            "box-rotated",
            0x10,
            0x01,
            false,
            Placement::Rotate(core::f32::consts::FRAC_PI_4),
        ),
        (
            "box-offset",
            0x10,
            0x01,
            false,
            Placement::Offset(0.25, 0.0),
        ),
    ];
    specs
        .into_iter()
        .enumerate()
        .map(|(index, (label, path, profile, hole, placement))| {
            // The row index is < 16; widen through u16 (exact in f32) to avoid a
            // silent `as` cast.
            let step = u16::try_from(index).map_or(0.0, f32::from);
            let x = START_X + STEP_X * step;
            let position = Vector {
                x,
                y: ROW_Y,
                z: ROW_Z,
            };
            Sample {
                label,
                shape: shape_at(position, path, profile, hole),
                placement,
            }
        })
        .collect()
}

/// The plywood [`TextureEntry`] for a placement — a single face whose value the
/// wire encoding applies to every face of the prim as the default.
fn plywood_entry(placement: Placement) -> TextureEntry {
    let mut face = TextureFace::new(TextureKey::from(PLYWOOD));
    match placement {
        Placement::Tile(n) => {
            face.scale_s = n;
            face.scale_t = n;
        }
        Placement::Rotate(radians) => face.rotation = radians,
        Placement::Offset(s, t) => {
            face.offset_s = s;
            face.offset_t = t;
        }
    }
    TextureEntry { faces: vec![face] }
}

/// Whether `position` is within half a metre of `target` on each axis — the
/// test used to match a streamed `ObjectUpdate` back to the prim we rezzed.
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
    let channel = env_or("SL_CHANNEL", "sl-client-rez-sample-prims");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));

    info!("logging in as {first} {last}...");
    let request = LoginRequest::new(first, last, password, start, channel, version);
    let client = Client::connect(LoginParams { login_uri, request }).await?;
    info!("login succeeded; running session");

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
    let (command_tx, command_rx) = mpsc::channel::<Command>(64);
    let (diag_tx, _diag_rx) = mpsc::channel(16);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

    let samples = samples();
    // The sample indices already matched to a streamed prim and textured, so a
    // repeated object update never re-textures or double-counts.
    let mut textured: HashSet<usize> = HashSet::new();

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::RegionHandshakeComplete => {
                info!("region ready; rezzing {} sample prims", samples.len());
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_1000()))
                    .await
                    .ok();
                for sample in &samples {
                    command_tx
                        .send(Command::RezObject {
                            shape: sample.shape.clone(),
                            group_id: None,
                        })
                        .await
                        .ok();
                }
            }
            Event::ObjectAdded(object) => {
                // Match the streamed prim back to the sample it was rezzed from
                // by position, then texture it once.
                if object.pcode != pcode::PRIMITIVE {
                    continue;
                }
                let Some(index) = samples
                    .iter()
                    .position(|sample| near(&object.motion.position, &sample.shape.position))
                else {
                    continue;
                };
                let Some(sample) = samples.get(index) else {
                    continue;
                };
                if !textured.insert(index) {
                    continue;
                }
                info!(
                    "rezzed {} as local id {} — texturing",
                    sample.label,
                    object.scoped_id()
                );
                command_tx
                    .send(Command::SetObjectImage {
                        local_id: object.scoped_id(),
                        media_url: None,
                        texture_entry: plywood_entry(sample.placement),
                    })
                    .await
                    .ok();
                if textured.len() == samples.len() {
                    // Every prim is rezzed and textured; give the texture updates
                    // a moment to round-trip, then log out. The prims stay in the
                    // region for the viewer to screenshot.
                    let command_tx = command_tx.clone();
                    tokio::spawn(async move {
                        sleep(Duration::from_secs(5)).await;
                        info!("all sample prims rezzed and textured; logging out");
                        command_tx.send(Command::Logout).await.ok();
                    });
                }
            }
            Event::LoggedOut => {
                info!("logged out — {} prims left in the region", textured.len());
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
