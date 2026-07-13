//! The primary and secondary form a friendship, the primary terminates it, and
//! both sides observe the removal — the buddy lists empty again on each side.

use std::time::Instant;

use sl_client_tokio::{Command, Event, ImDialog, InventoryFolderKey, TransactionId, Uuid};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, secs_metric};

/// The two avatars form a friendship, then the primary ends it and both sides
/// observe the termination — a full friendship-removal round-trip between two
/// distinct agents.
///
/// `TerminateFriendship` is a plain LLUDP message naming the former friend
/// (`ExBlock.OtherID`). The grid's friends service deletes the symmetric
/// friendship and then sends a `TerminateFriendship` back to *both* parties: one
/// to the terminator confirming the removal it requested, and one to the former
/// friend informing it that it has been dropped (OpenSim `RemoveFriendship` →
/// `client.SendTerminateFriend` for the caller, plus `LocalFriendshipTerminated`
/// → `friendClient.SendTerminateFriend` for the peer). Each side's session
/// surfaces this as an [`Event::FriendshipTerminated`] and drops the other agent
/// from its buddy cache.
///
/// Sequence (primary = terminator, secondary = the dropped friend):
///
/// 1. A clean friendship is established first so there is something to terminate:
///    any leftover friendship is pre-cleaned (OpenSim rejects an offer to an
///    existing friend), then the primary [`Command::OfferFriendship`]s the
///    secondary, the secondary observes the [`ImDialog::FriendshipOffered`] IM
///    and [`Command::AcceptFriendship`]s it, and the primary observes the
///    [`ImDialog::FriendshipAccepted`] IM. Both buddy lists are confirmed to
///    contain the other — the precondition for the termination under test.
/// 2. The primary [`Command::TerminateFriendship`]s the secondary; the delivery
///    of each side's notification is timed.
/// 3. The primary observes its own [`Event::FriendshipTerminated`] (the grid's
///    echo confirming the removal it requested), naming the secondary.
/// 4. The secondary — a separate session — observes the matching
///    [`Event::FriendshipTerminated`] naming the primary (the grid informing the
///    dropped friend).
/// 5. A [`Command::QueryFriends`] on each side reports that the other avatar is
///    gone from its buddy cache (each session removes the peer when it processes
///    its `TerminateFriendship`).
///
/// `2av`. Runs on OpenSim today (local secondary `Friend Tester`); `[opensim]`
/// only, the Aditi variant deferred to Phase Z pending its Aditi run.
/// The flow is plain LLUDP `ImprovedInstantMessage` plus `AcceptFriendship` and
/// `TerminateFriendship`, identical on both grids.
#[derive(Debug)]
pub struct FriendshipTerminate;

/// How long to settle after the pre-clean terminate so the grid's friends cache
/// reflects the removal before the offer that establishes the friendship under
/// test (mirrors `friendship-offer-accept`).
const PRECLEAN_SETTLE: std::time::Duration = std::time::Duration::from_secs(3);

impl GridTest for FriendshipTerminate {
    fn name(&self) -> &'static str {
        "friendship-terminate"
    }

    fn description(&self) -> &'static str {
        "Primary and secondary befriend; primary terminates; both buddy lists empty again"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Both avatars must be logged in and active before a friendship can be
            // formed or torn down between them.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;

            // Capture both agent ids: the secondary's while it is borrowed, then
            // release the borrow before reborrowing the primary.
            let secondary_id = secondary.agent_id().ok_or_else(|| {
                TestFailure::Assertion("secondary login did not report an agent id".to_owned())
            })?;
            let primary_id = ctx.primary().agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login did not report an agent id".to_owned())
            })?;

            // --- Establish a clean friendship to terminate -------------------

            // Pre-clean any leftover friendship from an earlier aborted run so the
            // offer below is not rejected as already-friends; settle so the grid's
            // friends cache reflects the removal before the offer.
            ctx.primary()
                .send(Command::TerminateFriendship(secondary_id.uuid().into()))
                .await?;
            tokio::time::sleep(PRECLEAN_SETTLE).await;

            // The primary offers friendship to the secondary.
            let offer_text = format!("sl-conformance friendship-terminate {primary_id}");
            ctx.primary()
                .send(Command::OfferFriendship {
                    to_agent_id: secondary_id,
                    message: offer_text,
                })
                .await?;

            // The secondary receives the matching friendship-offer IM and accepts
            // it, quoting the offer's transaction id; OpenSim ignores the
            // calling-card folder, so the nil folder suffices.
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

            // The primary observes the grid's `FriendshipAccepted` IM — the
            // friendship is now formed and both buddy caches hold the other.
            ctx.primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::InstantMessageReceived(im)
                        if im.from_agent_id == secondary_id
                            && im.dialog == ImDialog::FriendshipAccepted =>
                    {
                        Some(())
                    }
                    _ => None,
                })
                .await?;

            // Precondition: confirm both buddy lists contain the other before the
            // termination, so the removal below is a genuine state change.
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

            // --- Terminate and confirm removal on both sides -----------------

            // The primary terminates the friendship; time each side's notification.
            let terminated_at = Instant::now();
            ctx.primary()
                .send(Command::TerminateFriendship(secondary_id.uuid().into()))
                .await?;

            // The primary observes the grid's echo of its own termination, naming
            // the secondary as the former friend.
            let primary_drop = ctx
                .primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::FriendshipTerminated { other }
                        if other.uuid() == secondary_id.uuid() =>
                    {
                        Some(other.uuid())
                    }
                    _ => None,
                })
                .await?;
            let terminate_echo_rtt = terminated_at.elapsed();
            check_eq("primary drop other", &primary_drop, &secondary_id.uuid())?;

            // The secondary observes the grid informing it that the primary dropped
            // it, naming the primary as the former friend.
            let secondary_drop = ctx
                .secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::FriendshipTerminated { other } if other.uuid() == primary_id.uuid() => {
                        Some(other.uuid())
                    }
                    _ => None,
                })
                .await?;
            let terminate_notify_rtt = terminated_at.elapsed();
            check_eq("secondary drop other", &secondary_drop, &primary_id.uuid())?;

            // Confirm both buddy lists no longer contain the other: each session
            // dropped the peer when it processed its `TerminateFriendship` above, so
            // the snapshot read back here is empty of the former friend.
            let primary_still_has_secondary =
                friends_contain(ctx.primary(), secondary_id.uuid()).await?;
            check(
                !primary_still_has_secondary,
                "primary's buddy list still contains the secondary after terminate",
            )?;
            let secondary_still_has_primary = friends_contain(
                ctx.secondary().ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?,
                primary_id.uuid(),
            )
            .await?;
            check(
                !secondary_still_has_primary,
                "secondary's buddy list still contains the primary after terminate",
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing(
                &secs_metric("terminate_echo_rtt"),
                terminate_echo_rtt.as_secs_f64(),
            );
            metrics.set_timing(
                &secs_metric("terminate_notify_rtt"),
                terminate_notify_rtt.as_secs_f64(),
            );
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
