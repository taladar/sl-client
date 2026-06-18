//! Live exercise for the experience capabilities (ROADMAP #27): logs in and,
//! once the region capabilities are available, queries the agent's experience
//! relationships — the experiences it has admitted/blocked (`GetExperiences`),
//! the ones it owns / administers / created (`AgentExperiences` /
//! `GetAdminExperiences` / `GetCreatorExperiences`), and the region's
//! experience allow/block/trust lists (`RegionExperiences`) — then fetches the
//! metadata of any it found (`GetExperienceInfo`), printing the results.
//!
//! Experiences are a Second Life feature: stock OpenSim ships **no** experience
//! module, so these capabilities are usually absent there — the commands then
//! no-op (the cap is not in the map) and only a clean login/logout is observed.
//! Run against a Second Life region (or an OpenSim grid with an experience
//! module) to see real data. To look up a specific experience by name, set
//! `SL_EXPERIENCE_QUERY`. Uses the same environment variables as
//! `login_hold_logout` (`SL_LOGIN_URI`, `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`).

use std::time::Duration;

use sl_client_tokio::{
    Client, Command, DisconnectReason, Event, LoginParams, LoginRequest, Throttle,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// How long to wait for the experience replies before logging out.
const SETTLE: Duration = Duration::from_secs(6);

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
    let start = env_or("SL_START", "last");
    let channel = env_or("SL_CHANNEL", "sl-client-experiences");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));
    let search_query = std::env::var("SL_EXPERIENCE_QUERY").ok();

    info!("logging in as {first} {last}...");
    let request = LoginRequest::new(first, last, password, start, channel, version);
    let client = Client::connect(LoginParams { login_uri, request }).await?;
    info!("login succeeded; running session");

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(512);
    let (command_tx, command_rx) = mpsc::channel::<Command>(32);
    let run = tokio::spawn(client.run(event_tx, command_rx));

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::RegionHandshakeComplete => {
                info!("region handshake complete; querying experiences");
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_300()))
                    .await
                    .ok();
                command_tx
                    .send(Command::RequestExperiencePermissions)
                    .await
                    .ok();
                command_tx.send(Command::RequestOwnedExperiences).await.ok();
                command_tx.send(Command::RequestAdminExperiences).await.ok();
                command_tx
                    .send(Command::RequestCreatorExperiences)
                    .await
                    .ok();
                command_tx
                    .send(Command::RequestRegionExperiences)
                    .await
                    .ok();
                if let Some(query) = search_query.clone() {
                    command_tx
                        .send(Command::FindExperiences { query, page: 0 })
                        .await
                        .ok();
                }
                let command_tx = command_tx.clone();
                tokio::spawn(async move {
                    sleep(SETTLE).await;
                    command_tx.send(Command::Logout).await.ok();
                });
            }
            // Each id-list reply: fetch the metadata of whatever was returned so
            // the names print, then report the ids.
            Event::ExperiencePermissions { allowed, blocked } => {
                info!(
                    "experience preferences: {} allowed, {} blocked",
                    allowed.len(),
                    blocked.len()
                );
                request_info(&command_tx, allowed).await;
                request_info(&command_tx, blocked).await;
            }
            Event::OwnedExperiences(ids) => {
                info!("owned experiences: {}", ids.len());
                request_info(&command_tx, ids).await;
            }
            Event::AdminExperiences(ids) => {
                info!("admin experiences: {}", ids.len());
                request_info(&command_tx, ids).await;
            }
            Event::CreatorExperiences(ids) => {
                info!("creator experiences: {}", ids.len());
                request_info(&command_tx, ids).await;
            }
            Event::RegionExperiences {
                allowed,
                blocked,
                trusted,
            } => {
                info!(
                    "region experiences: {} allowed, {} blocked, {} trusted",
                    allowed.len(),
                    blocked.len(),
                    trusted.len()
                );
                request_info(&command_tx, allowed).await;
                request_info(&command_tx, trusted).await;
            }
            Event::ExperienceInfo(infos) | Event::ExperienceSearchResults(infos) => {
                for info in infos {
                    if info.missing {
                        info!("  experience {} could not be resolved", info.public_id);
                    } else {
                        info!(
                            "  experience {}: {:?} (grid={}, maturity={})",
                            info.public_id,
                            info.name,
                            info.properties.is_grid(),
                            info.maturity,
                        );
                    }
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

    run.await??;
    Ok(())
}

/// Requests the metadata for `experience_ids` over `GetExperienceInfo` (a no-op
/// for an empty list).
async fn request_info(
    command_tx: &mpsc::Sender<Command>,
    experience_ids: Vec<sl_client_tokio::Uuid>,
) {
    if !experience_ids.is_empty() {
        command_tx
            .send(Command::RequestExperienceInfo { experience_ids })
            .await
            .ok();
    }
}
