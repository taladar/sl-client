//! Live-test fixture for water-exclusion surfaces (`viewer-water-exclusion`):
//! logs in, rezzes a large cube straddling the region water height, textures every
//! face with the invisiprim-successor sentinel (`IMG_ALPHA_GRAD`) so the cube is a
//! water-exclusion surface, then logs out **leaving the cube rezzed** so the viewer
//! (run afterwards as the same avatar, once the presence clears) sees it punch a
//! hole in the sea.
//!
//! Run against the local OpenSim as a build-capable avatar. Configure via the same
//! environment variables as `rez_edit_object` (`SL_LOGIN_URI`, `SL_FIRST`,
//! `SL_LAST`, `SL_PASSWORD`, `SL_START`), plus optional placement overrides:
//! `SL_REZ_X` / `SL_REZ_Y` / `SL_REZ_Z` (region-local metres, default a spot near a
//! low region corner over water) and `SL_REZ_SIZE` (the cube edge in metres,
//! default 8). Pick a spot where the sea is visible (a low region corner, or the
//! void past the region edge); the cube must overlap the water height to show the
//! effect.

use std::time::Duration;

use sl_client_tokio::{
    Client, Command, DisconnectReason, Event, LoginParams, LoginRequest, PrimShape, TextureEntry,
    TextureFace, TextureKey, Throttle, Uuid, Vector, pcode,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// `IMG_ALPHA_GRAD` (`indra/llcommon/indra_constants.cpp`): the sentinel a
/// water-exclusion surface is textured with (the build tool's "Hide water" id).
const IMG_ALPHA_GRAD: Uuid = Uuid::from_u128(0xe97c_f410_8e61_7005_ec06_629e_ba4c_d1fb);

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

/// Reads a float environment variable or returns the given default.
fn env_f32(name: &str, default: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let login_uri: url::Url = env_or("SL_LOGIN_URI", "http://127.0.0.1:9000/").parse()?;
    let first = std::env::var("SL_FIRST")?;
    let last = std::env::var("SL_LAST")?;
    let password = std::env::var("SL_PASSWORD")?;
    let start = env_or("SL_START", "last").parse::<sl_client_tokio::StartLocation>()?;
    let channel = env_or("SL_CHANNEL", "sl-client-rez-water-exclusion");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));

    // The rez placement: a low region corner over water by default, overridable so
    // the cube can be relocated to wherever the sea is visible without recompiling.
    let rez = Vector {
        x: env_f32("SL_REZ_X", 24.0),
        y: env_f32("SL_REZ_Y", 24.0),
        z: env_f32("SL_REZ_Z", 20.0),
    };
    let size = env_f32("SL_REZ_SIZE", 8.0);

    info!("logging in as {first} {last}...");
    let request = LoginRequest::new(first, last, password, start, channel, version);
    let client = Client::connect(LoginParams { login_uri, request }).await?;
    info!("login succeeded; running session");

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
    let (command_tx, command_rx) = mpsc::channel::<Command>(16);
    let (diag_tx, _diag_rx) = mpsc::channel(16);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

    // Whether we have recognised (and textured) our freshly-rezzed cube yet.
    let mut done = false;

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::RegionHandshakeComplete => {
                info!(
                    "region handshake complete; rezzing a {size} m water-exclusion cube at {rez:?}"
                );
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_1000()))
                    .await
                    .ok();
                let mut shape = PrimShape::cube(rez.clone());
                shape.scale = Vector {
                    x: size,
                    y: size,
                    z: size,
                };
                command_tx
                    .send(Command::RezObject {
                        shape,
                        group_id: None,
                    })
                    .await
                    .ok();
            }
            Event::ObjectAdded(object) => {
                let position = &object.motion.position;
                let near = (position.x - rez.x).abs() < size
                    && (position.y - rez.y).abs() < size
                    && (position.z - rez.z).abs() < size;
                if done || object.pcode != pcode::PRIMITIVE || !near {
                    continue;
                }
                done = true;
                let local_id = object.scoped_id();
                info!("rezzed cube {local_id} at {position:?}; texturing it invisiprim");
                // A single face entry sets every face (the wire default applies one
                // face's value to all), making the whole cube a water-exclusion
                // surface.
                command_tx
                    .send(Command::SetObjectImage {
                        local_id,
                        media_url: None,
                        texture_entry: TextureEntry {
                            faces: vec![TextureFace::new(TextureKey::from(IMG_ALPHA_GRAD))],
                        },
                    })
                    .await
                    .ok();
                // Give the retexture a moment to persist, then log out leaving the
                // cube in-world for the viewer to inspect.
                let command_tx = command_tx.clone();
                tokio::spawn(async move {
                    sleep(Duration::from_secs(4)).await;
                    info!("cube textured; logging out (leaving it rezzed)");
                    command_tx.send(Command::Logout).await.ok();
                });
            }
            Event::LoggedOut => {
                info!("logged out cleanly; the water-exclusion cube remains in-world");
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
