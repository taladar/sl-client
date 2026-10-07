//! Accept a lure nobody offered and hold the grid to how it refuses.

use sl_client_tokio::{Command, LureId, Uuid};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, check_eq};
use crate::teleport_trace::watch_teleport;

/// Where each answer below is written down.
const SOURCE: &str = "book/src/gridspec/teleport.md (teleport-lure-unknown, 2026-10-07)";

/// A lure id no grid issued. It does not have the layout of OpenSim's packed
/// place either, so OpenSim reads it as a region that does not exist.
const UNKNOWN_LURE: Uuid = Uuid::from_u128(0x3b6b_7c62_8f8f_4e34_9c1a_79c2_e2ba_0fd1);

/// Whether the grid answers at all. Second Life says nothing — no start, no
/// failure, no alert — and the teleport ends on the client's own deadline.
const ANSWERED: Measured<bool> = Measured {
    second_life: false,
    opensim: true,
    source: SOURCE,
};

/// The reason of the `TeleportFailed` OpenSim answers with, sent instead of a
/// `TeleportStart` and with no alert beside it.
const OPENSIM_REASON: &str = "The region you tried to teleport to was not found";

/// Sends a `TeleportLureRequest` for a lure id nobody issued.
///
/// It is what a viewer meets when a lure has gone stale by the time its
/// Teleport button is pressed, provoked without a second avatar: the grid has
/// no such lure. **Second Life** says nothing whatever, and the teleport ends
/// thirty seconds later on the session's own deadline — which is why a client
/// needs one. **OpenSim** has nothing to look up — its lure ids *are* the
/// destination — so it unpacks the id into a region handle and a position,
/// fails to find the region, and refuses as it refuses any teleport to a
/// region that does not exist.
///
/// `1av`, and offline on both fake flavours.
#[derive(Debug)]
pub struct TeleportLureUnknown;

impl GridTest for TeleportLureUnknown {
    fn name(&self) -> &'static str {
        "teleport-lure-unknown"
    }

    fn description(&self) -> &'static str {
        "Accept a lure nobody offered and hold the refusal to the grid's own"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let origin = session.region_handle();

            session
                .send(Command::AcceptTeleportLure {
                    lure_id: LureId::from(UNKNOWN_LURE),
                })
                .await?;
            let trace = watch_teleport(session, REGION_TIMEOUT).await?;
            let now_in = session.region_handle();

            let metrics = ctx.metrics();
            trace.record("", metrics);

            let failure = trace.failure.as_ref().ok_or_else(|| {
                TestFailure::Assertion(format!(
                    "a lure nobody offered ended as [{}] rather than being refused",
                    trace.sequence()
                ))
            })?;
            check_eq("region after the refusal", &now_in, &origin)?;
            ANSWERED.check(
                "whether the grid answers a lure nobody offered",
                grid,
                &failure.from_grid,
            )?;
            check_eq(
                "the messages that ended it",
                &trace.sequence().as_str(),
                &"failed",
            )?;
            if failure.from_grid {
                check_eq(
                    "the refusal's reason",
                    &failure.reason.as_str(),
                    &OPENSIM_REASON,
                )?;
                check(
                    failure.alert.is_none(),
                    "OpenSim's refusal of a lure nobody offered carried an alert",
                )?;
            }
            Ok(())
        })
    }
}
