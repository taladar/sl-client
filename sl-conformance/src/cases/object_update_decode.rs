//! Receive and decode the region's object-update stream, counting primitives.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{Event, Object, RegionLocalObjectId, pcode};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, count_metric, is_opensim, secs_metric};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, where
/// this workspace's test object lives. On Second Life the avatar keeps `"last"`
/// (a named OpenSim region is meaningless there), and whatever region it lands
/// in supplies the objects.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// How long to observe the object-update stream after the region goes active.
///
/// After the region handshake the simulator streams the agent's interest list:
/// full `ObjectUpdate`s, compressed updates, and `ObjectUpdateCached` digests
/// (whose cache misses this client resolves with a `RequestMultipleObjects`, so
/// the full update — and its [`Event::ObjectAdded`] — follows a round trip
/// later). The window is generous enough to span that cache-miss round trip and
/// Second Life's larger, more staggered scene streaming.
const OBSERVE_WINDOW: Duration = Duration::from_secs(20);

/// Receives and decodes the region's object-update stream and counts the
/// primitives in it.
///
/// After login the simulator streams the agent's interest list as a mix of full
/// `ObjectUpdate`s, `ObjectUpdateCompressed`, and `ObjectUpdateCached` messages;
/// this client decodes each into a cached [`Object`] and surfaces the first
/// sighting of every region-local id as [`Event::ObjectAdded`]. The case
/// observes that stream for a window and tallies the objects by `PCode`:
/// primitives (ordinary prims), avatars, and everything else (trees, grass, …),
/// deduplicated by region-local id.
///
/// On the local OpenSim grid the avatar is forced into the "Default Region",
/// which holds this workspace's rezzed test object, so the case asserts at least
/// one primitive is decoded — proof the full-update decode and the cache-miss
/// refetch both work end to end. On Second Life the landing region's contents
/// are not controlled; a region that streams no primitives within the window is
/// recorded `partial` rather than failed. The primitive/avatar/total counts and
/// the latency to the first decoded object are recorded.
#[derive(Debug)]
pub struct ObjectUpdateDecode;

impl GridTest for ObjectUpdateDecode {
    fn name(&self) -> &'static str {
        "object-update-decode"
    }

    fn description(&self) -> &'static str {
        "Receive and decode the object-update stream, counting primitives"
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
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // Observe the object-update stream for the window, recording the
            // first sighting of every region-local id (an `ObjectAdded`) and the
            // latency to the very first one. A `Disconnected` mid-window
            // propagates and fails the case; a per-iteration timeout that
            // consumes the remaining window simply ends the observation.
            let mut seen: HashSet<RegionLocalObjectId> = HashSet::new();
            let mut primitives: u64 = 0;
            let mut avatars: u64 = 0;
            let mut others: u64 = 0;
            let mut first_object: Option<Duration> = None;
            let start = Instant::now();
            while let Some(remaining) = OBSERVE_WINDOW.checked_sub(start.elapsed()) {
                if remaining.is_zero() {
                    break;
                }
                match session
                    .wait_for(remaining, |event| match event {
                        Event::ObjectAdded(object) => Some((**object).clone()),
                        _ => None,
                    })
                    .await
                {
                    Ok(object) => {
                        // Deduplicate by region-local id: `ObjectAdded` already
                        // fires once per newly-seen id, but a belt-and-braces
                        // guard keeps the tally exact if that ever changes.
                        if !seen.insert(object.local_id) {
                            continue;
                        }
                        if first_object.is_none() {
                            first_object = Some(start.elapsed());
                        }
                        classify(&object, &mut primitives, &mut avatars, &mut others);
                    }
                    Err(TestFailure::Timeout(_)) => break,
                    Err(other) => return Err(other),
                }
            }

            let total = seen.len();
            if is_opensim(grid) {
                // The OpenSim "Default Region" holds this workspace's rezzed test
                // object, so at least one primitive must decode.
                check(
                    primitives >= 1,
                    "expected at least one primitive in the Default Region object stream",
                )?;
            } else if primitives == 0 {
                // On Second Life the landing region's contents are uncontrolled;
                // a region that streams no primitives in the window is a
                // legitimately incomplete dataset.
                ctx.mark_partial("landing region streamed no primitives within the window");
            }

            let metrics = ctx.metrics();
            metrics.set(
                &count_metric("primitives"),
                i64::try_from(primitives).unwrap_or(-1),
            );
            metrics.set(
                &count_metric("avatars"),
                i64::try_from(avatars).unwrap_or(-1),
            );
            metrics.set(&count_metric("other"), i64::try_from(others).unwrap_or(-1));
            metrics.set(&count_metric("objects"), i64::try_from(total).unwrap_or(-1));
            if let Some(latency) = first_object {
                metrics.set_timing(&secs_metric("first_object"), latency.as_secs_f64());
            }
            Ok(())
        })
    }
}

/// Tally one decoded object into the primitive / avatar / other buckets by its
/// `PCode`.
const fn classify(object: &Object, primitives: &mut u64, avatars: &mut u64, others: &mut u64) {
    match object.pcode {
        pcode::PRIMITIVE => *primitives = primitives.saturating_add(1),
        pcode::AVATAR => *avatars = avatars.saturating_add(1),
        _ => *others = others.saturating_add(1),
    }
}
