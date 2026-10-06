//! Find a parcel in the region that keeps strangers out, go up to it, and
//! record what the grid pushes — the ban line as the avatar it is drawn for
//! sees it.
//!
//! [`super::parcel_ban_enforcement`] needs land the primary owns, which the
//! test avatars have nowhere on Second Life. Meeting somebody else's ban line
//! needs nothing but a parcel that restricts access, and a parcel says so in
//! flags any resident can read. So this case looks for one:
//!
//! 1. Wait for the region and our own avatar's position. From here on one
//!    watcher sees every parcel record the grid pushes unasked, and every
//!    alert: a ban line comes when the grid chooses, and on Second Life that
//!    is the second after an arrival.
//! 2. **Sweep** the region's parcels. Every parcel record carries the bitmap of
//!    the 4 m squares the parcel covers, so the sweep asks for the first square
//!    no parcel read so far covers, marks that parcel's squares, and repeats —
//!    one request a parcel, not one a square.
//! 3. Pick the nearest parcel that somebody else owns and that is closed to
//!    strangers: by its allow list (`USE_ACCESS_LIST`), or to residents
//!    without payment information (`DENY_ANONYMOUS`) or age verification.
//! 4. A region with none is left for the next one the operator named
//!    (`ban_line_regions` in the fixtures file): a map lookup, a teleport to
//!    its middle, and the same sweep. When none of them has such a parcel the
//!    run is recorded `partial`.
//! 5. Fly to `STANDOFF_M` short of the parcel's nearest square (when further
//!    away than that) and come down; **walk** up to the line; lean on it for
//!    `LEAN_BUDGET`; walk back out to `PARKING_M` short of the parcel and stay
//!    there, so the next run — which logs in where this one logged out —
//!    arrives beside the line.
//!
//! The record: every region looked at with its parcel and restricted-parcel
//! counts (`regions`), the parcel picked (`target`, its restriction and how
//! far it was), every parcel record pushed unasked (`pushes`, each
//! `sequence/local id/result/collision @seconds`), how many of them were ban
//! lines in each part of the run (`ban_lines`) and the first one
//! (`ban_line`), every alert (`alerts`), where the lean started
//! (`leaned_from`), and whether the avatar was on the parcel at any point
//! (`entered`).
//!
//! `1av`, `[both]`, live only, from wherever the login lands. On the local
//! OpenSim no parcel restricts access unless a run of
//! `parcel-ban-enforcement` was interrupted, so there the case records
//! `partial`; it exists for Second Life. The case holds the grid to the
//! refusal — the alert, and the avatar kept off the parcel — and only records
//! the ban lines: aditi pushed them on some arrivals beside the parcel and on
//! no approach, and what decides it is not known.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, ControlFlags, Event, ParcelFlags, ParcelInfo, RegionCoordinates, RegionLocalParcelId,
    Uuid, Vector,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{Gait, LONG_TIMEOUT, REGION_TIMEOUT, steer_towards};

/// The edge of a parcel-grid square, in metres.
const STEP_M: f32 = 4.0;

/// How many parcel-grid squares a 256 m region has along an edge.
const SQUARES_PER_EDGE: usize = 64;

/// The most parcels the sweep reads before it stops looking.
const MAX_PARCELS: usize = 600;

/// How long the flight to the standoff point short of the parcel may take.
const APPROACH_BUDGET: Duration = Duration::from_secs(40);

/// How far short of the parcel's nearest square the flight stops, in metres;
/// the rest is walked. A flying avatar on Second Life carries a good ten
/// metres of momentum past where it lets go.
const STANDOFF_M: f32 = 30.0;

/// How long a walk — from the standoff point up to the line, or back out to
/// the parking spot — may take.
const WALK_BUDGET: Duration = Duration::from_secs(30);

/// How far short of the parcel's nearest square the walk up to the line aims,
/// in metres: with the steering helper's arrival radius, a few metres outside
/// the line.
const SHORT_OF_THE_LINE_M: f32 = 4.0;

/// How far short of the parcel's nearest square the avatar is left when the
/// run ends, in metres.
const PARKING_M: f32 = 10.0;

/// How long the avatar leans on the line.
const LEAN_BUDGET: Duration = Duration::from_secs(5);

/// How long the avatar is left standing for a flight's momentum to run out
/// and for it to come down.
const LANDING: Duration = Duration::from_secs(8);

/// How far past the parcel's nearest square the lean aims, in metres.
const PAST_THE_LINE_M: f32 = 6.0;

/// The whole case's budget: a sweep is a request a parcel, a mainland region
/// has a few hundred, and the operator may have named a dozen regions.
const CASE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// How long a region-name lookup waits for the map to answer.
const MAP_WINDOW: Duration = Duration::from_secs(10);

/// The height a teleport to a region's middle asks for, in metres; the grid
/// lands the avatar on whatever is under it.
const ARRIVAL_HEIGHT_M: f32 = 60.0;

/// The first sequence id of the sweep's parcel queries; each query takes the
/// next. Distinct from every other case's ids so the replies never alias.
const SEQUENCE_BASE: i32 = 5600;

/// How many sequence ids the sweep may use: one a parcel.
const SEQUENCE_SPAN: i32 = 4000;

/// What an avatar that walks into a parcel it may not enter gets: whether an
/// alert says so, and whether it ends up on the parcel anyway. Second Life
/// stops it at the line with `NOTIFY: Cannot enter parcel: …`; OpenSim lets
/// it a step in, says "You do not have access to the parcel" and puts it
/// back.
///
/// The ban line itself — the parcel record under a collision sequence id —
/// is recorded and not held to anything: aditi pushed it on two of four
/// arrivals beside the parcel and on none of a dozen approaches, and what
/// decides it is not known (`gridspec-sl-ban-line-trigger`).
const REFUSED: Measured<(bool, bool)> = Measured {
    second_life: (true, false),
    opensim: (true, false),
    source: "parcel-ban-line on aditi, parcel-ban-enforcement on OpenSim (2026-10-05, \
             book/src/gridspec/land.md)",
};

/// Walks into a parcel that keeps strangers out and records what the grid
/// pushes.
#[derive(Debug)]
pub struct ParcelBanLine;

impl GridTest for ParcelBanLine {
    fn name(&self) -> &'static str {
        "parcel-ban-line"
    }

    fn description(&self) -> &'static str {
        "Walk into a parcel that restricts access and record what the grid pushes"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let regions = ctx.ban_line_regions().to_vec();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let agent = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
                .uuid();
            let here = own_position(session, agent).await?;
            let mut seen = Seen::arriving(agent, here);

            // The login region first, then the regions the operator named.
            let mut visited: Vec<String> = Vec::new();
            let mut found = survey(session, &mut seen).await?;
            visited.push(format!("the login region: {}", found.describe()));
            for name in &regions {
                if found.picked.is_some() {
                    break;
                }
                match travel(session, agent, name).await? {
                    Ok(arrived) => {
                        seen = Seen::arriving(agent, arrived);
                        found = survey(session, &mut seen).await?;
                        visited.push(format!("{name}: {}", found.describe()));
                    }
                    Err(why) => visited.push(format!("{name}: {why}")),
                }
            }
            ctx.metrics().set("regions", visited.join(" | "));
            let Some((target, point, distance)) = found.picked else {
                ctx.mark_partial("no parcel in the regions looked at restricts access");
                return Ok(());
            };
            let kind = restriction(&target).unwrap_or("none");
            tracing::info!(
                local_id = target.local_id.0,
                kind,
                distance,
                x = point.x,
                y = point.y,
                "parcel-ban-line: flying at this parcel"
            );

            // Down first. A parcel's allow list only reaches so far above the
            // ground (the reference viewer draws its lines `PARCEL_HEIGHT`,
            // 50 m, high), and an avatar that logs in where it last hovered
            // would sail over the line it came to meet.
            let session = ctx.primary();
            session
                .send(Command::SetControls(ControlFlags::empty()))
                .await?;
            stand(session, &mut seen).await?;

            let from = seen.position.clone();
            let reach = (point.x - from.x).hypot(point.y - from.y).max(1.0);
            // A point on the line from where the avatar stands through the
            // parcel's nearest square, `metres` past that square's centre
            // (negative is short of it).
            let along = |metres: f32| Vector {
                x: (point.x + (point.x - from.x) / reach * metres).clamp(2.0, 254.0),
                y: (point.y + (point.y - from.y) / reach * metres).clamp(2.0, 254.0),
                z: from.z,
            };
            seen.target = Some(target.clone());
            let on_arrival = seen.ban_lines(target.local_id).count();

            // By air to well short of the parcel — over whatever stands in
            // between, and far enough out that the flight's momentum is spent
            // before the line — and down again.
            if reach > STANDOFF_M + 4.0 {
                steer_towards(
                    session,
                    from.clone(),
                    &along(-STANDOFF_M),
                    Gait::Flying,
                    APPROACH_BUDGET,
                    |event| seen.note(event),
                )
                .await?;
                stand(session, &mut seen).await?;
            }
            let after_flight = seen.ban_lines(target.local_id).count();

            // The rest on foot: up to the line, then into it. Second Life
            // lifts an avatar that *flies* into a closed parcel's line up over
            // its top, which is no way to find out what the line does.
            steer_towards(
                session,
                seen.position.clone(),
                &along(-SHORT_OF_THE_LINE_M),
                Gait::Walking,
                WALK_BUDGET,
                |event| seen.note(event),
            )
            .await?;
            let after_walk = seen.ban_lines(target.local_id).count();
            steer_towards(
                session,
                seen.position.clone(),
                &along(PAST_THE_LINE_M),
                Gait::Walking,
                LEAN_BUDGET,
                |event| seen.note(event),
            )
            .await?;
            let after_lean = seen.ban_lines(target.local_id).count();
            let closest = nearest_square(&target, &seen.position).map_or(f32::NAN, |near| near.1);

            // Park a few metres outside the parcel, so the next run — which
            // logs in wherever this one logged out — arrives beside the line
            // instead of where the grid puts an avatar it found somewhere it
            // may not be.
            steer_towards(
                session,
                seen.position.clone(),
                &along(-PARKING_M),
                Gait::Walking,
                WALK_BUDGET,
                |event| seen.note(event),
            )
            .await?;
            stand(session, &mut seen).await?;

            let lines: Vec<&Push> = seen.ban_lines(target.local_id).collect();
            let metrics = ctx.metrics();
            metrics.set(
                "target",
                format!(
                    "parcel {} ({kind}) {distance:.0} m away at {:.0}/{:.0}",
                    target.local_id.0, point.x, point.y
                ),
            );
            metrics.set(
                "parked",
                format!(
                    "{:.0}/{:.0}, z {:.0}, {:.0} m from the parcel",
                    seen.position.x,
                    seen.position.y,
                    seen.position.z,
                    nearest_square(&target, &seen.position).map_or(f32::NAN, |near| near.1)
                ),
            );
            metrics.set("pushes", seen.describe_pushes());
            metrics.set("alerts", seen.alerts.join(" | "));
            metrics.set(
                "entered",
                seen.entered_at.as_ref().map_or_else(
                    || "no".to_owned(),
                    |at| format!("at {:.0}/{:.0}, z {:.0}", at.x, at.y, at.z),
                ),
            );
            metrics.set(
                "ban_lines",
                format!(
                    "{on_arrival} on arrival, {} on the flight to {STANDOFF_M:.0} m short of the \
                     parcel, {} walking up to it, {} leaning on it, {} after",
                    after_flight.saturating_sub(on_arrival),
                    after_walk.saturating_sub(after_flight),
                    after_lean.saturating_sub(after_walk),
                    lines.len().saturating_sub(after_lean)
                ),
            );
            metrics.set(
                "leaned_from",
                format!("{closest:.0} m from the parcel's nearest square"),
            );
            metrics.set(
                "ban_line",
                lines.first().map_or_else(
                    || "none".to_owned(),
                    |push| {
                        format!(
                            "{:?} for parcel {} {:.1} s after arrival, the avatar {:.0} m from \
                             it; {} of its bitmap's squares set (the parcel's own: {})",
                            push.parcel.collision(),
                            push.parcel.local_id.0,
                            push.secs,
                            nearest_square(&target, &push.position).map_or(f32::NAN, |near| near.1),
                            squares(&push.parcel),
                            squares(&target),
                        )
                    },
                ),
            );
            REFUSED.check(
                "walking into a restricted parcel (an alert arrived, the avatar got onto it)",
                grid,
                &(!seen.alerts.is_empty(), seen.entered_at.is_some()),
            )
        })
    }
}

/// One parcel record the grid pushed, with when and where the avatar was.
struct Push {
    /// The record.
    parcel: ParcelInfo,
    /// Seconds since the avatar arrived in the region.
    secs: f64,
    /// Where the avatar was when it came.
    position: Vector,
}

/// Everything the grid said about parcels since the avatar arrived in a
/// region. One watcher sees the sweep, the flight and the wait after it,
/// because a ban line is pushed when the grid chooses — on aditi within a
/// second of an arrival near a closed parcel, while the sweep was still
/// asking its questions.
struct Seen {
    /// The avatar's agent id, to pick it out of the object stream.
    agent: Uuid,
    /// When the avatar arrived.
    arrived: Instant,
    /// Where the avatar last was.
    position: Vector,
    /// The parcel the flight aims at, once one is picked.
    target: Option<ParcelInfo>,
    /// Where the avatar first stood on the target parcel, if it ever did.
    entered_at: Option<Vector>,
    /// Every parcel record that was not an answer to the sweep.
    pushes: Vec<Push>,
    /// Every alert's text.
    alerts: Vec<String>,
}

impl Seen {
    /// A watcher for an avatar that has just arrived at `position`.
    fn arriving(agent: Uuid, position: Vector) -> Self {
        Self {
            agent,
            arrived: Instant::now(),
            position,
            target: None,
            entered_at: None,
            pushes: Vec::new(),
            alerts: Vec::new(),
        }
    }

    /// Records what `event` says.
    fn note(&mut self, event: &Event) {
        match event {
            Event::ObjectUpdated(object) | Event::ObjectAdded(object)
                if object.full_id.uuid() == self.agent =>
            {
                self.position = object.motion.position.clone();
                if self.entered_at.is_none()
                    && self.target.as_ref().is_some_and(|target| {
                        target.contains_point(self.position.x, self.position.y)
                    })
                {
                    self.entered_at = Some(self.position.clone());
                }
            }
            // The sweep's own answers carry its sequence ids; everything else
            // is the grid speaking unasked.
            Event::ParcelProperties(parcel) if !is_sweep_answer(parcel.sequence_id) => {
                self.pushes.push(Push {
                    parcel: (**parcel).clone(),
                    secs: self.arrived.elapsed().as_secs_f64(),
                    position: self.position.clone(),
                });
            }
            Event::AlertMessage {
                message,
                alert_info,
                ..
            } => {
                // A keyed-only alert has an empty plain message; its first
                // structured id says what arrived.
                self.alerts.push(if message.trim().is_empty() {
                    alert_info
                        .first()
                        .map(|info| info.message.clone())
                        .unwrap_or_default()
                } else {
                    message.clone()
                });
            }
            Event::AgentAlertMessage { message, .. } => self.alerts.push(message.clone()),
            _ => {}
        }
    }

    /// The ban lines pushed for the parcel `local_id`.
    fn ban_lines(&self, local_id: RegionLocalParcelId) -> impl Iterator<Item = &Push> {
        self.pushes.iter().filter(move |push| {
            push.parcel.local_id == local_id && push.parcel.collision().is_some()
        })
    }

    /// The pushes as the record shows them, repeats folded into a count:
    /// `sequence/local id/result/collision @seconds ×n`.
    fn describe_pushes(&self) -> String {
        let mut folded: Vec<(String, f64, usize)> = Vec::new();
        for push in &self.pushes {
            let shown = format!(
                "{}/{}/{:?}/{}",
                push.parcel.sequence_id,
                push.parcel.local_id.0,
                push.parcel.request_result,
                push.parcel
                    .collision()
                    .map_or_else(|| "-".to_owned(), |kind| format!("{kind:?}"))
            );
            match folded.last_mut() {
                Some((last, _, count)) if *last == shown => *count = count.saturating_add(1),
                _ => folded.push((shown, push.secs, 1)),
            }
        }
        folded
            .iter()
            .map(|(shown, secs, count)| format!("{shown} @{secs:.1}s ×{count}"))
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

/// Leaves the avatar alone for [`LANDING`], showing what arrives to `seen`.
///
/// # Errors
///
/// Propagates the wait's failures other than its own expected timeout.
async fn stand(session: &mut Session, seen: &mut Seen) -> Result<(), TestFailure> {
    match session
        .wait_for(LANDING, |event| {
            seen.note(event);
            None::<()>
        })
        .await
    {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

/// Whether `sequence_id` is one of the sweep's own.
const fn is_sweep_answer(sequence_id: i32) -> bool {
    sequence_id >= SEQUENCE_BASE && sequence_id < SEQUENCE_BASE.saturating_add(SEQUENCE_SPAN)
}

/// How many squares a parcel record's bitmap has set.
fn squares(parcel: &ParcelInfo) -> u32 {
    parcel.bitmap.iter().map(|byte| byte.count_ones()).sum()
}

/// How `parcel` keeps strangers out, if it does: by its allow list (with or
/// without its group let in beside it), or by payment information or age.
/// `None` for a parcel anyone may enter.
///
/// `USE_ACCESS_GROUP` alone is not a restriction. The flag *admits* the
/// parcel's group to a parcel that is otherwise closed (`USE_ACCESS_LIST`); on
/// a parcel that is open it changes nothing, as an aditi sandbox parcel with
/// only that flag showed by letting a stranger in. A ban list alone names
/// individuals, and the test avatar is on nobody's.
const fn restriction(parcel: &ParcelInfo) -> Option<&'static str> {
    let flags = parcel.flags();
    if flags.contains(ParcelFlags::USE_ACCESS_LIST) {
        if flags.contains(ParcelFlags::USE_ACCESS_GROUP) {
            Some("list and group")
        } else {
            Some("list")
        }
    } else if flags.contains(ParcelFlags::DENY_ANONYMOUS) {
        Some("payment")
    } else if flags.contains(ParcelFlags::DENY_AGEUNVERIFIED) {
        Some("age")
    } else {
        None
    }
}

/// What a sweep of one region found.
struct Survey {
    /// How many parcels the region has.
    parcels: usize,
    /// How many of them keep strangers out.
    restricted: usize,
    /// The nearest of those: the parcel, the centre of its nearest square, and
    /// how far that is.
    picked: Option<(ParcelInfo, Vector, f32)>,
}

impl Survey {
    /// The survey as the record shows it.
    fn describe(&self) -> String {
        format!("{} parcels, {} restricted", self.parcels, self.restricted)
    }
}

/// Sweeps the region the agent is in and picks the restricted parcel nearest
/// to the avatar that somebody else owns.
///
/// # Errors
///
/// Propagates the sweep's failures.
async fn survey(session: &mut Session, seen: &mut Seen) -> Result<Survey, TestFailure> {
    let parcels = sweep(session, seen).await?;
    let restricted: Vec<&ParcelInfo> = parcels
        .iter()
        .filter(|parcel| parcel.owner.uuid() != seen.agent && restriction(parcel).is_some())
        .collect();
    let picked = restricted
        .iter()
        .filter_map(|parcel| {
            nearest_square(parcel, &seen.position)
                .map(|(point, distance)| ((*parcel).clone(), point, distance))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2));
    Ok(Survey {
        parcels: parcels.len(),
        restricted: restricted.len(),
        picked,
    })
}

/// Teleports to the middle of the region called `name` and returns where the
/// avatar arrived, or why it did not get there — a region the map does not
/// know or a teleport the grid refuses is a region to skip, not a failure.
///
/// # Errors
///
/// Propagates the send and wait failures other than the waits' own timeouts.
async fn travel(
    session: &mut Session,
    agent: Uuid,
    name: &str,
) -> Result<Result<Vector, String>, TestFailure> {
    session
        .send(Command::RequestMapByName {
            name: name.to_owned(),
        })
        .await?;
    let found = session
        .wait_for(MAP_WINDOW, |event| match event {
            Event::MapBlock(block)
                if block
                    .name
                    .as_ref()
                    .is_some_and(|known| known.to_string().eq_ignore_ascii_case(name)) =>
            {
                Some(block.region_handle)
            }
            _ => None,
        })
        .await;
    let region_handle = match found {
        Ok(handle) => handle,
        Err(TestFailure::Timeout(_)) => return Ok(Err("not on the map".to_owned())),
        Err(other) => return Err(other),
    };
    session
        .send(Command::Teleport {
            region_handle,
            position: RegionCoordinates::new(128.0, 128.0, ARRIVAL_HEIGHT_M),
            look_at: Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
        })
        .await?;
    let arrived = session
        .wait_for(REGION_TIMEOUT, |event| match event {
            Event::RegionChanged { .. } => Some(Ok(())),
            Event::TeleportFailed { reason, .. } => Some(Err(reason.clone())),
            _ => None,
        })
        .await;
    match arrived {
        Ok(Ok(())) => {}
        Ok(Err(reason)) => return Ok(Err(format!("teleport refused ({reason})"))),
        Err(TestFailure::Timeout(_)) => return Ok(Err("teleport never finished".to_owned())),
        Err(other) => return Err(other),
    }
    own_position(session, agent).await.map(Ok)
}

/// Reads every parcel of the region, one request each, by asking for the first
/// square no parcel read so far covers, showing everything else that arrives
/// meanwhile to `seen`.
///
/// # Errors
///
/// Propagates the queries' send and wait failures.
async fn sweep(session: &mut Session, seen: &mut Seen) -> Result<Vec<ParcelInfo>, TestFailure> {
    let mut covered = vec![false; SQUARES_PER_EDGE * SQUARES_PER_EDGE];
    let mut parcels: Vec<ParcelInfo> = Vec::new();
    let mut sequence = SEQUENCE_BASE;
    while parcels.len() < MAX_PARCELS {
        let Some(index) = covered.iter().position(|seen| !seen) else {
            break;
        };
        let (column, row) = (index % SQUARES_PER_EDGE, index / SQUARES_PER_EDGE);
        let west = square_metres(column);
        let south = square_metres(row);
        session
            .send(Command::RequestParcelProperties {
                west,
                south,
                east: west + STEP_M,
                north: south + STEP_M,
                sequence_id: sequence,
                snap_selection: false,
            })
            .await?;
        let parcel = session
            .wait_for(LONG_TIMEOUT, |event| {
                seen.note(event);
                match event {
                    Event::ParcelProperties(parcel) if parcel.sequence_id == sequence => {
                        Some((**parcel).clone())
                    }
                    _ => None,
                }
            })
            .await?;
        sequence = sequence.wrapping_add(1);
        // The square asked about is done whatever came back, or a region
        // with unparcelled land would be asked about it forever.
        if let Some(seen) = covered.get_mut(index) {
            *seen = true;
        }
        if !parcel.request_result.has_data() {
            continue;
        }
        for (square, seen) in covered.iter_mut().enumerate() {
            if bit(&parcel.bitmap, square) {
                *seen = true;
            }
        }
        if !parcels.iter().any(|held| held.local_id == parcel.local_id) {
            parcels.push(parcel);
        }
    }
    Ok(parcels)
}

/// Whether square `index` (row-major from the south-west corner) is set in a
/// parcel bitmap — one bit a square, least-significant bit first.
fn bit(bitmap: &[u8], index: usize) -> bool {
    bitmap
        .get(index / 8)
        .is_some_and(|byte| byte & (1_u8 << (index % 8)) != 0)
}

/// The western (or southern) edge of parcel-grid column (or row) `square`, in
/// region metres.
fn square_metres(square: usize) -> f32 {
    f32::from(u16::try_from(square).unwrap_or(0)) * STEP_M
}

/// The centre of the square of `parcel` nearest to `here`, and how far it is.
fn nearest_square(parcel: &ParcelInfo, here: &Vector) -> Option<(Vector, f32)> {
    (0..SQUARES_PER_EDGE * SQUARES_PER_EDGE)
        .filter(|index| bit(&parcel.bitmap, *index))
        .map(|index| {
            let x = square_metres(index % SQUARES_PER_EDGE) + STEP_M / 2.0;
            let y = square_metres(index / SQUARES_PER_EDGE) + STEP_M / 2.0;
            let distance = (x - here.x).hypot(y - here.y);
            (Vector { x, y, z: here.z }, distance)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Waits for the avatar `agent` in the object stream and returns where it
/// stands.
///
/// # Errors
///
/// Propagates the wait's failures.
async fn own_position(session: &mut Session, agent: Uuid) -> Result<Vector, TestFailure> {
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
