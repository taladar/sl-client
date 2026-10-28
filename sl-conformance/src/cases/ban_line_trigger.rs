//! Find out what makes a grid push a ban line, by putting the agent's camera
//! in one place after another around a parcel that keeps strangers out and
//! counting the ban lines each place brings.
//!
//! [`super::parcel_ban_line`] saw Second Life push a ban line on some arrivals
//! beside a closed parcel and on no approach, by air or on foot. Every one of
//! those approaches had the agent's camera where a session puts it until a
//! client says otherwise — the region's middle, 128/128 at 30 m — while a
//! viewer's camera follows its avatar. The first run of this case moved the
//! avatar and the camera separately (2026-10-28): hovering, falling and
//! walking into the line brought nothing with the camera at the middle, and
//! the camera alone beside the line brought a ban line twice a second with the
//! avatar 171 m away. So it is the camera, and the trials are now all camera:
//!
//! 1. **arrival** — the login, the parcel sweep and a few seconds standing,
//!    the camera at the middle;
//! 2. **the ground** — the camera high over the parcel for long enough for its
//!    land patch to come, which gives the height of the ground there;
//! 3. **distance** — the camera `DISTANCES_M` short of the parcel's nearest
//!    square, far to near, `EYE_M` above the ground and looking at it;
//! 4. **turned** — `NEAR_M` short of it, turned aside by more and more, and
//!    pitched up;
//! 5. **height** — the camera over the square, `HEIGHTS_M` above the ground
//!    and looking down at it;
//! 6. **diagonal** — off to the side and above the top of the parcel's
//!    `COLUMN_M` column, looking down at the parcel and at the column's top
//!    edge;
//! 7. **turned further out** — `FURTHER_M` short of it, turned aside;
//! 8. **draw distance** — a short one inside the reach, a long one outside;
//! 9. **back at the middle**.
//!
//! Each trial watches for `TRIAL_WINDOW`; a line in its first `STRAGGLER_S`
//! is shown apart, as the previous place's.
//!
//! The record: the parcel (`target`), each trial's ban-line count with where
//! the camera was and the mean interval between the lines (`trials`), the
//! region and the avatar on arrival (`arrival`), every parcel record pushed
//! unasked (`pushes`) and every alert (`alerts`). Nothing is held to an
//! answer: this is a probe whose record is the measurement. The avatar is not
//! moved, so the next run arrives where this one did.
//!
//! `1av`, `[both]`, live only, from wherever the login lands, in the login
//! region only. On OpenSim no parcel restricts access unless a run of
//! `parcel-ban-enforcement` was interrupted, so there the case records
//! `partial`.

use std::time::Duration;

use sl_client_tokio::{Camera, Command, ControlFlags, Distance, ParcelInfo, Vector};

use super::parcel_ban_line::{
    Push, Seen, nearest_square, own_position, restriction, stand, survey, watch,
};
use super::terrain_editing::metre;
use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::REGION_TIMEOUT;

/// How long each trial watches.
const TRIAL_WINDOW: Duration = Duration::from_secs(6);

/// How far short of the parcel's nearest square the distance trials put the
/// camera, in metres, far to near.
const DISTANCES_M: [f32; 13] = [
    64.0, 48.0, 32.0, 24.0, 22.0, 21.0, 20.0, 18.0, 16.0, 12.0, 8.0, 4.0, 0.0,
];

/// How far above the ground the distance trials put the camera, in metres —
/// a viewer's camera behind a standing avatar.
const EYE_M: f32 = 2.0;

/// How far above the ground over the parcel's nearest square the height trials
/// put the camera, in metres.
const HEIGHTS_M: [f32; 12] = [
    10.0, 30.0, 50.0, 60.0, 64.0, 66.0, 68.0, 70.0, 72.0, 74.0, 80.0, 100.0,
];

/// The draw distance a session advertises, in metres, which the trials that
/// change it put back.
const DRAW_DISTANCE_M: f64 = 256.0;

/// The whole case's budget: one region's sweep and a score of trials.
const CASE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Puts the camera around a closed parcel and counts the ban lines.
#[derive(Debug)]
pub struct BanLineTrigger;

impl GridTest for BanLineTrigger {
    fn name(&self) -> &'static str {
        "ban-line-trigger"
    }

    fn description(&self) -> &'static str {
        "Put the camera around a closed parcel and count the ban lines each place brings"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let agent = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
                .uuid();
            let region = session
                .region_identity()
                .and_then(|identity| identity.sim_name.as_ref())
                .map_or_else(|| "?".to_owned(), ToString::to_string);
            let arrived = own_position(session, agent).await?;
            let mut seen = Seen::arriving(agent, arrived.clone());
            let found = survey(session, &mut seen).await?;
            let regions = found.describe();
            let Some((target, point, distance)) = found.picked else {
                ctx.metrics().set("regions", format!("{region}: {regions}"));
                ctx.mark_partial("no parcel in the login region restricts access");
                return Ok(());
            };
            let kind = restriction(&target).unwrap_or("none");
            seen.target = Some(target.clone());
            let mut trials = Trials::default();

            // The arrival, the camera where the session put it.
            session
                .send(Command::SetControls(ControlFlags::empty()))
                .await?;
            stand(session, &mut seen).await?;
            trials.end("arrival", &seen, &target, &Camera::region_center());

            // The ground at the parcel's nearest square, from the land patches
            // the camera's trip there brings (the sweep's did not reach it):
            // the avatar's own height until one has come.
            let looking = Camera::looking_at(
                Vector {
                    x: point.x,
                    y: point.y,
                    z: seen.position.z + GROUND_PROBE_M,
                },
                Vector {
                    z: seen.position.z,
                    ..point.clone()
                },
            );
            session.send(Command::SetCamera(looking)).await?;
            watch(session, &mut seen, TRIAL_WINDOW).await?;
            // Whatever came meanwhile is not a trial's.
            trials.skip(&seen, &target);
            let (ground, ground_from) = session
                .ground_height(metre(point.x).min(255), metre(point.y).min(255))
                .map_or((seen.position.z, "the avatar's height"), |height| {
                    (height, "the land patch")
                });

            for (label, camera, far) in placements(&point, &seen.position, ground) {
                let changes_far = far.is_some();
                if let Some(far) = far {
                    session.send(Command::SetDrawDistance(far)).await?;
                }
                trials.begin(&seen);
                session.send(Command::SetCamera(camera.clone())).await?;
                watch(session, &mut seen, TRIAL_WINDOW).await?;
                trials.end(&label, &seen, &target, &camera);
                if changes_far {
                    session
                        .send(Command::SetDrawDistance(Distance::new(DRAW_DISTANCE_M)))
                        .await?;
                }
            }

            let metrics = ctx.metrics();
            metrics.set("regions", format!("{region}: {regions}"));
            metrics.set(
                "target",
                format!(
                    "parcel {} ({kind}) {distance:.0} m away at {:.0}/{:.0}",
                    target.local_id.0, point.x, point.y
                ),
            );
            metrics.set(
                "arrival",
                format!(
                    "{region} {:.0}/{:.0}, z {:.0}, {:.0} m from the parcel; the ground at the \
                     parcel z {ground:.1}, from {ground_from}",
                    arrived.x,
                    arrived.y,
                    arrived.z,
                    nearest_square(&target, &arrived).map_or(f32::NAN, |near| near.1)
                ),
            );
            metrics.set("trials", trials.shown.join(" | "));
            metrics.set("pushes", seen.describe_pushes());
            metrics.set("alerts", seen.alerts.join(" | "));
            Ok(())
        })
    }
}

/// Every place the trials put the camera, in order: a label, the camera, and
/// a draw distance to advertise for that trial alone. `point` is the centre of
/// the parcel's nearest square, `avatar` where the avatar stands, `ground`
/// the height taken as the ground's.
fn placements(
    point: &Vector,
    avatar: &Vector,
    ground: f32,
) -> Vec<(String, Camera, Option<Distance>)> {
    let square = Vector {
        x: point.x,
        y: point.y,
        z: ground,
    };
    let below = Vector {
        z: ground - 1.0,
        ..square.clone()
    };
    // The direction the distance trials back away along: from the square
    // towards the avatar, or along -X when the avatar is on top of it.
    let (away_x, away_y) = {
        let dx = avatar.x - point.x;
        let dy = avatar.y - point.y;
        let length = dx.hypot(dy);
        if length < 1.0 {
            (-1.0, 0.0)
        } else {
            (dx / length, dy / length)
        }
    };
    let short_of = |metres: f32| Vector {
        x: away_x.mul_add(metres, point.x).clamp(0.5, 255.5),
        y: away_y.mul_add(metres, point.y).clamp(0.5, 255.5),
        z: ground + EYE_M,
    };
    // A point ten metres from `eye`, turned `degrees` anticlockwise from
    // facing the square, raised `rise` metres.
    let turned = |eye: &Vector, degrees: f32, rise: f32| {
        let (sin, cos) = degrees.to_radians().sin_cos();
        // Facing the square is `-away`; turn that anticlockwise.
        let x = (-away_x).mul_add(cos, away_y * sin);
        let y = (-away_y).mul_add(cos, -away_x * sin);
        Vector {
            x: x.mul_add(10.0, eye.x),
            y: y.mul_add(10.0, eye.y),
            z: eye.z + rise,
        }
    };

    let mut all = Vec::new();
    for metres in DISTANCES_M {
        let eye = short_of(metres);
        // Over the square itself the look is straight down.
        let look = if metres > 0.0 {
            square.clone()
        } else {
            below.clone()
        };
        all.push((
            format!("{metres:.0} m"),
            Camera::looking_at(eye, look),
            None,
        ));
    }
    let near = short_of(NEAR_M);
    for (label, degrees, rise) in [
        ("45° aside", 45.0, 0.0),
        ("60° aside", 60.0, 0.0),
        ("75° aside", 75.0, 0.0),
        ("sideways", 90.0, 0.0),
        ("away", 180.0, 0.0),
        ("towards it, pitched up", 0.0, 17.0),
    ] {
        let camera = Camera::looking_at(near.clone(), turned(&near, degrees, rise));
        all.push((format!("{NEAR_M:.0} m looking {label}"), camera, None));
    }
    for metres in HEIGHTS_M {
        let eye = Vector {
            z: ground + metres,
            ..square.clone()
        };
        all.push((
            format!("{metres:.0} m up"),
            Camera::looking_at(eye, below.clone()),
            None,
        ));
    }
    // Off to the side and above the top of the parcel's column, looking
    // down at the parcel — which puts the column's nearest edge outside the
    // view — and at that edge, which puts it in the middle of the view.
    let edge = Vector {
        z: ground + COLUMN_M,
        ..short_of(STEP_M / 2.0)
    };
    for above in DIAGONAL_ABOVE_M {
        let eye = Vector {
            z: ground + COLUMN_M + above,
            ..short_of(DIAGONAL_OUT_M)
        };
        all.push((
            format!("{DIAGONAL_OUT_M:.0} m out, {above:.0} m over the column, looking down at it"),
            Camera::looking_at(eye.clone(), square.clone()),
            None,
        ));
        all.push((
            format!(
                "{DIAGONAL_OUT_M:.0} m out, {above:.0} m over the column, looking at its top edge"
            ),
            Camera::looking_at(eye, edge.clone()),
            None,
        ));
    }
    // Turned aside further out, where the parcel covers a narrower angle.
    let further = short_of(FURTHER_M);
    for degrees in [30.0, 45.0, 60.0] {
        all.push((
            format!("{FURTHER_M:.0} m looking {degrees:.0}° aside"),
            Camera::looking_at(further.clone(), turned(&further, degrees, 0.0)),
            None,
        ));
    }
    // The draw distance: inside the reach with a draw distance shorter than
    // the way to the parcel, and outside it with a long one.
    all.push((
        format!("{NEAR_FAR_M:.0} m, draw distance {SHORT_DRAW_M:.0} m"),
        Camera::looking_at(short_of(NEAR_FAR_M), square.clone()),
        Some(Distance::new(SHORT_DRAW_M)),
    ));
    all.push((
        format!("{OUT_OF_REACH_M:.0} m, draw distance {LONG_DRAW_M:.0} m"),
        Camera::looking_at(short_of(OUT_OF_REACH_M), square.clone()),
        Some(Distance::new(LONG_DRAW_M)),
    ));
    all.push((
        "back at the middle".to_owned(),
        Camera::region_center(),
        None,
    ));
    all
}

/// How high above the ground the column a parcel's access reaches is taken to
/// stand, in metres: the reference viewer's `PARCEL_HEIGHT`.
const COLUMN_M: f32 = 50.0;

/// How far above the avatar the camera waits for the parcel's land patch, in
/// metres: above every height trial, so it brings no ban line.
const GROUND_PROBE_M: f32 = 100.0;

/// How far short of the parcel's nearest square's centre the diagonal trials
/// put the camera, in metres: 14 m from the square's edge.
const DIAGONAL_OUT_M: f32 = 16.0;

/// How far above the top of the column the diagonal trials put the camera, in
/// metres: 10 m, 17.2 m from the column's edge; 18 m, 22.8 m from it.
const DIAGONAL_ABOVE_M: [f32; 2] = [10.0, 18.0];

/// How far short of the parcel's nearest square's centre the camera stands
/// for the trials that turn it further out.
const FURTHER_M: f32 = 16.0;

/// The edge of a parcel-grid square, in metres.
const STEP_M: f32 = 4.0;

/// How far short of the parcel the camera stands for the trials that turn it.
const NEAR_M: f32 = 4.0;

/// How far short of the parcel the camera stands for the short-draw-distance
/// trial: inside the reach the distance trials found.
const NEAR_FAR_M: f32 = 12.0;

/// The short draw distance, in metres.
const SHORT_DRAW_M: f64 = 8.0;

/// How far short of the parcel the camera stands for the long-draw-distance
/// trial: outside the reach the distance trials found.
const OUT_OF_REACH_M: f32 = 32.0;

/// The long draw distance, in metres.
const LONG_DRAW_M: f64 = 1024.0;

/// How long after a trial moves the camera a ban line is still taken for the
/// previous place's: one is often on its way when the camera moves.
const STRAGGLER_S: f64 = 1.0;

/// The trials so far, as the record shows them.
#[derive(Default)]
struct Trials {
    /// How many ban lines had arrived when the last trial ended.
    counted: usize,
    /// When the current trial began, in seconds since the arrival.
    began: f64,
    /// One entry a trial: `label: n every s, first after t s (+k early)
    /// (camera d m from the parcel, z h)`.
    shown: Vec<String>,
}

impl Trials {
    /// Begins a trial now.
    fn begin(&mut self, seen: &Seen) {
        self.began = seen.arrived.elapsed().as_secs_f64();
    }

    /// Forgets the ban lines for `target` so far: they came between trials.
    fn skip(&mut self, seen: &Seen, target: &ParcelInfo) {
        self.counted = seen.ban_lines(target.local_id).count();
    }

    /// Ends the trial `label`: the ban lines for `target` since the last one
    /// ended, how often they came, and where `camera` was.
    fn end(&mut self, label: &str, seen: &Seen, target: &ParcelInfo, camera: &Camera) {
        let all: Vec<&Push> = seen.ban_lines(target.local_id).skip(self.counted).collect();
        let (early, lines): (Vec<&Push>, Vec<&Push>) = all
            .iter()
            .partition(|push| push.secs < self.began + STRAGGLER_S);
        let first = lines.first().map_or_else(String::new, |push| {
            format!(", first after {:.1} s", push.secs - self.began)
        });
        let early = if early.is_empty() {
            String::new()
        } else {
            format!(" (+{} early)", early.len())
        };
        let cadence = match (lines.first(), lines.last()) {
            (Some(first), Some(last)) if lines.len() > 1 => {
                let gaps = f64::from(u32::try_from(lines.len().saturating_sub(1)).unwrap_or(1));
                format!(" every {:.2} s", (last.secs - first.secs) / gaps)
            }
            _ => String::new(),
        };
        let from_parcel = nearest_square(target, &camera.center).map_or(f32::NAN, |near| near.1);
        self.shown.push(format!(
            "{label}: {}{cadence}{first}{early} (camera {from_parcel:.0} m from the parcel, z \
             {:.0})",
            lines.len(),
            camera.center.z,
        ));
        tracing::info!(label, ban_lines = lines.len(), "trial ended");
        self.counted = self.counted.saturating_add(all.len());
    }
}
