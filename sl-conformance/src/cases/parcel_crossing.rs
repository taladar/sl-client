//! Walk the avatar over a parcel line and record what the grid pushes — the
//! movement half of `gridspec-parcel-properties`.
//!
//! A viewer learns which parcel its agent stands on from the grid: the parcel
//! is pushed on arrival and again whenever the agent crosses into another one.
//! Nothing the viewer sends asks for the second push, so what it looks like —
//! its sequence id, its request result, how soon it comes — is the grid's to
//! decide and this case's to measure.
//!
//! 1. Wait for the region and our own avatar's position.
//! 2. On OpenSim, where the region is one parcel, divide off everything east of
//!    a line `LINE_OFFSET_M` east of the avatar (as the estate owner, like
//!    `parcel-divide-join`); the region is joined back afterwards under the
//!    same cleanup guard.
//! 3. Find the nearest parcel line: read the parcel under the avatar, then 4 m
//!    squares outwards east, west, north and south until one answers with
//!    another parcel's id. A region with no line within `SEARCH_RANGE_M` is
//!    recorded `partial`.
//! 4. Fly over the line to a point `OVERSHOOT_M` past it, watching every
//!    parcel record that arrives on the way, and for `SETTLE` after — then
//!    back to where the walk began, watching the same way.
//!
//! The record: the line's direction and distance (`line`), every parcel record
//! pushed during the walk (`pushes`, each `sequence/local id/result/snap`), and
//! the first push of the new parcel (`crossing_push`) with how long after the
//! walk began it came (`crossing_push_secs`), and the push of the first parcel
//! on the way back (`return_push`).
//!
//! `1av`, `[both]`, live only: the fake grid simulates no movement
//! (`server-fake-grid-parcel-on-movement`). OpenSim runs as the estate owner
//! (`--avatar estate-owner`); aditi as anyone, from wherever the login lands.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, Event, ParcelInfo, ParcelRequestResult, RegionLocalParcelId, Vector,
};

use crate::cases::parcel_divide_join::{RestoreOnDrop, restore_single_parcel};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, is_opensim, walk_within_region};

/// The OpenSim start location: the "Default Region" centre, so the divide
/// below has room on both sides of the avatar.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// How far east of the avatar the OpenSim divide draws its line, in metres
/// (a multiple of the 4 m parcel grid).
const LINE_OFFSET_M: f32 = 8.0;

/// How far from the avatar the search for a parcel line looks, in metres.
const SEARCH_RANGE_M: f32 = 96.0;

/// The step of the search, in metres: one parcel-grid square.
const STEP_M: f32 = 4.0;

/// How far past the line the walk goes, in metres.
const OVERSHOOT_M: f32 = 6.0;

/// How long to keep watching after the walk ends, for a push that comes late.
const SETTLE: Duration = Duration::from_secs(5);

/// How long the OpenSim divide is given to take effect before the search.
const EDIT_SETTLE: Duration = Duration::from_secs(2);

/// The first sequence id of this case's parcel queries; each query takes the
/// next. Distinct from every other case's ids so the replies never alias.
const SEQUENCE_BASE: i32 = 5400;

/// The push of the parcel walked onto: whether its sequence id is zero,
/// whether its result is a single parcel's, and whether it snaps the selection.
/// OpenSim pushes every parcel under sequence 0. Second Life numbers its
/// unsolicited pushes for the session — 0 on arrival, then 1, 2, … — so a
/// client's own request ids have to stay out of that range.
const CROSSING_PUSH: Measured<(bool, bool, bool)> = Measured {
    second_life: (false, true, false),
    opensim: (true, true, false),
    source: "parcel-crossing on aditi and OpenSim (2026-10-05, book/src/gridspec/land.md)",
};

/// The four directions the search looks in, as unit steps on the region plane,
/// with their names for the record.
const DIRECTIONS: [(&str, f32, f32); 4] = [
    ("east", 1.0, 0.0),
    ("west", -1.0, 0.0),
    ("north", 0.0, 1.0),
    ("south", 0.0, -1.0),
];

/// Walks the avatar over a parcel line and records the grid's push.
#[derive(Debug)]
pub struct ParcelCrossing;

impl GridTest for ParcelCrossing {
    fn name(&self) -> &'static str {
        "parcel-crossing"
    }

    fn description(&self) -> &'static str {
        "Walk over a parcel line and record the parcel the grid pushes"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            if !is_opensim(ctx.grid()) {
                return cross(ctx).await;
            }
            // The divide leaves the region split until it is joined back, so
            // the walk runs under the same two guards `parcel-divide-join`
            // uses: the awaited join for every path that returns, the drop
            // guard for one that never does.
            let mut guard = RestoreOnDrop {
                commander: ctx.primary().commander(),
                armed: true,
            };
            let outcome = cross(ctx).await;
            let restored = restore_single_parcel(ctx.primary()).await;
            guard.armed = false;
            match (outcome, restored) {
                (Ok(()), restored) => restored,
                (Err(failure), _) => Err(failure),
            }
        })
    }
}

/// The walk itself: find a line, cross it, record what came.
///
/// # Errors
///
/// Returns a [`TestFailure`] for a send or wait failure, an avatar that never
/// shows up, or a walk that cannot reach the far side.
async fn cross(ctx: &mut TestContext) -> Result<(), TestFailure> {
    let opensim = is_opensim(ctx.grid());
    let session = ctx.primary();
    session.wait_for_region(REGION_TIMEOUT).await?;
    let here = own_position(session).await?;
    tracing::info!(
        x = here.x,
        y = here.y,
        z = here.z,
        "parcel-crossing: the avatar stands here"
    );
    let mut sequence = SEQUENCE_BASE;

    if opensim {
        // Start from one parcel, whatever an interrupted run left behind.
        restore_single_parcel(session).await?;
        let line_x = ((here.x + LINE_OFFSET_M) / STEP_M).round() * STEP_M;
        session
            .send(Command::DivideParcel {
                west: line_x,
                south: 0.0,
                east: 256.0,
                north: 256.0,
            })
            .await?;
        tokio::time::sleep(EDIT_SETTLE).await;
    }

    let home = read_parcel(session, &here, next(&mut sequence)).await?;
    if !home.request_result.has_data() {
        ctx.mark_partial("the agent stands on land the region has no parcel for");
        return Ok(());
    }
    let Some(line) = find_line(session, &here, home.local_id, &mut sequence).await? else {
        ctx.mark_partial("no parcel line within reach of the avatar");
        return Ok(());
    };

    // Over the line, watching everything the grid sends about parcels.
    let target = Vector {
        x: (here.x + line.dx * (line.distance + OVERSHOOT_M)).clamp(2.0, 254.0),
        y: (here.y + line.dy * (line.distance + OVERSHOOT_M)).clamp(2.0, 254.0),
        z: here.z,
    };
    let started = Instant::now();
    // Each push with when it came.
    let mut pushes: Vec<(ParcelInfo, f64)> = Vec::new();
    let mut note = |event: &Event| {
        if let Event::ParcelProperties(parcel) = event {
            pushes.push(((**parcel).clone(), started.elapsed().as_secs_f64()));
        }
    };
    let over = walk_within_region(session, here.clone(), &target, &mut note).await?;
    watch(session, &mut note).await?;
    // And back, for the push of the parcel the walk started on: whether the
    // grid's push ids count up, and whether it pushes on every crossing.
    walk_within_region(session, over, &here, &mut note).await?;
    watch(session, &mut note).await?;

    // The crossing push, and the first push of the starting parcel after it
    // — on the way back, or sooner if the flight drifted back over the line
    // while it settled.
    let crossing_at = pushes
        .iter()
        .position(|(parcel, _)| parcel.local_id == line.beyond);
    let crossing = crossing_at.and_then(|at| pushes.get(at));
    let back = crossing_at.and_then(|at| {
        pushes
            .iter()
            .skip(at.saturating_add(1))
            .find(|(parcel, _)| parcel.local_id == home.local_id)
    });
    let grid = ctx.grid();
    let metrics = ctx.metrics();
    metrics.set(
        "line",
        format!("{} at {:.0} m", line.direction, line.distance),
    );
    metrics.set(
        "pushes",
        pushes
            .iter()
            .map(|(parcel, secs)| format!("{} @{secs:.1}s", describe(parcel)))
            .collect::<Vec<_>>()
            .join(" | "),
    );
    metrics.set(
        "crossing_push",
        crossing.map_or_else(|| "none".to_owned(), |(parcel, _)| describe(parcel)),
    );
    if let Some((_, secs)) = crossing {
        metrics.set_timing("crossing_push_secs", *secs);
    }
    metrics.set(
        "return_push",
        back.map_or_else(|| "none".to_owned(), |(parcel, _)| describe(parcel)),
    );
    let Some((pushed, _)) = crossing else {
        return Err(TestFailure::Assertion(
            "the grid pushed no parcel for the line the avatar crossed".to_owned(),
        ));
    };
    CROSSING_PUSH.check(
        "crossing push (sequence id is zero, single parcel, snaps the selection)",
        grid,
        &(
            pushed.sequence_id == 0,
            pushed.request_result == ParcelRequestResult::Single,
            pushed.snap_selection,
        ),
    )
}

/// Shows every event of the next [`SETTLE`] to `note`, for a push that comes
/// after the walk has stopped.
///
/// # Errors
///
/// Propagates the wait's failures other than its own expected timeout.
async fn watch(session: &mut Session, note: &mut impl FnMut(&Event)) -> Result<(), TestFailure> {
    match session
        .wait_for(SETTLE, |event| {
            note(event);
            None::<()>
        })
        .await
    {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

/// A parcel line the search found: which way, how far, and the parcel beyond.
struct Line {
    /// The direction's name, for the record.
    direction: &'static str,
    /// The direction's unit step east.
    dx: f32,
    /// The direction's unit step north.
    dy: f32,
    /// How far the first square of the other parcel is, in metres.
    distance: f32,
    /// The parcel beyond the line.
    beyond: RegionLocalParcelId,
}

/// Searches outwards from `here` in each of [`DIRECTIONS`] for the nearest
/// square that belongs to another parcel than `home`.
///
/// # Errors
///
/// Propagates the queries' send and wait failures.
async fn find_line(
    session: &mut Session,
    here: &Vector,
    home: RegionLocalParcelId,
    sequence: &mut i32,
) -> Result<Option<Line>, TestFailure> {
    let mut nearest: Option<Line> = None;
    for (direction, dx, dy) in DIRECTIONS {
        let mut distance = STEP_M;
        while distance <= SEARCH_RANGE_M
            && nearest.as_ref().is_none_or(|line| distance < line.distance)
        {
            let point = Vector {
                x: here.x + dx * distance,
                y: here.y + dy * distance,
                z: here.z,
            };
            if !(0.0..256.0).contains(&point.x) || !(0.0..256.0).contains(&point.y) {
                break;
            }
            let parcel = read_parcel(session, &point, next(sequence)).await?;
            tracing::info!(
                direction,
                distance,
                x = point.x,
                y = point.y,
                local_id = parcel.local_id.0,
                result = ?parcel.request_result,
                "parcel-crossing: searched"
            );
            if parcel.request_result.has_data() && parcel.local_id != home {
                nearest = Some(Line {
                    direction,
                    dx,
                    dy,
                    distance,
                    beyond: parcel.local_id,
                });
                break;
            }
            distance += STEP_M;
        }
    }
    Ok(nearest)
}

/// Reads the parcel under the 4 m square containing `point`.
///
/// # Errors
///
/// Propagates the send and wait failures.
async fn read_parcel(
    session: &mut Session,
    point: &Vector,
    sequence_id: i32,
) -> Result<ParcelInfo, TestFailure> {
    let west = (point.x / STEP_M).floor() * STEP_M;
    let south = (point.y / STEP_M).floor() * STEP_M;
    session
        .send(Command::RequestParcelProperties {
            west,
            south,
            east: west + STEP_M,
            north: south + STEP_M,
            sequence_id,
            snap_selection: false,
        })
        .await?;
    session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ParcelProperties(parcel) if parcel.sequence_id == sequence_id => {
                Some((**parcel).clone())
            }
            _ => None,
        })
        .await
}

/// Waits for our own avatar in the object stream and returns where it stands.
///
/// # Errors
///
/// Returns [`TestFailure::Assertion`] when the login reported no agent id, and
/// propagates the wait's failures.
async fn own_position(session: &mut Session) -> Result<Vector, TestFailure> {
    let agent = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                if object.full_id.uuid() == agent =>
            {
                Some(object.motion.position.clone())
            }
            _ => None,
        })
        .await
}

/// Takes the next sequence id.
const fn next(sequence: &mut i32) -> i32 {
    let id = *sequence;
    *sequence = sequence.wrapping_add(1);
    id
}

/// A parcel record as the record shows it: `sequence/local id/result/snap`.
fn describe(parcel: &ParcelInfo) -> String {
    format!(
        "{}/{}/{:?}/{}",
        parcel.sequence_id, parcel.local_id.0, parcel.request_result, parcel.snap_selection
    )
}
