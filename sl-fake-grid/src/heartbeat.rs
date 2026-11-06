//! The region heartbeat: one loop per region, ticking whether or not anybody
//! is standing in it.
//!
//! Everything else in the grid is driven by a client: a session that receives
//! a message changes the world and publishes the change. What belongs to the
//! *region* rather than to any one session — a script's timer, physics, a
//! `SimStats` that reports how the region is keeping up — needs a clock of its
//! own that outlives every session, and this is it.
//!
//! Each tick, in order:
//!
//! 1. the region clock advances by exactly one [step](HeartbeatConfig::step),
//!    never by the time measured since the last tick, so the region's notion
//!    of elapsed time is `ticks × step` on every machine;
//! 2. the region's scripts run their slice (`scripts::run_tick`):
//!    the instances are brought in line with what the prims hold, every
//!    runnable one runs its share of the tick, and what they said and did
//!    reaches the region — what they changed in its prims as one update
//!    burst ([`crate::updates`]), a terse update for a prim that only moved;
//! 3. every root agent's avatar moves one step (`movement::advance`): what
//!    its viewer's held keys ask for, kept on the ground, reported back at
//!    the imitated grid's cadence, and the parcel it walks onto pushed. It
//!    writes no prim, so it runs outside the scripts' update bracket;
//! 4. when the imitated grid's statistics are due, every root agent in the
//!    region is sent a `SimStats` whose time dilation says whether the steps
//!    kept up with the grid's clock ([`TimeDilation`]), with the region's
//!    active scripts and the share of them that ran.
//!
//! The rest of the work the loop exists for — physics — slots in beside these
//! as it is built, inside the same update bracket as the scripts' slice.
//!
//! # The step
//!
//! A live simulator steps its physics at 45 frames a second and is measured
//! at 45 (Second Life) and 55 (OpenSim). The fake grid does not copy that: its
//! [default step](DEFAULT_STEP) is a tenth of a second, chosen for what a test
//! run costs — a grid in every test of a suite ticking at 45 Hz is a lot of
//! wake-ups for nothing — while still finer than any cadence the grid reports
//! (statistics every 2 or 3 s). The rate a region reports in its statistics is
//! the imitated grid's own, scaled by the dilation; the step is not visible on
//! the wire. A test that wants a coarse clock, or a cross-check that wants a
//! fine one, sets [`RegionConfig::heartbeat`](crate::RegionConfig::heartbeat).
//!
//! # Driving it from a test
//!
//! A [`Stepped`](HeartbeatMode::Stepped) region never ticks on its own: a test
//! calls [`FakeGrid::advance_one_tick`](crate::FakeGrid::advance_one_tick) and
//! asserts on what that tick did, instead of sleeping and hoping. A stepped
//! region always keeps up — its clock is the test's — so its dilation is
//! [`TimeDilation::NORMAL`].
//!
//! # The lock order
//!
//! The crate's rule is session lock, then region lock, never the reverse. The
//! heartbeat starts from the region and has to reach the sessions to send, so
//! it never holds the region's world while it does: a tick serialises on its
//! own pulse lock, which no session path takes, reads the session table and
//! lets it go, and only then locks each session in turn (in session order, so
//! two runs send in the same order). The script slice keeps the same rule: it
//! runs the scripts under the region's lock and sends what they said to the
//! sessions after letting it go.

use std::time::{Duration, Instant};

use sl_proto::{RegionLoad, TimeDilation};
use tokio::sync::{Mutex, watch};

use crate::runtime::GridCore;

/// The step a region's clock advances by each tick unless its
/// [`HeartbeatConfig`] says otherwise: a tenth of a second.
pub const DEFAULT_STEP: Duration = Duration::from_millis(100);

/// Whether a region's heartbeat ticks by itself.
#[expect(
    clippy::module_name_repetitions,
    reason = "the name is read at its use sites -- `sl_fake_grid::HeartbeatMode`, \
              `RegionConfig::heartbeat` -- where a bare `Mode` would not say of what"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeartbeatMode {
    /// A task ticks the region every step of the grid's clock.
    #[default]
    Running,
    /// Nothing ticks the region but
    /// [`FakeGrid::advance_one_tick`](crate::FakeGrid::advance_one_tick).
    Stepped,
}

/// How a region's heartbeat runs ([`RegionConfig::heartbeat`](crate::RegionConfig::heartbeat)).
#[expect(
    clippy::module_name_repetitions,
    reason = "the name is read at its use sites -- `sl_fake_grid::HeartbeatConfig` beside \
              `RegionConfig` -- where a bare `Config` would not say of what"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeartbeatConfig {
    /// How far the region clock advances each tick. Must not be zero.
    pub step: Duration,
    /// Whether the region ticks by itself or only when a test says so.
    pub mode: HeartbeatMode,
}

impl HeartbeatConfig {
    /// A region ticking by itself every `step`.
    #[must_use]
    pub const fn running(step: Duration) -> Self {
        Self {
            step,
            mode: HeartbeatMode::Running,
        }
    }

    /// A region that ticks only when a test advances it, by `step` each time.
    #[must_use]
    pub const fn stepped(step: Duration) -> Self {
        Self {
            step,
            mode: HeartbeatMode::Stepped,
        }
    }
}

impl Default for HeartbeatConfig {
    /// A region ticking by itself every [`DEFAULT_STEP`].
    fn default() -> Self {
        Self::running(DEFAULT_STEP)
    }
}

/// What one tick of a region's heartbeat did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tick {
    /// Which tick this was: the first is `1`, and `0` is a region that has not
    /// ticked yet.
    pub number: u64,
    /// The region clock after the tick: `number × step`.
    pub region_time: Duration,
    /// The statistics the tick sent, or `None` on a tick they were not due.
    pub stats: Option<TickStats>,
}

/// The `SimStats` a tick sent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TickStats {
    /// The time dilation they reported.
    pub dilation: TimeDilation,
    /// How many root agents in the region they were sent to.
    pub sent_to: usize,
}

/// One region's heartbeat: its configuration, its clock and where its ticks
/// are published.
pub(crate) struct Heartbeat {
    /// How the region ticks.
    config: HeartbeatConfig,
    /// How many ticks make one statistics period: the imitated grid's
    /// interval in steps, rounded up, and at least one.
    ticks_per_stats: u64,
    /// The clock, behind the lock every tick serialises on.
    pulse: Mutex<Pulse>,
    /// The latest tick, for whoever watches the region.
    ticks: watch::Sender<Tick>,
}

/// A heartbeat's running state.
struct Pulse {
    /// The number of the last tick.
    number: u64,
    /// The tick the last statistics went out on (`0` before the first).
    stats_tick: u64,
    /// The grid's clock when they did, or when the heartbeat started.
    stats_at: Instant,
}

impl Heartbeat {
    /// A heartbeat that has not ticked yet, started at `now`, reporting
    /// statistics every `stats_interval`.
    pub(crate) fn new(config: HeartbeatConfig, stats_interval: Duration, now: Instant) -> Self {
        let (ticks, _) = watch::channel(Tick {
            number: 0,
            region_time: Duration::ZERO,
            stats: None,
        });
        Self {
            config,
            ticks_per_stats: ticks_per_stats(config.step, stats_interval),
            pulse: Mutex::new(Pulse {
                number: 0,
                stats_tick: 0,
                stats_at: now,
            }),
            ticks,
        }
    }

    /// Watches the region's ticks.
    pub(crate) fn subscribe(&self) -> watch::Receiver<Tick> {
        self.ticks.subscribe()
    }
}

/// How many steps make up `interval`, rounded up, and at least one.
fn ticks_per_stats(step: Duration, interval: Duration) -> u64 {
    let step = step.as_nanos().max(1);
    u64::try_from(interval.as_nanos().div_ceil(step))
        .unwrap_or(u64::MAX)
        .max(1)
}

/// `step × ticks`, saturating.
fn times(step: Duration, ticks: u64) -> Duration {
    let nanos = step.as_nanos().saturating_mul(u128::from(ticks));
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

/// Runs one tick of the region at `index`, returning what it did, or `None`
/// for a region the grid does not have.
pub(crate) async fn beat(core: &GridCore, index: usize) -> Option<Tick> {
    let heartbeat = &core.region(index)?.heartbeat;
    let mut pulse = heartbeat.pulse.lock().await;
    let now = (core.clock)();
    pulse.number = pulse.number.saturating_add(1);
    let number = pulse.number;
    let _scripts = crate::scripts::run_tick(core, index).await;
    move_agents(core, index, heartbeat.config.step, now).await;
    let since = number.saturating_sub(pulse.stats_tick);
    let stats = if since >= heartbeat.ticks_per_stats {
        let dilation = match heartbeat.config.mode {
            HeartbeatMode::Stepped => TimeDilation::NORMAL,
            HeartbeatMode::Running => TimeDilation::from_ratio(
                times(heartbeat.config.step, since),
                now.saturating_duration_since(pulse.stats_at),
            ),
        };
        pulse.stats_tick = number;
        pulse.stats_at = now;
        let sent_to = send_stats(core, index, dilation, now).await;
        Some(TickStats { dilation, sent_to })
    } else {
        None
    };
    let tick = Tick {
        number,
        region_time: times(heartbeat.config.step, number),
        stats,
    };
    heartbeat.ticks.send_replace(tick);
    drop(pulse);
    Some(tick)
}

/// Moves every root agent's avatar in the region at `index` by one `step`
/// ([`crate::movement`]), in session order, each session locked in turn.
async fn move_agents(core: &GridCore, index: usize, step: Duration, now: Instant) {
    for shared in sessions_in_order(core).await {
        if shared.is_closed() {
            continue;
        }
        shared
            .with_state(|state| {
                if state.region == index {
                    crate::movement::advance(state, step, now);
                }
            })
            .await;
    }
}

/// The grid's sessions in session order, the table read and let go.
async fn sessions_in_order(core: &GridCore) -> Vec<crate::driver::SharedSim> {
    let mut sessions: Vec<_> = core
        .sessions
        .lock()
        .await
        .iter()
        .map(|(seq, shared)| (*seq, shared.clone()))
        .collect();
    sessions.sort_unstable_by_key(|(seq, _)| *seq);
    sessions.into_iter().map(|(_, shared)| shared).collect()
}

/// Sends the region's statistics to every root agent in the region at
/// `index`, in session order, and counts them.
async fn send_stats(core: &GridCore, index: usize, dilation: TimeDilation, now: Instant) -> usize {
    // What the region's scripts report, read and let go before any session
    // is locked; a region with none says what its telemetry says.
    let scripts = core.region(index).and_then(|region| {
        let world = region.world.lock();
        let load = (!world.script_instances.is_empty()).then(|| world.script_instances.load());
        drop(world);
        load
    });
    let load = scripts.map_or_else(
        || RegionLoad::dilated(dilation),
        |(active, percent)| RegionLoad {
            dilation,
            active_scripts: Some(active),
            scripts_run_percent: Some(percent),
        },
    );
    let mut sent_to = 0_usize;
    for shared in sessions_in_order(core).await {
        if shared.is_closed() {
            continue;
        }
        let sent = shared
            .with_state(|state| {
                if state.region != index {
                    return false;
                }
                state
                    .sim
                    .send_region_stats(load, now)
                    .unwrap_or_else(|error| {
                        tracing::warn!(%error, "failed to send the region's statistics");
                        false
                    })
            })
            .await;
        if sent {
            sent_to = sent_to.saturating_add(1);
        }
    }
    sent_to
}

/// Ticks the region at `index` every step of its clock until the grid shuts
/// down; returns at once for a [stepped](HeartbeatMode::Stepped) region.
///
/// A tick that comes due while the last one is still running is skipped, not
/// queued: a region that fell behind runs fewer ticks in the time, which is
/// what its time dilation then reports.
pub(crate) async fn run_heartbeat(core: std::sync::Arc<GridCore>, index: usize) {
    let Some(config) = core.region(index).map(|region| region.heartbeat.config) else {
        return;
    };
    if matches!(config.mode, HeartbeatMode::Stepped) {
        return;
    }
    let mut shutdown = core.shutdown_tx.subscribe();
    let start = tokio::time::Instant::now();
    let mut interval =
        tokio::time::interval_at(start.checked_add(config.step).unwrap_or(start), config.step);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                // What the tick did is for a test's `advance_one_tick` and
                // the region's watchers; the loop has no use for it.
                let _tick = beat(&core, index).await;
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use pretty_assertions::assert_eq;

    use super::{ticks_per_stats, times};

    /// A statistics period is whole steps, rounded up, and never none.
    #[test]
    fn a_statistics_period_is_whole_steps() {
        let tenth = Duration::from_millis(100);
        assert_eq!(ticks_per_stats(tenth, Duration::from_secs(2)), 20);
        assert_eq!(ticks_per_stats(tenth, Duration::from_millis(2_050)), 21);
        assert_eq!(
            ticks_per_stats(Duration::from_secs(5), Duration::from_secs(3)),
            1
        );
        assert_eq!(ticks_per_stats(tenth, Duration::ZERO), 1);
    }

    /// The region clock is ticks times the step, never measured.
    #[test]
    fn the_region_clock_is_ticks_times_the_step() {
        assert_eq!(
            times(Duration::from_millis(100), 37),
            Duration::from_millis(3_700)
        );
        assert_eq!(times(Duration::from_millis(100), 0), Duration::ZERO);
    }
}
