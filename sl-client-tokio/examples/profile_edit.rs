//! Logs in to a Second Life / OpenSim grid and exercises the profile &
//! pick/classified **editing** surface (ROADMAP #29): it reads the agent's own
//! profile, picks and classifieds, fetches one pick's and one classified's full
//! details, then (unless `SL_READONLY=1`) edits the profile's about text and
//! runs a create → read-detail → delete cycle for both a pick and a classified.
//!
//! Needs the grid's profile module enabled (on OpenSim: `[UserProfiles]
//! ProfileServiceURL`); otherwise no replies are sent.
//!
//! Configure via environment variables:
//!   `SL_LOGIN_URI`  (default `http://127.0.0.1:9000/`)
//!   `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`  (required)
//!   `SL_START`      (default `last`)
//!   `SL_CHANNEL`    (default `sl-client-tokio-profile`)
//!   `SL_VERSION`    (default this crate's version)
//!   `SL_HOLD_SECS`  (default `25`)
//!   `SL_TARGET`     (optional avatar UUID to inspect; default the agent itself)
//!   `SL_READONLY`   (set to `1` to skip the profile/pick edits)

use std::time::Duration;

use sl_client_tokio::{
    AgentKey, ClassifiedCategory, ClassifiedUpdate, Client, Command, DisconnectReason, Error,
    Event, LindenAmount, LoginParams, LoginRequest, PickKey, PickUpdate, ProfileUpdate, Throttle,
    Uuid,
};
use sl_proto::ClassifiedKey;
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
    let channel = env_or("SL_CHANNEL", "sl-client-tokio-profile");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));
    let hold_secs: u64 = env_or("SL_HOLD_SECS", "25").parse()?;
    let readonly = env_or("SL_READONLY", "0") == "1";
    let target_override: Option<Uuid> = match std::env::var("SL_TARGET") {
        Ok(value) => Some(
            value
                .parse()
                .map_err(|_ignored| "SL_TARGET is not a valid UUID")?,
        ),
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
    let agent_id = client.agent_id().ok_or("no agent id after login")?;
    let target = target_override.unwrap_or_else(|| agent_id.uuid());
    info!("login succeeded; agent {agent_id}, inspecting {target}");

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
    let (command_tx, command_rx) = mpsc::channel::<Command>(16);
    let (diag_tx, _diag_rx) = mpsc::channel(16);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

    let mut requested = false;
    while let Some(event) = event_rx.recv().await {
        match event {
            Event::RegionHandshakeComplete | Event::RegionChanged { .. } if !requested => {
                requested = true;
                info!("region active; reading profile, picks and classifieds");
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_1000()))
                    .await
                    .ok();
                command_tx
                    .send(Command::RequestAvatarProperties(target))
                    .await
                    .ok();
                command_tx
                    .send(Command::RequestAvatarPicks(target))
                    .await
                    .ok();
                command_tx
                    .send(Command::RequestAvatarClassifieds(target))
                    .await
                    .ok();

                if !readonly && target == agent_id.uuid() {
                    // Edit the agent's own about text, then create a pick, read
                    // its details back, and remove it.
                    command_tx
                        .send(Command::UpdateProfile(ProfileUpdate {
                            about_text: "Edited by sl-client #29".to_owned(),
                            allow_publish: true,
                            ..ProfileUpdate::default()
                        }))
                        .await
                        .ok();
                    // A fixed id for this throwaway test pick (a real client
                    // would generate a fresh random UUID per new pick).
                    let pick_id =
                        PickKey::from(Uuid::from_u128(0x5C11_0029_0000_0000_0000_0000_0000_0001));
                    command_tx
                        .send(Command::UpdatePick(PickUpdate {
                            pick_id,
                            name: "sl-client test pick".to_owned(),
                            description: "created then deleted by #29".to_owned(),
                            ..PickUpdate::default()
                        }))
                        .await
                        .ok();
                    command_tx
                        .send(Command::RequestPickInfo {
                            creator_id: agent_id,
                            pick_id,
                        })
                        .await
                        .ok();

                    // The same create → read-detail → delete cycle for a
                    // classified ad.
                    let classified_id = ClassifiedKey::from(Uuid::from_u128(
                        0x5C11_0029_0000_0000_0000_0000_0000_0002,
                    ));
                    command_tx
                        .send(Command::UpdateClassified(ClassifiedUpdate {
                            classified_id,
                            category: ClassifiedCategory::Shopping,
                            name: "sl-client test classified".to_owned(),
                            description: "created then deleted by #29".to_owned(),
                            price_for_listing: LindenAmount(50),
                            ..ClassifiedUpdate::default()
                        }))
                        .await
                        .ok();
                    command_tx
                        .send(Command::RequestClassifiedInfo(classified_id))
                        .await
                        .ok();

                    // Re-read the profile to confirm the edit took.
                    command_tx
                        .send(Command::RequestAvatarProperties(target))
                        .await
                        .ok();
                    command_tx.send(Command::DeletePick(pick_id)).await.ok();
                    command_tx
                        .send(Command::DeleteClassified(classified_id))
                        .await
                        .ok();
                }

                let command_tx = command_tx.clone();
                tokio::spawn(async move {
                    sleep(Duration::from_secs(hold_secs)).await;
                    command_tx.send(Command::Logout).await.ok();
                });
            }
            Event::AvatarProperties(props) => {
                info!(
                    "profile of {}: born {}, about \"{}\"",
                    props.avatar_id, props.born_on, props.about_text
                );
            }
            Event::AvatarPicks { picks, .. } => {
                info!("{} pick(s):", picks.len());
                for pick in &picks {
                    info!("  pick {} \"{}\"", pick.pick_id, pick.name);
                    // Fetch each pick's full details.
                    command_tx
                        .send(Command::RequestPickInfo {
                            creator_id: AgentKey::from(target),
                            pick_id: pick.pick_id,
                        })
                        .await
                        .ok();
                }
            }
            Event::AvatarClassifieds { classifieds, .. } => {
                info!("{} classified(s):", classifieds.len());
                for classified in &classifieds {
                    info!(
                        "  classified {} \"{}\"",
                        classified.classified_id, classified.name
                    );
                    command_tx
                        .send(Command::RequestClassifiedInfo(classified.classified_id))
                        .await
                        .ok();
                }
            }
            Event::PickInfo(pick) => {
                info!(
                    "pick details: \"{}\" — {} (parcel {}, {:?})",
                    pick.name, pick.description, pick.parcel_id, pick.sim_name
                );
            }
            Event::ClassifiedInfo(classified) => {
                info!(
                    "classified details: \"{}\" — {} (L${}, category {})",
                    classified.name,
                    classified.description,
                    classified.price_for_listing,
                    classified.category.to_u32()
                );
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
