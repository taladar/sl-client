//! The primary offers friendship to the secondary, the secondary accepts, and
//! both buddy lists then contain the other avatar.

use std::time::Instant;

use sl_client_tokio::{Command, Event, ImDialog, InventoryFolderKey, TransactionId, Uuid};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, secs_metric};

/// The primary offers friendship to the secondary; the secondary accepts the
/// offer; and a buddy-list query on each side then reports the other avatar — a
/// full friendship-formation round-trip between two distinct agents.
///
/// A friendship offer is an `ImprovedInstantMessage` with the
/// `IM_FRIENDSHIP_OFFERED` dialog ([`ImDialog::FriendshipOffered`]), routed by
/// the grid's friends service to the named recipient (not broadcast to the
/// region like local chat). The recipient answers with an `AcceptFriendship`
/// carrying the offer's transaction id; the grid stores the (symmetric)
/// friendship and notifies the original offerer with a `FriendshipAccepted` IM
/// ([`ImDialog::FriendshipAccepted`]). Both sides then hold the other in their
/// buddy cache, read back here via [`Command::QueryFriends`] /
/// [`Event::FriendsSnapshot`].
///
/// Sequence (primary = offerer, secondary = accepter):
///
/// 1. Both avatars are pre-cleaned of any leftover friendship from an earlier
///    aborted run — OpenSim rejects an offer to an existing friend outright (it
///    replies "This person is already your friend" and forwards nothing), so a
///    stale friendship would make the offer silently never arrive. The primary
///    `TerminateFriendship`s the secondary (a no-op when they are not friends)
///    and the run settles briefly so the grid's friends cache reflects the
///    removal before the offer.
/// 2. The primary [`Command::OfferFriendship`]s the secondary.
/// 3. The secondary — a separate session — observes the matching
///    [`Event::InstantMessageReceived`] with [`ImDialog::FriendshipOffered`]
///    attributed to the primary, and answers with [`Command::AcceptFriendship`]
///    quoting the offer's transaction id (the IM's `id`).
/// 4. The primary observes the matching [`Event::InstantMessageReceived`] with
///    [`ImDialog::FriendshipAccepted`] attributed to the secondary — the grid
///    confirming the acceptance reached the offerer.
/// 5. A [`Command::QueryFriends`] on each side reports the other avatar in its
///    buddy cache (the offerer adds the accepter on the `FriendshipAccepted` IM,
///    the accepter adds the offerer on its own accept).
///
/// Finally the primary terminates the friendship again so re-runs start from a
/// clean slate (the next roadmap case, `friendship-terminate`, asserts that
/// path; here it is only cleanup).
///
/// OpenSim ignores the calling-card folder named in `AcceptFriendship` (it files
/// the card under a zero folder regardless), so the case passes the nil folder.
///
/// `2av`. Runs on OpenSim today (local secondary `Friend Tester`); `[opensim]`
/// only, the Aditi variant deferred to Phase Z pending a second Aditi avatar.
/// The flow is plain LLUDP `ImprovedInstantMessage` plus `AcceptFriendship` in
/// both directions, identical on both grids.
#[derive(Debug)]
pub struct FriendshipOfferAccept;

/// How long to settle after the pre-clean terminate so the grid's friends cache
/// reflects the removal before the offer (OpenSim updates the cache
/// synchronously on terminate, then notifies the peer; a short grace covers the
/// peer-notify hop).
const PRECLEAN_SETTLE: std::time::Duration = std::time::Duration::from_secs(3);

impl GridTest for FriendshipOfferAccept {
    fn name(&self) -> &'static str {
        "friendship-offer-accept"
    }

    fn description(&self) -> &'static str {
        "Primary offers friendship; secondary accepts; both buddy lists then contain the other"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Both avatars must be logged in and active before a friendship offer
            // can be routed between them.
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

            // Pre-clean: remove any leftover friendship from an earlier aborted
            // run so the offer is not rejected as already-friends. Terminate is a
            // no-op when they are not friends; settle so the grid's friends cache
            // reflects the removal before the offer.
            ctx.primary()
                .send(Command::TerminateFriendship(secondary_id.uuid().into()))
                .await?;
            tokio::time::sleep(PRECLEAN_SETTLE).await;

            // The primary offers friendship to the secondary; time the delivery.
            let offer_text = format!("sl-conformance friendship-offer-accept {primary_id}");
            let offered_at = Instant::now();
            ctx.primary()
                .send(Command::OfferFriendship {
                    to_agent_id: secondary_id,
                    message: offer_text,
                })
                .await?;

            // The secondary receives the matching friendship-offer IM from the
            // primary. Filtering on the sender and the FriendshipOffered dialog
            // ignores any unrelated background IM. Capture the offer's transaction
            // id (the IM's `id`) to quote it back on accept.
            let offer = ctx
                .secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::InstantMessageReceived(im)
                        if im.from_agent_id == primary_id
                            && im.dialog == ImDialog::FriendshipOffered =>
                    {
                        Some((**im).clone())
                    }
                    _ => None,
                })
                .await?;
            let offer_rtt = offered_at.elapsed();
            check_eq("offer dialog", &offer.dialog, &ImDialog::FriendshipOffered)?;
            check_eq("offer from_agent_id", &offer.from_agent_id, &primary_id)?;
            check_eq("offer to_agent_id", &offer.to_agent_id, &secondary_id)?;

            // The secondary accepts, quoting the offer's transaction id and naming
            // the primary as the new friend; the calling-card folder is ignored by
            // OpenSim, so the nil folder suffices.
            let accepted_at = Instant::now();
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .send(Command::AcceptFriendship {
                    transaction_id: TransactionId::from(offer.id),
                    friend_id: primary_id.uuid().into(),
                    calling_card_folder: InventoryFolderKey::from(Uuid::nil()),
                })
                .await?;

            // The primary observes the grid's `FriendshipAccepted` IM from the
            // secondary — confirmation that the acceptance reached the offerer (and
            // the signal that adds the secondary to the primary's buddy cache).
            let accepted = ctx
                .primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::InstantMessageReceived(im)
                        if im.from_agent_id == secondary_id
                            && im.dialog == ImDialog::FriendshipAccepted =>
                    {
                        Some((**im).clone())
                    }
                    _ => None,
                })
                .await?;
            let accept_rtt = accepted_at.elapsed();
            check_eq(
                "accept dialog",
                &accepted.dialog,
                &ImDialog::FriendshipAccepted,
            )?;
            check_eq(
                "accept from_agent_id",
                &accepted.from_agent_id,
                &secondary_id,
            )?;

            // Confirm both buddy lists: the primary now holds the secondary, and
            // the secondary now holds the primary. The primary's cache is updated
            // by the `FriendshipAccepted` IM observed above; the secondary's by its
            // own accept (processed in order ahead of the query on the same
            // channel).
            let primary_has_secondary = friends_contain(ctx.primary(), secondary_id.uuid()).await?;
            check(
                primary_has_secondary,
                "primary's buddy list does not contain the secondary after accept",
            )?;
            let secondary_has_primary = friends_contain(
                ctx.secondary().ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?,
                primary_id.uuid(),
            )
            .await?;
            check(
                secondary_has_primary,
                "secondary's buddy list does not contain the primary after accept",
            )?;

            // Cleanup: terminate the friendship so the next run starts clean. Best
            // effort — the assertions above already proved the friendship formed.
            ctx.primary()
                .send(Command::TerminateFriendship(secondary_id.uuid().into()))
                .await?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("offer_rtt"), offer_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("accept_rtt"), accept_rtt.as_secs_f64());
            Ok(())
        })
    }
}

/// Query `session`'s buddy cache and report whether it contains a friend whose id
/// is `peer`.
async fn friends_contain(session: &mut Session, peer: Uuid) -> Result<bool, TestFailure> {
    session.send(Command::QueryFriends).await?;
    session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::FriendsSnapshot(friends) => Some(
                friends
                    .iter()
                    .any(|presence| presence.friend.id.uuid() == peer),
            ),
            _ => None,
        })
        .await
}
