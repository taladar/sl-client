//! Go silent — transmit nothing at all — and record what the simulator does
//! about a client that has vanished: how long it keeps pinging and resending,
//! what it says when it gives the circuit up, and whether the avatar can log
//! in again afterwards.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{CircuitProbe, Command, DisconnectReason, Event};

use crate::circuit::{
    Seen, gaps, histogram, listen, median, offsets_of, seen_since, tally, timeline, transmissions,
};
use crate::context::{LoginAnswer, LoginAttempt, Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, count_metric, secs_metric};

/// How long the session stays in world before it goes silent, so the arrival
/// burst is over and the neighbours' child circuits are open.
const SETTLE: Duration = Duration::from_secs(12);

/// The longest the session stays silent.
const SILENCE_LIMIT: Duration = Duration::from_secs(200);

/// How long the root circuit must have been quiet before the simulator is
/// taken to have stopped sending on it. Under the session's own 45-second
/// inactivity timeout, so the session is still there to ask.
const QUIET: Duration = Duration::from_secs(30);

/// How long the listening is done in one piece.
const STEP: Duration = Duration::from_secs(1);

/// How long the circuit is given to answer a ping once the session transmits
/// again.
const RECOVERY: Duration = Duration::from_secs(15);

/// How long after the circuit is gone the next login is made: long enough
/// that it asks whether the grid let the avatar go, not whether it had
/// finished doing so in the same instant. (OpenSim, asked in that instant,
/// refuses the login as `presence`; see the book.)
const NEXT_LOGIN_DELAY: Duration = Duration::from_secs(5);

/// How many logins are tried after the silence before the case gives up on
/// getting the avatar back in.
const NEXT_LOGIN_ATTEMPTS: u32 = 4;

/// How long a refused login waits before the next one.
const NEXT_LOGIN_RETRY: Duration = Duration::from_secs(5);

/// The case's budget: the silence, and on a grid that rate-limits logins the
/// cooldown waited out before the login that follows it.
const CASE_TIMEOUT: Duration = Duration::from_secs(14 * 60);

/// Where the measured answers are written down.
const SOURCE: &str = "book/src/gridspec/session.md § Circuits (circuit-silence, 2026-10-06)";

/// The bounds on when the simulator sends its last datagram to a silent
/// client, in seconds of silence: 97.5–99 on Second Life, whose circuit
/// timeout is 100 s from the last datagram it received; 59.1 on OpenSim, whose
/// `AckTimeout` is 60.
const LAST_DATAGRAM: Measured<(f64, f64)> = Measured {
    second_life: (92.0, 104.0),
    opensim: (55.0, 64.0),
    source: SOURCE,
};

/// What the simulator says as it gives the circuit up: nothing on Second
/// Life, a `KickUser` with this reason on OpenSim.
const TIMEOUT_KICK: Measured<Option<&str>> = Measured {
    second_life: None,
    opensim: Some("Simulator logged you out due to connection timeout."),
    source: SOURCE,
};

/// Whether the avatar can log in again five seconds after the circuit is
/// gone.
const NEXT_LOGIN_ADMITTED: Measured<bool> = Measured {
    second_life: true,
    opensim: true,
    source: SOURCE,
};

/// What [`speak_again`] reports of a circuit that survived the silence.
const ANSWERS: &str = "the circuit answers";

/// The fewest pings a simulator sends a silent client before it gives up:
/// one every five seconds for at least fifty.
const FEWEST_PINGS: usize = 10;

/// How many of `seen` arrived in each ten seconds of the silence, as one
/// metric string — the shape of the simulator letting go.
fn per_ten_seconds(seen: &[&Seen]) -> String {
    let mut buckets: Vec<usize> = Vec::new();
    for datagram in seen {
        let mut bucket = 0_usize;
        let mut edge = 10.0;
        while datagram.offset >= edge {
            bucket = bucket.saturating_add(1);
            edge += 10.0;
        }
        if buckets.len() <= bucket {
            buckets.resize(bucket.saturating_add(1), 0);
        }
        if let Some(count) = buckets.get_mut(bucket) {
            *count = count.saturating_add(1);
        }
    }
    buckets
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

/// What one silence looked like from the client.
#[derive(Debug)]
struct Silence {
    /// Every datagram the simulators sent during it.
    seen: Vec<Seen>,
    /// How long the session transmitted nothing, in seconds.
    lasted: f64,
    /// How the session ended, if it did while silent.
    ended: Option<DisconnectReason>,
    /// The reason of the `KickUser` the simulator sent, if it sent one.
    kick: Option<String>,
}

impl Silence {
    /// The datagrams of the root circuit.
    fn root(&self) -> Vec<&Seen> {
        self.seen
            .iter()
            .filter(|datagram| !datagram.child)
            .collect()
    }

    /// The datagrams of the child circuits.
    fn child(&self) -> Vec<&Seen> {
        self.seen.iter().filter(|datagram| datagram.child).collect()
    }

    /// When the root circuit's last datagram came, in seconds of silence.
    fn last_root(&self) -> f64 {
        self.root().last().map_or(0.0, |datagram| datagram.offset)
    }
}

/// Transmit nothing until the session is ended, the root circuit has been
/// quiet for [`QUIET`], or [`SILENCE_LIMIT`].
async fn stay_silent(session: &mut Session) -> Result<Silence, TestFailure> {
    let already = session.diagnostics().len();
    let started = Instant::now();
    session
        .send(Command::ProbeCircuits(CircuitProbe::Silent))
        .await?;

    let mut kick = None;
    let mut ended = None;
    while started.elapsed() < SILENCE_LIMIT {
        ended = listen(session, STEP, |event| {
            if let Event::Kicked(details) = event {
                kick = Some(details.reason.clone());
            }
        })
        .await?;
        if ended.is_some() {
            break;
        }
        let last_root = seen_since(session, already, started)
            .iter()
            .rev()
            .find(|datagram| !datagram.child)
            .map_or(0.0, |datagram| datagram.offset);
        if started.elapsed().as_secs_f64() - last_root >= QUIET.as_secs_f64() {
            break;
        }
    }
    Ok(Silence {
        seen: seen_since(session, already, started)
            .into_iter()
            .filter(|datagram| datagram.offset >= 0.0)
            .collect(),
        lasted: started.elapsed().as_secs_f64(),
        ended,
        kick,
    })
}

/// Record what the root circuit carried during `silence`.
fn record_root(metrics: &mut Metrics, silence: &Silence) {
    let root = silence.root();
    let pings = offsets_of(root.iter().copied(), "StartPingCheck");
    let tail: Vec<&Seen> = root.iter().rev().take(4).rev().copied().collect();
    metrics.set(&count_metric("root_datagrams"), tally(root.len()));
    if let Some(last) = root.last() {
        metrics.set(&secs_metric("root_last_datagram"), last.offset);
    }
    metrics.set("root_datagrams_per_10s", per_ten_seconds(&root));
    metrics.set("root_messages", histogram(root.iter().copied()));
    metrics.set(
        "root_resent_messages",
        histogram(root.iter().copied().filter(|datagram| datagram.resent)),
    );
    metrics.set(
        "root_last_messages",
        tail.iter()
            .map(|datagram| format!("{}@{:.2}", datagram.name.unwrap_or("?"), datagram.offset))
            .collect::<Vec<_>>()
            .join(" "),
    );
    metrics.set(&count_metric("root_pings"), tally(pings.len()));
    metrics.set("root_ping_offsets", timeline(&pings));
    if let Some(gap) = median(&gaps(&pings)) {
        metrics.set(&secs_metric("root_ping_interval"), gap);
    }
}

/// Record what the child circuits carried during `silence`, and the reliable
/// layer and the ending of all of them.
fn record_rest(metrics: &mut Metrics, silence: &Silence) {
    let child = silence.child();
    let circuits: BTreeSet<_> = child.iter().map(|datagram| datagram.from).collect();
    metrics.set(&count_metric("child_circuits"), tally(circuits.len()));
    metrics.set("child_datagrams_per_10s", per_ten_seconds(&child));
    if let Some(last) = child.last() {
        metrics.set(&secs_metric("child_last_datagram"), last.offset);
    }
    metrics.set("child_messages", histogram(child.iter().copied()));
    if let Some(last) = offsets_of(child.iter().copied(), "StartPingCheck").last() {
        metrics.set(&secs_metric("child_last_ping"), *last);
    }

    let by_packet = transmissions(&silence.seen);
    let most = by_packet.values().map(Vec::len).max().unwrap_or(0);
    let last_resend = by_packet
        .values()
        .filter(|sent| sent.len() > 1)
        .filter_map(|sent| sent.last().copied())
        .max_by(f64::total_cmp);
    metrics.set(&count_metric("reliable_packets"), tally(by_packet.len()));
    metrics.set(
        &count_metric("most_transmissions_of_one_packet"),
        tally(most),
    );
    if let Some(last) = last_resend {
        metrics.set(&secs_metric("last_retransmission"), last);
    }

    metrics.set(&secs_metric("silent_for"), silence.lasted);
    metrics.set(
        "session_ended",
        silence
            .ended
            .as_ref()
            .map_or_else(|| "no".to_owned(), |reason| format!("{reason:?}")),
    );
    if silence.ended.is_some() {
        metrics.set(&secs_metric("session_ended_at"), silence.lasted);
    }
    metrics.set(
        "kick",
        silence.kick.clone().unwrap_or_else(|| "none".to_owned()),
    );
}

/// Hold `silence` to what `grid` was measured doing about one.
fn check_silence(grid: Grid, silence: &Silence) -> Result<(), TestFailure> {
    let (low, high) = *LAST_DATAGRAM.on(grid);
    let last_root = silence.last_root();
    check(
        (low..=high).contains(&last_root),
        &format!(
            "the simulator's last datagram came {last_root:.1} s into the silence, \
             outside {low}–{high} s ({SOURCE})"
        ),
    )?;
    let pings = offsets_of(silence.root(), "StartPingCheck").len();
    check(
        pings >= FEWEST_PINGS,
        &format!("the simulator pinged a silent client {pings} times"),
    )?;
    TIMEOUT_KICK.check(
        "what the simulator says as it gives the circuit up",
        grid,
        &silence.kick.as_deref(),
    )
}

/// Transmit again on a session that was not ended, and say what came of it:
/// whether the circuit answered a ping, or how the session ended meanwhile.
async fn speak_again(session: &mut Session) -> Result<String, TestFailure> {
    session
        .send(Command::ProbeCircuits(CircuitProbe::Off))
        .await?;
    let mut answers = false;
    let after = listen(session, RECOVERY, |event| {
        if matches!(event, Event::Ping { child: false, .. }) {
            answers = true;
        }
    })
    .await?;
    Ok(match (after, answers) {
        (Some(reason), _) => format!("ended: {reason:?}"),
        (None, true) => ANSWERS.to_owned(),
        (None, false) => "no answer".to_owned(),
    })
}

/// Log the avatar in again once its circuit is gone, and hold the first
/// answer to the measured one. A refusal is retried, so the case leaves
/// nobody in world — or half in it — behind.
async fn log_in_again(ctx: &mut TestContext) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    ctx.primary().disconnect().await?;
    tokio::time::sleep(NEXT_LOGIN_DELAY).await;
    for attempt in 1..=NEXT_LOGIN_ATTEMPTS {
        let answer = ctx
            .primary()
            .attempt_login(&LoginAttempt::default())
            .await?;
        match answer {
            LoginAnswer::Admitted(mut next) => {
                let metrics = ctx.metrics();
                if attempt == 1 {
                    metrics.set("next_login", "admitted");
                }
                metrics.set(&count_metric("next_login_attempts"), attempt);
                next.wait_for_region(REGION_TIMEOUT).await?;
                next.logout().await?;
                return NEXT_LOGIN_ADMITTED.check(
                    "whether the login after a dropped circuit gets in",
                    grid,
                    &(attempt == 1),
                );
            }
            LoginAnswer::Refused { failure, .. } => {
                if attempt == 1 {
                    let metrics = ctx.metrics();
                    metrics.set("next_login", format!("refused: {}", failure.reason));
                    metrics.set("next_login_message", failure.message);
                }
                tokio::time::sleep(NEXT_LOGIN_RETRY).await;
            }
            LoginAnswer::Challenged(challenge) => {
                return Err(TestFailure::Assertion(format!(
                    "the login after the silence was challenged: {}",
                    challenge.message
                )));
            }
        }
    }
    Err(TestFailure::Assertion(format!(
        "the avatar was still refused after {NEXT_LOGIN_ATTEMPTS} logins"
    )))
}

/// Goes silent and records the simulator's side of it.
///
/// The silence is of the circuits only: the event queue is the driver's HTTP
/// and goes on being polled, as a real viewer's would not if the machine had
/// gone. What a grid does about a client whose UDP alone has stopped is what
/// is measured.
#[derive(Debug)]
pub struct CircuitSilence;

impl GridTest for CircuitSilence {
    fn name(&self) -> &'static str {
        "circuit-silence"
    }

    fn description(&self) -> &'static str {
        "Transmit nothing; record the simulator's pings, resends and how it ends the circuit"
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

            let silence = stay_silent(session).await?;
            let metrics = ctx.metrics();
            record_root(metrics, &silence);
            record_rest(metrics, &silence);
            check_silence(ctx.grid(), &silence)?;

            // Speak again: is anybody still there?
            if silence.ended.is_none() {
                let after = speak_again(ctx.primary()).await?;
                ctx.metrics().set("after_silence", after.clone());
                if after == ANSWERS {
                    return Err(TestFailure::Assertion(format!(
                        "the circuit still answered after {:.0} s of silence ({SOURCE})",
                        silence.lasted
                    )));
                }
            }

            // The circuit is gone. Did the grid let go of the avatar with it?
            log_in_again(ctx).await
        })
    }
}
