//! The primary offers its calling card to the secondary, which observes the
//! offer; the secondary then accepts it.

use std::time::Instant;

use sl_client_tokio::{Command, Event, InventoryFolderKey, TransactionId, Uuid};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, secs_metric};

/// The primary offers its calling card to the secondary; the secondary observes
/// the matching [`Event::CallingCardOffered`] and accepts it — a calling-card
/// hand-off between two distinct agents.
///
/// A calling card is a reference card to an avatar, filed in the recipient's
/// Calling Cards inventory folder; offering one is *not* a friendship request
/// (see `friendship-offer-accept` for that). The offerer sends
/// [`Command::OfferCallingCard`] naming the recipient and a correlation
/// `transaction_id`; the recipient sees an [`Event::CallingCardOffered`] and
/// replies with [`Command::AcceptCallingCard`] (or `DeclineCallingCard`).
///
/// Sequence (primary = offerer, secondary = recipient):
///
/// 1. The primary [`Command::OfferCallingCard`]s the secondary, with a fresh
///    correlation transaction id.
/// 2. The secondary — a separate session — observes the matching
///    [`Event::CallingCardOffered`] attributed to the primary.
/// 3. The secondary [`Command::AcceptCallingCard`]s, quoting the offer's
///    transaction id.
///
/// **Why this is a partial run on OpenSim.** OpenSim's `XCallingCardModule`
/// surfaces the offer when both avatars share a region: `OnOfferCallingCard`
/// finds the recipient in-region, creates the calling-card inventory item, and
/// pushes it with `SendOfferCallingCard(from, itemID)` — so the secondary's
/// `CallingCardOffered` carries, as its `transaction`, the *new card's item id*,
/// not the offerer's chosen transaction id (the in-region path discards the
/// offerer's transaction entirely). The case therefore confirms the offer is
/// attributed to the primary but does not assert the observed transaction equals
/// the offered one. More importantly, OpenSim's `OnAcceptCallingCard` handler is
/// an empty no-op (the card was already filed at offer time), so it sends the
/// offerer **nothing** back — the offerer-side [`Event::CallingCardAccepted`]
/// confirmation has no OpenSim path to observe. The full
/// offer → accept → offerer-confirm round-trip is Second Life only and deferred
/// to Phase Z (aditi); the run is marked partial to record that gap.
///
/// `2av`. `[opensim]` only; the Aditi variant (which can prove the offerer-side
/// accept confirmation) is deferred to Phase Z pending a second Aditi avatar.
#[derive(Debug)]
pub struct CallingCard;

impl GridTest for CallingCard {
    fn name(&self) -> &'static str {
        "calling-card"
    }

    fn description(&self) -> &'static str {
        "Primary offers its calling card; secondary observes the offer and accepts it"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Both avatars must be logged in and in-region: OpenSim only surfaces
            // the offer (rather than forwarding it as a dialog-211 IM) when the
            // recipient is a root agent in the offerer's region.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;

            // Capture both agent ids: the secondary's while it is borrowed, then
            // release the borrow before reborrowing the primary. The offer is
            // attributed to the primary and addressed to the secondary.
            let secondary_id = secondary.agent_id().ok_or_else(|| {
                TestFailure::Assertion("secondary login did not report an agent id".to_owned())
            })?;
            let primary_id = ctx.primary().agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login did not report an agent id".to_owned())
            })?;

            // The primary offers its calling card to the secondary with a fresh
            // correlation id; time the delivery. (OpenSim's in-region path discards
            // this transaction id and substitutes the new card's item id when it
            // pushes the offer to the recipient, so it is purely a request-side
            // correlator here.)
            let offer_transaction = TransactionId::from(Uuid::new_v4());
            let offered_at = Instant::now();
            ctx.primary()
                .send(Command::OfferCallingCard {
                    to_agent_id: secondary_id,
                    transaction_id: offer_transaction,
                })
                .await?;

            // The secondary observes the matching calling-card offer from the
            // primary. Filtering on the offering agent ignores any unrelated
            // background offer; capture the observed transaction (OpenSim sets it to
            // the freshly created card's item id) to quote back on accept.
            let offer = ctx
                .secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::CallingCardOffered {
                        offering_agent,
                        transaction,
                    } if *offering_agent == primary_id => Some(*transaction),
                    _ => None,
                })
                .await?;
            let offer_rtt = offered_at.elapsed();

            // The secondary accepts, quoting the offer's transaction id. OpenSim
            // ignores the destination folder (the card is already filed), so the nil
            // folder suffices. This is fire-and-forget: OpenSim's accept handler is a
            // no-op and sends the offerer no confirmation (see the type docs), so
            // there is no offerer-side event to await on this grid.
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .send(Command::AcceptCallingCard {
                    transaction_id: offer,
                    calling_card_folder: InventoryFolderKey::from(Uuid::nil()),
                })
                .await?;

            // The offer half is fully verified above (the `wait_for` filter only
            // matches an offer attributed to the primary, so a successful match is
            // the assertion); the offerer-side accept confirmation is Second Life
            // only (OpenSim's OnAcceptCallingCard is an empty no-op), so the OpenSim
            // run is partial by construction.
            ctx.mark_partial(
                "OpenSim's accept handler is a no-op and sends no confirmation to the \
                 offerer; the offer→accept→offerer-confirm round-trip is SL-only (Phase Z)",
            );

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("offer_rtt"), offer_rtt.as_secs_f64());
            Ok(())
        })
    }
}
