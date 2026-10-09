//! Arrive in a region and take a census of its object-update stream: which
//! message carries which objects, what a viewer's draw distance, camera and
//! interest-list mode change about it, and what a kill takes away.
//!
//! A simulator tells a viewer about the objects round it in five messages —
//! `ObjectUpdate` (every field), `ObjectUpdateCompressed` (the same, packed),
//! `ObjectUpdateCached` (an id and a checksum to answer from a cache),
//! `ImprovedTerseObjectUpdate` (motion alone) and `KillObject`. Which one it
//! picks, how many objects it puts in one, and which objects it sends at all
//! is where two grids that speak the same messages part.
//!
//! The case runs in legs, each a watch of the stream after one change:
//!
//! 1. the arrival;
//! 2. the draw distance taken down to 32 m;
//! 3. the camera taken to the far corner of the region, looking out of it;
//! 4. the interest-list mode set to `360`, where the grid grants the
//!    capability, and back to `default`;
//! 5. the camera brought back;
//! 6. the draw distance brought back.
//!
//! What the handshake flags change is `object-handshake-flags`' to find out:
//! each combination is a login of its own.
//!
//! The first leg's circuits are probed from the first datagram
//! ([`GridTest::probes_arrival`]), so a message's length and reliability are
//! read off the wire; what it named comes from the
//! [`Event::ObjectStreamBatch`](sl_client_tokio::Event::ObjectStreamBatch) the
//! session makes of it.
//!
//! The fake grid sends every object as a full `ObjectUpdate` at arrival and
//! nothing after it, whatever the viewer says of its range. That is a gap of
//! its own (`server-fake-grid-object-update-forms`,
//! `server-world-update-scheduling`), so the rows about forms and range are
//! held on the live grids alone; what the fake grid does do — the capability,
//! its answer, the agent's own appearance — is held on every grid.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    Camera, Command, Distance, InterestListMode, InterestListReply, ObjectUpdateForm, Vector, pcode,
};

use crate::circuit::processed_since;
use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::object_stream::{Leg, World, record, record_datagrams, watch};
use crate::registry::{GridTest, TestFuture};
use crate::support::{check, check_eq, content_is_ours, is_opensim};

/// Where the measured answers are written down.
const SOURCE: &str = "book/src/gridspec/objects.md (object-update-decode, 2026-10-09)";

/// The OpenSim start location: the "Default Region" (1000,1000), centred, where
/// this workspace's test objects live. On Second Life the avatar keeps `"last"`
/// (a named OpenSim region is meaningless there), and whatever region it lands
/// in supplies the objects.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// How long a live arrival is watched: past Second Life's staggered
/// streaming.
const ARRIVAL_WATCH: Duration = Duration::from_secs(25);

/// How long the stream is watched after a change on a live grid.
const CHANGE_WATCH: Duration = Duration::from_secs(20);

/// How long the fake grid's arrival is watched: it sends everything at once.
const FAKE_ARRIVAL_WATCH: Duration = Duration::from_secs(6);

/// How long the stream is watched after a change on the fake grid.
const FAKE_CHANGE_WATCH: Duration = Duration::from_secs(3);

/// The draw distance the session logs in with, and returns to.
const FAR_M: f64 = 256.0;

/// The draw distance the near legs run at.
const NEAR_M: f64 = 32.0;

/// The case's budget.
const CASE_TIMEOUT: Duration = Duration::from_secs(420);

/// Whether the seed grants `InterestList`.
const GRANTS_INTEREST_LIST: Measured<bool> = Measured {
    second_life: true,
    opensim: false,
    source: SOURCE,
};

/// Whether objects beyond the draw distance are taken away. Second Life kills
/// them within three seconds; OpenSim sends a region's objects whatever the
/// draw distance.
const KILLS_BEYOND_DRAW_DISTANCE: Measured<bool> = Measured {
    second_life: true,
    opensim: false,
    source: SOURCE,
};

/// A camera at `eye`, looking along the unit vector `at` (level).
fn camera(eye: [f32; 3], at: [f32; 2]) -> Camera {
    let [x, y, z] = eye;
    let [at_x, at_y] = at;
    Camera {
        center: Vector { x, y, z },
        at_axis: Vector {
            x: at_x,
            y: at_y,
            z: 0.0,
        },
        left_axis: Vector {
            x: -at_y,
            y: at_x,
            z: 0.0,
        },
        up_axis: Vector {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        },
    }
}

/// What the legs of one run held.
#[derive(Debug)]
struct Run {
    /// The arrival.
    arrival: Leg,
    /// The draw distance taken down.
    near: Leg,
    /// The answer to the switch to `360`, where the capability is granted.
    full_360: Option<Leg>,
    /// The draw distance brought back.
    far_again: Leg,
}

/// Hold what every grid does, fake or live.
fn check_everywhere(grid: Grid, granted: bool, run: &Run) -> Result<(), TestFailure> {
    check(
        run.arrival.own_appearance,
        "no AvatarAppearance about the agent itself came with the arrival, though the handshake \
         reply said the viewer understands one",
    )?;
    check_eq(
        "cache probes sent to a viewer that said its cache is empty",
        &run.arrival.named(ObjectUpdateForm::Cached),
        &0,
    )?;
    GRANTS_INTEREST_LIST.check("whether the seed grants InterestList", grid, &granted)?;
    if let Some(leg) = &run.full_360 {
        check_eq(
            "the answer to a switch to the 360 interest list",
            &leg.interest_list,
            &Some(InterestListReply {
                mode: InterestListMode::Full360,
                previous_mode: InterestListMode::Default,
            }),
        )?;
    }
    Ok(())
}

/// Hold what the live grids do and the fake grid does not yet.
fn check_live(grid: Grid, run: &Run) -> Result<(), TestFailure> {
    let primitives = run
        .arrival
        .added
        .get(&pcode::PRIMITIVE)
        .copied()
        .unwrap_or(0);
    if primitives > 0 {
        check(
            run.arrival.named(ObjectUpdateForm::Compressed) > 0,
            &format!("a region's prims came, and none as ObjectUpdateCompressed ({SOURCE})"),
        )?;
    }
    KILLS_BEYOND_DRAW_DISTANCE.check(
        "whether objects beyond the draw distance are killed",
        grid,
        &(run.near.kills() > 0),
    )?;
    if run.near.kills() > 0 {
        // Second Life names a linkset's root and nothing under it.
        check_eq(
            "child prims named by an out-of-range kill",
            &run.near.killed("child"),
            &0,
        )?;
        check(
            run.far_again.arrivals() > 0,
            &format!(
                "{} objects were killed for being out of range and none came back with the draw \
                 distance ({SOURCE})",
                run.near.kills()
            ),
        )?;
    }
    Ok(())
}

/// Takes a census of the object-update stream through each change a viewer
/// can make to it.
#[derive(Debug)]
pub struct ObjectUpdateDecode;

impl GridTest for ObjectUpdateDecode {
    fn name(&self) -> &'static str {
        "object-update-decode"
    }

    fn description(&self) -> &'static str {
        "Take a census of the object-update stream: forms, range, camera, interest list, kills"
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

    fn probes_arrival(&self) -> bool {
        true
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let (arrival_watch, change_watch) = if grid.is_fake() {
                (FAKE_ARRIVAL_WATCH, FAKE_CHANGE_WATCH)
            } else {
                (ARRIVAL_WATCH, CHANGE_WATCH)
            };
            let mut world = World::new();
            let started = Instant::now();

            let arrival = watch(ctx.primary(), &mut world, arrival_watch).await?;
            let seen = processed_since(ctx.primary(), 0, started);
            record(ctx.metrics(), "arrival", &arrival);
            record_datagrams(ctx.metrics(), &seen);

            ctx.primary()
                .send(Command::SetDrawDistance(Distance::new(NEAR_M)))
                .await?;
            let near = watch(ctx.primary(), &mut world, change_watch).await?;
            record(ctx.metrics(), "near", &near);

            let away = camera([250.0, 250.0, 200.0], [0.707, 0.707]);
            ctx.primary().send(Command::SetCamera(away)).await?;
            let leg = watch(ctx.primary(), &mut world, change_watch).await?;
            record(ctx.metrics(), "camera_away", &leg);

            let granted = ctx.primary().cap("InterestList").is_some();
            ctx.metrics().set("interest_list_granted", granted);
            let mut full_360 = None;
            if granted {
                ctx.primary()
                    .send(Command::SetInterestListMode(InterestListMode::Full360))
                    .await?;
                let leg = watch(ctx.primary(), &mut world, change_watch).await?;
                record(ctx.metrics(), "mode_360", &leg);
                full_360 = Some(leg);
                ctx.primary()
                    .send(Command::SetInterestListMode(InterestListMode::Default))
                    .await?;
                let leg = watch(ctx.primary(), &mut world, change_watch).await?;
                record(ctx.metrics(), "mode_default", &leg);
            }

            let back = camera([128.0, 128.0, 30.0], [1.0, 0.0]);
            ctx.primary().send(Command::SetCamera(back)).await?;
            let leg = watch(ctx.primary(), &mut world, change_watch).await?;
            record(ctx.metrics(), "camera_back", &leg);

            ctx.primary()
                .send(Command::SetDrawDistance(Distance::new(FAR_M)))
                .await?;
            let far_again = watch(ctx.primary(), &mut world, change_watch).await?;
            record(ctx.metrics(), "far_again", &far_again);

            let run = Run {
                arrival,
                near,
                full_360,
                far_again,
            };
            let primitives = run
                .arrival
                .added
                .get(&pcode::PRIMITIVE)
                .copied()
                .unwrap_or(0);
            if content_is_ours(grid) {
                // The OpenSim "Default Region" holds this workspace's rezzed test
                // objects and the fake grid's region is the fixture catalogue, so
                // on both at least one primitive must decode.
                check(
                    primitives >= 1,
                    "expected at least one primitive in the region's object stream",
                )?;
            } else if primitives == 0 {
                ctx.mark_partial("landing region streamed no primitives within the window");
            }
            check_everywhere(grid, granted, &run)?;
            if grid.is_fake() {
                Ok(())
            } else {
                check_live(grid, &run)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::camera;

    /// A level camera's left is a quarter turn anticlockwise of where it
    /// looks, and its up is up.
    #[test]
    fn a_level_camera_is_a_right_handed_frame() {
        let view = camera([1.0, 2.0, 3.0], [1.0, 0.0]);
        assert!(view.left_axis.x.abs() < 1e-6 && (view.left_axis.y - 1.0).abs() < 1e-6);
        assert!((view.up_axis.z - 1.0).abs() < 1e-6);
    }
}
