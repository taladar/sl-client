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
//! 1. **Announced** — an [`Event::NeighborDiscovered`] naming the region's
//!    handle.
//! 2. **Seeded** — an [`Event::NeighborSeed`] for the same simulator address.
//!    The client POSTs it itself; without that POST a real simulator withholds
//!    the neighbour's objects entirely (its `SendInitialData` is gated on it).
//! 3. **Streaming** — an object stamped with the *neighbour's* region handle,
//!    which can only have come down the child circuit.
//!
//! On the fake grid the neighbour and its contents are known, so the case
//! names them: the region to the east, and the border scene's marker pillar,
//! which nothing in the agent's own region could be mistaken for. On a live
//! grid the neighbours are whatever the avatar is standing next to, so the
//! case holds every one of them to the first two facts and the neighbourhood
//! as a whole to the third, and records how they were announced
//! (`book/src/gridspec/teleport.md`, *Neighbours and crossings*): Second Life
//! names all of them in one event-queue batch, OpenSim one every half second.

use std::time::Duration;

use sl_client_tokio::{Event, RegionHandle};

use crate::context::{TestContext, TestFailure};
use crate::crossing::watch_neighbours;
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, check_eq, is_fake, is_opensim, secs_metric};

/// Where the measured answers are written down.
const SOURCE: &str = "book/src/gridspec/teleport.md § Neighbours and crossings \
                      (neighbour-child-circuits, 2026-10-07)";

/// Where the avatar logs in on OpenSim: the middle of the block's
/// south-western region, which has three neighbours.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// How long a live grid's arrival is watched for its neighbours.
const LIVE_WINDOW: Duration = Duration::from_secs(40);

/// The gap without news of a neighbour that ends the watch.
const LIVE_QUIET: Duration = Duration::from_secs(10);

/// Two announcements further apart than this were not sent together.
const TOGETHER: Duration = Duration::from_millis(200);

/// Whether a region's neighbours are all announced at once. Second Life names
/// them in one event-queue batch; OpenSim sleeps half a second between them
/// (`EntityTransferModule.EnableChildAgents`).
const ANNOUNCED_TOGETHER: Measured<bool> = Measured {
    second_life: true,
    opensim: false,
    source: SOURCE,
};

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
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
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
            if !is_fake(ctx.grid()) {
                return live_neighbours(ctx).await;
            }
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

/// The live grids' run: whatever neighbours the avatar's region has, each
/// announced and seeded, and at least one of them streaming.
async fn live_neighbours(ctx: &mut TestContext) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let session = ctx.primary();
    session.wait_for_region(REGION_TIMEOUT).await?;
    let agent = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let home = session.region_handle();
    let found = watch_neighbours(session, agent, LIVE_WINDOW, LIVE_QUIET).await?;
    found.record("neighbours_", home, ctx.metrics());
    if found.neighbours.is_empty() {
        ctx.mark_partial("the region the avatar stands in announced no neighbours");
        return Ok(());
    }
    for neighbour in &found.neighbours {
        check(
            neighbour.seeded.is_some(),
            &format!(
                "the neighbour at {:?} was announced and never seeded",
                neighbour.info.grid_coordinates
            ),
        )?;
    }
    check(
        found
            .neighbours
            .iter()
            .any(|neighbour| neighbour.streamed.is_some()),
        "no neighbour streamed an object down its child circuit",
    )?;
    let offsets: Vec<f64> = found
        .neighbours
        .iter()
        .map(|neighbour| neighbour.announced)
        .collect();
    let spread = offsets.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - offsets.iter().copied().fold(f64::INFINITY, f64::min);
    // One neighbour cannot be announced apart from itself.
    if found.neighbours.len() > 1 {
        ANNOUNCED_TOGETHER.check(
            "whether a region's neighbours are announced together",
            grid,
            &(spread <= TOGETHER.as_secs_f64()),
        )?;
    } else {
        ctx.mark_partial("one neighbour says nothing about how several are announced");
    }
    ctx.metrics().set("announcement_spread_secs", spread);
    Ok(())
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
