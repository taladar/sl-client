//! Logs in **twice** to demonstrate the inventory disk cache (INVENTORY B9/B10).
//!
//! The first ("cold") login enables the background inventory crawl and the disk
//! cache, fetches the agent's inventory tree, and writes a Firestorm-compatible
//! `<agent-uuid>.inv.llsd.gz` file under the cache directory on logout. The
//! second ("warm") login — same account, same cache directory — loads that file
//! **before** the login skeleton arrives and reconciles the two: every folder
//! whose version is unchanged keeps its cached contents and is *skipped* by the
//! background crawl.
//!
//! The observable proof is the number of `InventoryDescendents` replies seen per
//! login: many on the cold first login (the whole tree is fetched), few or none
//! on the warm second login (only changed/new folders are refetched). The two
//! counts are logged at the end of each session.
//!
//! Configure via environment variables:
//!   `SL_LOGIN_URI`  (default `http://127.0.0.1:9000/`)
//!   `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`  (required)
//!   `SL_START`      (default `last`)
//!   `SL_CHANNEL`    (default `sl-client-tokio-inventory-cache`)
//!   `SL_VERSION`    (default this crate's version)
//!   `SL_HOLD_SECS`  (default `15`; the background crawl runs during the hold)
//!   `SL_CACHE_DIR`  (default a per-process dir under the system temp dir)

use std::path::PathBuf;
use std::time::Duration;

use sl_client_tokio::{
    Client, ClientDirectories, Command, DisconnectReason, Error, Event, InventoryCacheConfig,
    LoginParams, LoginRequest, StartLocation, Throttle,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

/// The login parameters, rebuilt fresh for each of the two sessions (a
/// `LoginParams` is consumed by [`Client::connect`]).
fn login_params() -> Result<LoginParams, Box<dyn std::error::Error + Send + Sync>> {
    let login_uri: url::Url = env_or("SL_LOGIN_URI", "http://127.0.0.1:9000/").parse()?;
    let first = std::env::var("SL_FIRST")?;
    let last = std::env::var("SL_LAST")?;
    let password = std::env::var("SL_PASSWORD")?;
    let start = env_or("SL_START", "last").parse::<StartLocation>()?;
    let channel = env_or("SL_CHANNEL", "sl-client-tokio-inventory-cache");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));
    let request = LoginRequest::new(first, last, password, start, channel, version);
    Ok(LoginParams { login_uri, request })
}

/// Runs one cache-enabled session: logs in with the background crawl and the
/// disk cache pointed at `cache_dir`, holds for `hold_secs` (during which the
/// crawl fetches whatever the cache did not already cover), logs out (writing
/// the cache), and returns the number of `InventoryDescendents` replies observed
/// — the count of folders actually refetched this login. `label` tags the logs.
async fn run_session(
    cache_dir: PathBuf,
    hold_secs: u64,
    label: &str,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    info!("[{label}] logging in...");
    let params = login_params()?;
    let mut client = match Client::connect(params).await {
        Ok(client) => client,
        Err(Error::MfaChallenge(_)) => {
            return Err("this example does not support interactive MFA".into());
        }
        Err(other) => return Err(other.into()),
    };
    let agent_id = client.agent_id().ok_or("no agent id after login")?;
    info!("[{label}] login succeeded; agent {agent_id}");

    // Point the disk cache at `cache_dir`, enable it, and turn on the background
    // crawl so the whole tree fills in (cold) or only the changed folders do
    // (warm). These must be set before `run`.
    client.set_directories(ClientDirectories {
        agent_cache_dir: Some(cache_dir.clone()),
        ..ClientDirectories::default()
    });
    client.set_inventory_cache_config(InventoryCacheConfig {
        enabled: true,
        cache_library: true,
    });
    client.set_background_inventory_fetch(true);
    info!(
        "[{label}] inventory cache dir is {}; background crawl on",
        cache_dir.display()
    );

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
    let (command_tx, command_rx) = mpsc::channel::<Command>(16);
    let (diag_tx, _diag_rx) = mpsc::channel(16);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

    let mut started = false;
    let mut descendents_replies = 0usize;
    let mut fetched_folders = 0usize;
    let mut fetched_items = 0usize;

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::InventorySkeleton(folders) => {
                info!("[{label}] inventory skeleton: {} folders", folders.len());
            }
            Event::RegionHandshakeComplete | Event::RegionChanged { .. } if !started => {
                started = true;
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_1000()))
                    .await
                    .ok();
                // Let the background crawl run for the hold, then log out (which
                // writes the cache).
                let command_tx = command_tx.clone();
                tokio::spawn(async move {
                    sleep(Duration::from_secs(hold_secs)).await;
                    command_tx.send(Command::Logout).await.ok();
                });
            }
            Event::InventoryDescendents { folders, items, .. } => {
                descendents_replies = descendents_replies.saturating_add(1);
                fetched_folders = fetched_folders.saturating_add(folders.len());
                fetched_items = fetched_items.saturating_add(items.len());
            }
            Event::LoggedOut => {
                info!("[{label}] logged out cleanly; cache written");
                break;
            }
            Event::Disconnected(reason) => {
                match reason {
                    DisconnectReason::Timeout => {
                        warn!("[{label}] disconnected: inactivity timeout");
                    }
                    other => warn!("[{label}] disconnected: {other:?}"),
                }
                break;
            }
            _ => {}
        }
    }

    run.await??;
    info!(
        "[{label}] fetched {descendents_replies} folder-contents replies \
         ({fetched_folders} sub-folders, {fetched_items} items)"
    );
    Ok(descendents_replies)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    let hold_secs: u64 = env_or("SL_HOLD_SECS", "15").parse()?;
    let cache_dir = match std::env::var("SL_CACHE_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_ignored) => {
            std::env::temp_dir().join(format!("sl-client-inventory-cache-{}", std::process::id()))
        }
    };

    // Cold login: empty cache dir, so the whole tree is fetched and cached.
    let cold = run_session(cache_dir.clone(), hold_secs, "cold").await?;
    // Warm login: the cache written above is loaded; version-matching folders
    // are skipped, so far fewer (ideally zero) folders are refetched.
    let warm = run_session(cache_dir.clone(), hold_secs, "warm").await?;

    info!(
        "inventory cache result: cold login fetched {cold} folder-contents replies, \
         warm login fetched {warm} — the cache skipped {} version-matching folder(s)",
        cold.saturating_sub(warm)
    );
    if warm < cold {
        info!("the disk cache spared a refetch on the warm login (B9/B10 working)");
    } else {
        warn!(
            "the warm login fetched no fewer folders than the cold one; check that \
             {} is writable and that the grid serves the inventory fetch capability",
            cache_dir.display()
        );
    }
    Ok(())
}
