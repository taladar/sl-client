//! What one teleport looked like from the client: every teleport event from
//! the request to the one that ended it, in order, with the fields each
//! carried.
//!
//! The `teleport-*` cases all watch the same handful of events and differ in
//! what they ask of them, so the watching is here once. A [`TeleportTrace`]
//! ends at the first of a `TeleportLocal`, a `TeleportFailed` or the arrival in
//! another region ([`Event::RegionChanged`]); what a grid sent before that —
//! whether it started the teleport at all, which progress lines it narrated,
//! with which flags — is the shape the two live grids disagree on
//! (`book/src/gridspec/teleport.md`).

use std::net::SocketAddr;
use std::time::Duration;

use sl_client_tokio::{
    AlertInfo, Command, Diagnostic, Event, GridCoordinates, MapRegionInfo, Maturity,
    RegionCoordinates, RegionHandle, Vector,
};

use crate::context::{Session, TestFailure};
use crate::metrics::Metrics;
use crate::support::REPLY_TIMEOUT;

/// How many grid cells to pad a neighbour search by on each side of the
/// agent's own region: one covers every immediate neighbour.
const BLOCK_MARGIN: u32 = 1;

/// The quiet gap (no further `MapBlockReply`) that marks a map reply fully
/// drained. OpenSim's world-map worker batches regions with a ~50 ms sleep
/// between batches, so this stays comfortably above that cadence.
const BLOCK_DRAIN_QUIET: Duration = Duration::from_secs(2);

/// How long the diagnostic channel is given to deliver what the session
/// recorded beside a failure.
const DIAGNOSTIC_GRACE: Duration = Duration::from_millis(500);

/// An intra-region teleport's completion (`TeleportLocal`).
#[derive(Debug, Clone, PartialEq)]
pub struct LocalArrival {
    /// Where the simulator says the agent landed.
    pub position: RegionCoordinates,
    /// Which way the simulator says the agent faces.
    pub look_at: Vector,
    /// The `TeleportLocal` flags.
    pub flags: u32,
}

/// An inter-region teleport's `TeleportFinish`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finish {
    /// The destination region.
    pub region_handle: RegionHandle,
    /// The destination simulator.
    pub sim: SocketAddr,
    /// The destination's rating.
    pub maturity: Maturity,
    /// The `TeleportFinish` flags.
    pub flags: u32,
}

/// The arrival in another region ([`Event::RegionChanged`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arrival {
    /// The region arrived in.
    pub region_handle: RegionHandle,
    /// Its simulator.
    pub sim: SocketAddr,
    /// Whether the client dropped the world it left (a distant teleport) or
    /// kept it (a neighbour it was already holding).
    pub world_reset: bool,
}

/// A refused or abandoned teleport (`TeleportFailed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The reason: a sentence on OpenSim, usually a key on Second Life.
    pub reason: String,
    /// The alert Second Life attaches; OpenSim sends none.
    pub alert: Option<AlertInfo>,
    /// Whether the grid sent this failure. `false` is the session's own
    /// deadline: the grid neither carried the teleport out nor refused it.
    pub from_grid: bool,
}

/// Every teleport event one request produced, up to the one that ended it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TeleportTrace {
    /// The events in the order they arrived, by label (`started`, `progress`,
    /// `finished`, `local`, `failed`, `region-changed`).
    pub order: Vec<&'static str>,
    /// The flags of each `TeleportStart`.
    pub starts: Vec<u32>,
    /// Each progress line and its flags.
    pub progress: Vec<(String, u32)>,
    /// The `TeleportFinish`, when one came.
    pub finish: Option<Finish>,
    /// The `TeleportLocal` that ended it, for a local teleport.
    pub local: Option<LocalArrival>,
    /// The arrival that ended it, for an inter-region one.
    pub arrival: Option<Arrival>,
    /// The failure that ended it.
    pub failure: Option<Failure>,
}

impl TeleportTrace {
    /// The order of the events, comma-separated.
    #[must_use]
    pub fn sequence(&self) -> String {
        self.order.join(",")
    }

    /// The progress lines alone, in order.
    #[must_use]
    pub fn lines(&self) -> Vec<&str> {
        self.progress
            .iter()
            .map(|(line, _flags)| line.as_str())
            .collect()
    }

    /// How the teleport ended: `local`, `moved`, `failed`, or `unfinished` for
    /// a trace that was never completed.
    #[must_use]
    pub const fn outcome(&self) -> &'static str {
        if self.local.is_some() {
            "local"
        } else if self.arrival.is_some() {
            "moved"
        } else if self.failure.is_some() {
            "failed"
        } else {
            "unfinished"
        }
    }

    /// How the grid answered: `local` or `moved` for a teleport it carried
    /// out, `refused` for one it failed, and `unanswered` for one that ended on
    /// the session's own deadline with the grid having said nothing.
    #[must_use]
    pub const fn answer(&self) -> &'static str {
        match &self.failure {
            Some(failure) if failure.from_grid => "refused",
            Some(_deadline) => "unanswered",
            None => self.outcome(),
        }
    }

    /// Record the whole trace under `prefix`: the order, every flags word in
    /// hexadecimal, the lines, and the fields of whatever ended it.
    pub fn record(&self, prefix: &str, metrics: &mut Metrics) {
        let hex = |flags: u32| format!("{flags:#07x}");
        let key = |name: &str| format!("{prefix}{name}");
        metrics.set(&key("sequence"), self.sequence());
        metrics.set(&key("outcome"), self.outcome());
        metrics.set(&key("answer"), self.answer());
        metrics.set(
            &key("start_flags"),
            self.starts
                .iter()
                .map(|flags| hex(*flags))
                .collect::<Vec<_>>()
                .join(" "),
        );
        metrics.set(&key("progress_lines"), self.lines().join(" | "));
        metrics.set(
            &key("progress_flags"),
            self.progress
                .iter()
                .map(|(_line, flags)| hex(*flags))
                .collect::<Vec<_>>()
                .join(" "),
        );
        if let Some(finish) = &self.finish {
            metrics.set(&key("finish_flags"), hex(finish.flags));
            metrics.set(&key("finish_maturity"), format!("{:?}", finish.maturity));
        }
        if let Some(local) = &self.local {
            metrics.set(&key("local_flags"), hex(local.flags));
            metrics.set(
                &key("local_position"),
                format!(
                    "{:.2} {:.2} {:.2}",
                    local.position.x(),
                    local.position.y(),
                    local.position.z()
                ),
            );
            metrics.set(
                &key("local_look_at"),
                format!(
                    "{:.3} {:.3} {:.3}",
                    local.look_at.x, local.look_at.y, local.look_at.z
                ),
            );
        }
        if let Some(arrival) = &self.arrival {
            metrics.set(&key("world_reset"), arrival.world_reset);
        }
        if let Some(failure) = &self.failure {
            metrics.set(&key("failure_from_grid"), failure.from_grid);
            metrics.set(&key("failure_reason"), failure.reason.clone());
            metrics.set(
                &key("failure_alert"),
                failure
                    .alert
                    .as_ref()
                    .map_or_else(|| "none".to_owned(), |alert| alert.message.clone()),
            );
            metrics.set(
                &key("failure_alert_params"),
                failure
                    .alert
                    .as_ref()
                    .map_or_else(String::new, |alert| alert.extra_params.trim().to_owned()),
            );
        }
    }
}

/// Watches the teleport just asked for until it ends: a `TeleportLocal`, a
/// `TeleportFailed`, or the arrival in another region.
///
/// `timeout` bounds the wait for each event, not the whole teleport.
///
/// # Errors
///
/// Propagates a [`Session::wait_for`] timeout or disconnect — a teleport that
/// neither arrives nor fails is the hang this exists to catch.
pub async fn watch_teleport(
    session: &mut Session,
    timeout: Duration,
) -> Result<TeleportTrace, TestFailure> {
    let mut trace = TeleportTrace::default();
    let diagnostics_before = session.diagnostics().len();
    session
        .wait_for(timeout, |event| match event {
            Event::TeleportStarted { flags } => {
                trace.order.push("started");
                trace.starts.push(flags.0);
                None
            }
            Event::TeleportProgress {
                message,
                teleport_flags,
            } => {
                trace.order.push("progress");
                trace.progress.push((message.clone(), *teleport_flags));
                None
            }
            Event::TeleportFinished {
                region_handle,
                sim,
                maturity,
                flags,
            } => {
                trace.order.push("finished");
                trace.finish = Some(Finish {
                    region_handle: *region_handle,
                    sim: *sim,
                    maturity: *maturity,
                    flags: flags.0,
                });
                None
            }
            Event::TeleportLocal {
                position,
                look_at,
                flags,
            } => {
                trace.order.push("local");
                trace.local = Some(LocalArrival {
                    position: *position,
                    look_at: look_at.clone(),
                    flags: flags.0,
                });
                Some(())
            }
            Event::TeleportFailed { reason, alert_info } => {
                trace.order.push("failed");
                trace.failure = Some(Failure {
                    reason: reason.clone(),
                    alert: alert_info.clone(),
                    from_grid: true,
                });
                Some(())
            }
            Event::RegionChanged {
                region_handle,
                sim,
                world_reset,
                ..
            } => {
                trace.order.push("region-changed");
                trace.arrival = Some(Arrival {
                    region_handle: *region_handle,
                    sim: *sim,
                    world_reset: *world_reset,
                });
                Some(())
            }
            _ => None,
        })
        .await?;
    if let Some(failure) = trace.failure.as_mut() {
        // The session's own deadline ends a teleport with the same event a
        // grid's refusal does; the diagnostic it records beside it is the
        // difference, and it reaches the harness on another task.
        tokio::time::sleep(DIAGNOSTIC_GRACE).await;
        failure.from_grid =
            !session
                .diagnostics()
                .iter()
                .skip(diagnostics_before)
                .any(|diagnostic| {
                    matches!(
                        diagnostic,
                        Diagnostic::ExpectedReplyMissing { request, .. }
                            if request == Diagnostic::TELEPORT_REQUEST
                    )
                });
    }
    Ok(trace)
}

/// Asks for a teleport to `position` in the region `region_handle`, facing
/// `look_at`.
///
/// # Errors
///
/// Propagates a closed command channel.
pub async fn request_teleport(
    session: &Session,
    region_handle: RegionHandle,
    position: (f32, f32, f32),
    look_at: (f32, f32, f32),
) -> Result<(), TestFailure> {
    let (x, y, z) = position;
    let (look_x, look_y, look_z) = look_at;
    session
        .send(Command::Teleport {
            region_handle,
            position: RegionCoordinates::new(x, y, z),
            look_at: Vector {
                x: look_x,
                y: look_y,
                z: look_z,
            },
        })
        .await
}

/// Finds a region next to the agent's own on the world map: the first block
/// within one cell whose grid coordinates differ from `origin`.
///
/// # Errors
///
/// [`TestFailure::Assertion`] when the map reports no other region there, and
/// whatever [`Session::wait_for`] raises.
pub async fn neighbouring_region(
    session: &mut Session,
    origin: GridCoordinates,
) -> Result<MapRegionInfo, TestFailure> {
    session
        .send(Command::RequestMapBlocks {
            min_x: origin.x().saturating_sub(BLOCK_MARGIN),
            max_x: origin.x().saturating_add(BLOCK_MARGIN),
            min_y: origin.y().saturating_sub(BLOCK_MARGIN),
            max_y: origin.y().saturating_add(BLOCK_MARGIN),
        })
        .await?;
    drain_map_blocks(session)
        .await?
        .into_iter()
        .find(|block| block.grid_coordinates != origin && block.name.is_some())
        .ok_or_else(|| {
            TestFailure::Assertion(
                "the world map reported no region next to the agent's own, so there is no \
                 other region to teleport to"
                    .to_owned(),
            )
        })
}

/// Looks a region up on the world map by its exact name.
///
/// # Errors
///
/// [`TestFailure::Assertion`] when the map knows no region of that name, and
/// whatever [`Session::wait_for`] raises.
pub async fn region_named(session: &mut Session, name: &str) -> Result<MapRegionInfo, TestFailure> {
    session
        .send(Command::RequestMapByName {
            name: name.to_owned(),
        })
        .await?;
    drain_map_blocks(session)
        .await?
        .into_iter()
        .find(|block| {
            block
                .name
                .as_ref()
                .is_some_and(|found| found.to_string().eq_ignore_ascii_case(name))
        })
        .ok_or_else(|| {
            TestFailure::Assertion(format!("the world map knows no region named {name:?}"))
        })
}

/// Drains the [`Event::MapBlock`] entries a map reply yields until none
/// arrives for [`BLOCK_DRAIN_QUIET`]. The first block is awaited with the full
/// [`REPLY_TIMEOUT`] (OpenSim queues the request onto a worker thread).
async fn drain_map_blocks(session: &mut Session) -> Result<Vec<MapRegionInfo>, TestFailure> {
    let mut blocks = Vec::new();
    loop {
        let timeout = if blocks.is_empty() {
            REPLY_TIMEOUT
        } else {
            BLOCK_DRAIN_QUIET
        };
        match session
            .wait_for(timeout, |event| match event {
                Event::MapBlock(region) => Some((**region).clone()),
                _ => None,
            })
            .await
        {
            Ok(region) => blocks.push(region),
            Err(TestFailure::Timeout(_)) => return Ok(blocks),
            Err(other) => return Err(other),
        }
    }
}
