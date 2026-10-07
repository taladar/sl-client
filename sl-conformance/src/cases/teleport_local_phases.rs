//! Drive a local (intra-region) teleport and hold the grid to the shape it
//! was measured sending: the order of the messages, their flags, and which
//! way the `TeleportLocal` says the agent faces.

use std::time::Instant;

use sl_client_tokio::TeleportFlags;

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, count_metric, secs_metric};
use crate::teleport_trace::{LocalArrival, request_teleport, watch_teleport};

/// Where each answer below is written down.
const SOURCE: &str = "book/src/gridspec/teleport.md (teleport-local-phases, 2026-10-07)";

/// The region-local destination of the teleport: off the region's centre, so
/// that "towards the region's origin" is a direction with three distinct
/// components, at a modest height.
///
/// A local teleport request carries a region-local position, and the point is
/// inside the region whichever one the avatar logged in to, keeping the request
/// a genuinely *local* teleport.
const DESTINATION: (f32, f32, f32) = (120.0, 136.0, 40.0);

/// The facing the request asks for: north, which is neither the east a grid
/// falls back to nor the direction of the region's origin from
/// [`DESTINATION`], so the three possible answers are told apart.
const REQUESTED_LOOK_AT: (f32, f32, f32) = (0.0, 1.0, 0.0);

/// How far a stated look-at may be from the direction it is classified as.
const LOOK_AT_TOLERANCE: f32 = 0.05;

/// The messages a local teleport is answered with: a `TeleportStart`, then
/// the `TeleportLocal`, and no progress line between them, on both grids.
const SEQUENCE: Measured<&str> = Measured {
    second_life: "started,local",
    opensim: "started,local",
    source: SOURCE,
};

/// The flags of the `TeleportStart` and the `TeleportLocal` (they agree):
/// Second Life adds `WITHIN_REGION` to the request's `VIA_LOCATION`; OpenSim
/// sends the request's alone.
const FLAGS: Measured<u32> = Measured {
    second_life: TeleportFlags::VIA_LOCATION | TeleportFlags::WITHIN_REGION,
    opensim: TeleportFlags::VIA_LOCATION,
    source: SOURCE,
};

/// Which way the `TeleportLocal` says the agent faces: OpenSim the direction
/// the request asked for; Second Life the direction from the landing position
/// towards the region's origin, whatever was asked.
const LOOK_AT: Measured<&str> = Measured {
    second_life: "towards-region-origin",
    opensim: "requested",
    source: SOURCE,
};

/// Names the rule a `TeleportLocal`'s look-at follows: `requested`,
/// `towards-region-origin`, or `other` when it is neither.
fn look_at_rule(local: &LocalArrival) -> &'static str {
    let (asked_x, asked_y, asked_z) = REQUESTED_LOOK_AT;
    let near = |x: f32, y: f32, z: f32| {
        (local.look_at.x - x).abs() < LOOK_AT_TOLERANCE
            && (local.look_at.y - y).abs() < LOOK_AT_TOLERANCE
            && (local.look_at.z - z).abs() < LOOK_AT_TOLERANCE
    };
    let at = local.position;
    let length = at
        .x()
        .mul_add(at.x(), at.y().mul_add(at.y(), at.z() * at.z()))
        .sqrt();
    if near(asked_x, asked_y, asked_z) {
        "requested"
    } else if length > f32::EPSILON && near(-at.x() / length, -at.y() / length, -at.z() / length) {
        "towards-region-origin"
    } else {
        "other"
    }
}

/// Drives a local teleport and holds the grid to the shape of its answer.
///
/// A teleport whose destination is the agent's *current* region is answered
/// with a `TeleportStart` and a `TeleportLocal`: the circuit is not torn down
/// and re-established, so no `RegionChanged` handover follows, and neither
/// grid narrates it with a progress line. The two differ in the flags — Second
/// Life marks the pair `WITHIN_REGION` — and in the look-at the `TeleportLocal`
/// states, which a viewer applies to the avatar at once: OpenSim echoes the
/// request, Second Life points the agent at the region's south-west corner.
///
/// The case records the whole trace — order, flags, landing position, look-at
/// — and the request-to-arrival time. A landing position other than the one
/// asked for is recorded and not held: a parcel's landing point or a telehub
/// may redirect an in-region teleport on either grid.
#[derive(Debug)]
pub struct TeleportLocalPhases;

impl GridTest for TeleportLocalPhases {
    fn name(&self) -> &'static str {
        "teleport-local-phases"
    }

    fn description(&self) -> &'static str {
        "Drive a local teleport and hold its order, flags and look-at to the grid's measured shape"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // Teleport within the agent's *current* region so the request is
            // intra-region: the destination region handle is the one the agent
            // is already in.
            let region_handle = session.region_handle().ok_or_else(|| {
                TestFailure::Assertion("no region handle after the region handshake".to_owned())
            })?;

            let started_at = Instant::now();
            request_teleport(session, region_handle, DESTINATION, REQUESTED_LOOK_AT).await?;
            let trace = watch_teleport(session, REGION_TIMEOUT).await?;
            let elapsed = started_at.elapsed();

            let metrics = ctx.metrics();
            trace.record("", metrics);
            metrics.set("phase_sequence", trace.sequence());
            metrics.set(
                &count_metric("progress_updates"),
                i64::try_from(trace.progress.len()).unwrap_or(-1),
            );
            metrics.set_timing(&secs_metric("teleport"), elapsed.as_secs_f64());

            if let Some(failure) = &trace.failure {
                return Err(TestFailure::Assertion(format!(
                    "local teleport failed: {}",
                    failure.reason
                )));
            }
            let local = trace.local.as_ref().ok_or_else(|| {
                TestFailure::Assertion(format!(
                    "a teleport within the agent's own region ended as [{}], not with a \
                     TeleportLocal",
                    trace.sequence()
                ))
            })?;
            let (wanted_x, wanted_y, _wanted_z) = DESTINATION;
            let redirected = (local.position.x() - wanted_x).abs() > 1.0
                || (local.position.y() - wanted_y).abs() > 1.0;
            let rule = look_at_rule(local);
            let metrics = ctx.metrics();
            metrics.set("landing_redirected", redirected);
            metrics.set("look_at_rule", rule);

            SEQUENCE.check(
                "the messages a local teleport is answered with",
                grid,
                &trace.sequence().as_str(),
            )?;
            check(
                trace.starts.len() == 1,
                &format!("expected one TeleportStart, got {:?}", trace.starts),
            )?;
            for flags in &trace.starts {
                FLAGS.check("the TeleportStart flags of a local teleport", grid, flags)?;
            }
            FLAGS.check("the TeleportLocal flags", grid, &local.flags)?;
            LOOK_AT.check("which way the TeleportLocal faces the agent", grid, &rule)?;
            Ok(())
        })
    }
}
