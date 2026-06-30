//! The primary administers a group it owns: it assigns a fellow member (the
//! secondary) to a role, removes that assignment, and then ejects the member —
//! confirming each as an observable grid transition.
//!
//! Where [`super::group_join_leave`] proves a member churning its *own*
//! membership and [`super::group_roster`] proves the *read* side of a group, this
//! proves the **admin** side: an owner mutating another member's role assignments
//! ([`Command::ChangeGroupRoleMembers`], a `GroupRoleChanges`) and removing the
//! member outright ([`Command::EjectGroupMembers`], an `EjectGroupMemberRequest`).
//! Both require group powers the founder/owner holds — `RoleAssignMember` for the
//! role change, `MemberEject` for the ejection.
//!
//! The case is `2av` because the subject of the administration must be a member
//! other than the acting owner: an owner cannot eject itself (it leaves instead),
//! and a self role-change would not exercise the cross-member path. So the
//! **primary** owns the group and does the administering, while the
//! **secondary** — having joined the open-enrollment group — is the member it
//! acts on.
//!
//! Both halves are observable on OpenSim:
//!
//! - **Role change.** `GroupsModule.GroupRoleChanges` adds/removes the member↔role
//!   row and sends no direct reply, so the case never trusts the optimistic local
//!   cache: after each change it re-requests the role↔member pairings
//!   ([`Command::RequestGroupRoleMembers`] → [`Event::GroupRoleMembers`]) and
//!   asserts against the grid's authoritative roster, polling to absorb the brief
//!   write lag. It assigns the secondary to a non-owner assignable role (the stock
//!   "Officers" role, identified as the role whose id is neither the nil "Everyone"
//!   role nor the profile's owner role), confirms the pairing appears, then removes
//!   it and confirms the pairing is gone.
//! - **Ejection.** `GroupsModule.EjectGroupMember` removes the member and replies
//!   to the ejector with `EjectGroupMemberReply`
//!   ([`Event::EjectGroupMemberResult`] carrying `success`) *and* sends the ejectee
//!   an `AgentDropGroup` ([`Event::DroppedFromGroup`]) — the membership-list update
//!   that proves the member is genuinely out, not merely acked. The case asserts
//!   both, mirroring how [`super::group_join_leave`] asserts a voluntary leave.
//!
//! The group comes from [`support::membership_group`] (index 0): a throwaway
//! created per run on OpenSim (the primary becomes founder/owner), or a reused
//! pre-made group on Second Life (avoiding the per-run L$100 and a founder slot).
//! Because the ejection removes the secondary, a reused pre-made group is restored
//! to its founder-only state for the next run.
//!
//! `2av`. Runs on OpenSim today (local secondary `Friend Tester`, Groups V2
//! enabled — see the setup memory); the Aditi variant — and a multi-member
//! role/roster assertion that wants a third avatar — is deferred to Phase Z
//! pending more Aditi avatars (and a configured pre-made group).

use std::time::{Duration, Instant, SystemTime};

use sl_client_tokio::{
    Command, Event, GroupKey, GroupRoleChange, GroupRoleKey, GroupRoleMember, GroupRoleMemberChange,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{self, GroupSource, REGION_TIMEOUT, REPLY_TIMEOUT, check, secs_metric};

/// Settle after creating a throwaway group, so its roles are persisted and
/// queryable before the admin requests go out. Only applied on the created
/// (OpenSim) path; a reused pre-made group is already settled.
const CREATE_SETTLE: Duration = Duration::from_secs(2);

/// How long a role↔member re-fetch poll keeps re-requesting before giving up.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait between role↔member re-fetches.
const VERIFY_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Fetch a group's role↔member pairings by issuing a fresh
/// [`Command::RequestGroupRoleMembers`] and returning the grid's authoritative
/// reply (rather than the optimistic local cache).
async fn fetch_role_members(
    session: &mut Session,
    group_id: GroupKey,
) -> Result<Vec<GroupRoleMember>, TestFailure> {
    support::send_then_wait(
        session,
        Command::RequestGroupRoleMembers(group_id),
        REPLY_TIMEOUT,
        |event| match event {
            Event::GroupRoleMembers {
                group_id: replied,
                pairs,
                ..
            } if *replied == group_id => Some(pairs.clone()),
            _ => None,
        },
    )
    .await
}

/// Re-fetch the role↔member pairings until they satisfy `predicate`, or fail with
/// `description` once [`VERIFY_TIMEOUT`] elapses. Absorbs the brief lag of
/// OpenSim's fire-and-forget `GroupRoleChanges` write.
async fn poll_role_members<P>(
    session: &mut Session,
    group_id: GroupKey,
    mut predicate: P,
    description: &str,
) -> Result<(), TestFailure>
where
    P: FnMut(&[GroupRoleMember]) -> bool,
{
    let start = Instant::now();
    loop {
        let pairs = fetch_role_members(session, group_id).await?;
        if predicate(&pairs) {
            return Ok(());
        }
        if start.elapsed() >= VERIFY_TIMEOUT {
            return Err(TestFailure::Assertion(format!(
                "role↔member roster never reached expected state: {description}"
            )));
        }
        tokio::time::sleep(VERIFY_POLL_INTERVAL).await;
    }
}

/// The primary assigns and unassigns a fellow member to a role, then ejects them,
/// asserting each as an observable grid transition.
#[derive(Debug)]
pub struct GroupAdmin;

impl GridTest for GroupAdmin {
    fn name(&self) -> &'static str {
        "group-admin"
    }

    fn description(&self) -> &'static str {
        "Owner assigns/unassigns a member's role and ejects them, each as an observable transition"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Both avatars must be logged in and active: the primary owns the
            // group and administers it, the secondary is the member it acts on.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for_region(REGION_TIMEOUT)
                .await?;

            // The group the primary administers: a pre-made group on grids that
            // configure one (Second Life), or a throwaway created here (the OpenSim
            // default). The name carries a wall-clock suffix so repeated
            // create-per-run does not collide on the grid's unique-name
            // constraint; it is ignored on the pre-made path. The primary owns the
            // group either way, so it holds the admin powers.
            let unique = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |since| since.as_millis());
            let group = support::membership_group(
                ctx,
                0,
                &format!("sl-client group-admin {unique}"),
                "throwaway group for the group-admin conformance case",
            )
            .await?;
            let group_id = group.group_id;

            // A freshly created group's role rows are persisted by the create
            // itself, but allow a brief settle on that path so the admin queries do
            // not race the write burst.
            if matches!(group.source, GroupSource::Created) {
                tokio::time::sleep(CREATE_SETTLE).await;
            }

            // The secondary joins the open-enrollment group; once a member it is a
            // valid subject for a role change and an ejection.
            let secondary_id = {
                let secondary = ctx.secondary().ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?;
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
                secondary.agent_id().ok_or_else(|| {
                    TestFailure::Assertion("secondary has no agent id after login".to_owned())
                })?
            };

            // Resolve the role to assign the secondary to. The profile names the
            // owner role; the target is an assignable role that is neither that
            // owner role nor the nil "Everyone" role every member already holds —
            // the stock "Officers" role on a default group.
            let owner_role = {
                let session = ctx.primary();
                session.send(Command::RequestGroupProfile(group_id)).await?;
                session
                    .wait_for(REPLY_TIMEOUT, |event| match event {
                        Event::GroupProfileReceived(profile) if profile.group_id == group_id => {
                            Some(profile.owner_role)
                        }
                        _ => None,
                    })
                    .await?
            };
            let target_role: GroupRoleKey = {
                let session = ctx.primary();
                session.send(Command::RequestGroupRoles(group_id)).await?;
                let roles = session
                    .wait_for(REPLY_TIMEOUT, |event| match event {
                        Event::GroupRoleData {
                            group_id: replied,
                            roles,
                            ..
                        } if *replied == group_id => Some(roles.clone()),
                        _ => None,
                    })
                    .await?;
                roles
                    .iter()
                    .find_map(|role| match role.role_id {
                        Some(id) if id != owner_role => Some(id),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        TestFailure::Assertion(
                            "group has no assignable non-owner role to test role changes against"
                                .to_owned(),
                        )
                    })?
            };

            // Assign the secondary to the target role. `GroupRoleChanges` has no
            // direct reply, so confirm it by re-fetching the authoritative
            // role↔member roster until the new pairing appears.
            let assigned_at = Instant::now();
            ctx.primary()
                .send(Command::ChangeGroupRoleMembers {
                    group_id,
                    changes: vec![GroupRoleMemberChange {
                        role_id: Some(target_role),
                        member_id: secondary_id,
                        change: GroupRoleChange::Add,
                    }],
                })
                .await?;
            poll_role_members(
                ctx.primary(),
                group_id,
                |pairs| {
                    pairs.iter().any(|pair| {
                        pair.member_id == secondary_id && pair.role_id == Some(target_role)
                    })
                },
                "secondary was not paired with the target role after the add",
            )
            .await?;
            let role_add_rtt = assigned_at.elapsed();

            // Remove the secondary from the target role and confirm the pairing is
            // gone — proving the change is a real transition, not a one-way add.
            let unassigned_at = Instant::now();
            ctx.primary()
                .send(Command::ChangeGroupRoleMembers {
                    group_id,
                    changes: vec![GroupRoleMemberChange {
                        role_id: Some(target_role),
                        member_id: secondary_id,
                        change: GroupRoleChange::Remove,
                    }],
                })
                .await?;
            poll_role_members(
                ctx.primary(),
                group_id,
                |pairs| {
                    !pairs.iter().any(|pair| {
                        pair.member_id == secondary_id && pair.role_id == Some(target_role)
                    })
                },
                "secondary was still paired with the target role after the remove",
            )
            .await?;
            let role_remove_rtt = unassigned_at.elapsed();

            // Eject the secondary. OpenSim replies to the ejector with an
            // `EjectGroupMemberReply` (the ack) and sends the ejectee an
            // `AgentDropGroup` (the membership-list update). Assert both: the reply
            // proves the command was accepted, the drop proves the member is
            // genuinely out of the group — and restores a reused pre-made group to
            // its founder-only state for next time.
            let ejected_at = Instant::now();
            ctx.primary()
                .send(Command::EjectGroupMembers {
                    group_id,
                    member_ids: vec![secondary_id],
                })
                .await?;
            let eject_ok = ctx
                .primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::EjectGroupMemberResult {
                        group_id: replied,
                        success,
                    } if *replied == group_id => Some(*success),
                    _ => None,
                })
                .await?;
            check(eject_ok, "the eject request was rejected")?;
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::DroppedFromGroup { group_id: dropped } if *dropped == group_id => {
                        Some(())
                    }
                    _ => None,
                })
                .await?;
            let eject_rtt = ejected_at.elapsed();

            let metrics = ctx.metrics();
            if let Some(create_rtt) = group.create_rtt {
                metrics.set_timing(&secs_metric("group_create"), create_rtt.as_secs_f64());
            }
            metrics.set("group_source", group.source.label());
            metrics.set_timing(&secs_metric("role_add"), role_add_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("role_remove"), role_remove_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("eject"), eject_rtt.as_secs_f64());
            metrics.set("role_assigned", true);
            metrics.set("role_unassigned", true);
            metrics.set("eject_success", eject_ok);
            metrics.set("dropped_from_group", true);
            Ok(())
        })
    }
}
