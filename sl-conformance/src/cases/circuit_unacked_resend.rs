//! Stop acknowledging the simulator's reliable packets and count how often,
//! and how far apart, it sends one again.

use std::time::{Duration, Instant};

use sl_client_tokio::{CircuitProbe, Command, Event};

use crate::circuit::{
    Seen, gaps, histogram, listen, median, seen_since, tally, timeline, transmissions,
};
use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, count_metric, secs_metric};

/// How long the session stays in world before the probe, so the arrival burst
/// has been delivered and acknowledged and what is resent afterwards is what
/// the probe provoked.
const SETTLE: Duration = Duration::from_secs(12);

/// How long acknowledgements are withheld.
const WITHHOLD: Duration = Duration::from_secs(45);

/// How long the circuit is given to answer a ping once acknowledgements flow
/// again.
const RECOVERY: Duration = Duration::from_secs(15);

/// The case's budget.
const CASE_TIMEOUT: Duration = Duration::from_secs(180);

/// Where the measured answers are written down.
const SOURCE: &str = "book/src/gridspec/session.md § Circuits (circuit-unacked-resend, 2026-10-06)";

/// How many times an unacknowledged reliable packet is sent before the
/// simulator gives it up: four on Second Life, and no limit on OpenSim, which
/// was still sending the 140th copy when the window closed.
const TRANSMISSIONS: Measured<Option<usize>> = Measured {
    second_life: Some(4),
    opensim: None,
    source: SOURCE,
};

/// The fewest transmissions that count as "never gives up" inside
/// [`WITHHOLD`]: far past any budget a grid could be counting to.
const UNBOUNDED_AT_LEAST: usize = 30;

/// The bounds on the gap between two transmissions, in seconds: a second on
/// Second Life (1.00–1.32 measured), a third of one on OpenSim (0.30–0.42).
const RESEND_INTERVAL: Measured<(f64, f64)> = Measured {
    second_life: (0.8, 1.6),
    opensim: (0.2, 0.8),
    source: SOURCE,
};

/// The reply whose retransmissions are counted: a reliable message the
/// simulator sends once, on request, on every grid.
const REPLY: &str = "RegionInfo";

/// What became of the one reply nobody acknowledged.
#[derive(Debug)]
struct Resends {
    /// When each transmission of it arrived, the first included.
    offsets: Vec<f64>,
    /// How many of its retransmissions carried the `RESENT` flag.
    flagged: usize,
    /// How many times it came under another sequence number instead.
    renumbered: usize,
}

impl Resends {
    /// Read the [`REPLY`]'s transmissions out of `seen`.
    fn of(seen: &[Seen]) -> Result<Self, TestFailure> {
        let reply: Vec<&Seen> = seen
            .iter()
            .filter(|datagram| !datagram.child && datagram.is(REPLY))
            .collect();
        let first = reply
            .first()
            .ok_or_else(|| TestFailure::Assertion(format!("the simulator never sent a {REPLY}")))?;
        let same: Vec<&Seen> = reply
            .iter()
            .copied()
            .filter(|datagram| datagram.sequence == first.sequence)
            .collect();
        Ok(Self {
            offsets: same.iter().map(|datagram| datagram.offset).collect(),
            flagged: same
                .iter()
                .skip(1)
                .filter(|datagram| datagram.resent)
                .count(),
            renumbered: reply.len().saturating_sub(same.len()),
        })
    }

    /// How many retransmissions there were.
    const fn retransmissions(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }

    /// The median gap between two transmissions, in seconds.
    fn interval(&self) -> Option<f64> {
        median(&gaps(&self.offsets))
    }
}

/// Record the reply's `resends` and the reliable layer as a whole over the
/// window `seen` covers.
fn record(metrics: &mut Metrics, seen: &[Seen], resends: &Resends) {
    // Which messages were sent reliably at all, and which of them a second
    // time: a grid may give some of its reliable traffic up after one
    // transmission and send the state again instead.
    let first_sends = seen
        .iter()
        .filter(|datagram| datagram.reliable && !datagram.resent);
    let later_sends = seen
        .iter()
        .filter(|datagram| datagram.reliable && datagram.resent);
    metrics.set("reliable_by_message", histogram(first_sends));
    metrics.set("resent_by_message", histogram(later_sends));

    let by_packet = transmissions(seen);
    let most = by_packet.values().map(Vec::len).max().unwrap_or(0);
    let resent_packets = by_packet.values().filter(|sent| sent.len() > 1).count();
    metrics.set(&count_metric("reliable_packets"), tally(by_packet.len()));
    metrics.set(
        &count_metric("reliable_packets_resent"),
        tally(resent_packets),
    );
    metrics.set(
        &count_metric("most_transmissions_of_one_packet"),
        tally(most),
    );

    metrics.set(
        &count_metric("reply_transmissions"),
        tally(resends.offsets.len()),
    );
    metrics.set("reply_transmission_offsets", timeline(&resends.offsets));
    metrics.set("reply_transmission_gaps", timeline(&gaps(&resends.offsets)));
    if let Some(gap) = resends.interval() {
        metrics.set(&secs_metric("reply_resend_interval"), gap);
    }
    if let Some(last) = resends.offsets.last() {
        metrics.set(&secs_metric("reply_last_transmission"), *last);
    }
    metrics.set(
        &count_metric("reply_retransmissions_flagged_resent"),
        tally(resends.flagged),
    );
    metrics.set(
        &count_metric("reply_sent_under_another_sequence"),
        tally(resends.renumbered),
    );
}

/// Hold the reply's `resends` to what `grid` was measured doing.
fn check_resends(grid: Grid, resends: &Resends) -> Result<(), TestFailure> {
    let sent = resends.offsets.len();
    check(
        resends.flagged == resends.retransmissions() && resends.renumbered == 0,
        &format!(
            "{} of {} retransmissions carried the RESENT flag and {} came under another \
             sequence number",
            resends.flagged,
            resends.retransmissions(),
            resends.renumbered
        ),
    )?;
    match TRANSMISSIONS.on(grid) {
        Some(_) => TRANSMISSIONS.check(
            "how many times an unacknowledged packet is sent",
            grid,
            &Some(sent),
        )?,
        None => check(
            sent >= UNBOUNDED_AT_LEAST,
            &format!(
                "the unacknowledged {REPLY} was sent {sent} times and then given up; this \
                 grid never gives one up ({SOURCE})"
            ),
        )?,
    }
    let (low, high) = *RESEND_INTERVAL.on(grid);
    let interval = resends.interval().unwrap_or(0.0);
    check(
        (low..=high).contains(&interval),
        &format!("the {REPLY} was resent every {interval:.2} s, outside {low}–{high} s ({SOURCE})"),
    )
}

/// Withholds every acknowledgement for 45 seconds and records what the
/// simulator does with one reliable reply nobody acknowledged.
///
/// Everything else about the circuit goes on — the session answers the
/// simulator's pings, sends its own and its `AgentUpdate`s — so this is the
/// simulator's reliable layer on a live circuit, not its treatment of a client
/// that has gone ([`circuit-silence`](super::circuit_silence) is that).
#[derive(Debug)]
pub struct CircuitUnackedResend;

impl GridTest for CircuitUnackedResend {
    fn name(&self) -> &'static str {
        "circuit-unacked-resend"
    }

    fn description(&self) -> &'static str {
        "Withhold acknowledgements; count and time the simulator's retransmissions"
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
            if let Some(reason) = listen(session, SETTLE, |_event| {}).await? {
                return Err(TestFailure::Disconnected(format!("{reason:?}")));
            }

            let already = session.diagnostics().len();
            let started = Instant::now();
            session
                .send(Command::ProbeCircuits(CircuitProbe::WithholdAcks))
                .await?;
            session.send(Command::RequestRegionInfo).await?;
            let ended = listen(session, WITHHOLD, |_event| {}).await?;
            let seen = seen_since(session, already, started);
            let resends = Resends::of(&seen)?;

            let metrics = ctx.metrics();
            record(metrics, &seen, &resends);
            metrics.set(
                "disconnected_while_withholding",
                ended
                    .as_ref()
                    .map_or_else(|| "no".to_owned(), |reason| format!("{reason:?}")),
            );
            if let Some(reason) = ended {
                return Err(TestFailure::Disconnected(format!("{reason:?}")));
            }

            // Acknowledge again: does the circuit carry on?
            let session = ctx.primary();
            session
                .send(Command::ProbeCircuits(CircuitProbe::Off))
                .await?;
            let mut answered = false;
            let after = listen(session, RECOVERY, |event| {
                if matches!(event, Event::Ping { child: false, .. }) {
                    answered = true;
                }
            })
            .await?;
            let alive = answered && after.is_none();
            ctx.metrics().set("circuit_alive_afterwards", alive);
            check(
                alive,
                "the circuit did not carry on once acknowledgements flowed again",
            )?;
            check_resends(ctx.grid(), &resends)
        })
    }
}
