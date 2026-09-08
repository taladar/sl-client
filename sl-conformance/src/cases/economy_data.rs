//! Request the grid's economy data and confirm the price/capacity reply.

use sl_client_tokio::{Command, EconomyData, Event};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, send_then_wait};

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
/// This runs on every grid (`1av`), and asserts a different thing on each.
///
/// **On a live grid it records rather than asserts.** The prices are that
/// grid's policy and can be changed by the people who run it, so the only
/// claims worth making are that the reply parsed and that its capacity is
/// coherent. What the case does instead is write all seventeen fields down:
/// this is the only measurement of what a live grid's price list *is*, and
/// [`ImitatedGrid::prices`](sl_fake_grid::ImitatedGrid::prices) is derived from
/// the aditi run of it (2026-09-08).
///
/// **On a fake grid it asserts the whole table**, field for field, against the
/// flavour's own list. What that catches is a grid quoting the wrong grid: the
/// two lists differ in twelve of seventeen fields, so an
/// [`ImitatedGrid`](sl_fake_grid::ImitatedGrid) whose price list was not wired
/// to the flavour — or a builder default that quietly handed an
/// OpenSim-flavoured grid Second Life's prices — fails here naming both tables.
/// It runs on **both** fake flavours for that reason, unlike almost every other
/// offline case.
///
/// What it does **not** replace is the encoder-slot check: both sides of this
/// comparison come from the same constant, so a price written into the wrong
/// wire slot only fails when the two slots happen to differ, and neither real
/// list is all-distinct. That check lives where it belongs, in `sl-proto`'s own
/// `send_economy_data` round trip, over a synthetic table whose every amount is
/// a different number.
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
        &[Grid::FakeSl, Grid::FakeOpensim, Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
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

            // Every field of the reply, not a headline selection. This case is
            // the only measurement of what a grid's price list *is*, and
            // `ImitatedGrid::prices` answers a fake grid from it, so a field
            // left unrecorded is a field the fake grid has to invent. The two
            // Land Impact figures are region policy rather than grid policy and
            // the five scalars are `f32`, so the three groups are recorded with
            // the types they arrived in.
            let metrics = ctx.metrics();
            metrics.set("object_capacity", i64::from(data.object_capacity.0));
            metrics.set("object_count", i64::from(data.object_count.0));
            for (name, amount) in [
                ("price_energy_unit", &data.price_energy_unit),
                ("price_object_claim", &data.price_object_claim),
                ("price_public_object_decay", &data.price_public_object_decay),
                (
                    "price_public_object_delete",
                    &data.price_public_object_delete,
                ),
                ("price_parcel_claim", &data.price_parcel_claim),
                ("price_upload", &data.price_upload),
                ("price_rent_light", &data.price_rent_light),
                ("teleport_min_price", &data.teleport_min_price),
                ("price_parcel_rent", &data.price_parcel_rent),
            ] {
                metrics.set(name, i64::try_from(amount.0).unwrap_or(-1));
            }
            // The one price a grid may decline to quote, recorded as the `-1` it
            // arrived as rather than as a `0` — a grid that charges nothing for
            // a group and one that will not say are different answers, and a
            // record that flattened them would read as the former.
            metrics.set(
                "price_group_create",
                data.price_group_create
                    .as_ref()
                    .map_or(-1, |amount| i64::try_from(amount.0).unwrap_or(-1)),
            );
            for (name, scalar) in [
                ("price_parcel_claim_factor", data.price_parcel_claim_factor),
                ("teleport_price_exponent", data.teleport_price_exponent),
                ("energy_efficiency", data.energy_efficiency),
                ("price_object_rent", data.price_object_rent),
                ("price_object_scale_factor", data.price_object_scale_factor),
            ] {
                metrics.set(name, f64::from(scalar));
            }

            // On a fake grid the answer is knowable, so assert all seventeen
            // fields at once rather than spot-checking: the flavour picked the
            // list, and this is what makes "the grid quotes the grid it says it
            // is" a test rather than a claim in a doc comment.
            if let Some(imitates) = grid.imitates() {
                check_eq(&format!("{grid} price list"), &data, &imitates.prices())?;
            }
            Ok(())
        })
    }
}
