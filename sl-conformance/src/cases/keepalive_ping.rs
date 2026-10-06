//! Watch the keep-alive pings of both ends of the circuit: time the client's
//! round trip, and count the simulator's own `StartPingCheck`s on the root
//! circuit and on every child circuit.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use sl_client_tokio::{CircuitProbe, Command, Event};

use crate::circuit::{
    Seen, gaps, histogram, listen, median, offsets_of, seen_since, tally, timeline, transmissions,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, count_metric, secs_metric};

/// How long the circuits are watched: long enough for six of the pings either
/// end sends five seconds apart.
const WATCH: Duration = Duration::from_secs(32);

/// The bounds a simulator's ping cadence is held to, in seconds. Measured on
/// 2026-10-06 (`book/src/gridspec/session.md` § Circuits): 5.1 s on Second
/// Life, root and child alike; 5.28 s on OpenSim, whose ten half-second ticks
/// each run a little long; 5.00 s on the fake grid.
const SIM_PING_INTERVAL: (f64, f64) = (4.5, 5.8);

/// The case's budget: the login, the watch and a margin.
const CASE_TIMEOUT: Duration = Duration::from_secs(120);

/// What [`WATCH`] of both ends' pings looked like.
#[derive(Debug)]
struct Watched {
    /// The client's ping round trips on the root circuit, in seconds.
    root_rtts: Vec<f64>,
    /// The client's ping round trips on child circuits, in seconds.
    child_rtts: Vec<f64>,
    /// Every datagram the simulators sent meanwhile.
    seen: Vec<Seen>,
}

impl Watched {
    /// When the simulator pinged the root circuit.
    fn root_pings(&self) -> Vec<f64> {
        offsets_of(
            self.seen.iter().filter(|datagram| !datagram.child),
            "StartPingCheck",
        )
    }

    /// The child circuits that carried anything at all.
    fn child_circuits(&self) -> BTreeSet<SocketAddr> {
        self.seen
            .iter()
            .filter(|datagram| datagram.child)
            .map(|datagram| datagram.from)
            .collect()
    }

    /// When each child circuit was pinged, by the simulator that pinged it.
    fn child_pings(&self) -> BTreeMap<SocketAddr, Vec<f64>> {
        let mut pings: BTreeMap<SocketAddr, Vec<f64>> = BTreeMap::new();
        for datagram in self
            .seen
            .iter()
            .filter(|datagram| datagram.child && datagram.is("StartPingCheck"))
        {
            pings
                .entry(datagram.from)
                .or_default()
                .push(datagram.offset);
        }
        pings
    }
}

/// Watch the probed circuits for [`WATCH`].
///
/// The client's own pings are driven by the session's timer, so this only
/// listens. Root and child round trips are kept apart: the root's is the
/// "ping to sim" a viewer displays.
async fn watch(session: &mut Session) -> Result<Watched, TestFailure> {
    let already = session.diagnostics().len();
    let started = Instant::now();
    session
        .send(Command::ProbeCircuits(CircuitProbe::Observe))
        .await?;
    let mut root_rtts = Vec::new();
    let mut child_rtts = Vec::new();
    let ended = listen(session, WATCH, |event| {
        if let Event::Ping { child, rtt, .. } = event {
            if *child {
                child_rtts.push(rtt.as_secs_f64());
            } else {
                root_rtts.push(rtt.as_secs_f64());
            }
        }
    })
    .await?;
    if let Some(reason) = ended {
        return Err(TestFailure::Disconnected(format!("{reason:?}")));
    }
    session
        .send(Command::ProbeCircuits(CircuitProbe::Off))
        .await?;
    Ok(Watched {
        root_rtts,
        child_rtts,
        seen: seen_since(session, already, started),
    })
}

/// Record what was `watched`.
fn record(metrics: &mut Metrics, watched: &Watched) {
    let root_pings = watched.root_pings();
    let child_pings = watched.child_pings();
    let child_gaps: Vec<f64> = child_pings
        .values()
        .flat_map(|offsets| gaps(offsets))
        .collect();
    // The reliable traffic of a circuit whose acknowledgements flow: the
    // baseline `circuit-unacked-resend` is read against.
    let reliable = watched
        .seen
        .iter()
        .filter(|datagram| datagram.reliable && !datagram.resent);
    let resent = transmissions(&watched.seen)
        .values()
        .filter(|sent| sent.len() > 1)
        .count();

    metrics.set("reliable_by_message", histogram(reliable));
    metrics.set(&count_metric("reliable_packets_resent"), tally(resent));
    metrics.set(
        &count_metric("client_pings_answered"),
        tally(watched.root_rtts.len()),
    );
    if let Some(rtt) = median(&watched.root_rtts) {
        metrics.set_timing(&secs_metric("ping_rtt"), rtt);
    }
    if let Some(rtt) = median(&watched.child_rtts) {
        metrics.set_timing(&secs_metric("child_ping_rtt"), rtt);
    }
    metrics.set(&count_metric("sim_pings_root"), tally(root_pings.len()));
    metrics.set("sim_ping_root_gaps", timeline(&gaps(&root_pings)));
    if let Some(gap) = median(&gaps(&root_pings)) {
        metrics.set(&secs_metric("sim_ping_root_interval"), gap);
    }
    metrics.set(
        &count_metric("child_circuits"),
        tally(watched.child_circuits().len()),
    );
    metrics.set(
        &count_metric("child_circuits_pinged"),
        tally(child_pings.len()),
    );
    if let Some(gap) = median(&child_gaps) {
        metrics.set(&secs_metric("sim_ping_child_interval"), gap);
    }
}

/// Hold what was `watched` to the measured cadence.
fn check_pings(watched: &Watched) -> Result<(), TestFailure> {
    // A genuine measurement, not a degenerate zero-or-huge value: the round
    // trip must complete inside the reply window.
    let rtt = median(&watched.root_rtts).ok_or_else(|| {
        TestFailure::Assertion(
            "no keep-alive ping round trip was observed on the root circuit".to_owned(),
        )
    })?;
    check(
        rtt < REPLY_TIMEOUT.as_secs_f64(),
        &format!("ping RTT {rtt:?} should be well under the reply timeout"),
    )?;

    let (low, high) = SIM_PING_INTERVAL;
    let root_interval = median(&gaps(&watched.root_pings())).ok_or_else(|| {
        TestFailure::Assertion("the simulator pinged the root circuit fewer than twice".to_owned())
    })?;
    check(
        (low..=high).contains(&root_interval),
        &format!("the simulator pings the root circuit every {root_interval:.2} s"),
    )?;
    let child_pings = watched.child_pings();
    let child_circuits = watched.child_circuits().len();
    check(
        child_pings.len() == child_circuits,
        &format!(
            "the simulators pinged {} of {child_circuits} child circuits",
            child_pings.len()
        ),
    )?;
    let child_gaps: Vec<f64> = child_pings
        .values()
        .flat_map(|offsets| gaps(offsets))
        .collect();
    match median(&child_gaps) {
        Some(interval) => check(
            (low..=high).contains(&interval),
            &format!("a child circuit is pinged every {interval:.2} s"),
        ),
        None => Ok(()),
    }
}

/// Watches both ends' keep-alive pings for 32 seconds.
///
/// Once a region is active the client sends a `StartPingCheck` on the root
/// circuit every few seconds — the reference viewer's circuit ping — and the
/// simulator answers with a `CompletePingCheck` echoing the ping id. The session
/// times that round trip and surfaces it as [`Event::Ping`], the "ping to sim" a
/// viewer displays. The simulator pings the client the same way, on the root
/// circuit and on each child circuit, and the session answers; with the
/// circuits probed ([`CircuitProbe::Observe`]) each of those pings is reported
/// as it arrives, so the case can count them and measure how far apart they
/// are.
///
/// Runs on every grid: the ping exchange is plain LLUDP, present on OpenSim,
/// Second Life and the offline fake grid alike.
#[derive(Debug)]
pub struct KeepalivePing;

impl GridTest for KeepalivePing {
    fn name(&self) -> &'static str {
        "keepalive-ping"
    }

    fn description(&self) -> &'static str {
        "Time the client's ping round trip and measure the simulator's own ping cadence"
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
            let watched = watch(session).await?;
            record(ctx.metrics(), &watched);
            check_pings(&watched)
        })
    }
}
