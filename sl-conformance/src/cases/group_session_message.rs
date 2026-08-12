//! The primary opens a group IM session, sends a message, and the secondary —
//! a fellow group member — receives it; the primary then leaves the session.

use std::time::{Duration, Instant, SystemTime};

use sl_client_tokio::{Command, Event, GroupKey};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    self, GroupSource, REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, count_metric, is_aditi,
    secs_metric,
};

/// How a group-session message was delivered to the recipient.
#[derive(Clone, Copy, Debug)]
enum Delivery {
    /// A UDP `ImprovedInstantMessage` with the `IM_SESSION_SEND` dialog — the
    /// path taken once the recipient is already a session participant. Surfaces
    /// as [`Event::GroupSessionMessage`].
    SessionMessage,
    /// A CAPS event-queue `ChatterBoxInvitation` that carries the first message
    /// inline — OpenSim's path for delivering to a member who has not yet joined
    /// the session. Surfaces as [`Event::ConferenceInvited`] with `from_group`.
    Invitation,
}

impl Delivery {
    /// The metric label recorded for this delivery path.
    const fn label(self) -> &'static str {
        match self {
            Self::SessionMessage => "group-session-message",
            Self::Invitation => "chatterbox-invitation",
        }
    }
}

/// The primary creates a group, the secondary joins it, then the primary opens
/// the group's IM session and sends a message that the secondary — a fellow
/// member — observes; finally the primary leaves the session.
///
/// A group IM is an `ImprovedInstantMessage` with `from_group` set, routed by
/// the grid's group-messaging service to every online member rather than
/// broadcast to the region like local chat or addressed to one recipient like a
/// 1:1 IM ([`super::im_1to1`]). The session id is the group id itself.
///
/// The case takes an open-enrollment group the primary owns — created fresh per
/// run, or a pre-made fixture group reused (see [`support::membership_group`];
/// the latter avoids Second Life's per-run group-creation fee) — and has the
/// secondary [`Command::JoinGroup`] it, so on OpenSim it depends only on Groups
/// V2 being enabled, not on any pre-existing group. Both avatars then
/// [`Command::StartGroupSession`] to register as session participants before the
/// primary
/// [`Command::SendGroupMessage`]s a marker tagged with its own agent id.
///
/// OpenSim's `GroupsMessagingModule` delivers a group message to a member who is
/// already a session participant as a UDP `IM_SESSION_SEND`
/// ([`Event::GroupSessionMessage`]), but to a member who has not yet joined the
/// session as a CAPS `ChatterBoxInvitation` carrying the first message inline
/// ([`Event::ConferenceInvited`] with `from_group`). The secondary pre-joins the
/// session to take the canonical [`Event::GroupSessionMessage`] path; the
/// predicate also accepts the invitation path so a lost join/send race still
/// proves delivery rather than flaking, recording which path was taken.
///
/// `LeaveGroupSession` is a client-side session-registry teardown with no
/// observable OpenSim protocol effect — `GroupsMessagingModule.OnInstantMessage`
/// ignores the `SessionDrop` dialog over UDP — so the case confirms only that
/// the circuit stays healthy across the leave (a keep-alive ping still
/// round-trips), the same "acceptance = absence of failure" shape that
/// [`super::throttle_set`] uses for a fire-and-forget command.
///
/// `2av`. Runs on OpenSim today (local secondary `Friend Tester`, Groups V2
/// enabled — see the setup memory); the Aditi variant is deferred to Phase Z
/// pending its Aditi run.
#[derive(Debug)]
pub struct GroupSessionMessage;

/// How long to let the simulator register both avatars as session participants
/// before the primary sends, so the message takes the UDP `IM_SESSION_SEND`
/// path. Generous because it bridges two reliable IMs through the
/// group-messaging service; the dual-path predicate makes correctness not depend
/// on it.
const SESSION_SETTLE: Duration = Duration::from_secs(3);

/// How long each delivery attempt waits before re-sending a fresh marker
/// (Second Life propagates a fresh join through its group service
/// asynchronously, so an early message can miss the new member).
const DELIVERY_ATTEMPT_WINDOW: Duration = Duration::from_secs(10);

/// How many markers to send before concluding delivery genuinely fails.
const DELIVERY_ATTEMPTS: u32 = 6;

impl GridTest for GroupSessionMessage {
    fn name(&self) -> &'static str {
        "group-session-message"
    }

    fn description(&self) -> &'static str {
        "Primary opens a group IM session and sends; a fellow member receives it, then the primary leaves"
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

            // The marker and the delivery predicate are keyed on the primary's
            // agent id (the sender). Capture it up front — the secondary's borrow
            // cannot be held across a `ctx.primary()` reborrow.
            let primary_id = ctx.primary().agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login did not report an agent id".to_owned())
            })?;

            // The group to message over: a pre-made group on grids that
            // configure one (Second Life), or a throwaway created here (the
            // OpenSim default). The name carries a wall-clock suffix so repeated
            // create-per-run does not collide on the grid's unique-name
            // constraint; it is ignored on the pre-made path. The primary owns
            // the group either way, so it can drive the session.
            let unique = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |since| since.as_millis());
            let group = support::membership_group(
                ctx,
                0,
                &format!("slc sess {unique}"),
                "throwaway group for the group-session-message conformance case",
            )
            .await?;
            let group_id = group.group_id;

            // The secondary joins the open-enrollment group; once a member, it is
            // eligible to receive the group's session traffic.
            let joined_at = Instant::now();
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
            let join_rtt = joined_at.elapsed();
            check(join_ok, "secondary failed to join the group")?;

            // Both avatars open the group's IM session. Registering the secondary
            // as a participant biases delivery toward the canonical UDP
            // `IM_SESSION_SEND` path; the primary opens its own session per the
            // start/send/leave lifecycle under test.
            ctx.primary()
                .send(Command::StartGroupSession(group_id))
                .await?;
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .send(Command::StartGroupSession(group_id))
                .await?;
            tokio::time::sleep(SESSION_SETTLE).await;

            // The primary sends a marker tagged with its own agent id and the
            // attempt number (so the predicate ignores any unrelated background
            // group traffic); the secondary observes it, by either delivery
            // path. Second Life's distributed group service propagates a fresh
            // join asynchronously — a message sent seconds after the join can
            // miss the new member — so the send is retried with a fresh marker
            // until one is delivered. (The harness forwards every session's
            // events off the run loop's bounded channel continuously, so the
            // primary keeps transmitting and the secondary keeps decoding even
            // while we read only the secondary.)
            let sent_at = Instant::now();
            let mut attempt: u32 = 0;
            let (heard_group, delivery) = loop {
                attempt = attempt.saturating_add(1);
                let marker = format!("group-session conformance {primary_id} a{attempt}");
                ctx.primary()
                    .send(Command::SendGroupMessage {
                        group_id,
                        message: marker.clone(),
                    })
                    .await?;
                match ctx
                    .secondary()
                    .ok_or_else(|| {
                        TestFailure::Assertion(
                            "two-account test ran without a secondary".to_owned(),
                        )
                    })?
                    .wait_for(DELIVERY_ATTEMPT_WINDOW, move |event| match event {
                        Event::GroupSessionMessage {
                            group_id: group,
                            from_agent_id,
                            message,
                            ..
                        } if *from_agent_id == primary_id && *message == marker => {
                            Some((*group, Delivery::SessionMessage))
                        }
                        Event::ConferenceInvited {
                            session_id,
                            from_agent_id,
                            from_group,
                            message,
                            ..
                        } if *from_group && *from_agent_id == primary_id && *message == marker => {
                            Some((GroupKey::from(*session_id), Delivery::Invitation))
                        }
                        _ => None,
                    })
                    .await
                {
                    Ok(heard) => break heard,
                    Err(TestFailure::Timeout(_)) if attempt < DELIVERY_ATTEMPTS => {}
                    Err(TestFailure::Timeout(_)) if is_aditi(grid) => {
                        // Observed live: a session message sent into a
                        // seconds-old group is dropped by Second Life (no
                        // delivery by either path) until the group service has
                        // propagated — sometimes past this case's whole retry
                        // budget. Record the honest gap rather than failing.
                        ctx.metrics()
                            .set(&count_metric("delivery_attempts"), i64::from(attempt));
                        ctx.mark_partial(
                            "no group-session message was delivered within the retry budget — \
                             Second Life drops messages into a freshly created group until its \
                             group service has propagated the membership",
                        );
                        return Ok(());
                    }
                    Err(other) => return Err(other),
                }
            };
            let deliver_rtt = sent_at.elapsed();

            // The message arrived on the group's own session (session id == group
            // id), tagged to the primary as sender (matched by the predicates).
            check_eq("group session id", &heard_group, &group_id)?;

            // The primary leaves the session. OpenSim has no observable effect for
            // a UDP `SessionDrop`, so confirm the circuit survives the leave: a
            // keep-alive ping still round-trips on the root circuit.
            ctx.primary()
                .send(Command::LeaveGroupSession(group_id))
                .await?;
            let leave_ping = ctx
                .primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::Ping {
                        child: false, rtt, ..
                    } => Some(*rtt),
                    _ => None,
                })
                .await?;

            // On the reused pre-made path, the secondary leaves the group so the
            // fixture is reset to its founding-member-only state for the next
            // run. On the throwaway path the group is discarded with the run, so
            // this is unnecessary and skipped.
            if group.source == GroupSource::Premade {
                ctx.secondary()
                    .ok_or_else(|| {
                        TestFailure::Assertion(
                            "two-account test ran without a secondary".to_owned(),
                        )
                    })?
                    .send(Command::LeaveGroup(group_id))
                    .await?;
            }

            let metrics = ctx.metrics();
            if let Some(create_rtt) = group.create_rtt {
                metrics.set_timing(&secs_metric("group_create"), create_rtt.as_secs_f64());
            }
            metrics.set("group_source", group.source.label());
            metrics.set_timing(&secs_metric("group_join"), join_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("deliver_rtt"), deliver_rtt.as_secs_f64());
            metrics.set("delivery_path", delivery.label());
            metrics.set_timing(&secs_metric("post_leave_ping"), leave_ping.as_secs_f64());
            metrics.set("leave_circuit_healthy", true);
            Ok(())
        })
    }
}
