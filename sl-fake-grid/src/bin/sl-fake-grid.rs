//! The standalone fake grid: a loopback login + CAPS + UDP simulator an
//! unmodified viewer (this workspace's or Firestorm) can log into.
//!
//! Point the viewer's grid manager / `SL_LOGIN_URI` at the printed login
//! URI (`http://127.0.0.1:<port>/`).

use clap::Parser;
use sl_fake_grid::{AccountConfig, FakeGridBuilder, RegionConfig};

/// Command-line options.
#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Options {
    /// The TCP port for the login + CAPS endpoint (0 = ephemeral).
    #[arg(long, default_value_t = 9100)]
    http_port: u16,

    /// An account as `First:Last:password` (repeatable).
    #[arg(long = "account", value_name = "FIRST:LAST:PASSWORD")]
    accounts: Vec<String>,

    /// The region name.
    #[arg(long, default_value = "Fake Region")]
    region: String,

    /// How long an empty EventQueueGet poll is held before the 502
    /// re-poll answer, in seconds.
    #[arg(long, default_value_t = 30)]
    hold_secs: u64,
}

/// Parses one `First:Last:password` account argument.
fn parse_account(raw: &str) -> Option<AccountConfig> {
    let mut parts = raw.splitn(3, ':');
    let first = parts.next()?;
    let last = parts.next()?;
    let password = parts.next()?;
    Some(AccountConfig::new(first, last, password))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let options = Options::parse();

    let mut builder = FakeGridBuilder::new()
        .http_port(options.http_port)
        .event_queue_hold(std::time::Duration::from_secs(options.hold_secs))
        .region(RegionConfig {
            name: options.region.clone(),
            ..RegionConfig::default()
        });
    let mut any_account = false;
    for raw in &options.accounts {
        match parse_account(raw) {
            Some(account) => {
                builder = builder.account(account);
                any_account = true;
            }
            None => {
                tracing::error!("unparsable --account {raw:?} (want First:Last:password)");
                return Err("bad --account argument".into());
            }
        }
    }
    if !any_account {
        // A usable default so `sl-fake-grid` alone is enough for a smoke test.
        builder = builder.account(AccountConfig::new("Test", "User", "password"));
        tracing::info!("no --account given; created Test User / password");
    }

    let grid = builder.start().await?;
    tracing::info!("fake grid ready: login URI {}", grid.login_uri());

    let mut logins = grid.logins();
    loop {
        tokio::select! {
            notice = logins.recv() => {
                match notice {
                    Ok(notice) => tracing::info!(
                        "avatar {} {} arrived in {}",
                        notice.first_name,
                        notice.last_name,
                        notice.region_name
                    ),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::warn!("missed {missed} login notices");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            result = tokio::signal::ctrl_c() => {
                if let Err(error) = result {
                    tracing::error!("waiting for ctrl-c failed: {error}");
                }
                tracing::info!("shutting down");
                grid.shutdown();
                break;
            }
        }
    }
    Ok(())
}
