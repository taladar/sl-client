//! The region next door is announced, seeded, and streams its scene.
//!
//! A simulator tells an arriving agent about each adjacent region
//! (`EnableSimulator`) and then hands over that neighbour's seed capability
//! (`EstablishAgentCommunication`). The client opens a **child** circuit to each
//! and POSTs the seed, after which the neighbour streams its own scene down that
//! circuit — which is why you can see across a border before you walk over it,
//! and why walking over one is a promotion rather than a reconnection.
//!
//! Three facts, in the order they have to happen:
//!
//! 1. **Announced** — an [`Event::NeighborDiscovered`] naming the eastern
//!    region's handle.
//! 2. **Seeded** — an [`Event::NeighborSeed`] for the same simulator address.
//!    The client POSTs it itself; without that POST a real simulator withholds
//!    the neighbour's objects entirely (its `SendInitialData` is gated on it).
//! 3. **Streaming** — an object stamped with the *neighbour's* region handle,
//!    which can only have come down the child circuit. The border scene's marker
//!    pillar is the object; nothing in the agent's own region carries that
//!    handle.
//!
//! Fake grid only. This needs two adjacent regions whose contents are known, and
//! neither live grid reliably offers a pair an avatar may walk between — the
//! very reason the render catalogue's roadmap task handed this case here.

use std::time::Duration;

use sl_client_tokio::{Event, RegionHandle};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check_eq, secs_metric};

/// How long to wait for each leg of the neighbour hand-shake. Generous: the
/// seed POST is an HTTP round trip the client makes on its own, and the
/// neighbour's first object update follows it.
const NEIGHBOUR_TIMEOUT: Duration = Duration::from_secs(30);

/// Observes the neighbour announcement, its seed capability, and the first
/// object the child circuit streams.
#[derive(Debug)]
pub struct NeighbourChildCircuits;

impl GridTest for NeighbourChildCircuits {
    fn name(&self) -> &'static str {
        "neighbour-child-circuits"
    }

    fn description(&self) -> &'static str {
        "A neighbouring region is announced, seeded, and streams its scene to a child circuit"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Fake]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let east = east_region_handle(ctx)?;
            let started = std::time::Instant::now();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // 1. The announcement, and the simulator address it names — the key
            //    the child circuit is opened under.
            let sim = session
                .wait_for(NEIGHBOUR_TIMEOUT, |event| match event {
                    Event::NeighborDiscovered(info) if info.region_handle == east => Some(info.sim),
                    _other => None,
                })
                .await?;

            // 2. The seed capability for that same simulator.
            let seed_sim = session
                .wait_for(NEIGHBOUR_TIMEOUT, |event| match event {
                    Event::NeighborSeed { sim: from, .. } => Some(*from),
                    _other => None,
                })
                .await?;
            check_eq("neighbour seed simulator", &seed_sim, &sim)?;

            // 3. An object stamped with the neighbour's handle: the child
            //    circuit is not merely open, it is carrying the region.
            let local_id = session
                .wait_for(NEIGHBOUR_TIMEOUT, |event| match event {
                    Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                        if object.region_handle == east =>
                    {
                        Some(object.local_id)
                    }
                    _other => None,
                })
                .await?;
            check_eq(
                "the neighbour's streamed object",
                &local_id,
                &sl_fake_grid::fixtures::border::MARKER_LOCAL_ID,
            )?;

            let elapsed = started.elapsed().as_secs_f64();
            ctx.metrics().set_timing(&secs_metric("neighbour"), elapsed);
            Ok(())
        })
    }
}

/// The handle of the region east of the one the agent logged into, from the
/// grid itself.
fn east_region_handle(ctx: &TestContext) -> Result<RegionHandle, TestFailure> {
    let fake = ctx
        .fake()
        .ok_or_else(|| TestFailure::Assertion("this case runs on the fake grid only".to_owned()))?;
    fake.grid()
        .region_handle(crate::fake::EAST_REGION)
        .ok_or_else(|| {
            TestFailure::Assertion(format!(
                "the grid serves no region called {:?}",
                crate::fake::EAST_REGION
            ))
        })
}
