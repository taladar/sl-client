//! Drive a local (intra-region) teleport and assert the observable phase
//! sequence from the request to arrival.

use std::time::Instant;

use sl_client_tokio::{Command, Event, RegionCoordinates, Vector};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, count_metric, secs_metric};

/// The region-local destination of the teleport: the centre of the 256 m region
/// at a modest height.
///
/// A local teleport request carries a region-local position; the simulator
/// clamps `Z` up to ground level, so the exact height only needs to be
/// non-negative. The centre `(128, 128)` is always inside the region regardless
/// of which region the avatar logged in to, keeping the request a genuinely
/// *local* teleport (the destination region is the agent's current region).
const DESTINATION: (f32, f32, f32) = (128.0, 128.0, 30.0);

/// One teleport phase observed on the circuit between the request and arrival.
///
/// The variants mirror the client-facing teleport [`Event`]s the session emits
/// as it walks the sequence, in the order the reference viewer models it:
/// *Starting* (`TeleportStart`), *Progress* (`TeleportProgress`), then the
/// terminal *Complete* — which for an intra-region teleport is the dedicated
/// `TeleportLocal` (the circuit does not change), and for the border-crossing
/// case a `RegionChanged` handover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The simulator acknowledged the request and began the teleport
    /// (`TeleportStart` → [`Event::TeleportStarted`]).
    Started,
    /// A progress update arrived mid-teleport (`TeleportProgress` →
    /// [`Event::TeleportProgress`]).
    Progress,
    /// The intra-region teleport completed without a circuit change
    /// (`TeleportLocal` → [`Event::TeleportLocal`]).
    Local,
    /// The destination region's handshake completed after a border crossing
    /// ([`Event::RegionChanged`]) — the cross-region completion, tolerated for
    /// an avatar that logged in adjacent to the target.
    RegionChanged,
}

impl Phase {
    /// The short label recorded in the `phase_sequence` metric.
    const fn label(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Progress => "progress",
            Self::Local => "local",
            Self::RegionChanged => "region-changed",
        }
    }

    /// Whether this phase terminates the teleport (arrival), ending observation.
    const fn is_terminal(self) -> bool {
        matches!(self, Self::Local | Self::RegionChanged)
    }
}

/// Drives a local teleport and asserts the phase sequence from request to
/// arrival.
///
/// A viewer teleport walks a small state machine — *Starting* → *Progress* →
/// arrival — announced by the simulator with `TeleportStart`, zero or more
/// `TeleportProgress` frames, and a terminal message. For a teleport whose
/// destination is the agent's *current* region the terminal message is
/// `TeleportLocal`: the circuit is not torn down and re-established, so no
/// `RegionChanged` handover follows. OpenSim's intra-region path emits only
/// `TeleportStart` then `TeleportLocal` (no intermediate `TeleportProgress`),
/// which is the complete local sequence for that grid.
///
/// The case teleports to the centre of the agent's current region, collects the
/// teleport phases the session surfaces until it arrives, and asserts that the
/// sequence began with *Starting* and ended at a terminal phase (`TeleportLocal`
/// for the expected intra-region case, or a `RegionChanged` handover tolerated
/// for an avatar that logged in adjacent to the target). It records the ordered
/// phase sequence, the count of progress updates, and the request-to-arrival
/// time.
#[derive(Debug)]
pub struct TeleportLocalPhases;

impl GridTest for TeleportLocalPhases {
    fn name(&self) -> &'static str {
        "teleport-local-phases"
    }

    fn description(&self) -> &'static str {
        "Drive a local teleport and assert the Starting -> Progress -> Complete phase sequence"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // Teleport to the centre of the agent's *current* region so the
            // request is intra-region: the destination region handle is the one
            // the agent is already in.
            let region_handle = session.region_handle().ok_or_else(|| {
                TestFailure::Assertion("no region handle after the region handshake".to_owned())
            })?;
            let (x, y, z) = DESTINATION;

            let started_at = Instant::now();
            session
                .send(Command::Teleport {
                    region_handle,
                    position: RegionCoordinates::new(x, y, z),
                    look_at: Vector {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                })
                .await?;

            // Collect the teleport phases the session surfaces until it arrives
            // (a terminal phase) or a `TeleportFailed` fails the case. A local
            // teleport is quick, but keep the window at the region timeout so the
            // border-crossing completion path (a full circuit handshake) also
            // fits.
            let mut phases: Vec<Phase> = Vec::new();
            loop {
                let phase = session
                    .wait_for(REGION_TIMEOUT, |event| match event {
                        Event::TeleportStarted => Some(Ok(Phase::Started)),
                        Event::TeleportProgress { .. } => Some(Ok(Phase::Progress)),
                        Event::TeleportLocal => Some(Ok(Phase::Local)),
                        Event::RegionChanged { .. } => Some(Ok(Phase::RegionChanged)),
                        Event::TeleportFailed { reason, .. } => Some(Err(reason.clone())),
                        _ => None,
                    })
                    .await?;
                match phase {
                    Ok(phase) => {
                        let terminal = phase.is_terminal();
                        phases.push(phase);
                        if terminal {
                            break;
                        }
                    }
                    Err(reason) => {
                        return Err(TestFailure::Assertion(format!(
                            "local teleport failed: {reason}"
                        )));
                    }
                }
            }
            let elapsed = started_at.elapsed();

            // The sequence must open with the Starting phase: the simulator
            // acknowledged the request before doing anything else.
            check(
                phases.first() == Some(&Phase::Started),
                "expected the teleport to begin with a Starting (TeleportStart) phase",
            )?;
            // ... and end at arrival — the intra-region TeleportLocal for the
            // expected local case, or a RegionChanged handover for an avatar that
            // logged in adjacent to the target region.
            let arrival = phases
                .last()
                .copied()
                .ok_or_else(|| TestFailure::Assertion("no teleport phases observed".to_owned()))?;
            check(
                arrival.is_terminal(),
                "expected the teleport to end at an arrival phase (TeleportLocal / RegionChanged)",
            )?;

            let progress_updates = phases.iter().filter(|p| **p == Phase::Progress).count();
            let sequence = phases
                .iter()
                .map(|p| p.label())
                .collect::<Vec<_>>()
                .join(",");

            let metrics = ctx.metrics();
            metrics.set("phase_sequence", sequence);
            metrics.set(
                &count_metric("progress_updates"),
                i64::try_from(progress_updates).unwrap_or(-1),
            );
            metrics.set_timing(&secs_metric("teleport"), elapsed.as_secs_f64());
            Ok(())
        })
    }
}
