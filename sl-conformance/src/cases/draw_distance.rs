//! Change the draw distance and watch the grid open and close the neighbouring
//! regions' child agents in answer.
//!
//! A viewer advertises its draw distance in the `Far` field of every
//! `AgentUpdate`. A simulator reads it as how far the agent can see, and holds
//! a child agent in each neighbouring region that distance reaches:
//! `EnableSimulator` announces one ([`Event::NeighborDiscovered`]),
//! `DisableSimulator` on the child circuit takes it away again
//! ([`Event::NeighborRetired`]).
//!
//! The two grids read "reaches" differently
//! (`book/src/gridspec/teleport.md`, *Neighbours and crossings*):
//!
//! - **OpenSim** adds 64 m to the draw distance, clamps the sum to 96–255 m,
//!   and holds every region a square of that half-width around the *avatar*
//!   touches. It answers a change within a second, either way.
//! - **Second Life** compares the draw distance with a fixed figure per
//!   neighbour: 128 m for a region sharing an edge, 128·√2 m for one touching
//!   at a corner (measured from a spot eight metres from two borders, so not
//!   a distance to anything). It announces a neighbour within a second of the
//!   distance reaching it, and retires one fifty seconds after it stopped.
//!
//! So the case steps the draw distance down and up again and holds each grid
//! to its own answer at each step: 100 m from the middle of a region retires
//! every neighbour on Second Life and none on OpenSim; 32 m retires them on
//! both; 512 m brings them all back.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Distance, Event, RegionHandle};

use crate::context::{Session, TestContext, TestFailure};
use crate::crossing::{slot_offset, tally, watch_neighbours};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, check_eq, is_opensim, secs_metric};

/// Where the measured answers are written down.
const SOURCE: &str =
    "book/src/gridspec/teleport.md § Neighbours and crossings (draw-distance, 2026-10-07)";

/// Where the avatar logs in on OpenSim: the middle of a region, 128 m from
/// every border — the one spot where OpenSim's answer depends on the draw
/// distance alone.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// How long to watch the arrival for its neighbour announcements.
const NEIGHBOUR_WINDOW: Duration = Duration::from_secs(30);

/// The gap without a new announcement that ends that watch.
const NEIGHBOUR_QUIET: Duration = Duration::from_secs(6);

/// A draw distance that reaches no neighbour on Second Life and — from the
/// middle of a region — every neighbour on OpenSim.
const BETWEEN_M: f64 = 100.0;

/// A draw distance that reaches no neighbour on either grid.
const NEAR_M: f64 = 32.0;

/// A draw distance that reaches every neighbour on both.
const FAR_M: f64 = 512.0;

/// How long a step waits for a grid to retire its neighbours. Second Life
/// takes fifty seconds.
const RETIRE_WINDOW: Duration = Duration::from_secs(75);

/// How long a step waits for a grid to announce them again.
const ANNOUNCE_WINDOW: Duration = Duration::from_secs(20);

/// How long a step that is expected to change nothing watches for a change.
/// Past OpenSim's second; a Second Life retirement would take longer, but
/// there the step is expected to retire.
const UNCHANGED_WINDOW: Duration = Duration::from_secs(15);

/// A retirement that takes longer than this was not prompt.
const PROMPT: Duration = Duration::from_secs(10);

/// The case's overall budget: a neighbour watch and three steps, one of them
/// Second Life's fifty-second retirement.
const CASE_TIMEOUT: Duration = Duration::from_secs(6 * 60);

/// Whether a draw distance of 100 m, from the middle of a region, retires the
/// neighbours.
const HUNDRED_METRES_RETIRES: Measured<bool> = Measured {
    second_life: true,
    opensim: false,
    source: SOURCE,
};

/// Whether a neighbour the draw distance stopped reaching is retired within
/// ten seconds.
const RETIRES_PROMPTLY: Measured<bool> = Measured {
    second_life: false,
    opensim: true,
    source: SOURCE,
};

/// Steps the draw distance and holds the grid to what it does with its
/// neighbours at each step.
#[derive(Debug)]
pub struct DrawDistance;

impl GridTest for DrawDistance {
    fn name(&self) -> &'static str {
        "draw-distance"
    }

    fn description(&self) -> &'static str {
        "Step the draw distance and watch the grid retire and announce neighbouring regions"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let agent = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
            let home = session.region_handle().ok_or_else(|| {
                TestFailure::Assertion("login established no region handle".to_owned())
            })?;

            let found = watch_neighbours(session, agent, NEIGHBOUR_WINDOW, NEIGHBOUR_QUIET).await?;
            let held: Vec<RegionHandle> = found
                .neighbours
                .iter()
                .map(|neighbour| neighbour.info.region_handle)
                .collect();
            if held.is_empty() {
                // A region with nothing next to it: there is no neighbour for
                // any draw distance to reach.
                found.record("neighbours_", Some(home), ctx.metrics());
                ctx.mark_partial("the region the avatar stands in announced no neighbours");
                return Ok(());
            }
            let all = held.len();

            // 100 m: between the two grids' answers.
            let expected = if *HUNDRED_METRES_RETIRES.on(grid) {
                0
            } else {
                all
            };
            let between = step(session, BETWEEN_M, all, expected).await?;
            HUNDRED_METRES_RETIRES.check(
                "whether a draw distance of 100 m retires the neighbours",
                grid,
                &(between.held == 0),
            )?;

            // 32 m: reaches nothing on either grid. Where 100 m already
            // retired everything there is nothing left for it to retire.
            let near = if between.held == 0 {
                Step::default()
            } else {
                step(session, NEAR_M, between.held, 0).await?
            };
            check_eq(
                "the neighbours still held at a draw distance of 32 m",
                &near.held,
                &0,
            )?;
            // Whichever step retired them says how promptly this grid does it.
            let retirement = if between.held == 0 { &between } else { &near };
            let prompt = retirement
                .changed_after
                .is_some_and(|after| after <= PROMPT);
            RETIRES_PROMPTLY.check(
                "whether a neighbour out of reach is retired within ten seconds",
                grid,
                &prompt,
            )?;

            // 512 m: every neighbour again, and promptly on both grids.
            let far = step(session, FAR_M, 0, all).await?;
            check_eq(
                "the neighbours held again at a draw distance of 512 m",
                &far.held,
                &all,
            )?;
            check(
                far.changed_after.is_some_and(|after| after <= PROMPT),
                "the neighbours were not announced again within ten seconds of the draw distance \
                 reaching them",
            )?;

            let metrics = ctx.metrics();
            found.record("neighbours_", Some(home), metrics);
            metrics.set("held_at_100m", tally(between.held));
            metrics.set("held_at_32m", tally(near.held));
            metrics.set("held_at_512m", tally(far.held));
            metrics.set(
                "retired_order",
                retirement
                    .retired
                    .iter()
                    .map(|handle| {
                        handle.map_or_else(
                            || "?".to_owned(),
                            |handle| {
                                let (dx, dy) = slot_offset(home, handle);
                                format!("{dx}/{dy}")
                            },
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            if let Some(after) = retirement.changed_after {
                metrics.set_timing(&secs_metric("retire"), after.as_secs_f64());
            }
            if let Some(after) = far.changed_after {
                metrics.set_timing(&secs_metric("announce"), after.as_secs_f64());
            }
            Ok(())
        })
    }
}

/// What one step of the draw distance did.
#[derive(Debug, Clone, Default)]
struct Step {
    /// How many neighbours are held once the step has settled.
    held: usize,
    /// How long after the change the last neighbour came or went, when any
    /// did.
    changed_after: Option<Duration>,
    /// The regions retired, in order.
    retired: Vec<Option<RegionHandle>>,
}

/// Sets the draw distance to `metres` and watches the neighbours until
/// `expected` of them are held — starting from `before` — or the step's
/// window has passed.
///
/// A step expected to leave things as they are watches only long enough to
/// see a prompt grid change them anyway.
async fn step(
    session: &mut Session,
    metres: f64,
    before: usize,
    expected: usize,
) -> Result<Step, TestFailure> {
    let window = if expected == before {
        UNCHANGED_WINDOW
    } else if expected < before {
        RETIRE_WINDOW
    } else {
        ANNOUNCE_WINDOW
    };
    session
        .send(Command::SetDrawDistance(Distance::new(metres)))
        .await?;
    let started = Instant::now();
    let mut outcome = Step {
        held: before,
        ..Step::default()
    };
    while outcome.held != expected || expected == before {
        let remaining = window.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        let change = session
            .wait_for(remaining, |event| match event {
                Event::NeighborDiscovered(_) => Some(None),
                Event::NeighborRetired { region_handle, .. } => Some(Some(*region_handle)),
                _other => None,
            })
            .await;
        match change {
            Ok(None) => outcome.held = outcome.held.saturating_add(1),
            Ok(Some(handle)) => {
                outcome.held = outcome.held.saturating_sub(1);
                outcome.retired.push(handle);
            }
            Err(TestFailure::Timeout(_)) => break,
            Err(other) => return Err(other),
        }
        outcome.changed_after = Some(started.elapsed());
    }
    tracing::info!(
        metres,
        held = outcome.held,
        after = ?outcome.changed_after,
        "the draw distance was stepped"
    );
    Ok(outcome)
}
