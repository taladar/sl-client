//! With the two avatars befriended, the primary grants the secondary the full
//! set of friendship rights and both sides observe the `ChangeUserRights` echo;
//! a buddy-list query then confirms the new rights in both directions.

use std::time::Instant;

use sl_client_tokio::{
    Command, Event, Friend, FriendRights, ImDialog, InventoryFolderKey, TransactionId, Uuid,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, count_metric, secs_metric};

/// Two befriended avatars exercise the friendship-rights channel: the primary
/// grants the secondary the full `see-online | see-on-map | modify-objects` set
/// via `GrantUserRights`, both sessions observe the matching
/// `ChangeUserRights` echo, and a buddy-list query then reports the new rights
/// from each side.
///
/// A friendship is born with `CAN_SEE_ONLINE` granted both ways (OpenSim's
/// `FriendsModule.AddFriendship` stores it for each side); every other right
/// starts cleared, and a client raises one with
/// [`Command::GrantUserRights`]. OpenSim's `FriendsModule.GrantRights` persists
/// the new bitfield, then **always echoes it back to the grantor**
/// (`remoteClient.SendChangeUserRights(requester, friendID, rights)`) and
/// notifies the friend (`LocalGrantRights` →
/// `friendClient.SendChangeUserRights(requester, friendID, rights)`). The two
/// echoes carry the same `AgentData.AgentID` (the grantor), so the session
/// distinguishes them by direction: the grantor sees its own id and folds the
/// echo into [`Event::FriendRightsChanged`] with `granted_to_us = false`
/// (updating the friend's `rights_granted`); the friend sees a foreign id and
/// folds it with `granted_to_us = true` (updating `rights_received`).
///
/// Sequence (primary = grantor, secondary = grantee):
///
/// 1. A clean friendship is established first (pre-clean any leftover, offer,
///    accept, confirm the grid's `FriendshipAccepted`) — the precondition for
///    `GrantRights`, which only acts on a friend already in the grantor's
///    server-side cache. That cache is refreshed asynchronously and races the
///    `FriendshipAccepted` IM, so the case settles briefly (see `GRANT_SETTLE`)
///    before granting; without the settle `GrantRights` finds an empty cache and
///    echoes nothing.
/// 2. The primary [`Command::GrantUserRights`]s the secondary the full
///    `CAN_SEE_ONLINE | CAN_SEE_ON_MAP | CAN_MODIFY_OBJECTS` bitfield.
/// 3. The primary observes its own echo as [`Event::FriendRightsChanged`] naming
///    the secondary, carrying the full bitfield, with `granted_to_us = false`.
/// 4. The secondary — a separate session — observes the matching
///    [`Event::FriendRightsChanged`] naming the primary, the full bitfield,
///    with `granted_to_us = true`.
/// 5. A [`Command::QueryFriends`] on each side confirms the cached friendship
///    now reflects the grant: the primary's secondary entry has the full set in
///    `rights_granted`, and the secondary's primary entry has the full set in
///    `rights_received`. (The reverse direction — the secondary's grant to the
///    primary — stays at the default `CAN_SEE_ONLINE`, untouched.)
///
/// The grant keeps the see-online bit set (it was already granted), so it does
/// not toggle presence and provokes no spurious online/offline notification.
///
/// `2av`, `[opensim]` only. The flow is plain LLUDP `GrantUserRights` /
/// `ChangeUserRights`, identical on both grids; the Aditi variant is deferred to
/// Phase Z pending a second Aditi avatar. The friendship is terminated at the
/// end so re-runs start from a clean slate.
#[derive(Debug)]
pub struct GrantUserRights;

/// How long to settle after the pre-clean terminate so the grid's friends cache
/// reflects the removal before the offer that establishes the friendship under
/// test (mirrors the friendship cases).
const PRECLEAN_SETTLE: std::time::Duration = std::time::Duration::from_secs(3);

/// How long to settle after the `FriendshipAccepted` IM before granting rights.
/// OpenSim's `GrantRights` only acts on a friend present in the *grantor's*
/// server-side friends cache (`GetFriendsFromCache(requester)`); that cache is
/// refreshed by `LocalFriendshipApproved` → `RecacheFriends` on the offerer, but
/// the refresh races the `FriendshipAccepted` IM the primary keys off — granting
/// the instant the IM lands finds the cache still empty and `GrantRights` returns
/// without echoing anything. A short settle lets the recache land first.
const GRANT_SETTLE: std::time::Duration = std::time::Duration::from_secs(3);

/// The full friendship-rights bitfield this case grants: see-online, see-on-map,
/// and modify-objects together.
const FULL_RIGHTS: i32 =
    FriendRights::CAN_SEE_ONLINE | FriendRights::CAN_SEE_ON_MAP | FriendRights::CAN_MODIFY_OBJECTS;

impl GridTest for GrantUserRights {
    fn name(&self) -> &'static str {
        "grant-user-rights"
    }

    fn description(&self) -> &'static str {
        "Primary grants a friend full rights; both observe the echo and buddy lists confirm them"
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
            // formed and a rights change routed between them.
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

            // --- Establish a clean friendship (the grant's precondition) -------

            // Pre-clean any leftover friendship from an earlier aborted run so the
            // offer below is not rejected as already-friends; settle so the grid's
            // friends cache reflects the removal before the offer.
            ctx.primary()
                .send(Command::TerminateFriendship(secondary_id.uuid().into()))
                .await?;
            tokio::time::sleep(PRECLEAN_SETTLE).await;

            // The primary offers friendship to the secondary.
            let offer_text = format!("sl-conformance grant-user-rights {primary_id}");
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
            // friendship is now formed and the secondary is in the primary's
            // friends cache, so `GrantRights` will act on it.
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

            // --- Grant the full rights set -----------------------------------

            // Let the grid's grantor-side friends cache catch up to the freshly
            // formed friendship before granting (see `GRANT_SETTLE`); otherwise
            // `GrantRights` finds an empty cache and echoes nothing.
            tokio::time::sleep(GRANT_SETTLE).await;

            // The primary grants the secondary see-online | see-on-map |
            // modify-objects; time both the grantor echo and the grantee notify.
            let granted = FriendRights(FULL_RIGHTS);
            let granted_at = Instant::now();
            ctx.primary()
                .send(Command::GrantUserRights {
                    target: secondary_id.uuid().into(),
                    rights: granted,
                })
                .await?;

            // The primary observes its own echo: the rights it now grants the
            // secondary (`granted_to_us = false`). Matching on the friend id
            // ignores any unrelated rights change.
            let echo = ctx
                .primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::FriendRightsChanged {
                        friend_id,
                        rights,
                        granted_to_us,
                    } if friend_id.uuid() == secondary_id.uuid() && !*granted_to_us => {
                        Some(*rights)
                    }
                    _ => None,
                })
                .await?;
            let echo_rtt = granted_at.elapsed();
            check_eq("grantor echo rights", &echo, &granted)?;

            // The secondary observes the grid's notification: the rights the
            // primary now grants it (`granted_to_us = true`).
            let notify = ctx
                .secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::FriendRightsChanged {
                        friend_id,
                        rights,
                        granted_to_us,
                    } if friend_id.uuid() == primary_id.uuid() && *granted_to_us => Some(*rights),
                    _ => None,
                })
                .await?;
            let notify_rtt = granted_at.elapsed();
            check_eq("grantee notify rights", &notify, &granted)?;

            // --- Confirm both buddy lists reflect the grant ------------------

            // The primary's secondary entry now carries the full set in
            // `rights_granted` (what this agent grants the friend).
            let primary_view = friend_in_snapshot(ctx.primary(), secondary_id.uuid())
                .await?
                .ok_or_else(|| {
                    TestFailure::Assertion(
                        "primary's buddy list does not contain the secondary".to_owned(),
                    )
                })?;
            check_eq(
                "primary's rights granted to secondary",
                &primary_view.rights_granted,
                &granted,
            )?;

            // The secondary's primary entry now carries the full set in
            // `rights_received` (what the friend grants this agent); the reverse
            // direction it grants the primary is untouched at the default.
            let secondary_view = friend_in_snapshot(
                ctx.secondary().ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?,
                primary_id.uuid(),
            )
            .await?
            .ok_or_else(|| {
                TestFailure::Assertion(
                    "secondary's buddy list does not contain the primary".to_owned(),
                )
            })?;
            check_eq(
                "secondary's rights received from primary",
                &secondary_view.rights_received,
                &granted,
            )?;
            check(
                secondary_view.rights_granted.0 & FriendRights::CAN_SEE_ONLINE != 0,
                "secondary's grant to the primary lost its default see-online right",
            )?;

            // Cleanup: terminate the friendship so the next run starts clean. Best
            // effort — the assertions above already proved the grant propagated.
            ctx.primary()
                .send(Command::TerminateFriendship(secondary_id.uuid().into()))
                .await?;

            let metrics = ctx.metrics();
            metrics.set(&count_metric("granted_rights"), granted.0);
            metrics.set_timing(&secs_metric("grant_echo_rtt"), echo_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("grant_notify_rtt"), notify_rtt.as_secs_f64());
            Ok(())
        })
    }
}

/// Query `session`'s buddy cache and return the friend whose id is `peer`, or
/// `None` when it is absent. The returned [`Friend`] carries the rights in both
/// directions, which the caller asserts on.
async fn friend_in_snapshot(
    session: &mut Session,
    peer: Uuid,
) -> Result<Option<Friend>, TestFailure> {
    session.send(Command::QueryFriends).await?;
    session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::FriendsSnapshot(friends) => Some(
                friends
                    .iter()
                    .find(|presence| presence.friend.id.uuid() == peer)
                    .map(|presence| presence.friend),
            ),
            _ => None,
        })
        .await
}
