//! What a lure is worth once its offerer has logged out, and what becomes of
//! an offer made to somebody who is not there.

use std::time::Duration;

use sl_client_tokio::{Command, Event, ImDialog};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::lure::{
    NOTICE_WINDOW, Notice, offer_and_receive, offered_id, record_im, record_notices, watch_notices,
};
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check};
use crate::teleport_trace::watch_teleport;

/// Where each answer below is written down.
const SOURCE: &str = "book/src/gridspec/teleport.md (teleport-lure-offline, 2026-10-07)";

/// How long the offerer has been gone when its lure is accepted: long enough
/// for a grid to have noticed.
const GONE_FOR: Duration = Duration::from_secs(10);

/// How long the returning avatar is watched for a stored offer after each
/// thing that might bring one: arriving, and asking for stored messages.
const STORED_WINDOW: Duration = Duration::from_secs(20);

/// The whole case, which on Second Life waits out a login cooldown in the
/// middle.
const CASE_TIMEOUT: Duration = Duration::from_secs(600);

/// How accepting a lure is answered once the avatar that offered it has logged
/// out: carried out, on both grids. The lure is a place, not a person.
const OFFERER_GONE: Measured<&str> = Measured {
    second_life: "local",
    opensim: "local",
    source: SOURCE,
};

/// Whether an offer made to an avatar that is logged out is delivered when it
/// logs in.
const STORED_OFFER_DELIVERED: Measured<bool> = Measured {
    second_life: false,
    opensim: false,
    source: SOURCE,
};

/// Accepts a lure whose offerer has logged out, then offers a teleport to the
/// logged-out avatar and sees whether it is there on its return.
///
/// 1. The primary offers the secondary a teleport and logs out.
/// 2. The secondary accepts the lure ten seconds later. OpenSim's lure is the
///    place, so there is nothing about the offerer left to check; Second
///    Life's outlives the session that made it too, and lands the accepter
///    where the offerer stood.
/// 3. The secondary offers the absent primary a teleport and is watched for
///    [`NOTICE_WINDOW`]: an instant message to somebody offline is answered
///    with a line saying it was stored, and an offer may or may not be.
/// 4. The primary logs back in and is watched for the offer — first for what
///    the grid delivers on its own, then after asking for stored messages.
///
/// `2av`.
#[derive(Debug)]
pub struct TeleportLureOffline;

impl GridTest for TeleportLureOffline {
    fn name(&self) -> &'static str {
        "teleport-lure-offline"
    }

    fn description(&self) -> &'static str {
        "Accept a lure whose offerer logged out, and offer a teleport to an avatar that is offline"
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

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
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

            // --- A lure whose offerer has gone.
            let message = format!("sl-conformance teleport-lure-offline {primary_id}");
            let offer = offer_and_receive(primary, secondary, &message).await?;
            primary.disconnect().await?;
            tokio::time::sleep(GONE_FOR).await;
            secondary
                .send(Command::AcceptTeleportLure {
                    lure_id: offered_id(&offer),
                })
                .await?;
            let gone = watch_teleport(secondary, REGION_TIMEOUT).await?;

            // --- An offer to somebody who is not there.
            let stored_message =
                format!("sl-conformance teleport-lure-offline stored {secondary_id}");
            secondary
                .send(Command::OfferTeleport {
                    targets: vec![primary_id],
                    message: stored_message.clone(),
                })
                .await?;
            let offerer_of_absent = watch_notices(secondary, NOTICE_WINDOW).await?;

            // --- The absent one returns.
            primary.relogin().await?;
            check(
                primary.is_connected(),
                "the offerer should be connected after its relogin",
            )?;
            let mut on_return = watch_notices(primary, STORED_WINDOW).await?;
            let delivered_unasked = holds_offer(&on_return, secondary_id, &stored_message);
            // Both ways of asking: the capability where the grid grants it, and
            // the UDP trigger the client falls back to where it does not.
            primary.send(Command::RequestOfflineMessages).await?;
            primary.send(Command::RetrieveInstantMessages).await?;
            let after_asking = watch_notices(primary, STORED_WINDOW).await?;
            let delivered_asked = holds_offer(&after_asking, secondary_id, &stored_message);
            on_return.extend(after_asking);
            // Leave the region as it was found: nothing waits on this.
            let _region = primary
                .wait_for(Duration::from_secs(1), |event| {
                    matches!(event, Event::RegionHandshakeComplete).then_some(())
                })
                .await;

            let metrics = ctx.metrics();
            record_im("offer_", &offer, metrics);
            gone.record("offerer_gone_", metrics);
            record_notices("offerer_of_absent", &offerer_of_absent, metrics);
            record_notices("absent_on_return", &on_return, metrics);
            metrics.set("stored_offer_delivered_unasked", delivered_unasked);
            metrics.set("stored_offer_delivered_asked", delivered_asked);

            OFFERER_GONE.check(
                "how accepting a lure is answered once its offerer has logged out",
                grid,
                &gone.answer(),
            )?;
            STORED_OFFER_DELIVERED.check(
                "whether an offer to a logged-out avatar is delivered on its return",
                grid,
                &(delivered_unasked || delivered_asked),
            )
        })
    }
}

/// Whether `notices` holds the teleport offer `message` from `from`.
fn holds_offer(notices: &[Notice], from: sl_client_tokio::AgentKey, message: &str) -> bool {
    notices.iter().any(|notice| {
        notice.is_im(ImDialog::LureUser, from)
            && matches!(notice, Notice::Im { message: found, .. } if found == message)
    })
}
