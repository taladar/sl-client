//! One avatar offers another a teleport and the other accepts it — once from
//! within the offerer's region and once from the region next door — and the
//! whole exchange is recorded: the offer as it was delivered, the teleport it
//! led to, and what the offerer was told.

use std::time::Instant;

use sl_client_tokio::{Command, GridCoordinates, RegionHandle, TeleportFlags};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::lure::{
    NOTICE_WINDOW, id_kind, offer_and_receive, offered_id, record_im, record_notices, watch_notices,
};
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, check_eq, secs_metric};
use crate::teleport_trace::{neighbouring_region, request_teleport, watch_teleport};

/// Where each answer below is written down.
const SOURCE: &str = "book/src/gridspec/teleport.md (teleport-offer-accept, 2026-10-07)";

/// Where both avatars log in on OpenSim: one spot of the region the grid's
/// other three border.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// Where the accepter asks to land in the neighbouring region before the second
/// offer.
const NEXT_DOOR: (f32, f32, f32) = (128.0, 128.0, 30.0);

/// How a lure id reads: OpenSim packs the offerer's region and position into
/// it, Second Life's says nothing.
pub(crate) const LURE_ID_KIND: Measured<&str> = Measured {
    second_life: "opaque",
    opensim: "place",
    source: SOURCE,
};

/// The order of the messages of an accepted lure from within the offerer's
/// region. Second Life puts a progress line between the start and the end.
const LOCAL_SEQUENCE: Measured<&str> = Measured {
    second_life: "started,progress,local",
    opensim: "started,local",
    source: SOURCE,
};

/// The flags of every message of an accepted lure from within the offerer's
/// region.
const LOCAL_FLAGS: Measured<u32> = Measured {
    second_life: TeleportFlags::VIA_LURE | TeleportFlags::WITHIN_REGION,
    opensim: TeleportFlags::VIA_LURE,
    source: SOURCE,
};

/// The progress lines of an accepted lure from within the offerer's region.
const LOCAL_LINES: Measured<&[&str]> = Measured {
    second_life: &["completing"],
    opensim: &[],
    source: SOURCE,
};

/// The order of the messages of an accepted lure from another region.
const REMOTE_SEQUENCE: Measured<&str> = Measured {
    second_life: "started,progress,progress,progress,finished,region-changed",
    opensim: "started,finished,region-changed",
    source: SOURCE,
};

/// The progress lines of an accepted lure from another region: Second Life's
/// `completing` comes *first*, ahead of the two lines every teleport to a
/// location has.
const REMOTE_LINES: Measured<&[&str]> = Measured {
    second_life: &["completing", "resolving", "Sending to destination."],
    opensim: &[],
    source: SOURCE,
};

/// The flags of the `TeleportStart` and each progress line of an accepted lure
/// from another region.
const REMOTE_FLAGS: Measured<u32> = Measured {
    second_life: TeleportFlags::VIA_LURE,
    opensim: TeleportFlags::VIA_LURE,
    source: SOURCE,
};

/// The flags of its `TeleportFinish`. OpenSim's says `VIA_LOCATION` whatever
/// kind of teleport it finishes.
const FINISH_FLAGS: Measured<u32> = Measured {
    second_life: TeleportFlags::VIA_LURE,
    opensim: TeleportFlags::VIA_LOCATION,
    source: SOURCE,
};

/// What the offer's binary bucket holds: on Second Life the destination, as
/// text; on OpenSim nothing.
const OFFER_HAS_BUCKET: Measured<bool> = Measured {
    second_life: true,
    opensim: false,
    source: SOURCE,
};

/// Offers a teleport, accepts it from the offerer's own region, then again from
/// the region next door.
///
/// A lure offer is a `StartLure` from the offerer, which the grid delivers to
/// the target as an `ImprovedInstantMessage` of dialog `IM_LURE_USER`. The
/// message's id is the lure id the target quotes back in a `TeleportLureRequest`
/// to accept. Everything else about the offer differs by grid — what the id is,
/// what the binary bucket holds, whose position the message states — and so
/// does the teleport an acceptance starts, which is an ordinary one flagged
/// `VIA_LURE`.
///
/// The offerer (the primary) stays put; the accepter (the secondary) accepts
/// once standing beside it, teleports to a neighbouring region, and accepts a
/// second offer from there. After each acceptance the case watches the offerer
/// for [`NOTICE_WINDOW`], since what a grid tells the *offerer* about an
/// accepted lure is one of the things being measured.
///
/// `2av`.
#[derive(Debug)]
pub struct TeleportOfferAccept;

impl GridTest for TeleportOfferAccept {
    fn name(&self) -> &'static str {
        "teleport-offer-accept"
    }

    fn description(&self) -> &'static str {
        "Offer a teleport and accept it from the same region and from the next, recording both"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        match grid {
            Grid::Aditi => super::teleport_cross_region::ADITI_START,
            Grid::Opensim => OPENSIM_START,
            Grid::FakeSl | Grid::FakeOpensim => "last",
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let (primary, secondary) = ctx.primary_and_secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            primary.wait_for_region(REGION_TIMEOUT).await?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;
            let offerer_region = primary.region_handle().ok_or_else(|| {
                TestFailure::Assertion("the offerer's login reported no region handle".to_owned())
            })?;
            let started_together = secondary.region_handle() == Some(offerer_region);
            let primary_id = crate::lure::agent_id(primary, "offerer")?;

            // --- The first offer, as the avatars stand.
            let message = format!("sl-conformance teleport-offer-accept first {primary_id}");
            let offered_at = Instant::now();
            let first_offer = offer_and_receive(primary, secondary, &message).await?;
            let offer_rtt = offered_at.elapsed();
            let offerer_after_offer = watch_notices(primary, NOTICE_WINDOW).await?;

            let accepted_at = Instant::now();
            secondary
                .send(Command::AcceptTeleportLure {
                    lure_id: offered_id(&first_offer),
                })
                .await?;
            let first = watch_teleport(secondary, REGION_TIMEOUT).await?;
            let first_rtt = accepted_at.elapsed();
            let after_first = secondary.region_handle();
            let offerer_after_accept = watch_notices(primary, NOTICE_WINDOW).await?;
            let accepter_after_accept = watch_notices(secondary, NOTICE_WINDOW).await?;

            // --- The accepter steps into the region next door, and is offered
            // a teleport back.
            let next_door =
                neighbouring_region(secondary, GridCoordinates::from(offerer_region)).await?;
            let next_door_handle = RegionHandle::from(next_door.grid_coordinates);
            request_teleport(secondary, next_door_handle, NEXT_DOOR, (1.0, 0.0, 0.0)).await?;
            let away = watch_teleport(secondary, REGION_TIMEOUT).await?;
            // Nobody accepts a lure two milliseconds after arriving somewhere,
            // and OpenSim cannot take it: the region just left is still
            // waiting to see the agent settle in the new one, finds it gone
            // again, and fails the *first* teleport twenty-five seconds later
            // while the second hangs.
            let settling = watch_notices(secondary, NOTICE_WINDOW).await?;

            let message = format!("sl-conformance teleport-offer-accept second {primary_id}");
            let second_offer = offer_and_receive(primary, secondary, &message).await?;
            let accepted_at = Instant::now();
            secondary
                .send(Command::AcceptTeleportLure {
                    lure_id: offered_id(&second_offer),
                })
                .await?;
            let second = watch_teleport(secondary, REGION_TIMEOUT).await?;
            let second_rtt = accepted_at.elapsed();
            let after_second = secondary.region_handle();
            let offerer_after_second = watch_notices(primary, NOTICE_WINDOW).await?;

            let metrics = ctx.metrics();
            metrics.set("started_together", started_together);
            record_im("offer_", &first_offer, metrics);
            record_im("second_offer_", &second_offer, metrics);
            metrics.set("lure_ids_differ", first_offer.id != second_offer.id);
            record_notices("offerer_after_offer", &offerer_after_offer, metrics);
            record_notices("offerer_after_accept", &offerer_after_accept, metrics);
            record_notices("accepter_after_accept", &accepter_after_accept, metrics);
            record_notices("offerer_after_second", &offerer_after_second, metrics);
            record_notices("accepter_settling_next_door", &settling, metrics);
            first.record("first_", metrics);
            away.record("away_", metrics);
            second.record("second_", metrics);
            metrics.set_timing(&secs_metric("offer_rtt"), offer_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("first_teleport"), first_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("second_teleport"), second_rtt.as_secs_f64());

            // Both acceptances brought the accepter to the offerer's region.
            check(
                first.failure.is_none(),
                "the first accepted lure was refused",
            )?;
            check_eq(
                "region after the first lure",
                &after_first,
                &Some(offerer_region),
            )?;
            check(
                away.arrival.is_some(),
                "the accepter did not reach the region next door",
            )?;
            check(
                second.failure.is_none(),
                "the second accepted lure was refused",
            )?;
            check_eq(
                "region after the second lure",
                &after_second,
                &Some(offerer_region),
            )?;

            // The shape of each, which is what the grids are held to.
            LURE_ID_KIND.check("how a lure id reads", grid, &id_kind(&first_offer))?;
            if started_together {
                LOCAL_SEQUENCE.check(
                    "the messages of a lure accepted within the offerer's region",
                    grid,
                    &first.sequence().as_str(),
                )?;
                for flags in &first.starts {
                    LOCAL_FLAGS.check("a local lure's TeleportStart flags", grid, flags)?;
                }
                for (_line, flags) in &first.progress {
                    LOCAL_FLAGS.check("a local lure's TeleportProgress flags", grid, flags)?;
                }
                LOCAL_LINES.check(
                    "a local lure's progress lines",
                    grid,
                    &first.lines().as_slice(),
                )?;
                if let Some(local) = &first.local {
                    LOCAL_FLAGS.check("a local lure's TeleportLocal flags", grid, &local.flags)?;
                }
            }
            REMOTE_SEQUENCE.check(
                "the messages of a lure accepted from another region",
                grid,
                &second.sequence().as_str(),
            )?;
            REMOTE_LINES.check(
                "a remote lure's progress lines",
                grid,
                &second.lines().as_slice(),
            )?;
            OFFER_HAS_BUCKET.check(
                "whether an offer's binary bucket holds anything",
                grid,
                &!crate::lure::bucket_text(&first_offer).is_empty(),
            )?;
            check(
                offerer_after_accept.is_empty() && offerer_after_second.is_empty(),
                "the offerer was told something about an accepted lure",
            )?;
            for flags in &second.starts {
                REMOTE_FLAGS.check("a remote lure's TeleportStart flags", grid, flags)?;
            }
            for (_line, flags) in &second.progress {
                REMOTE_FLAGS.check("a remote lure's TeleportProgress flags", grid, flags)?;
            }
            if let Some(finish) = &second.finish {
                FINISH_FLAGS.check("a remote lure's TeleportFinish flags", grid, &finish.flags)?;
            }
            Ok(())
        })
    }
}
