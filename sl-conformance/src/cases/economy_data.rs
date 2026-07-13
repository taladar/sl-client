//! Request the grid's economy data and confirm the price/capacity reply.

use sl_client_tokio::{Command, EconomyData, Event};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, send_then_wait};

/// Requests the grid's economy data and confirms the price/capacity reply.
///
/// A viewer asks the simulator for the grid-wide economy prices (asset upload,
/// object/parcel claim, group creation, teleport minimum, ...) and the region's
/// object capacity with an `EconomyDataRequest`; the simulator answers with a
/// single `EconomyData` message, surfaced here as [`Event::EconomyData`]. The
/// case issues the request and awaits that reply, then asserts the region
/// capacity is sane (a positive object capacity, and a current count that does
/// not exceed it) and records the headline prices and capacity as metrics.
///
/// The prices themselves are grid policy — OpenSim's defaults differ from
/// Second Life's — so the case does not assert specific amounts, only that the
/// reply parsed and carries a coherent capacity. This runs on both grids
/// (`1av`): OpenSim answers from its `EconomyDataRequest` handler and Aditi from
/// the live grid economy.
///
/// Named `…Case` rather than `EconomyData` to avoid clashing with the
/// [`EconomyData`] reply type this case decodes.
#[expect(
    clippy::module_name_repetitions,
    reason = "the bare `EconomyData` name is the reply type; the case struct needs a distinct name"
)]
#[derive(Debug)]
pub struct EconomyDataCase;

impl GridTest for EconomyDataCase {
    fn name(&self) -> &'static str {
        "economy-data"
    }

    fn description(&self) -> &'static str {
        "request economy data"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // Issue the `EconomyDataRequest` and await the single `EconomyData`
            // reply carrying the grid prices and this region's object capacity.
            let data: EconomyData = send_then_wait(
                session,
                Command::RequestEconomyData,
                REPLY_TIMEOUT,
                |event| match event {
                    Event::EconomyData(data) => Some((**data).clone()),
                    _ => None,
                },
            )
            .await?;

            // The prices are grid policy (OpenSim's defaults are not SL's), so
            // assert only the region capacity is coherent: a positive object
            // capacity (Land Impact budget), with the current usage within it.
            // The Land Impact fields are `u32`, so non-negativity is guaranteed
            // by the type; the meaningful checks are a non-zero budget and a
            // usage that does not exceed it.
            check(
                data.object_capacity.0 > 0,
                "expected a positive region object capacity in the economy data",
            )?;
            check(
                data.object_count <= data.object_capacity,
                "expected the region object usage to be within the capacity",
            )?;

            let metrics = ctx.metrics();
            metrics.set("object_capacity", i64::from(data.object_capacity.0));
            metrics.set("object_count", i64::from(data.object_count.0));
            metrics.set(
                "price_upload",
                i64::try_from(data.price_upload.0).unwrap_or(-1),
            );
            metrics.set(
                "price_group_create",
                i64::try_from(data.price_group_create.0).unwrap_or(-1),
            );
            metrics.set(
                "teleport_min_price",
                i64::try_from(data.teleport_min_price.0).unwrap_or(-1),
            );
            metrics.set(
                "price_parcel_claim",
                i64::try_from(data.price_parcel_claim.0).unwrap_or(-1),
            );
            Ok(())
        })
    }
}
