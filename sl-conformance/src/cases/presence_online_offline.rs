//! With the two avatars befriended, the secondary logs out then back in and the
//! primary observes the matching `OfflineNotification` / `OnlineNotification`.

use std::time::Instant;

use sl_client_tokio::{Command, Event, ImDialog, InventoryFolderKey, TransactionId, Uuid};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check_eq, secs_metric};

/// Two befriended avatars exchange presence: the secondary goes offline and then
/// comes back, and the primary — a separate session that holds the see-online
/// right — observes the grid's offline and online notifications in turn.
///
/// Presence flows over `OnlineNotification` / `OfflineNotification`, which the
/// grid's friends service sends only to friends granted the see-online right.
/// OpenSim grants `CanSeeOnline` in **both** directions when a friendship is
/// formed (`FriendsModule.AddFriendship` stores `CanSeeOnline` for each side), so
/// a fresh friendship is all the rights setup this case needs. A login surfaces
/// as `OnlineNotification` (OpenSim `FriendsModule.StatusChange(_, true)`, fired
/// once the returning agent is a root agent) and a logout as
/// `OfflineNotification` (`StatusChange(_, false)` from `OnClientClosed`); the
/// session folds either into [`Event::FriendsOnline`] / [`Event::FriendsOffline`]
/// and maintains its presence set accordingly.
///
/// Both avatars are already logged in when a case starts, so the only way to
/// witness a *transition* is to drive one. The case reuses the mid-run
/// logout/login support that [`super::offline_msg_fetch`] introduced:
///
/// 1. A clean friendship is established first (pre-clean any leftover, offer,
///    accept, confirm the grid's `FriendshipAccepted`) so the see-online right is
///    granted both ways — the precondition for any notification to flow.
/// 2. The secondary [disconnects](crate::context::Session::disconnect); the
///    primary observes [`Event::FriendsOffline`] naming the secondary (the grid
///    informing a see-online friend that the peer went offline).
/// 3. The secondary [relogs in](crate::context::Session::relogin) — on OpenSim
///    inheriting the "already logged in" retry that evicts the stale presence the
///    disconnect leaves — and the primary observes [`Event::FriendsOnline`]
///    naming the secondary (the grid announcing the peer is back).
///
/// Each observation is matched on the secondary's id within the notification's
/// id list, so an unrelated friend's presence change cannot satisfy it. Where
/// [`super::friendship_offer_accept`] proves the *friendship* forms, this proves
/// the *presence channel* the friendship opens carries both edges of the
/// online/offline transition.
///
/// `2av`, `[opensim]` only. The flow is plain LLUDP `OnlineNotification` /
/// `OfflineNotification`, identical on both grids; the Aditi variant is deferred
/// to Phase Z pending a second Aditi avatar. The friendship is terminated at the
/// end so re-runs start from a clean slate.
#[derive(Debug)]
pub struct PresenceOnlineOffline;

/// How long to settle after the pre-clean terminate so the grid's friends cache
/// reflects the removal before the offer that establishes the friendship under
/// test (mirrors the friendship cases).
const PRECLEAN_SETTLE: std::time::Duration = std::time::Duration::from_secs(3);

impl GridTest for PresenceOnlineOffline {
    fn name(&self) -> &'static str {
        "presence-online-offline"
    }

    fn description(&self) -> &'static str {
        "Befriended peer logs out then in; primary observes the offline then online notification"
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
            // formed and presence routed between them.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;

            // Capture both agent ids: the secondary's while it is borrowed, then
            // release the borrow before reborrowing the primary. The secondary's id
            // is stable across its mid-run logout/login.
            let secondary_id = secondary.agent_id().ok_or_else(|| {
                TestFailure::Assertion("secondary login did not report an agent id".to_owned())
            })?;
            let primary_id = ctx.primary().agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login did not report an agent id".to_owned())
            })?;

            // --- Establish a clean friendship (grants see-online both ways) ----

            // Pre-clean any leftover friendship from an earlier aborted run so the
            // offer below is not rejected as already-friends; settle so the grid's
            // friends cache reflects the removal before the offer.
            ctx.primary()
                .send(Command::TerminateFriendship(secondary_id.uuid().into()))
                .await?;
            tokio::time::sleep(PRECLEAN_SETTLE).await;

            // The primary offers friendship to the secondary.
            let offer_text = format!("sl-conformance presence-online-offline {primary_id}");
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
            // friendship is now formed and both sides hold the see-online right.
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

            // --- Peer goes offline -------------------------------------------

            // The secondary logs out (keeping the identity needed to relogin); the
            // grid's `OnClientClosed` fires `StatusChange(_, false)`, notifying the
            // primary as a see-online friend.
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .disconnect()
                .await?;

            // Time the offline notification from *after* the disconnect completes,
            // mirroring the online timing below: the grid emits the notification as
            // it tears the secondary's circuit down (inside `disconnect`'s logout
            // sequence), so by here it is usually already buffered and observed
            // near-instantly — measuring the grid's notify latency, not our own
            // logout grace.
            let offline_at = Instant::now();

            // The primary observes the offline notification naming the secondary.
            // Matching on the id inside the notification's list ignores any other
            // friend's presence change.
            let offline_for = ctx
                .primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::FriendsOffline(ids)
                        if ids.iter().any(|id| id.uuid() == secondary_id.uuid()) =>
                    {
                        Some(secondary_id.uuid())
                    }
                    _ => None,
                })
                .await?;
            let offline_notify_rtt = offline_at.elapsed();
            check_eq(
                "offline notification id",
                &offline_for,
                &secondary_id.uuid(),
            )?;

            // --- Peer comes back online --------------------------------------

            // The secondary logs back in. On OpenSim the disconnect left a stale
            // presence; the relogin's first attempt evicts it and retries. Once it
            // is a root agent the grid fires `StatusChange(_, true)`.
            let online_at = Instant::now();
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .relogin()
                .await?;
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for_region(REGION_TIMEOUT)
                .await?;

            // The primary observes the online notification naming the secondary.
            let online_for = ctx
                .primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::FriendsOnline(ids)
                        if ids.iter().any(|id| id.uuid() == secondary_id.uuid()) =>
                    {
                        Some(secondary_id.uuid())
                    }
                    _ => None,
                })
                .await?;
            let online_notify_rtt = online_at.elapsed();
            check_eq("online notification id", &online_for, &secondary_id.uuid())?;

            // Cleanup: terminate the friendship so the next run starts clean. Best
            // effort — the assertions above already proved both transitions.
            ctx.primary()
                .send(Command::TerminateFriendship(secondary_id.uuid().into()))
                .await?;

            let metrics = ctx.metrics();
            metrics.set_timing(
                &secs_metric("offline_notify_rtt"),
                offline_notify_rtt.as_secs_f64(),
            );
            metrics.set_timing(
                &secs_metric("online_notify_rtt"),
                online_notify_rtt.as_secs_f64(),
            );
            Ok(())
        })
    }
}
