//! One avatar offers another a teleport, the other declines it — and then
//! accepts the lure it declined, and a second lure twice, to see how long a
//! lure lasts.

use sl_client_tokio::{Command, ImDialog};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::lure::{
    NOTICE_WINDOW, offer_and_receive, offered_id, record_im, record_notices, watch_notices,
};
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, check_eq};
use crate::teleport_trace::watch_teleport;

/// Where each answer below is written down.
const SOURCE: &str = "book/src/gridspec/teleport.md (teleport-offer-decline, 2026-10-07)";

/// How accepting a lure that was declined is answered. A decline is the end
/// of a lure on Second Life, which then says nothing to an acceptance of it;
/// OpenSim's lure is a place and cannot be withdrawn.
const DECLINED_LURE: Measured<&str> = Measured {
    second_life: "unanswered",
    opensim: "local",
    source: SOURCE,
};

/// How accepting a lure a second time is answered: Second Life's is spent by
/// its first use.
const REUSED_LURE: Measured<&str> = Measured {
    second_life: "unanswered",
    opensim: "local",
    source: SOURCE,
};

/// Declines an offered teleport and accepts the declined lure anyway; then
/// accepts a second offer twice.
///
/// A viewer declines an offer with an `ImprovedInstantMessage` of dialog
/// `IM_LURE_DECLINED` to the offerer, its id the lure id. Whether the grid
/// passes that on is the first thing measured: the case watches the offerer
/// for [`NOTICE_WINDOW`] after the decline. Neither grid does.
///
/// The second is what a decline does to the lure itself, and the third whether
/// a lure can be used more than once. Nothing a viewer does asks either
/// question — its Decline button is the end of the offer and its Teleport
/// button is pressed once — but a fake grid has to answer both, and a lure that
/// outlives its decline is a teleport the user refused and can still be made
/// to take.
///
/// Both avatars stand in one region, so each acceptance that works is a local
/// teleport.
///
/// `2av`.
#[derive(Debug)]
pub struct TeleportOfferDecline;

impl GridTest for TeleportOfferDecline {
    fn name(&self) -> &'static str {
        "teleport-offer-decline"
    }

    fn description(&self) -> &'static str {
        "Decline an offered teleport, then accept the declined lure twice, recording each answer"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        super::teleport_offer_accept::TeleportOfferAccept.start_location(grid)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let (primary, secondary) = ctx.primary_and_secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            primary.wait_for_region(REGION_TIMEOUT).await?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;
            let primary_id = crate::lure::agent_id(primary, "offerer")?;
            let secondary_id = crate::lure::agent_id(secondary, "accepter")?;

            let message = format!("sl-conformance teleport-offer-decline {primary_id}");
            let offer = offer_and_receive(primary, secondary, &message).await?;
            // Anything the offer itself provoked is not the decline's.
            let before_decline = watch_notices(primary, NOTICE_WINDOW).await?;

            secondary
                .send(Command::DeclineTeleportLure {
                    from_agent_id: primary_id,
                    lure_id: offered_id(&offer),
                })
                .await?;
            let offerer_after_decline = watch_notices(primary, NOTICE_WINDOW).await?;
            let decliner_after_decline = watch_notices(secondary, NOTICE_WINDOW).await?;
            let decline_reached = offerer_after_decline
                .iter()
                .any(|notice| notice.is_im(ImDialog::LureDeclined, secondary_id));

            // The declined lure, accepted after all.
            secondary
                .send(Command::AcceptTeleportLure {
                    lure_id: offered_id(&offer),
                })
                .await?;
            let declined = watch_teleport(secondary, REGION_TIMEOUT).await?;
            let offerer_after_accept = watch_notices(primary, NOTICE_WINDOW).await?;

            // A second offer, accepted, and then accepted again.
            let message = format!("sl-conformance teleport-offer-decline again {primary_id}");
            let again = offer_and_receive(primary, secondary, &message).await?;
            secondary
                .send(Command::AcceptTeleportLure {
                    lure_id: offered_id(&again),
                })
                .await?;
            let used = watch_teleport(secondary, REGION_TIMEOUT).await?;
            secondary
                .send(Command::AcceptTeleportLure {
                    lure_id: offered_id(&again),
                })
                .await?;
            let reused = watch_teleport(secondary, REGION_TIMEOUT).await?;

            let metrics = ctx.metrics();
            record_im("offer_", &offer, metrics);
            record_notices("offerer_before_decline", &before_decline, metrics);
            record_notices("offerer_after_decline", &offerer_after_decline, metrics);
            record_notices("decliner_after_decline", &decliner_after_decline, metrics);
            record_notices("offerer_after_accept", &offerer_after_accept, metrics);
            metrics.set("decline_reaches_offerer", decline_reached);
            declined.record("declined_", metrics);
            used.record("used_", metrics);
            reused.record("reused_", metrics);

            check(
                !decline_reached,
                "the offerer was sent the IM_LURE_DECLINED, which neither grid was measured passing on",
            )?;
            check(
                offerer_after_decline.is_empty(),
                "the offerer was told something about a declined lure",
            )?;
            DECLINED_LURE.check(
                "how accepting a declined lure is answered",
                grid,
                &declined.answer(),
            )?;
            check_eq("a lure's first use", &used.answer(), &"local")?;
            REUSED_LURE.check(
                "how accepting a lure a second time is answered",
                grid,
                &reused.answer(),
            )
        })
    }
}
