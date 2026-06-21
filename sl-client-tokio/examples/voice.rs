//! Live exercise for voice-chat *signalling* (ROADMAP #26): logs in and, once
//! the region capabilities are available, requests the agent's voice account
//! (`ProvisionVoiceAccountRequest`) and the current parcel's voice channel
//! (`ParcelVoiceInfoRequest`), printing whatever the grid returns.
//!
//! This client implements only the grid-side **signalling** — it never opens a
//! Vivox SIP/RTP session or a WebRTC peer connection (the audio transport is out
//! of scope). The example therefore asks for the legacy Vivox account by
//! default; a WebRTC provision would additionally need a JSEP offer SDP from a
//! real WebRTC engine, which this client does not embed.
//!
//! Stock OpenSim ships **without** a voice module, so the
//! `ProvisionVoiceAccountRequest` / `ParcelVoiceInfoRequest` capabilities are
//! usually absent there — the commands then no-op (the cap is not in the map)
//! and only a clean login/logout is observed. Configure a FreeSWITCH/Vivox voice
//! module (or run against a Second Life region) to see real credentials. Uses
//! the same environment variables as `tokio_login_hold_logout` (`SL_LOGIN_URI`,
//! `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`).

use std::time::Duration;

use sl_client_tokio::{
    Client, Command, DisconnectReason, Event, LoginParams, LoginRequest, Throttle,
    VoiceProvisionRequest,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// How long to wait for the voice replies before logging out.
const VOICE_SETTLE: Duration = Duration::from_secs(6);

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
    let channel = env_or("SL_CHANNEL", "sl-client-voice");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));

    info!("logging in as {first} {last}...");
    let request = LoginRequest::new(first, last, password, start, channel, version);
    let client = Client::connect(LoginParams { login_uri, request }).await?;
    info!("login succeeded; running session");

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(512);
    let (command_tx, command_rx) = mpsc::channel::<Command>(16);
    let (diag_tx, _diag_rx) = mpsc::channel(16);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::RegionHandshakeComplete => {
                info!("region handshake complete; requesting voice account + parcel voice");
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_300()))
                    .await
                    .ok();
                command_tx
                    .send(Command::RequestVoiceAccount {
                        request: VoiceProvisionRequest::vivox(),
                    })
                    .await
                    .ok();
                command_tx.send(Command::RequestParcelVoiceInfo).await.ok();
                let command_tx = command_tx.clone();
                tokio::spawn(async move {
                    sleep(VOICE_SETTLE).await;
                    command_tx.send(Command::Logout).await.ok();
                });
            }
            Event::VoiceAccountProvisioned(info) => {
                if info.is_webrtc() {
                    info!(
                        "voice account (WebRTC): viewer_session={:?} jsep_type={:?} sdp_len={}",
                        info.viewer_session,
                        info.jsep_type,
                        info.jsep_sdp.as_deref().map_or(0, str::len),
                    );
                } else {
                    info!(
                        "voice account (Vivox): username={:?} sip_hostname={:?} server={:?}",
                        info.username, info.sip_uri_hostname, info.account_server_name,
                    );
                }
            }
            Event::ParcelVoiceInfo(info) => {
                info!(
                    "parcel voice: parcel={} region={:?} channel_uri={:?}",
                    info.parcel_local_id, info.region_name, info.channel_uri,
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
