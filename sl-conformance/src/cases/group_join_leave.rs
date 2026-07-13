//! The secondary joins a group the primary owns, then leaves it, confirming
//! both the join and the leave as observable grid transitions.
//!
//! Where [`super::group_create_activate`] proves the founder's create/activate
//! lifecycle and [`super::group_session_message`] proves a group's messaging,
//! this proves plain **membership churn**: an avatar joining an open-enrollment
//! group with [`Command::JoinGroup`] and then leaving it with
//! [`Command::LeaveGroup`].
//!
//! The case is `2av` because the *member* doing the join/leave cannot be the
//! group's owner: the founder/owner is the group's last owner and a grid will
//! not let the last owner drop the group out from under it. So the primary
//! owns the group while the **secondary** performs the membership round-trip the
//! case is actually testing.
//!
//! Both ends are observable on OpenSim. A join replies with `JoinGroupReply`
//! ([`Event::JoinGroupResult`] carrying `success`). A leave is a two-event
//! transition: `GroupsModule.LeaveGroupRequest` sends `LeaveGroupReply`
//! ([`Event::LeaveGroupResult`] with `success`) *and then* `AgentDropGroup`
//! ([`Event::DroppedFromGroup`]) — the membership-list update that proves the
//! agent is genuinely out of the group, not merely acked. The case asserts both
//! so the leave is a real transition, not a bare reply.
//!
//! The group the secondary churns through comes from
//! [`support::membership_group`]: on OpenSim a throwaway group is created per run
//! (free, disposable), while on Second Life a pre-made group from
//! [`crate::fixtures`] is reused so the run does not spend L$100 and a founder
//! group slot every time (see that module for why). Because the secondary leaves
//! at the end, the pre-made group is left exactly as it was found — its only
//! member the founding primary — so consecutive runs stay clean.
//!
//! `2av`. Runs on OpenSim today (local secondary `Friend Tester`, Groups V2
//! enabled — see the setup memory); the Aditi variant is deferred to Phase Z
//! pending its Aditi run (and a configured pre-made group).

use std::time::{Instant, SystemTime};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{self, REGION_TIMEOUT, REPLY_TIMEOUT, check, secs_metric};
use sl_client_tokio::{Command, Event};

/// The secondary joins a group the primary owns and then leaves it, asserting
/// the join reply, the leave reply, and the follow-up drop-from-group.
#[derive(Debug)]
pub struct GroupJoinLeave;

impl GridTest for GroupJoinLeave {
    fn name(&self) -> &'static str {
        "group-join-leave"
    }

    fn description(&self) -> &'static str {
        "Secondary joins a group the primary owns, then leaves it, confirming the join and the drop"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Both avatars must be logged in and active before group membership
            // can be changed: the primary owns the group, the secondary joins and
            // leaves it.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for_region(REGION_TIMEOUT)
                .await?;

            // The group the secondary churns through: a pre-made group on grids
            // that configure one (Second Life), or a throwaway created here (the
            // OpenSim default). The name carries a wall-clock suffix so repeated
            // create-per-run does not collide on the grid's unique-name
            // constraint; it is ignored on the pre-made path.
            let unique = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |since| since.as_millis());
            let group = support::membership_group(
                ctx,
                0,
                &format!("sl-client group-join-leave {unique}"),
                "throwaway group for the group-join-leave conformance case",
            )
            .await?;
            let group_id = group.group_id;

            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;

            // The secondary joins the open-enrollment group; the grid acks with a
            // `JoinGroupReply` reporting success.
            let joined_at = Instant::now();
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
            let join_rtt = joined_at.elapsed();
            check(join_ok, "secondary failed to join the group")?;

            // The secondary leaves the group. OpenSim's `LeaveGroupRequest` first
            // sends a `LeaveGroupReply` (the ack) and then an `AgentDropGroup`
            // (the membership-list update). Assert both: the reply proves the
            // command was accepted, the drop proves the agent is genuinely out of
            // the group — and leaves a reused pre-made group clean for next time.
            let left_at = Instant::now();
            secondary.send(Command::LeaveGroup(group_id)).await?;
            let leave_ok = secondary
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::LeaveGroupResult {
                        group_id: left,
                        success,
                    } if *left == group_id => Some(*success),
                    _ => None,
                })
                .await?;
            check(leave_ok, "secondary's leave-group request was rejected")?;
            secondary
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::DroppedFromGroup { group_id: dropped } if *dropped == group_id => {
                        Some(())
                    }
                    _ => None,
                })
                .await?;
            let leave_rtt = left_at.elapsed();

            let metrics = ctx.metrics();
            if let Some(create_rtt) = group.create_rtt {
                metrics.set_timing(&secs_metric("group_create"), create_rtt.as_secs_f64());
            }
            metrics.set("group_source", group.source.label());
            metrics.set_timing(&secs_metric("group_join"), join_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("group_leave"), leave_rtt.as_secs_f64());
            metrics.set("join_success", join_ok);
            metrics.set("leave_success", leave_ok);
            metrics.set("dropped_from_group", true);
            Ok(())
        })
    }
}
