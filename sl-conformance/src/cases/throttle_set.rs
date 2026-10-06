//! Apply two bandwidth `Throttle` presets, confirm the simulator accepts them,
//! and measure whether either changes how fast it sends.

use std::time::{Duration, Instant};

use sl_client_tokio::{CircuitProbe, Command, Diagnostic, Event, ScopedObjectId, Throttle, pcode};

use crate::circuit::{Seen, listen, seen_since};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, count_metric, secs_metric};

/// How long the session stays in world collecting the scene before it asks
/// for any of it again.
const SETTLE: Duration = Duration::from_secs(12);

/// The most objects one burst asks for. A few hundred full updates are tens
/// of kilobytes: seconds of the 50 kbps preset's task rate and a fraction of a
/// second of the 1000 kbps one's.
const BURST_OBJECTS: usize = 300;

/// How long a throttle is given to take effect before the burst under it.
const APPLY: Duration = Duration::from_secs(2);

/// How long the burst under the 1000 kbps preset is watched.
const FAST_WINDOW: Duration = Duration::from_secs(15);

/// How long the burst under the 50 kbps preset is watched. With
/// [`FAST_WINDOW`] and the two [`APPLY`]s it also carries the case past the
/// client's retransmission budget for the first `AgentThrottle` — four
/// transmissions at a timeout of at most ten seconds — so a throttle the
/// simulator never acknowledged has been given up, and reported, by the end.
const SLOW_WINDOW: Duration = Duration::from_secs(30);

/// The case's budget.
const CASE_TIMEOUT: Duration = Duration::from_secs(150);

/// The messages a re-requested object comes back as.
const OBJECT_MESSAGES: &[&str] = &[
    "ObjectUpdate",
    "ObjectUpdateCompressed",
    "ObjectUpdateCached",
    "ImprovedTerseObjectUpdate",
];

/// What one re-request of the scene looked like on the wire.
#[derive(Debug, Clone, Copy)]
struct Burst {
    /// The object datagrams that arrived in the window.
    datagrams: usize,
    /// Their bytes.
    bytes: usize,
    /// Seconds from the request to the datagram that completed half of those
    /// bytes — how fast the burst came, whatever trickled in after it.
    half_at: f64,
}

/// Apply `throttle`, ask for `objects` again and watch the answer for
/// `window`.
async fn burst(
    session: &mut Session,
    throttle: Throttle,
    objects: &[ScopedObjectId],
    window: Duration,
) -> Result<Burst, TestFailure> {
    session.send(Command::SetThrottle(throttle)).await?;
    if let Some(reason) = listen(session, APPLY, |_event| {}).await? {
        return Err(TestFailure::Disconnected(format!("{reason:?}")));
    }
    let already = session.diagnostics().len();
    let started = Instant::now();
    session
        .send(Command::RequestObjects {
            local_ids: objects.to_vec(),
        })
        .await?;
    if let Some(reason) = listen(session, window, |_event| {}).await? {
        return Err(TestFailure::Disconnected(format!("{reason:?}")));
    }
    let answer: Vec<Seen> = seen_since(session, already, started)
        .into_iter()
        .filter(|datagram| {
            !datagram.child
                && datagram.offset >= 0.0
                && OBJECT_MESSAGES.iter().any(|name| datagram.is(name))
        })
        .collect();
    let bytes: usize = answer.iter().map(|datagram| datagram.len).sum();
    let mut running = 0_usize;
    let mut half_at = 0.0;
    for datagram in &answer {
        running = running.saturating_add(datagram.len);
        if running.saturating_mul(2) >= bytes {
            half_at = datagram.offset;
            break;
        }
    }
    Ok(Burst {
        datagrams: answer.len(),
        bytes,
        half_at,
    })
}

/// Record one [`Burst`] under `prefix`.
fn record(ctx: &mut TestContext, prefix: &str, burst: Burst) {
    let metrics = ctx.metrics();
    metrics.set(
        &count_metric(&format!("{prefix}_datagrams")),
        u32::try_from(burst.datagrams).unwrap_or(u32::MAX),
    );
    metrics.set(
        &format!("{prefix}_bytes"),
        u32::try_from(burst.bytes).unwrap_or(u32::MAX),
    );
    metrics.set(
        &secs_metric(&format!("{prefix}_half_delivered")),
        burst.half_at,
    );
}

/// Applies two bandwidth throttle presets and measures a burst under each.
///
/// A viewer tells the simulator how to split its UDP send bandwidth across the
/// seven traffic categories with an `AgentThrottle` message. It is fire-and-
/// forget: the simulator simply re-weights its outboxes and never replies. So
/// "accepted" cannot be asserted from a reply — instead it is the *absence* of a
/// failure: `AgentThrottle` is sent reliably, and an accepted packet is acked by
/// the sim's reliable-UDP layer rather than retransmitted to exhaustion (which,
/// in our client, abandons the packet and records a diagnostic naming it).
///
/// Whether the throttle *does* anything is the other half. The case collects
/// the scene, then asks for up to 300 of its objects again
/// (`RequestMultipleObjects`) twice — under the 1000 kbps preset, whose task
/// rate is 310 kbps, and under the 50 kbps preset, whose task rate is 10 —
/// with the circuits probed, and records how many bytes of object updates came
/// back and how long half of them took. A simulator that honours the throttle
/// takes many times longer over the second burst.
///
/// Runs on every grid: `AgentThrottle` is plain LLUDP, handled by OpenSim,
/// Second Life and the offline fake grid alike.
#[derive(Debug)]
pub struct ThrottleSet;

impl GridTest for ThrottleSet {
    fn name(&self) -> &'static str {
        "throttle-set"
    }

    fn description(&self) -> &'static str {
        "Apply two Throttle presets; confirm they are accepted and measure a burst under each"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::ProbeCircuits(CircuitProbe::Observe))
                .await?;

            // The scene, as it arrives: the prims of the root circuit.
            let mut objects: Vec<ScopedObjectId> = Vec::new();
            let ended = listen(session, SETTLE, |event| {
                if let Event::ObjectAdded(object) = event
                    && objects.len() < BURST_OBJECTS
                    && object.pcode == pcode::PRIMITIVE
                {
                    objects.push(object.scoped_id());
                }
            })
            .await?;
            if let Some(reason) = ended {
                return Err(TestFailure::Disconnected(format!("{reason:?}")));
            }
            let root = session.circuit_id();
            objects.retain(|object| Some(object.circuit()) == root);
            check(
                !objects.is_empty(),
                "the region sent no object to ask for again",
            )?;

            let fast_throttle = Throttle::preset_1000();
            let slow_throttle = Throttle::preset_50();
            let fast = burst(session, fast_throttle, &objects, FAST_WINDOW).await?;
            let slow = burst(session, slow_throttle, &objects, SLOW_WINDOW).await?;

            // The circuit is still up, and neither throttle was retransmitted
            // to exhaustion. Losing an ordinary reliable packet does not fail
            // the session — it is reported as this diagnostic and the circuit
            // carries on — so this, not the circuit's survival, is what says
            // the simulator took the throttles.
            let throttle_dropped = session.diagnostics().iter().any(|diagnostic| {
                matches!(
                    diagnostic,
                    Diagnostic::ExpectedReplyMissing { request, .. }
                        if request == "AgentThrottle"
                )
            });
            session
                .send(Command::ProbeCircuits(CircuitProbe::Off))
                .await?;
            // Leave the circuit as a viewer would have it.
            session.send(Command::SetThrottle(fast_throttle)).await?;

            let requested = objects.len();
            let metrics = ctx.metrics();
            metrics.set(
                &count_metric("objects_requested"),
                u32::try_from(requested).unwrap_or(u32::MAX),
            );
            metrics.set("fast_throttle_total_kbps", f64::from(fast_throttle.total()));
            metrics.set("slow_throttle_total_kbps", f64::from(slow_throttle.total()));
            record(ctx, "fast_burst", fast);
            record(ctx, "slow_burst", slow);

            check(
                !throttle_dropped,
                "AgentThrottle was retransmitted to exhaustion (never acked by the simulator)",
            )?;
            check(
                fast.datagrams > 0,
                "the simulator sent nothing back for the objects asked for again",
            )
        })
    }
}
