//! Log out while sitting on an object, with a second avatar watching, and
//! record what the grid does with the seat and where the next login lands.
//!
//! The sitter rezzes a cube beside itself, sits on it and logs out without
//! standing up; the observer stands at the same spot and looks at the seat.
//! Three things are measured:
//!
//! 1. **What the observer is sent.** The seated avatar is a child of the seat
//!    in the object stream; the logout has to take it out again. The case
//!    records how long after the `LogoutRequest` the avatar's `KillObject`
//!    reaches the observer, and whether the seat itself is re-sent.
//! 2. **The logout's own answer** while seated, as `logout-clean` records it
//!    standing.
//! 3. **The next login.** It asks for `last`: the case records whether the
//!    avatar comes back seated, and how far from the seat it stands if not.
//!
//! The sitter deletes its seat from the session it logs back in with,
//! whatever happened in between.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    Camera, Command, DeRezDestination, Event, FolderType, InventoryFolder, InventoryFolderKey,
    Object, ObjectKey, PrimShape, RegionLocalObjectId, ScopedObjectId, TransactionId, Uuid, Vector,
};

use crate::context::{Commander, LoginAnswer, LoginAttempt, Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    Logout, REGION_TIMEOUT, REPLY_TIMEOUT, count_metric, is_opensim, log_out, secs_metric,
    settle_scene_with_avatar, wait_for_own_new_object,
};

/// Where the measured answers are written down.
const SOURCE: &str = "book/src/gridspec/session.md § Logout (logout-seated, 2026-10-06)";

/// The OpenSim start location: the "Default Region", centred, where both
/// avatars land side by side.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The overall budget for settling each avatar's initial scene.
const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// The idle gap that ends a settle.
const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// How long to wait for the rezzed seat to appear.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the observer watches after the sitter's `LogoutRequest`: past the
/// client's own logout timeout, so a grid that removes the avatar only once
/// the circuit is gone is still seen doing it.
const WATCH: Duration = Duration::from_secs(20);

/// How far from the sitter the seat is rezzed, in metres: beside it, well
/// inside the range a grid sits an avatar from without walking it there.
const SEAT_OFFSET_M: f32 = 1.5;

/// How far back and up from a spot the camera that looks at it stands, in
/// metres.
const CAMERA_BACK_M: f32 = 4.0;

/// The case's overall budget: two avatars moved into place, a watch, and on a
/// grid that rate-limits logins the cooldown waited out before the sitter's
/// second login.
const CASE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Whether the observer is sent the seated avatar's `KillObject` after the
/// logout.
///
/// Only OpenSim's answer is a measurement, here and below: aditi did not
/// answer the sit request on any of three runs (2026-10-06), so the case has
/// yet to get as far as a seated logout there. Second Life's are what the
/// reference viewer expects.
const OBSERVER_SEES_KILL: Measured<bool> = Measured {
    second_life: true,
    opensim: true,
    source: SOURCE,
};

/// Whether the avatar is seated again when it next logs in.
const SEATED_AFTER_RELOGIN: Measured<bool> = Measured {
    second_life: false,
    opensim: false,
    source: SOURCE,
};

/// Deletes the seat if the case body never gets to (the runner's overall
/// timeout, an unwind): the awaited delete covers every path that returns.
struct DeleteOnDrop {
    /// The command channel of the session the sitter has at the moment.
    commander: Commander,
    /// The sitter's Trash folder, once its login skeleton has been read.
    trash: Option<InventoryFolderKey>,
    /// The seat as that session knows it, while it does.
    seat: Option<ScopedObjectId>,
}

impl Drop for DeleteOnDrop {
    fn drop(&mut self) {
        if let (Some(seat), Some(trash)) = (self.seat.take(), self.trash) {
            let _queued = self.commander.try_send(delete_command(seat, trash));
        }
    }
}

/// The command that deletes `seat` into `trash`.
fn delete_command(seat: ScopedObjectId, trash: InventoryFolderKey) -> Command {
    Command::DerezObjects {
        local_ids: vec![seat],
        destination: DeRezDestination::Trash(trash),
        transaction_id: TransactionId::from(Uuid::new_v4()),
        group_id: None,
    }
}

/// The Trash folder of a login skeleton, or failing that its root.
fn trash_folder(folders: &[InventoryFolder]) -> Option<InventoryFolderKey> {
    folders
        .iter()
        .find(|folder| folder.folder_type == FolderType::Trash.to_code())
        .or_else(|| folders.iter().find(|folder| folder.parent_id.is_none()))
        .map(|folder| folder.folder_id)
}

/// The straight-line distance between two region positions, in metres.
fn distance(a: &Vector, b: &Vector) -> f32 {
    let (dx, dy, dz) = (a.x - b.x, a.y - b.y, a.z - b.z);
    dx.mul_add(dx, dy.mul_add(dy, dz * dz)).sqrt()
}

/// What the observer saw in the [`WATCH`] after the sitter's `LogoutRequest`.
#[derive(Debug, Clone, Copy, Default)]
struct Watched {
    /// Seconds from the start of the watch to the avatar's removal.
    kill_secs: Option<f64>,
    /// How many times the seat was re-sent before the avatar was removed.
    seat_updates_before_kill: u32,
    /// How many times the seat was re-sent after it.
    seat_updates_after_kill: u32,
}

/// Watch the observer's object stream for [`WATCH`]: when the sitter's avatar
/// is removed, and what the seat does around it.
async fn watch_seat(
    observer: &mut Session,
    avatar: ScopedObjectId,
    seat: ScopedObjectId,
) -> Result<Watched, TestFailure> {
    let started = Instant::now();
    let mut watched = Watched::default();
    let outcome = observer
        .wait_for(WATCH, |event| {
            match event {
                Event::ObjectRemoved { local_id, .. } if *local_id == avatar => {
                    let _first = watched
                        .kill_secs
                        .get_or_insert_with(|| started.elapsed().as_secs_f64());
                }
                Event::ObjectUpdated(object) if object.scoped_id() == seat => {
                    if watched.kill_secs.is_some() {
                        watched.seat_updates_after_kill =
                            watched.seat_updates_after_kill.saturating_add(1);
                    } else {
                        watched.seat_updates_before_kill =
                            watched.seat_updates_before_kill.saturating_add(1);
                    }
                }
                _other => {}
            }
            None::<()>
        })
        .await;
    match outcome {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(watched),
        Err(other) => Err(other),
    }
}

/// The first sighting of the avatar `agent` in `session`'s object stream for
/// which `accept` holds.
async fn sight_avatar(
    session: &mut Session,
    agent: Uuid,
    timeout: Duration,
    accept: impl Fn(&Object) -> bool + Send + Sync,
) -> Result<Object, TestFailure> {
    session
        .wait_for(timeout, |event| match event {
            Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                if object.full_id.uuid() == agent && accept(object) =>
            {
                Some((**object).clone())
            }
            _other => None,
        })
        .await
}

/// Point `session`'s camera at `spot`. A grid streams objects by where the
/// agent's camera is, not its avatar, so this is what brings the seat into
/// the stream of an avatar standing somewhere else in the region.
async fn look_at(session: &Session, spot: &Vector) -> Result<(), TestFailure> {
    session
        .send(Command::SetCamera(Camera::looking_at(
            Vector {
                x: spot.x - CAMERA_BACK_M,
                y: spot.y,
                z: spot.z + CAMERA_BACK_M,
            },
            spot.clone(),
        )))
        .await
}

/// The first sighting of the object `full_id` in `session`'s stream.
async fn sight_object(
    session: &mut Session,
    full_id: ObjectKey,
    timeout: Duration,
) -> Result<Object, TestFailure> {
    session
        .wait_for(timeout, |event| match event {
            Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                if object.full_id == full_id =>
            {
                Some((**object).clone())
            }
            _other => None,
        })
        .await
}

/// What the setup hands the measurement.
struct Seated {
    /// The seat, as the sitter's own stream showed it.
    seat: Object,
    /// The seat in the observer's stream.
    observed_seat: ScopedObjectId,
    /// The sitter's avatar in the observer's stream.
    observed_avatar: ScopedObjectId,
}

/// Move the sitter into place, rez the seat beside it, sit it down, and have
/// the observer find both.
async fn sit_down(ctx: &mut TestContext, guard: &mut DeleteOnDrop) -> Result<Seated, TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let build_height = build_position.as_ref().map(|position| position.z);
    let two_accounts =
        || TestFailure::Assertion("this case needs a second avatar (--secondary)".to_owned());

    let sitter = ctx.primary();
    let sitter_id = sitter
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    // The Trash folder comes from the login skeleton, which is emitted before
    // the region is ready.
    let folders = sitter
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::InventorySkeleton(folders) => Some(folders.clone()),
            _other => None,
        })
        .await?;
    guard.trash = trash_folder(&folders);
    sitter.wait_for_region(REGION_TIMEOUT).await?;
    let settled = settle_scene_with_avatar(
        sitter,
        grid,
        build_position.clone(),
        SETTLE_WINDOW,
        SETTLE_IDLE,
    )
    .await?;
    let sitter_at = settled.avatar.ok_or_else(|| {
        TestFailure::Assertion("the sitter never appeared in its own object stream".to_owned())
    })?;
    // The observer comes to the same spot — before the seat exists, so that
    // settling its scene does not swallow the seat's arrival. From where a
    // login lands on aditi, 70 m off, a camera pointed at the seat was not
    // enough: the cube never entered the observer's stream.
    let observer = ctx.secondary().ok_or_else(two_accounts)?;
    observer.wait_for_region(REGION_TIMEOUT).await?;
    let _settled =
        settle_scene_with_avatar(observer, grid, build_position, SETTLE_WINDOW, SETTLE_IDLE)
            .await?;
    // At the build location's own height where there is one: the flight
    // there ends in the air and the avatar then drops to the ground, so the
    // height it arrived at is not where it stands.
    let seat_height = build_height.unwrap_or(sitter_at.z);
    let sitter = ctx.primary();
    sitter
        .send(Command::RezObject {
            shape: PrimShape::cube(Vector {
                x: sitter_at.x + SEAT_OFFSET_M,
                y: sitter_at.y,
                z: seat_height,
            }),
            group_id: None,
        })
        .await?;
    let seat = wait_for_own_new_object(sitter, &settled.seen, STEP_TIMEOUT)
        .await?
        .map_err(|reason| TestFailure::Assertion(format!("the seat was not rezzed: {reason}")))?;
    guard.seat = Some(seat.scoped_id());

    let observer = ctx.secondary().ok_or_else(two_accounts)?;
    look_at(observer, &seat.motion.position).await?;
    let observed_seat = sight_object(observer, seat.full_id, STEP_TIMEOUT)
        .await?
        .scoped_id();

    let sitter = ctx.primary();
    sitter
        .send(Command::Sit {
            target: seat.full_id,
            offset: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        })
        .await?;
    let autopilot = sitter
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::SitResult {
                sit_object,
                autopilot,
                ..
            } if *sit_object == seat.full_id => Some(*autopilot),
            _other => None,
        })
        .await?;
    // Recorded, not asserted: OpenSim sets the flag on every sit response,
    // whatever the distance.
    ctx.metrics().set("sit_autopilot", autopilot);

    // The observer sees the sitter as a child of the seat.
    let observer = ctx.secondary().ok_or_else(two_accounts)?;
    let seat_local = seat.local_id;
    let avatar = sight_avatar(observer, sitter_id, REPLY_TIMEOUT, |object| {
        object.parent_id == seat_local
    })
    .await?;
    Ok(Seated {
        seat,
        observed_seat,
        observed_avatar: avatar.scoped_id(),
    })
}

/// Log the seated sitter out under the observer's eyes, then log it back in
/// at `last` and see where it is.
async fn measure(
    ctx: &mut TestContext,
    guard: &mut DeleteOnDrop,
    seated: &Seated,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let (logout, watched): (Logout, Watched) = {
        let (sitter, observer) = ctx.primary_and_secondary().ok_or_else(|| {
            TestFailure::Assertion("this case needs a second avatar (--secondary)".to_owned())
        })?;
        let (logout, watched) = tokio::join!(
            log_out(sitter),
            watch_seat(observer, seated.observed_avatar, seated.observed_seat)
        );
        (logout?, watched?)
    };
    // The sitter's session is gone, and its commands with it.
    guard.seat = None;
    let metrics = ctx.metrics();
    metrics.set_timing(&secs_metric("logout"), logout.seconds);
    metrics.set("logout_reply_received", logout.reply_received);
    metrics.set("observer_saw_kill", watched.kill_secs.is_some());
    if let Some(kill) = watched.kill_secs {
        metrics.set_timing(&secs_metric("observer_kill"), kill);
    }
    metrics.set(
        &count_metric("seat_updates_before_kill"),
        watched.seat_updates_before_kill,
    );
    metrics.set(
        &count_metric("seat_updates_after_kill"),
        watched.seat_updates_after_kill,
    );

    // The next login, at `last` rather than wherever the case was placed.
    let sitter_id = ctx
        .primary()
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    let answer = ctx
        .primary()
        .attempt_login(&LoginAttempt {
            start: Some("last".to_owned()),
            ..LoginAttempt::default()
        })
        .await?;
    let seated_again = match answer {
        LoginAnswer::Admitted(mut next) => {
            ctx.metrics().set("next_login", "admitted");
            let own = sight_avatar(&mut next, sitter_id, REGION_TIMEOUT, |_object| true).await?;
            let seated_again = own.parent_id != RegionLocalObjectId(0);
            let metrics = ctx.metrics();
            metrics.set("relogin_seated", seated_again);
            if !seated_again {
                metrics.set(
                    "relogin_distance_from_seat_m",
                    f64::from(distance(&own.motion.position, &seated.seat.motion.position)),
                );
            }
            // The runner logs the primary out, and the seat is the sitter's
            // to delete: hand both the live session.
            guard.commander = next.commander();
            *ctx.primary() = *next;
            seated_again
        }
        LoginAnswer::Refused { failure, .. } => {
            let metrics = ctx.metrics();
            metrics.set("next_login", format!("refused: {}", failure.reason));
            metrics.set("next_login_message", failure.message);
            return Err(TestFailure::Assertion(
                "the login after a seated logout was refused".to_owned(),
            ));
        }
        LoginAnswer::Challenged(challenge) => {
            return Err(TestFailure::Assertion(format!(
                "the login after the logout was challenged: {}",
                challenge.message
            )));
        }
    };

    OBSERVER_SEES_KILL.check(
        "whether the observer is sent the seated avatar's removal",
        grid,
        &watched.kill_secs.is_some(),
    )?;
    SEATED_AFTER_RELOGIN.check(
        "whether the avatar is seated again at its next login",
        grid,
        &seated_again,
    )
}

/// Delete the seat from whichever session the sitter has now: find it in the
/// stream (looking at it, since the login may have landed elsewhere), delete
/// it and wait for it to go.
async fn delete_seat(
    sitter: &mut Session,
    seat: &Object,
    trash: InventoryFolderKey,
) -> Result<(), TestFailure> {
    look_at(sitter, &seat.motion.position).await?;
    let scoped = sight_object(sitter, seat.full_id, STEP_TIMEOUT)
        .await?
        .scoped_id();
    sitter.send(delete_command(scoped, trash)).await?;
    sitter
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::ObjectRemoved { local_id, .. } if *local_id == scoped => Some(()),
            _other => None,
        })
        .await
}

/// Logs a seated avatar out while a second one watches the seat.
#[derive(Debug)]
pub struct LogoutSeated;

impl GridTest for LogoutSeated {
    fn name(&self) -> &'static str {
        "logout-seated"
    }

    fn description(&self) -> &'static str {
        "Log out while seated: what an observer is sent, and where the next login lands"
    }

    fn grids(&self) -> &'static [Grid] {
        // Not the fake grid: its residents are not shown to each other, so
        // there is no observer to send the removal to.
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn rezzes_objects(&self) -> bool {
        true
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
            let mut guard = DeleteOnDrop {
                commander: ctx.primary().commander(),
                trash: None,
                seat: None,
            };
            // A failure here leaves the guard armed: the sitter's session is
            // still the one that rezzed the seat, and dropping the guard
            // queues the delete on it.
            let seated = sit_down(ctx, &mut guard).await?;
            let outcome = measure(ctx, &mut guard, &seated).await;
            guard.seat = None;
            let deleted = match guard.trash {
                Some(trash) => delete_seat(ctx.primary(), &seated.seat, trash).await,
                None => Ok(()),
            };
            outcome.and(deleted)
        })
    }
}
