//! The secondary is invited to two group IM sessions; it accepts one (the
//! registry entry promotes to joined) and declines the other (the entry is
//! removed), exercising `AcceptChatInvite` / `DeclineChatInvite`.

use std::time::{Duration, Instant, SystemTime};

use sl_client_tokio::{
    AgentKey, ChatLifecycleView, ChatSessionInfo, ChatSessionKind, Command, Event, GroupKey,
    InviteChannel,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    self, GroupSource, REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, count_metric, is_aditi,
    secs_metric,
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
/// On Second Life the groups are pre-made: the founder's start in a group it
/// founded seconds before is refused `unverified` (its refusal of a non-member),
/// while a group both avatars have been members of for a while works at once.
/// There the secondary stays a member between runs — an invitation reaches any
/// member who has not opened the session, and a group left with its founder
/// alone is purged — so it joins only when it is not a member yet. Either way
/// the primary waits for the grid to take its start, retrying a refused one for
/// a minute, before it sends.
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
/// the client-side registry transition under test.
///
/// `2av`, `[both]`. On OpenSim it founds two throwaway groups (Groups V2
/// enabled); on aditi it takes the fixtures' first two `premade_groups`.
#[derive(Debug)]
pub struct ChatInviteAcceptDecline;

/// How long to let the secondary's group membership settle on the group service
/// before the primary sends into the session, so the message-distribution pass
/// lists the secondary as a member and routes it the invitation. Generous
/// because it bridges the join reply through the Groups V2 backend.
const MEMBERSHIP_SETTLE: Duration = Duration::from_secs(2);

/// How long each invitation attempt waits before re-sending a fresh marker
/// (Second Life propagates a fresh group/join through its group service
/// asynchronously and drops messages into it meanwhile).
const DELIVERY_ATTEMPT_WINDOW: Duration = Duration::from_secs(10);

/// How many markers to send before concluding delivery genuinely fails.
const DELIVERY_ATTEMPTS: u32 = 6;

/// How long the primary keeps asking to start a group session the grid refuses.
///
/// Second Life refuses the founder's start in a group founded seconds earlier
/// with `unverified` — its refusal of a non-member — and accepts the same start
/// in the same group hours later (measured on aditi 2026-11-19: refused 2.4 s
/// after the `CreateGroupReply`, taken 2.5 h after it). How long the refusal
/// lasts in between is not measured, so the start is retried for a minute.
const START_BUDGET: Duration = Duration::from_secs(60);

/// How long to wait between two starts the grid refused.
const START_RETRY_INTERVAL: Duration = Duration::from_secs(5);

/// Why [`provoke_group_invitation`] has no invitation to hand back.
#[derive(Debug)]
enum NoInvitation {
    /// The grid refused every start of the group's session within
    /// [`START_BUDGET`]; the error of the last refusal.
    StartRefused(String),
    /// The session started, but none of the [`DELIVERY_ATTEMPTS`] markers
    /// reached the secondary as an invitation.
    NotDelivered {
        /// Whether the secondary joined the group during this run.
        fresh_join: bool,
    },
}

/// The outcome when the secondary got no invitation for the `half` half. A
/// refused start is the grid saying so, and on Second Life it is the young
/// group's refusal (see [`START_BUDGET`]), so it is an honest partial there and
/// a failure on OpenSim, which takes any member's start. An undelivered
/// invitation after a start the grid took is a partial on Second Life only when
/// the secondary joined moments before (its group service propagates a join
/// asynchronously and drops messages meanwhile), and a failure otherwise.
fn no_invitation_outcome(
    ctx: &mut TestContext,
    grid: Grid,
    half: &str,
    why: NoInvitation,
) -> Result<(), TestFailure> {
    match why {
        NoInvitation::StartRefused(error) if is_aditi(grid) => {
            ctx.mark_partial(&format!(
                "the primary's start of the {half} half's group session was refused \
                 \"{error}\" for {} s — Second Life refuses the founder's start in a group \
                 founded moments before; name groups both avatars are members of as \
                 premade_groups in the fixtures",
                START_BUDGET.as_secs()
            ));
            Ok(())
        }
        NoInvitation::StartRefused(error) => Err(TestFailure::Assertion(format!(
            "the primary's start of the {half} half's group session was refused: {error}"
        ))),
        NoInvitation::NotDelivered { fresh_join: true } if is_aditi(grid) => {
            ctx.mark_partial(&format!(
                "no group-session invitation was delivered within the retry budget for the \
                 {half} half — the secondary joined the group during the run, and Second \
                 Life drops messages to a fresh member until its group service has \
                 propagated the membership"
            ));
            Ok(())
        }
        NoInvitation::NotDelivered { .. } => Err(TestFailure::Assertion(format!(
            "group invitation was never delivered ({half} half)"
        ))),
    }
}

impl GridTest for ChatInviteAcceptDecline {
    fn name(&self) -> &'static str {
        "chat-invite-accept-decline"
    }

    fn description(&self) -> &'static str {
        "Secondary accepts one group-IM invitation and declines another; assert the registry transitions"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
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
            let accept = match provoke_group_invitation(ctx, primary_id, 0, "accept").await? {
                Ok(invitation) => invitation,
                Err(why) => return no_invitation_outcome(ctx, grid, "accept", why),
            };
            let accept_session = ChatSessionKind::Group {
                group_id: accept.group_id,
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
                    session: accept_session,
                })
                .await?;
            let accepted = require_session(ctx, accept_session, "after accept").await?;
            check_eq(
                "accepted session lifecycle",
                &matches!(accepted.lifecycle, ChatLifecycleView::Joined),
                &true,
            )?;

            // --- Decline path: invite the secondary to a second group, decline. ---
            let decline = match provoke_group_invitation(ctx, primary_id, 1, "decline").await? {
                Ok(invitation) => invitation,
                Err(why) => return no_invitation_outcome(ctx, grid, "decline", why),
            };
            let decline_session = ChatSessionKind::Group {
                group_id: decline.group_id,
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
                    session: decline_session,
                })
                .await?;
            let after_decline = query_session(ctx, decline_session).await?;
            check(
                after_decline.is_none(),
                "declined group session should be removed from the registry",
            )?;

            // On OpenSim's reused pre-made path, the secondary leaves the groups
            // it joined so the fixtures are restored to founder-only for the
            // next run — a *fresh* join is what makes OpenSim route the first
            // message as an invitation rather than a plain session message. On
            // Second Life the secondary stays a member: an invitation reaches
            // any member who has not opened the session, and a group left with
            // its founder alone is purged by the grid. On the throwaway path the
            // groups are discarded with the run, so this is skipped.
            for invitation in [&accept, &decline] {
                if invitation.source == GroupSource::Premade
                    && invitation.joined_this_run
                    && !is_aditi(grid)
                {
                    let group_id = invitation.group_id;
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
            for (half, invitation) in [("accept", &accept), ("decline", &decline)] {
                metrics.set_timing(
                    &secs_metric(&format!("{half}_invite_rtt")),
                    invitation.invite_rtt.as_secs_f64(),
                );
                metrics.set(&format!("{half}_group_source"), invitation.source.label());
                metrics.set(
                    &format!("{half}_secondary_joined_this_run"),
                    invitation.joined_this_run,
                );
                metrics.set(
                    &count_metric(&format!("{half}_start_attempt")),
                    i64::from(invitation.start_attempts),
                );
            }
            metrics.set("accept_promoted_to_joined", true);
            metrics.set("decline_removed_entry", true);
            Ok(())
        })
    }
}

/// A group-session invitation the secondary received, and how it came about.
#[derive(Debug)]
struct Invitation {
    /// The group whose session the secondary was invited to.
    group_id: GroupKey,
    /// Where the group came from.
    source: GroupSource,
    /// Whether the secondary joined the group during this run (it was not yet
    /// a member), which decides whether the run leaves it again.
    joined_this_run: bool,
    /// How many starts the primary sent before the grid took one.
    start_attempts: u32,
    /// The time from the first marker sent to the invitation arriving.
    invite_rtt: Duration,
}

/// Takes a group (a pre-made one from [`crate::fixtures`] by `index`, or a fresh
/// throwaway — see [`support::membership_group`]), has the secondary join it as a
/// member unless it already is one, has the primary start the group's IM session
/// until the grid takes the start, then has it send a marker message — which the
/// secondary, not yet a session participant, receives as a `ChatterBoxInvitation`
/// ([`Event::ConferenceInvited`] with `from_group`).
///
/// `index` and `purpose` distinguish the two groups this case needs (one to
/// accept, one to decline): `index` selects distinct pre-made groups by position,
/// and `purpose` distinguishes the group name and marker text.
///
/// The inner `Err` says why no invitation arrived; the outer one is a failure of
/// the run itself.
async fn provoke_group_invitation(
    ctx: &mut TestContext,
    primary_id: AgentKey,
    index: usize,
    purpose: &str,
) -> Result<Result<Invitation, NoInvitation>, TestFailure> {
    // The throwaway group's name carries a wall-clock suffix so repeated
    // create-per-run does not collide on the grid's unique-name constraint; it is
    // ignored on the pre-made path. The primary owns the group either way.
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let group = support::membership_group(
        ctx,
        index,
        &format!("slc inv {purpose} {unique}"),
        "throwaway group for the chat-invite-accept-decline conformance case",
    )
    .await?;
    let group_id = group.group_id;

    // The secondary must be a member eligible to receive the group's session
    // traffic — but it does NOT open the session, so the first message reaches
    // it as an invitation, not a session message. A pre-made group on Second
    // Life already has it as a member (it is never left there; see the
    // cleanup), so it joins only when its membership list lacks the group.
    let joined_this_run = {
        let secondary = ctx.secondary().ok_or_else(|| {
            TestFailure::Assertion("two-account test ran without a secondary".to_owned())
        })?;
        let already_member = group.source == GroupSource::Premade
            && secondary_memberships(secondary).await?.contains(&group_id);
        if already_member {
            false
        } else {
            secondary.send(Command::JoinGroup(group_id)).await?;
            let join_ok = secondary
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::JoinGroupResult {
                        group_id: joined,
                        success,
                    } if *joined == group_id => Some(*success),
                    _ => None,
                })
                .await?;
            check(join_ok, "secondary failed to join the group")?;
            // Let the membership commit on the group service before the primary
            // sends, so the distribution pass lists the secondary and routes it
            // the invitation.
            tokio::time::sleep(MEMBERSHIP_SETTLE).await;
            true
        }
    };

    let start_attempts = match start_group_session(ctx, group_id).await? {
        Ok(attempts) => attempts,
        Err(error) => return Ok(Err(NoInvitation::StartRefused(error))),
    };

    // The primary sends a marker tagged with its own agent id and the attempt
    // number (so the predicate ignores unrelated background group traffic).
    // Second Life's group service propagates a fresh join asynchronously and
    // drops messages to the new member meanwhile (see
    // [`super::group_session_message`]), so the send is retried with a fresh
    // marker; delivery exhaustion is handed back for the caller to record
    // honestly.
    let sent_at = Instant::now();
    let mut attempt: u32 = 0;
    let invited_session = loop {
        attempt = attempt.saturating_add(1);
        let marker = format!("chat-invite {purpose} {primary_id} a{attempt}");
        ctx.primary()
            .send(Command::SendGroupMessage {
                group_id,
                message: marker.clone(),
            })
            .await?;
        match ctx
            .secondary()
            .ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?
            .wait_for(DELIVERY_ATTEMPT_WINDOW, move |event| match event {
                Event::ConferenceInvited {
                    session_id,
                    from_agent_id,
                    session: ChatSessionKind::Group { group_id: invited },
                    message,
                    ..
                } if *from_agent_id == primary_id && *invited == group_id && *message == marker => {
                    Some(*session_id)
                }
                _ => None,
            })
            .await
        {
            Ok(session) => break session,
            Err(TestFailure::Timeout(_)) if attempt < DELIVERY_ATTEMPTS => {}
            Err(TestFailure::Timeout(_)) => {
                return Ok(Err(NoInvitation::NotDelivered {
                    fresh_join: joined_this_run,
                }));
            }
            Err(other) => return Err(other),
        }
    };
    let invite_rtt = sent_at.elapsed();
    check_eq(
        "invited group session id",
        &GroupKey::from(invited_session),
        &group_id,
    )?;
    Ok(Ok(Invitation {
        group_id,
        source: group.source,
        joined_this_run,
        start_attempts,
        invite_rtt,
    }))
}

/// The groups the secondary is a member of, from a fresh
/// [`Command::RequestAgentDataUpdate`]'s membership list.
async fn secondary_memberships(secondary: &mut Session) -> Result<Vec<GroupKey>, TestFailure> {
    secondary.send(Command::RequestAgentDataUpdate).await?;
    secondary
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::GroupMemberships(groups) => {
                Some(groups.iter().map(|group| group.group_id).collect())
            }
            _ => None,
        })
        .await
}

/// Has the primary start `group_id`'s session until the grid takes the start,
/// and returns how many starts that took — or, once [`START_BUDGET`] has run
/// out, the error of the last refusal.
///
/// The grid answers every start with a `ChatterBoxSessionStartReply`
/// ([`Event::ChatSessionStarted`]), OpenSim's group module included. A refused
/// start drops the session, so the next start opens it afresh.
async fn start_group_session(
    ctx: &mut TestContext,
    group_id: GroupKey,
) -> Result<Result<u32, String>, TestFailure> {
    let started = Instant::now();
    let mut attempts: u32 = 0;
    loop {
        attempts = attempts.saturating_add(1);
        let primary = ctx.primary();
        primary.send(Command::StartGroupSession(group_id)).await?;
        let (success, error) = primary
            .wait_for(REPLY_TIMEOUT, |event| match event {
                Event::ChatSessionStarted {
                    temp_session_id,
                    session_id,
                    success,
                    error,
                    ..
                } if GroupKey::from(temp_session_id.get()) == group_id
                    || GroupKey::from(session_id.get()) == group_id =>
                {
                    Some((*success, error.clone()))
                }
                _ => None,
            })
            .await?;
        if success {
            return Ok(Ok(attempts));
        }
        tracing::info!(
            group = %group_id.uuid(),
            attempt = attempts,
            error = %error,
            "the grid refused the primary's group session start"
        );
        if started.elapsed() >= START_BUDGET {
            return Ok(Err(error));
        }
        tokio::time::sleep(START_RETRY_INTERVAL).await;
    }
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
