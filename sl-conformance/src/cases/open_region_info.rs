//! Observe OpenSim's `OpenRegionInfo` per-region limits bag.
//!
//! `OpenRegionInfo` is an OpenSim-specific CAPS event-queue push (Firestorm
//! `llpanelopenregionsettings.cpp`, delivered to `/message/OpenRegionInfo`): a
//! bag of per-region limits and client-behaviour hints that go beyond the
//! standard Second Life protocol — prim/link/scale limits, build bounds, the
//! `say`/`shout`/`whisper` chat ranges, a UTC offset, and so on. Second Life
//! never sends it; only OpenSim grids do, and only when the optional
//! `OpenRegionSettings` region module is loaded. The runtime already decodes the
//! push into a typed [`Event::OpenRegionInfo`] (rather than dropping it to a
//! `Diagnostic::UnknownCapsEvent`); this case observes that event and records
//! the advertised limits.
//!
//! The event is **pushed**, not requested: the simulator sends it over the event
//! queue shortly after the client's capabilities are established, so the case
//! waits for it after region arrival rather than issuing a command. Every field
//! of [`OpenRegionInfo`] is optional (the sim sends only the keys it overrides),
//! so the one invariant the case asserts is that the bag carries **at least one**
//! advertised field — an empty push would decode to all-`None`.
//!
//! When the grid does not load the `OpenRegionSettings` module the push never
//! arrives; the case then marks the run [partial](TestContext::mark_partial)
//! rather than failing, mirroring the other cases whose data depends on
//! optional grid configuration. The decode path itself is covered by `sl-proto`'s
//! unit tests. `[opensim] 1av`. No new client code — the CAPS event, its
//! [`OpenRegionInfo`] type, and the `open_region_info_from_llsd` parser already
//! existed; only the runtime crates gained a re-export of [`OpenRegionInfo`].

use std::time::Duration;

use sl_client_tokio::{Event, OpenRegionInfo};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, count_metric};

/// How long to wait for the `OpenRegionInfo` push after region arrival. The
/// event queue delivers it shortly after the capabilities are set up, so a grid
/// that loads the module answers well within this window; a grid without the
/// module waits it out and the run is marked partial.
const PUSH_TIMEOUT: Duration = Duration::from_secs(20);

/// Counts how many of an [`OpenRegionInfo`] bag's fields the grid advertised
/// (each `Some` field counts one). A richer configuration overrides more limits;
/// the count is recorded so the reporter can trend "override richness" and a
/// caller can tell an empty push (`0`) from a populated one.
fn advertised_count(info: &OpenRegionInfo) -> usize {
    [
        info.allow_minimap.is_some(),
        info.allow_physical_prims.is_some(),
        info.draw_distance.is_some(),
        info.force_draw_distance.is_some(),
        info.terrain_detail_scale.is_some(),
        info.max_drag_distance.is_some(),
        info.min_hole_size.is_some(),
        info.max_hollow_size.is_some(),
        info.max_inventory_items_transfer.is_some(),
        info.max_link_count.is_some(),
        info.max_link_count_phys.is_some(),
        info.max_position.is_some(),
        info.min_position.is_some(),
        info.max_prim_scale.is_some(),
        info.max_phys_prim_scale.is_some(),
        info.min_prim_scale.is_some(),
        info.offset_of_utc.is_some(),
        info.offset_of_utc_dst.is_some(),
        info.render_water.is_some(),
        info.say_distance.is_some(),
        info.shout_distance.is_some(),
        info.whisper_distance.is_some(),
        info.teen_mode.is_some(),
        info.show_tags.is_some(),
        info.enforce_max_build.is_some(),
        info.max_groups.is_some(),
        info.allow_parcel_windlight.is_some(),
    ]
    .into_iter()
    .filter(|advertised| *advertised)
    .count()
}

/// Observes OpenSim's `OpenRegionInfo` push and records the advertised limits.
///
/// Named `…Case` rather than `OpenRegionInfo` to avoid clashing with the
/// [`OpenRegionInfo`] bag type this case observes.
#[expect(
    clippy::module_name_repetitions,
    reason = "the bare `OpenRegionInfo` name is the bag type; the case struct needs a distinct name"
)]
#[derive(Debug)]
pub struct OpenRegionInfoCase;

impl GridTest for OpenRegionInfoCase {
    fn name(&self) -> &'static str {
        "open-region-info"
    }

    fn description(&self) -> &'static str {
        "Observe OpenSim's OpenRegionInfo per-region limits bag"
    }

    fn grids(&self) -> &'static [Grid] {
        // OpenSim-only: Second Life never sends `OpenRegionInfo`.
        &[Grid::Opensim]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // The push is unsolicited — wait for it rather than requesting it.
            // On a grid that loads the `OpenRegionSettings` module it arrives
            // over the event queue shortly after the capabilities are set up.
            let info = session
                .wait_for(PUSH_TIMEOUT, |event| match event {
                    Event::OpenRegionInfo(info) => Some(info.as_ref().clone()),
                    _ => None,
                })
                .await;

            let info = match info {
                Ok(info) => info,
                Err(_absent) => {
                    // No push within the window: the region does not load the
                    // optional `OpenRegionSettings` module (the standalone
                    // OpenSim default). Record nothing to compare against a
                    // configured grid's counts, and pass as partial.
                    ctx.mark_partial(
                        "region sent no OpenRegionInfo push \
                         (OpenRegionSettings module not loaded)",
                    );
                    return Ok(());
                }
            };

            let advertised = advertised_count(&info);
            check(
                advertised >= 1,
                "expected the OpenRegionInfo push to advertise at least one limit",
            )?;

            let metrics = ctx.metrics();
            metrics.set(
                &count_metric("advertised_limits"),
                i64::try_from(advertised).unwrap_or(-1),
            );
            if let Some(max_link_count) = info.max_link_count {
                metrics.set("max_link_count", i64::from(max_link_count));
            }
            if let Some(max_groups) = info.max_groups {
                metrics.set("max_groups", i64::from(max_groups));
            }
            if let Some(max_prim_scale) = info.max_prim_scale {
                metrics.set("max_prim_scale", f64::from(max_prim_scale));
            }
            if let Some(say_distance) = info.say_distance {
                metrics.set("say_distance", f64::from(say_distance));
            }
            if let Some(shout_distance) = info.shout_distance {
                metrics.set("shout_distance", f64::from(shout_distance));
            }
            if let Some(whisper_distance) = info.whisper_distance {
                metrics.set("whisper_distance", f64::from(whisper_distance));
            }
            Ok(())
        })
    }
}
