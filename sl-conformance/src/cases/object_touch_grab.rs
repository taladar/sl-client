//! Touch an object and drive a full press-drag-release grab cycle against it.
//!
//! The two ways a viewer physically interacts with an in-world object, both
//! exercised against one primitive:
//!
//! 1. **Touch** (a left-click): [`Command::TouchObject`] sends an `ObjectGrab`
//!    immediately followed by an `ObjectDeGrab` with no drag in between — the
//!    click that fires a script's `touch_start`/`touch_end` (or a
//!    `CLICK_ACTION_*` such as pay/buy).
//! 2. **Grab / drag / degrab** (a press-drag-release): [`Command::GrabObject`]
//!    begins the grab, [`Command::GrabObjectUpdate`] reports the drag as the
//!    avatar moves the object, and [`Command::DegrabObject`] releases it.
//!
//! All four are unacknowledged at the application layer — the simulator sends no
//! reply a viewer waits on (any visible effect is a *script's* reaction, which a
//! stock prim need not have). So "no error" cannot be read from a reply; it is
//! instead the circuit staying healthy — a keep-alive ping still round-trips —
//! after the interaction, exactly as [`super::draw_distance`] confirms an
//! unreliable `AgentUpdate`. The messages themselves are reliable, so a failure
//! to encode or enqueue any of them propagates from `send` and fails the case
//! before that check.
//!
//! The case first watches the region's object-update stream for a primitive to
//! act on (the same interest-list stream `object-update-decode` decodes and
//! `object-properties` queries), then touches it and runs the grab cycle. `1av`,
//! `[both]`. On OpenSim the avatar is forced into the "Default Region", which
//! holds this workspace's rezzed test object, so a primitive is guaranteed and
//! its absence fails the case. On Second Life the landing region's contents are
//! uncontrolled; a region that streams no primitive within the window is
//! recorded `partial` rather than failed. The aditi run is deferred with the
//! rest of the Aditi batch (no aditi record this session).

use std::time::Duration;

use sl_client_tokio::{Command, Event, Object, Vector, pcode};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, count_metric, is_opensim, secs_metric};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, where
/// this workspace's test object lives. On Second Life the avatar keeps `"last"`
/// (a named OpenSim region is meaningless there), and whatever region it lands
/// in supplies the object.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// How long to watch the object-update stream for a primitive to act on.
///
/// Generous enough to span the `ObjectUpdateCached` cache-miss round trip (a
/// digest whose full update this client refetches with `RequestMultipleObjects`)
/// and Second Life's larger, more staggered scene streaming.
const FIND_WINDOW: Duration = Duration::from_secs(20);

/// How long to observe the circuit after the interaction for a keep-alive ping.
///
/// The touch and grab messages have no application-level reply, so the circuit
/// staying healthy — a keep-alive ping (≈ 5 s interval) still round-tripping —
/// is the "no error" signal. Kept generous for Aditi network jitter and load.
const OBSERVE_WINDOW: Duration = Duration::from_secs(15);

/// The grab origin: the object's centre (a zero offset from it).
const CENTRE: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// The drag distance applied by the single `ObjectGrabUpdate`, in metres — a
/// small nudge along the region X axis, standing in for a viewer dragging the
/// grabbed object.
const DRAG_M: f32 = 0.25;

/// Milliseconds attributed to the single drag update (`time_since_last`).
const DRAG_MS: u32 = 100;

/// Touches an object and drives a press-drag-release grab cycle against it,
/// confirming the circuit stays healthy afterwards.
#[derive(Debug)]
pub struct ObjectTouchGrab;

impl GridTest for ObjectTouchGrab {
    fn name(&self) -> &'static str {
        "object-touch-grab"
    }

    fn description(&self) -> &'static str {
        "Touch and grab/degrab an object"
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
            let grid = ctx.grid();

            // Find a primitive in the region's interest-list stream to act on.
            // `ObjectAdded` fires on the first sighting of each region-local id;
            // the first one whose `PCode` is a primitive (not the self avatar) is
            // the object this case touches. A per-iteration timeout that consumes
            // the remaining window ends the search empty-handed.
            let object: Option<Object> = {
                let session = ctx.primary();
                session.wait_for_region(REGION_TIMEOUT).await?;
                match session
                    .wait_for(FIND_WINDOW, |event| match event {
                        Event::ObjectAdded(object) if object.pcode == pcode::PRIMITIVE => {
                            Some((**object).clone())
                        }
                        _ => None,
                    })
                    .await
                {
                    Ok(object) => Some(object),
                    // No primitive streamed inside the window; handled per grid
                    // below (a hard failure on OpenSim, `partial` on SL).
                    Err(TestFailure::Timeout(_)) => None,
                    Err(other) => return Err(other),
                }
            };

            let object = match object {
                Some(object) => object,
                None if is_opensim(grid) => {
                    return Err(TestFailure::Assertion(
                        "no primitive appeared in the Default Region object stream".to_owned(),
                    ));
                }
                None => {
                    // On Second Life the landing region's contents are
                    // uncontrolled; a region that streams no primitive in the
                    // window is a legitimately incomplete dataset.
                    ctx.mark_partial(
                        "landing region streamed no primitive to act on within the window",
                    );
                    return Ok(());
                }
            };
            let target_id = object.full_id;
            let scoped = object.scoped_id();
            // The object's region-local position, dragged a short way along X.
            let start = object.motion.position;
            let dragged = Vector {
                x: start.x + DRAG_M,
                y: start.y,
                z: start.z,
            };

            let session = ctx.primary();

            // 1. Touch (left-click): an `ObjectGrab` immediately followed by an
            //    `ObjectDeGrab`, with no drag between them.
            session
                .send(Command::TouchObject { local_id: scoped })
                .await?;

            // 2. A full press-drag-release: grab at the centre, drag once, then
            //    release. `ObjectGrabUpdate` is keyed by the persistent
            //    `object_id`, not the region-local id, so it uses `target_id`.
            session
                .send(Command::GrabObject {
                    local_id: scoped,
                    grab_offset: CENTRE,
                })
                .await?;
            session
                .send(Command::GrabObjectUpdate {
                    object_id: target_id,
                    grab_offset_initial: CENTRE,
                    grab_position: dragged,
                    time_since_last: DRAG_MS,
                })
                .await?;
            session
                .send(Command::DegrabObject { local_id: scoped })
                .await?;

            // The touch and grab messages carry no application-level reply, so a
            // keep-alive ping answered after them is the "no error" signal: the
            // interaction was accepted and the session is still live. Wait one
            // observation window for the next root-simulator ping; a
            // `Disconnected` mid-window propagates and fails the case.
            let rtt = match session
                .wait_for(OBSERVE_WINDOW, |event| match event {
                    Event::Ping {
                        child: false, rtt, ..
                    } => Some(*rtt),
                    _ => None,
                })
                .await
            {
                Ok(rtt) => rtt,
                // No ping inside the window means the circuit went quiet after
                // the interaction — the case's failure.
                Err(TestFailure::Timeout(_)) => {
                    return Err(TestFailure::Assertion(
                        "no keep-alive ping observed after touching and grabbing the object"
                            .to_owned(),
                    ));
                }
                Err(other) => return Err(other),
            };

            let metrics = ctx.metrics();
            metrics.set("object_id", target_id.to_string());
            metrics.set("pcode", i64::from(object.pcode));
            metrics.set(&count_metric("touches"), 1_i64);
            metrics.set(&count_metric("grab_updates"), 1_i64);
            metrics.set("drag_m", f64::from(DRAG_M));
            metrics.set_timing(&secs_metric("ping_rtt"), rtt.as_secs_f64());
            Ok(())
        })
    }
}
