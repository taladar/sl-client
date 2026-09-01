//! The standalone fake grid: a loopback login + CAPS + UDP simulator an
//! unmodified viewer (this workspace's or Firestorm) can log into.
//!
//! Point the viewer's grid manager / `SL_LOGIN_URI` at the printed login
//! URI (`http://127.0.0.1:<port>/`). The same host also serves
//! `get_grid_info`, the world-map tiles, and the economy helper scripts.

use clap::Parser;
use sl_fake_grid::{AccountConfig, FakeGridBuilder, GridIdentity, RegionConfig, catalogue};

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

    /// A region as `Name` or `Name@X,Y` (grid coordinates; repeatable — the
    /// first is the start region, and a viewer can teleport between them
    /// from the map). Default: one "Fake Region" at 1000,1000; unplaced
    /// regions are laid out eastwards from there.
    #[arg(long = "region", value_name = "NAME[@X,Y]")]
    regions: Vec<String>,

    /// The grid name reported by `get_grid_info` (`gridname`).
    #[arg(long, default_value = "Fake Grid")]
    grid_name: String,

    /// The grid nickname reported by `get_grid_info` (`gridnick`).
    #[arg(long, default_value = "fakegrid")]
    grid_nick: String,

    /// How long an empty EventQueueGet poll is held before the 502
    /// re-poll answer, in seconds.
    #[arg(long, default_value_t = 30)]
    hold_secs: u64,

    /// Rez the named prim catalogue in every region instead of the stock
    /// content: one prim per rendering feature (textured, sphere-shaped,
    /// per-face styled, mesh, sculpt, PBR, legacy material, projecting
    /// light, flexi, particles, animated texture, hover text, media,
    /// reflection probe, linkset) in a west-to-east row a few metres north
    /// of the arrival point, with every asset they reference served. This is
    /// the same fixture the automated tiers load.
    #[arg(long)]
    catalogue: bool,
}

/// Parses one `Name` / `Name@X,Y` region argument; `index` places an
/// unplaced region east of the default origin.
fn parse_region(raw: &str, index: u32) -> Option<RegionConfig> {
    let default = RegionConfig::default();
    let (name, coordinates) = match raw.split_once('@') {
        Some((name, coordinates)) => {
            let (x, y) = coordinates.split_once(',')?;
            (name, (x.trim().parse().ok()?, y.trim().parse().ok()?))
        }
        None => (raw, (default.grid_x.checked_add(index)?, default.grid_y)),
    };
    if name.trim().is_empty() {
        return None;
    }
    Some(RegionConfig {
        name: name.trim().to_owned(),
        grid_x: coordinates.0,
        grid_y: coordinates.1,
        ..default
    })
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
        .grid_identity(GridIdentity {
            name: options.grid_name.clone(),
            nick: options.grid_nick.clone(),
            ..GridIdentity::default()
        });
    // The catalogue replaces a region's content wholesale; a region built
    // without it keeps the stock scenario.
    let dress = |region: RegionConfig| {
        if options.catalogue {
            catalogue().into_region(region)
        } else {
            region
        }
    };
    if options.regions.is_empty() {
        builder = builder.region(dress(RegionConfig::default()));
    }
    for (index, raw) in options.regions.iter().enumerate() {
        match u32::try_from(index)
            .ok()
            .and_then(|index| parse_region(raw, index))
        {
            Some(region) => {
                tracing::info!(
                    "region {:?} at {},{}",
                    region.name,
                    region.grid_x,
                    region.grid_y
                );
                builder = builder.region(dress(region));
            }
            None => {
                tracing::error!("unparsable --region {raw:?} (want Name or Name@X,Y)");
                return Err("bad --region argument".into());
            }
        }
    }
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

    if options.catalogue {
        for entry in sl_fake_grid::fixtures::catalogue::entries() {
            let position = entry.position();
            tracing::info!(
                "catalogue prim {:?} (local id {}) at <{}, {}, {}>",
                entry.name,
                entry.local_id.0,
                position.x,
                position.y,
                position.z
            );
        }
    }

    let grid = builder.start().await?;
    tracing::info!(
        "fake grid ready: login URI {} (also get_grid_info, map tiles, currency.php/landtool.php)",
        grid.login_uri()
    );

    let mut logins = grid.logins();
    let mut teleports = grid.teleports();
    loop {
        tokio::select! {
            notice = teleports.recv() => {
                match notice {
                    Ok(notice) => tracing::info!(
                        "avatar {} teleported to {} (session {} -> {})",
                        notice.agent_id,
                        notice.region_name,
                        notice.from_seq,
                        notice.to_seq
                    ),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::warn!("missed {missed} teleport notices");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
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
