//! The primary posts a group notice; the secondary — a fellow group member who
//! accepts notices — receives it, and the primary then finds the same notice in
//! the group's notice history.
//!
//! Where [`super::group_session_message`] proves a live group *conversation* and
//! [`super::group_join_leave`] proves membership churn, this proves the group's
//! **notice** path: a one-shot announcement posted by a member with the
//! [`NOTICES_SEND`](sl_client_tokio::group_powers::NOTICES_SEND) power, fanned out
//! by the grid to every member who accepts notices and recorded in the group's
//! persistent notice history.
//!
//! A notice is posted as an `ImprovedInstantMessage` with the
//! [`ImDialog::GroupNotice`] dialog, the subject and body joined with a `|` on the
//! wire (see [`Command::SendGroupNotice`]). OpenSim's
//! `GroupsModule.OnInstantMessage` stores the notice and then relays a
//! `GroupNotice` IM to each member whose membership has `AcceptNotices` set — which
//! is the default for both the founder and a freshly joined member, so no explicit
//! accept-notices toggle is needed. The relayed IM is attributed *from the group*
//! (its `from_agent_id` is the group id, `from_group` is set), carries the same
//! `subject|body`, and its session id
//! ([`InstantMessage::id`](sl_client_tokio::InstantMessage::id)) is the new
//! notice's id.
//!
//! The case is `2av` because send-and-receive needs a distinct receiver: the
//! primary owns the group and posts the notice, while the **secondary** — having
//! joined the open-enrollment group — is the member that receives it. (The grid
//! also relays the notice back to the posting founder, but proving delivery to a
//! *different* avatar is the point.) After observing the live delivery, the
//! primary fetches the group's notice list with
//! [`Command::RequestGroupNotices`] and asserts the notice it just posted is in
//! the history with the same id and subject — a cross-check that the notice was
//! genuinely persisted, not merely echoed, exercising the `GroupNoticesListReply`
//! read path ([`Event::GroupNotices`]) alongside the IM-delivery path.
//!
//! The group comes from [`support::membership_group`] (index 0): a throwaway
//! created per run on OpenSim (the primary becomes founder/owner), or a reused
//! pre-made group on Second Life (avoiding the per-run L$100 and a founder slot).
//! Because the secondary leaves any group it joined, a reused pre-made group is
//! restored to its founder-only state for the next run.
//!
//! `2av`. Runs on OpenSim today (local secondary `Friend Tester`, Groups V2
//! enabled — see the setup memory); the Aditi variant is deferred to Phase Z
//! pending a second Aditi avatar (and a configured pre-made group).

use std::time::{Instant, SystemTime};

use sl_client_tokio::{Command, Event, GroupNoticeKey, ImDialog};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    self, GroupSource, REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, count_metric, secs_metric,
};

/// The primary posts a group notice that the secondary — a fellow member —
/// receives, and which the primary then finds in the group's notice history.
#[derive(Debug)]
pub struct GroupNotice;

impl GridTest for GroupNotice {
    fn name(&self) -> &'static str {
        "group-notice"
    }

    fn description(&self) -> &'static str {
        "Primary posts a group notice; a fellow member receives it and it appears in the notice history"
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

            // The group the notice is posted to: a pre-made group on grids that
            // configure one (Second Life), or a throwaway created here (the
            // OpenSim default). The name carries a wall-clock suffix so repeated
            // create-per-run does not collide on the grid's unique-name
            // constraint; it is ignored on the pre-made path. The primary owns
            // the group either way, so it holds the notice-send power.
            let unique = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |since| since.as_millis());
            let group = support::membership_group(
                ctx,
                0,
                &format!("sl-client group-notice {unique}"),
                "throwaway group for the group-notice conformance case",
            )
            .await?;
            let group_id = group.group_id;

            // The secondary joins the open-enrollment group; once a member (with
            // `AcceptNotices` set by default), it is eligible to receive the
            // group's notices.
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

            // The primary posts the notice. Both subject and body carry the
            // wall-clock marker so the receive predicate ignores any unrelated
            // background notice; neither contains a `|`, which the wire uses to
            // split subject from body.
            let subject = format!("sl-client group-notice {unique}");
            let body = format!("group-notice conformance body {unique}");
            let wire_message = format!("{subject}|{body}");
            let posted_at = Instant::now();
            ctx.primary()
                .send(Command::SendGroupNotice {
                    group_id,
                    subject: subject.clone(),
                    message: body.clone(),
                    attachment: None,
                })
                .await?;

            // The secondary receives the relayed notice IM. It is attributed from
            // the group (not the posting avatar), so the predicate keys on the
            // group-notice dialog and the exact `subject|body` rather than a
            // sender id. (The harness keeps forwarding every session's events, so
            // the primary's post is delivered even while we read only the
            // secondary.)
            let received = {
                let wire_message = wire_message.clone();
                ctx.secondary()
                    .ok_or_else(|| {
                        TestFailure::Assertion(
                            "two-account test ran without a secondary".to_owned(),
                        )
                    })?
                    .wait_for(REPLY_TIMEOUT, move |event| match event {
                        Event::InstantMessageReceived(im)
                            if im.dialog == ImDialog::GroupNotice && im.message == wire_message =>
                        {
                            Some((**im).clone())
                        }
                        _ => None,
                    })
                    .await?
            };
            let deliver_rtt = posted_at.elapsed();

            // The notice arrived as a group notice attributed to the group itself
            // (`from_group`, `from_agent_id == group_id`), carrying the posted
            // `subject|body`. Its session id is the new notice's id. (Dialog and
            // text were matched by the predicate; re-assert them so a regression
            // names the wrong field.)
            check_eq("received dialog", &received.dialog, &ImDialog::GroupNotice)?;
            check(
                received.from_group,
                "group notice was not flagged from_group",
            )?;
            check_eq(
                "received from_agent_id",
                &received.from_agent_id.uuid(),
                &group_id.uuid(),
            )?;
            check_eq("received message", &received.message, &wire_message)?;
            let received_notice_id = GroupNoticeKey::from(received.id);

            // Cross-check the post persisted: the primary fetches the group's
            // notice list and finds the notice it just posted, matched by id and
            // subject. This exercises the `GroupNoticesListReply` read path and
            // proves the notice was stored, not merely echoed over IM.
            let listed_at = Instant::now();
            ctx.primary()
                .send(Command::RequestGroupNotices(group_id))
                .await?;
            let notices = {
                ctx.primary()
                    .wait_for(REPLY_TIMEOUT, move |event| match event {
                        Event::GroupNotices {
                            group_id: listed,
                            notices,
                        } if *listed == group_id => Some(notices.clone()),
                        _ => None,
                    })
                    .await?
            };
            let list_rtt = listed_at.elapsed();
            let listed = notices
                .iter()
                .find(|notice| notice.notice_id == received_notice_id)
                .ok_or_else(|| {
                    TestFailure::Assertion(format!(
                        "posted notice {received_notice_id} not found in the group's {} listed \
                         notice(s)",
                        notices.len()
                    ))
                })?;
            check_eq("listed notice subject", &listed.subject, &subject)?;
            check(
                !listed.has_attachment,
                "attachment-free notice reported an attachment in the history",
            )?;

            // On the reused pre-made path, the secondary leaves the group so the
            // fixture is reset to its founder-only state for the next run. On the
            // throwaway path the group is discarded with the run, so this is
            // unnecessary and skipped.
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
            metrics.set_timing(&secs_metric("notice_deliver"), deliver_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("notice_list"), list_rtt.as_secs_f64());
            metrics.set("notice_received", true);
            metrics.set("notice_in_history", true);
            metrics.set(
                &count_metric("listed_notices"),
                i64::try_from(notices.len()).unwrap_or(-1),
            );
            Ok(())
        })
    }
}
