//! Logs in to a Second Life / OpenSim grid, holds the session alive past the
//! server inactivity timeout, then logs out cleanly.
//!
//! Configure via environment variables:
//!   `SL_LOGIN_URI`  (default `http://127.0.0.1:9000/`)
//!   `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`  (required)
//!   `SL_START`      (default `last`)
//!   `SL_CHANNEL`    (default `sl-client-tokio-example`)
//!   `SL_VERSION`    (default this crate's version)
//!   `SL_HOLD_SECS`  (default `90`)

use std::time::Duration;

use sl_client_tokio::{Client, Command, DisconnectReason, Error, Event, LoginParams, LoginRequest};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

/// Prompts on the terminal for a multi-factor one-time code.
async fn prompt_mfa_code() -> Result<String, Box<dyn std::error::Error>> {
    info!("enter your multi-factor one-time code:");
    let line = tokio::task::spawn_blocking(|| {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        Ok::<String, std::io::Error>(input.trim().to_owned())
    })
    .await??;
    Ok(line)
}

/// Connects, performing the interactive MFA retry if the grid challenges.
async fn connect_with_mfa(
    login_uri: &str,
    mut request: LoginRequest,
) -> Result<Client, Box<dyn std::error::Error>> {
    loop {
        let params = LoginParams {
            login_uri: login_uri.to_owned(),
            request: request.clone(),
        };
        match Client::connect(params).await {
            Ok(client) => return Ok(client),
            Err(Error::MfaChallenge(challenge)) => {
                info!(
                    "multi-factor authentication required: {}",
                    challenge.message
                );
                let code = prompt_mfa_code().await?;
                request = request.with_mfa(code, challenge.mfa_hash);
            }
            Err(other) => return Err(other.into()),
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
    let channel = env_or("SL_CHANNEL", "sl-client-tokio-example");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));
    let hold_secs: u64 = env_or("SL_HOLD_SECS", "90").parse()?;

    info!("logging in...");
    let request = LoginRequest::new(first, last, password, start, channel, version);
    let client = connect_with_mfa(&login_uri, request).await?;
    info!("login succeeded; running session");

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(64);
    let (command_tx, command_rx) = mpsc::channel::<Command>(8);
    let run = tokio::spawn(client.run(event_tx, command_rx));

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::CircuitEstablished { sim } => info!("circuit established to {sim}"),
            Event::RegionHandshakeComplete => {
                info!("region handshake complete; holding for {hold_secs}s");
                let command_tx = command_tx.clone();
                tokio::spawn(async move {
                    sleep(Duration::from_secs(hold_secs)).await;
                    info!("hold elapsed; requesting logout");
                    command_tx.send(Command::Logout).await.ok();
                });
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
        }
    }

    run.await??;
    Ok(())
}
