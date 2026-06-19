//! Logs in to a Second Life / OpenSim grid, advertises a bandwidth throttle so
//! the simulator streams terrain, and reports the decoded `LayerData` terrain
//! patches (ROADMAP #18): per-layer patch counts and the LAND height range.
//!
//! Configure via environment variables:
//!   `SL_LOGIN_URI`  (default `http://127.0.0.1:9000/`)
//!   `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`  (required)
//!   `SL_START`      (default `last`)
//!   `SL_CHANNEL`    (default `sl-client-tokio-terrain`)
//!   `SL_VERSION`    (default this crate's version)
//!   `SL_HOLD_SECS`  (default `20`)

use std::collections::BTreeSet;
use std::time::Duration;

use sl_client_tokio::{
    Client, Command, DisconnectReason, Error, Event, LoginParams, LoginRequest, Throttle,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

/// Running summary of the LAND terrain patches seen.
#[derive(Default)]
struct LandStats {
    /// The distinct LAND patch grid positions seen.
    patches: BTreeSet<(u32, u32)>,
    /// The lowest and highest decoded ground heights, in metres.
    range: Option<(f32, f32)>,
}

impl LandStats {
    /// Folds one LAND patch's heights into the running min/max and grid set.
    fn record(&mut self, patch_x: u32, patch_y: u32, heights: &[f32]) {
        self.patches.insert((patch_x, patch_y));
        for &height in heights {
            self.range = Some(match self.range {
                Some((lo, hi)) => (lo.min(height), hi.max(height)),
                None => (height, height),
            });
        }
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
    let channel = env_or("SL_CHANNEL", "sl-client-tokio-terrain");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));
    let hold_secs: u64 = env_or("SL_HOLD_SECS", "20").parse()?;

    info!("logging in...");
    let request = LoginRequest::new(first, last, password, start, channel, version);
    let params = LoginParams {
        login_uri: login_uri.clone(),
        request,
    };
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

    let mut land = LandStats::default();
    let mut other_layers = 0u32;

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::RegionHandshakeComplete | Event::RegionChanged { .. } => {
                info!("region active; advertising throttle and collecting terrain");
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_1000()))
                    .await
                    .ok();
                let command_tx = command_tx.clone();
                tokio::spawn(async move {
                    sleep(Duration::from_secs(hold_secs)).await;
                    command_tx.send(Command::Logout).await.ok();
                });
            }
            Event::TerrainPatch(patch) => {
                if patch.layer.is_land() {
                    land.record(patch.patch_x, patch.patch_y, &patch.values);
                } else {
                    other_layers = other_layers.saturating_add(1);
                }
            }
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

    match land.range {
        Some((lo, hi)) => info!(
            "LAND: {} patches decoded, height range {lo:.2}..{hi:.2} m; \
             {other_layers} non-land patches",
            land.patches.len(),
        ),
        None => warn!(
            "no LAND terrain decoded ({other_layers} non-land patches) — \
             is the throttle/region streaming terrain?"
        ),
    }

    run.await??;
    Ok(())
}
