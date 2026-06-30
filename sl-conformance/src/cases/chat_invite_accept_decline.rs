//! The secondary is invited to two group IM sessions; it accepts one (the
//! registry entry promotes to joined) and declines the other (the entry is
//! removed), exercising `AcceptChatInvite` / `DeclineChatInvite`.

use std::time::{Duration, Instant, SystemTime};

use sl_client_tokio::{
    AgentKey, ChatLifecycleView, ChatSessionInfo, ChatSessionKind, Command, Event, GroupKey,
    ImSessionId, InviteChannel,
};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    self, GroupSource, REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, secs_metric,
};

/// The secondary is invited to two distinct group IM sessions and answers each
/// differently: it [`Command::AcceptChatInvite`]s one and
/// [`Command::DeclineChatInvite`]s the other, and the case asserts the
/// chat-session registry's resulting state transitions.
///
/// A pending invitation is a chat-session registry entry whose lifecycle is
/// [`ChatLifecycleView::Invited`]; accepting it promotes the entry to
/// [`ChatLifecycleView::Joined`] and declining it removes the entry entirely
/// (the registry tracks only live sessions). To provoke a *real* invitation —
/// not a synthetic registry insert — the primary takes an open-enrollment group
/// (created fresh, or a pre-made fixture group reused; see
/// [`support::membership_group`]), the secondary joins it as a member, and the
/// primary opens the group's IM session and sends one message: because the
/// secondary has
/// not itself joined the session, OpenSim's `GroupsMessagingModule` delivers
/// that first message as a CAPS `ChatterBoxInvitation`
/// ([`Event::ConferenceInvited`] with `from_group`), the same path
/// [`super::group_session_message`] documents for a not-yet-joined member. The
/// case does this twice — once for the accept, once for the decline — since once
/// a group's session entry has been answered, a second message in the same group
/// arrives as a plain session message rather than a fresh invitation.
///
/// The accept/decline themselves are driven on the secondary and asserted
/// against its registry via [`Command::QueryChatSessions`]: before answering, the
/// group session is present as `Invited` (inviter == the primary, a text-channel
/// invite); after [`Command::AcceptChatInvite`] it is `Joined`; after
/// [`Command::DeclineChatInvite`] (on the other group) it is gone.
///
/// The CAPS `ChatSessionRequest` accept/decline POST is the Second Life path;
/// OpenSim exposes no such capability, so there the accept is the optimistic
/// local join (the simulator already added us when it routed the invite) and the
/// decline is a UDP `SessionLeave` the module ignores — both observable only as
/// the client-side registry transition under test. Asserting the `ChatSessionRequest`
/// POST and its reply roster is the Aditi variant, deferred to Phase Z pending a
/// second Aditi avatar.
///
/// `2av`. Runs on OpenSim today (local secondary `Friend Tester`, Groups V2
/// enabled — see the setup memory); `[opensim]` only, Aditi → Phase Z.
#[derive(Debug)]
pub struct ChatInviteAcceptDecline;

/// How long to let the secondary's group membership settle on the group service
/// before the primary sends into the session, so the message-distribution pass
/// lists the secondary as a member and routes it the invitation. Generous
/// because it bridges the join reply through the Groups V2 backend.
const MEMBERSHIP_SETTLE: Duration = Duration::from_secs(2);

impl GridTest for ChatInviteAcceptDecline {
    fn name(&self) -> &'static str {
        "chat-invite-accept-decline"
    }

    fn description(&self) -> &'static str {
        "Secondary accepts one group-IM invitation and declines another; assert the registry transitions"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Both avatars must be logged in and active before group traffic can
            // be routed between them.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for_region(REGION_TIMEOUT)
                .await?;

            // The invitation is attributed to the primary (the founder who opens
            // the session and sends). Capture its id up front — the secondary's
            // borrow cannot be held across a `ctx.primary()` reborrow.
            let primary_id = ctx.primary().agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login did not report an agent id".to_owned())
            })?;

            // --- Accept path: invite the secondary to a group, then accept. ---
            let (accept_group, accept_source, accept_rtt) =
                provoke_group_invitation(ctx, primary_id, 0, "accept").await?;
            let accept_session = ChatSessionKind::Group {
                group_id: accept_group,
            };

            // Before accepting, the registry shows the group session as a pending
            // invitation from the primary on its text channel.
            let invited = require_session(ctx, accept_session, "before accept").await?;
            check_invited(&invited, primary_id)?;

            // Accept it; the registry entry promotes from `Invited` to `Joined`.
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .send(Command::AcceptChatInvite {
                    session_id: ImSessionId::from(accept_group.uuid()),
                    from_group: true,
                })
                .await?;
            let accepted = require_session(ctx, accept_session, "after accept").await?;
            check_eq(
                "accepted session lifecycle",
                &matches!(accepted.lifecycle, ChatLifecycleView::Joined),
                &true,
            )?;

            // --- Decline path: invite the secondary to a second group, decline. ---
            let (decline_group, decline_source, decline_rtt) =
                provoke_group_invitation(ctx, primary_id, 1, "decline").await?;
            let decline_session = ChatSessionKind::Group {
                group_id: decline_group,
            };

            // Before declining, the registry shows this group session as a pending
            // invitation too (distinct id from the accepted one).
            let invited = require_session(ctx, decline_session, "before decline").await?;
            check_invited(&invited, primary_id)?;

            // Decline it; the registry entry is removed entirely.
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .send(Command::DeclineChatInvite {
                    session_id: ImSessionId::from(decline_group.uuid()),
                    from_group: true,
                })
                .await?;
            let after_decline = query_session(ctx, decline_session).await?;
            check(
                after_decline.is_none(),
                "declined group session should be removed from the registry",
            )?;

            // On the reused pre-made path, the secondary leaves the groups it
            // joined so the fixtures are restored to founder-only for the next
            // run — a *fresh* join is what makes the first message arrive as an
            // invitation rather than a plain session message. On the throwaway
            // path the groups are discarded with the run, so this is skipped.
            for (group_id, source) in [
                (accept_group, accept_source),
                (decline_group, decline_source),
            ] {
                if source == GroupSource::Premade {
                    ctx.secondary()
                        .ok_or_else(|| {
                            TestFailure::Assertion(
                                "two-account test ran without a secondary".to_owned(),
                            )
                        })?
                        .send(Command::LeaveGroup(group_id))
                        .await?;
                }
            }

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("accept_invite_rtt"), accept_rtt.as_secs_f64());
            metrics.set_timing(
                &secs_metric("decline_invite_rtt"),
                decline_rtt.as_secs_f64(),
            );
            metrics.set("accept_group_source", accept_source.label());
            metrics.set("decline_group_source", decline_source.label());
            // On OpenSim there is no `ChatSessionRequest` capability, so accept /
            // decline are observable only as the client-side registry transition;
            // the cap POST is the Aditi (Phase Z) variant.
            metrics.set("chat_session_request_cap", "absent");
            metrics.set("accept_promoted_to_joined", true);
            metrics.set("decline_removed_entry", true);
            Ok(())
        })
    }
}

/// Takes a group (a pre-made one from [`crate::fixtures`] by `index`, or a fresh
/// throwaway — see [`support::membership_group`]), has the secondary join it as a
/// member, then has the primary open the group's IM session and send one marker
/// message — which the secondary, not yet a session participant, receives as a
/// `ChatterBoxInvitation` ([`Event::ConferenceInvited`] with `from_group`).
/// Returns the group's id, where it came from (so the caller can leave a reused
/// group afterward), and the send→invitation round-trip time.
///
/// `index` and `purpose` distinguish the two groups this case needs (one to
/// accept, one to decline): `index` selects distinct pre-made groups by position,
/// and `purpose` distinguishes the group name and marker text.
async fn provoke_group_invitation(
    ctx: &mut TestContext,
    primary_id: AgentKey,
    index: usize,
    purpose: &str,
) -> Result<(GroupKey, GroupSource, Duration), TestFailure> {
    // The throwaway group's name carries a wall-clock suffix so repeated
    // create-per-run does not collide on the grid's unique-name constraint; it is
    // ignored on the pre-made path. The primary owns the group either way.
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let group = support::membership_group(
        ctx,
        index,
        &format!("sl-client chat-invite {purpose} {unique}"),
        "throwaway group for the chat-invite-accept-decline conformance case",
    )
    .await?;
    let group_id = group.group_id;

    // The secondary joins the open-enrollment group, becoming a member eligible
    // to receive the group's session traffic — but it does NOT open the session,
    // so the first message reaches it as an invitation, not a session message.
    let join_ok = {
        let secondary = ctx.secondary().ok_or_else(|| {
            TestFailure::Assertion("two-account test ran without a secondary".to_owned())
        })?;
        secondary.send(Command::JoinGroup(group_id)).await?;
        secondary
            .wait_for(REPLY_TIMEOUT, |event| match event {
                Event::JoinGroupResult {
                    group_id: joined,
                    success,
                } if *joined == group_id => Some(*success),
                _ => None,
            })
            .await?
    };
    check(join_ok, "secondary failed to join the group")?;

    // Let the membership commit on the group service before the primary sends, so
    // the distribution pass lists the secondary and routes it the invitation.
    tokio::time::sleep(MEMBERSHIP_SETTLE).await;

    // The primary opens the group session and sends a marker tagged with its own
    // agent id (so the predicate ignores unrelated background group traffic).
    ctx.primary()
        .send(Command::StartGroupSession(group_id))
        .await?;
    let marker = format!("chat-invite {purpose} {primary_id}");
    let sent_at = Instant::now();
    ctx.primary()
        .send(Command::SendGroupMessage {
            group_id,
            message: marker.clone(),
        })
        .await?;

    // The secondary observes the invitation: a `ChatterBoxInvitation` from the
    // primary on this group's session, carrying the marker inline.
    let invited_session = {
        let marker = marker.clone();
        ctx.secondary()
            .ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?
            .wait_for(REPLY_TIMEOUT, move |event| match event {
                Event::ConferenceInvited {
                    session_id,
                    from_agent_id,
                    from_group,
                    message,
                    ..
                } if *from_group
                    && *from_agent_id == primary_id
                    && *session_id == group_id.uuid()
                    && *message == marker =>
                {
                    Some(*session_id)
                }
                _ => None,
            })
            .await?
    };
    let invite_rtt = sent_at.elapsed();
    check_eq(
        "invited group session id",
        &GroupKey::from(invited_session),
        &group_id,
    )?;
    Ok((group_id, group.source, invite_rtt))
}

/// Queries the secondary's chat-session registry and returns the entry for
/// `kind`, or `None` when the registry has no such session.
async fn query_session(
    ctx: &mut TestContext,
    kind: ChatSessionKind,
) -> Result<Option<ChatSessionInfo>, TestFailure> {
    let secondary = ctx.secondary().ok_or_else(|| {
        TestFailure::Assertion("two-account test ran without a secondary".to_owned())
    })?;
    secondary.send(Command::QueryChatSessions).await?;
    let sessions = secondary
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::ChatSessions(sessions) => {
                Some(std::sync::Arc::<[ChatSessionInfo]>::clone(sessions))
            }
            _ => None,
        })
        .await?;
    Ok(sessions.iter().find(|info| info.kind == kind).cloned())
}

/// Like [`query_session`], but fails the test if the session is absent — used at
/// the points the case requires the session to exist (the two `Invited` checks
/// and the post-accept `Joined` check). `phase` names the moment for the failure
/// message.
async fn require_session(
    ctx: &mut TestContext,
    kind: ChatSessionKind,
    phase: &str,
) -> Result<ChatSessionInfo, TestFailure> {
    query_session(ctx, kind).await?.ok_or_else(|| {
        TestFailure::Assertion(format!("group session missing from the registry {phase}"))
    })
}

/// Asserts that `info` is a pending text-channel invitation from `inviter`.
fn check_invited(info: &ChatSessionInfo, inviter: AgentKey) -> Result<(), TestFailure> {
    match &info.lifecycle {
        ChatLifecycleView::Invited {
            inviter: actual,
            channel,
            ..
        } => {
            check_eq("invitation inviter", actual, &inviter)?;
            check_eq("invitation channel", channel, &InviteChannel::Text)?;
            Ok(())
        }
        ChatLifecycleView::Joined => Err(TestFailure::Assertion(
            "expected a pending invitation, found a joined session".to_owned(),
        )),
    }
}
