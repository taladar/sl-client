//! Provoke the two teleport refusals any avatar can provoke — a region that
//! does not exist and a landmark the grid does not hold — and hold the grid to
//! how it was measured refusing each.

use std::time::Instant;

use sl_client_tokio::{AssetKey, Command, RegionHandle, TeleportFlags, Uuid};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, secs_metric};
use crate::teleport_trace::{TeleportTrace, request_teleport, watch_teleport};

/// Where each answer below is written down.
const SOURCE: &str = "book/src/gridspec/teleport.md (teleport-failed, 2026-10-07)";

/// Grid coordinates (region indices) of the non-existent destination region.
///
/// Every grid the case runs on keeps its regions around `(1000, 1000)`;
/// `(2000, 2000)` is far outside any of them, so no region occupies the handle
/// and the destination lookup fails. Choosing coordinates in the void — rather
/// than reusing the current region's handle with an illegal position — is what
/// forces the *different-region* path.
const VOID_REGION_GRID: (u32, u32) = (2000, 2000);

/// The region-local landing position of the (doomed) teleport request. The
/// exact value is irrelevant since the destination region does not exist, but a
/// plausible in-region position keeps the request well-formed.
const DESTINATION: (f32, f32, f32) = (128.0, 128.0, 30.0);

/// A landmark asset id no grid holds.
const NO_SUCH_LANDMARK: u128 = 0x1111_1111_2222_3333_4444_5555_5555_5555;

/// How a grid refuses one kind of request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Refusal {
    /// The flags of the `TeleportStart` sent before the refusal, if the grid
    /// starts a teleport it is about to refuse.
    start: Option<u32>,
    /// The progress lines sent before the refusal.
    lines: &'static [&'static str],
    /// The `TeleportFailed` reason.
    reason: &'static str,
    /// The key of the alert attached, if any.
    alert: Option<&'static str>,
}

/// A teleport to a region handle no region answers to. Second Life starts
/// the teleport, says `resolving`, then fails it over the event queue with the
/// key `no_host`, repeated in an alert; OpenSim answers with a UDP
/// `TeleportFailed` carrying a sentence, and nothing before or beside it.
const UNKNOWN_REGION: Measured<Refusal> = Measured {
    second_life: Refusal {
        start: Some(TeleportFlags::VIA_LOCATION),
        lines: &["resolving"],
        reason: "no_host",
        alert: Some("no_host"),
    },
    opensim: Refusal {
        start: None,
        lines: &[],
        reason: "The region you tried to teleport to was not found",
        alert: None,
    },
    source: SOURCE,
};

/// A teleport to a landmark asset the grid does not hold. Second Life starts
/// it as a landmark teleport and names the kind before failing it as
/// `nolandmark_tport`; OpenSim again sends the sentence alone.
const UNKNOWN_LANDMARK: Measured<Refusal> = Measured {
    second_life: Refusal {
        start: Some(TeleportFlags::VIA_LANDMARK),
        lines: &["sending_landmark"],
        reason: "nolandmark_tport",
        alert: Some("nolandmark_tport"),
    },
    opensim: Refusal {
        start: None,
        lines: &[],
        reason: "Could not find the landmark asset data",
        alert: None,
    },
    source: SOURCE,
};

/// What the trace of a refused request amounts to, in the terms of
/// [`Refusal`], borrowed from the trace.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Observed<'trace> {
    /// The flags of the first `TeleportStart`, if one came.
    start: Option<u32>,
    /// The progress lines ahead of the failure.
    lines: Vec<&'trace str>,
    /// The failure's reason.
    reason: &'trace str,
    /// The key of the failure's alert, if it carried one.
    alert: Option<&'trace str>,
}

/// What a refused request's trace amounts to; `None` for a teleport that was
/// not refused.
fn observed(trace: &TeleportTrace) -> Option<Observed<'_>> {
    let failure = trace.failure.as_ref()?;
    Some(Observed {
        start: trace.starts.first().copied(),
        lines: trace.lines(),
        reason: failure.reason.as_str(),
        alert: failure.alert.as_ref().map(|alert| alert.message.as_str()),
    })
}

/// Watches one doomed request to its end, records it under `prefix` and holds
/// the grid to `expected`.
async fn refused(
    ctx: &mut TestContext,
    prefix: &str,
    what: &str,
    expected: &Measured<Refusal>,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let started_at = Instant::now();
    let trace = watch_teleport(ctx.primary(), REGION_TIMEOUT).await?;
    let elapsed = started_at.elapsed();
    let metrics = ctx.metrics();
    trace.record(prefix, metrics);
    metrics.set_timing(
        &secs_metric(&format!("{prefix}failure")),
        elapsed.as_secs_f64(),
    );
    let Some(Observed {
        start,
        lines,
        reason,
        alert,
    }) = observed(&trace)
    else {
        return Err(TestFailure::Assertion(format!(
            "expected {what} to be refused, but it ended as [{}]",
            trace.sequence()
        )));
    };
    // A failure reason must accompany the refusal — an empty string would mean
    // the simulator refused the teleport without telling the viewer why, which
    // no client could surface.
    check(
        !reason.trim().is_empty(),
        "expected TeleportFailed to carry a non-empty failure reason",
    )?;
    check(
        trace.starts.len() <= 1,
        &format!("a refused teleport was started {:?} times", trace.starts),
    )?;
    // What was observed borrows from the trace, so it is compared with the
    // measured answer field by field rather than as one value.
    let wanted = expected.on(grid);
    check(
        start == wanted.start
            && lines == wanted.lines
            && reason == wanted.reason
            && alert == wanted.alert,
        &format!(
            "{what} was refused with start {start:?}, lines {lines:?}, reason {reason:?} and \
             alert {alert:?}; the grid was measured sending {wanted:?} ({})",
            expected.source
        ),
    )
}

/// Asks for a teleport to the landmark `landmark`.
async fn request_landmark(session: &Session, landmark: AssetKey) -> Result<(), TestFailure> {
    session
        .send(Command::TeleportViaLandmark {
            landmark: Some(landmark),
        })
        .await
}

/// Drives two teleports that cannot succeed and asserts the session reports
/// each failure the way the grid was measured reporting it.
///
/// A teleport whose destination cannot be found never reaches an arrival; the
/// simulator answers with `TeleportFailed`, and the session leaves the
/// teleporting state and stays connected to the current region. *How* it
/// answers is where the two grids part: Second Life starts the teleport,
/// narrates the step it got to, and fails it over the **event queue** with a
/// localisation key repeated in an `AlertInfo`; OpenSim sends a UDP
/// `TeleportFailed` in place of the start, its reason an English sentence and
/// no alert. A client that listens for the failure on one transport only hangs
/// on the other grid until its own timeout.
///
/// The second request is made after the first failure, so the case also shows
/// the session is usable again once a teleport has been refused.
#[derive(Debug)]
pub struct TeleportFailed;

impl GridTest for TeleportFailed {
    fn name(&self) -> &'static str {
        "teleport-failed"
    }

    fn description(&self) -> &'static str {
        "Teleport to a void region and an unknown landmark and hold each refusal to the grid's own"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;

            // A region handle in the void: no region occupies these coordinates,
            // so the destination lookup fails and the teleport is refused.
            let (grid_x, grid_y) = VOID_REGION_GRID;
            request_teleport(
                ctx.primary(),
                RegionHandle::from_grid(grid_x, grid_y),
                DESTINATION,
                (1.0, 0.0, 0.0),
            )
            .await?;
            refused(
                ctx,
                "void_",
                "a teleport to a region that does not exist",
                &UNKNOWN_REGION,
            )
            .await?;

            request_landmark(
                ctx.primary(),
                AssetKey::from(Uuid::from_u128(NO_SUCH_LANDMARK)),
            )
            .await?;
            refused(
                ctx,
                "landmark_",
                "a teleport to a landmark the grid does not hold",
                &UNKNOWN_LANDMARK,
            )
            .await
        })
    }
}
