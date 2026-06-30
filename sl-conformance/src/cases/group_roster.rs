//! Fetch a group's full roster: its profile, members, roles, role↔member
//! pairings, and the agent's selectable titles.
//!
//! Where [`super::group_create_activate`] proves the founder's create/activate
//! lifecycle and [`super::group_join_leave`] proves membership churn, this proves
//! the **read** side of a group: the five roster queries a viewer issues when it
//! opens a group's profile floater.
//!
//! - [`Command::RequestGroupProfile`] → [`Event::GroupProfileReceived`]: the
//!   group's charter, founder, member/role counts, and owner role.
//! - [`Command::RequestGroupMembers`] → [`Event::GroupMembers`]: the member
//!   roster, each with its powers, title, and owner flag.
//! - [`Command::RequestGroupRoles`] → [`Event::GroupRoleData`]: the group's
//!   roles (a stock group has at least the default "Everyone" role and an
//!   "Owners" role).
//! - [`Command::RequestGroupRoleMembers`] → [`Event::GroupRoleMembers`]: the
//!   role↔member pairings.
//! - [`Command::RequestGroupTitles`] → [`Event::GroupTitles`]: the titles the
//!   requesting agent may wear, one of which is selected.
//!
//! Rather than assert these five replies in isolation, the case cross-checks
//! them against each other so the run proves they describe the *same*,
//! self-consistent group: the profile names a founder and an owner role; the
//! member roster must then contain that founder flagged as an owner; the role
//! list must contain that owner role; and the role↔member pairings must pair the
//! founder with the owner role. This catches a query returning a stale or
//! mismatched roster, not merely an empty one.
//!
//! The group comes from [`support::membership_group`] (index 0): on OpenSim a
//! throwaway group is created per run (free, the primary becomes founder/owner),
//! while on Second Life a pre-made group from [`crate::fixtures`] is reused so the
//! run does not spend L$100 and a founder group slot each time. The case only
//! reads the group, so it leaves it exactly as it found it. On the created path
//! the founder is the primary itself, so the case additionally pins the reported
//! founder to the primary's own agent id.
//!
//! `1av`. Needs Groups V2 enabled on OpenSim (see the setup memory). Listed
//! `[both]`: a single avatar can run it on Aditi too, but the Aditi run is
//! batched with the rest of the deferred Aditi records.

use std::time::{Duration, Instant, SystemTime};

use sl_client_tokio::{Command, Event};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    self, GroupSource, REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, count_metric, secs_metric,
};

/// Settle after creating a throwaway group, so its members/roles are persisted
/// and queryable before the roster requests go out. Only applied on the created
/// (OpenSim) path; a reused pre-made group is already settled.
const CREATE_SETTLE: Duration = Duration::from_secs(2);

/// Fetches a group's profile, members, roles, role↔member pairings, and titles,
/// asserting the five replies describe the same self-consistent group.
#[derive(Debug)]
pub struct GroupRoster;

impl GridTest for GroupRoster {
    fn name(&self) -> &'static str {
        "group-roster"
    }

    fn description(&self) -> &'static str {
        "Fetch a group's profile, members, roles, role-member pairings, and titles, cross-checked"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;

            // The group whose roster we read: a pre-made group on grids that
            // configure one (Second Life), or a throwaway created here (the
            // OpenSim default, leaving the primary as founder/owner). The name
            // carries a wall-clock suffix so repeated create-per-run does not
            // collide on the grid's unique-name constraint; it is ignored on the
            // pre-made path.
            let unique = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |since| since.as_millis());
            let group = support::membership_group(
                ctx,
                0,
                &format!("sl-client group-roster {unique}"),
                "throwaway group for the group-roster conformance case",
            )
            .await?;
            let group_id = group.group_id;

            // A freshly created group's member/role rows are persisted by the
            // create itself, but allow a brief settle on that path so the roster
            // queries do not race the write burst.
            if matches!(group.source, GroupSource::Created) {
                tokio::time::sleep(CREATE_SETTLE).await;
            }

            let session = ctx.primary();
            let my_agent_id = session.agent_id();

            // Profile: the anchor for the cross-checks — it names the founder, the
            // owner role, and the member/role counts the other queries must agree
            // with.
            let profile_at = Instant::now();
            session.send(Command::RequestGroupProfile(group_id)).await?;
            let profile = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::GroupProfileReceived(profile) if profile.group_id == group_id => {
                        Some((**profile).clone())
                    }
                    _ => None,
                })
                .await?;
            let profile_rtt = profile_at.elapsed();
            let founder = profile.founder_id;
            let owner_role = profile.owner_role;
            check(
                profile.member_count >= 1,
                "group profile reported no members",
            )?;
            check(profile.role_count >= 1, "group profile reported no roles")?;

            // Member roster: must contain the founder flagged as an owner.
            let members_at = Instant::now();
            session.send(Command::RequestGroupMembers(group_id)).await?;
            let (reported_member_count, members) = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::GroupMembers {
                        group_id: replied,
                        member_count,
                        members,
                        ..
                    } if *replied == group_id => Some((*member_count, members.clone())),
                    _ => None,
                })
                .await?;
            let members_rtt = members_at.elapsed();
            check(!members.is_empty(), "member roster was empty")?;
            check(
                members
                    .iter()
                    .any(|member| member.agent_id == founder && member.is_owner),
                "the profile's founder is not present in the member roster as an owner",
            )?;

            // Roles: must contain the profile's owner role.
            let roles_at = Instant::now();
            session.send(Command::RequestGroupRoles(group_id)).await?;
            let (reported_role_count, roles) = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::GroupRoleData {
                        group_id: replied,
                        role_count,
                        roles,
                        ..
                    } if *replied == group_id => Some((*role_count, roles.clone())),
                    _ => None,
                })
                .await?;
            let roles_rtt = roles_at.elapsed();
            check(!roles.is_empty(), "role list was empty")?;
            check(
                roles.iter().any(|role| role.role_id == Some(owner_role)),
                "the profile's owner role is not present in the role list",
            )?;

            // Role↔member pairings: must pair the founder with the owner role.
            let role_members_at = Instant::now();
            session
                .send(Command::RequestGroupRoleMembers(group_id))
                .await?;
            let pairs = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::GroupRoleMembers {
                        group_id: replied,
                        pairs,
                        ..
                    } if *replied == group_id => Some(pairs.clone()),
                    _ => None,
                })
                .await?;
            let role_members_rtt = role_members_at.elapsed();
            check(!pairs.is_empty(), "role-member pairings were empty")?;
            check(
                pairs
                    .iter()
                    .any(|pair| pair.member_id == founder && pair.role_id == Some(owner_role)),
                "the founder is not paired with the owner role",
            )?;

            // Titles: the agent's own selectable titles; at least one is the
            // currently selected title.
            let titles_at = Instant::now();
            session.send(Command::RequestGroupTitles(group_id)).await?;
            let titles = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::GroupTitles {
                        group_id: replied,
                        titles,
                        ..
                    } if *replied == group_id => Some(titles.clone()),
                    _ => None,
                })
                .await?;
            let titles_rtt = titles_at.elapsed();
            check(!titles.is_empty(), "no group titles were returned")?;
            let selected_titles = titles.iter().filter(|title| title.selected).count();

            // On the created path the primary is the group's founder, so pin the
            // reported founder to its own agent id (a stronger check than the
            // generic cross-consistency above). On the pre-made path the founder
            // may be another avatar the primary merely belongs under, so this is
            // skipped there.
            if matches!(group.source, GroupSource::Created)
                && let Some(me) = my_agent_id
            {
                check_eq("group founder", &founder, &me)?;
            }

            let members_returned = i64::try_from(members.len()).unwrap_or(-1);
            let roles_returned = i64::try_from(roles.len()).unwrap_or(-1);
            let pairs_returned = i64::try_from(pairs.len()).unwrap_or(-1);
            let titles_returned = i64::try_from(titles.len()).unwrap_or(-1);
            let selected_returned = i64::try_from(selected_titles).unwrap_or(-1);

            let metrics = ctx.metrics();
            metrics.set("group_source", group.source.label());
            if let Some(create_rtt) = group.create_rtt {
                metrics.set_timing(&secs_metric("group_create"), create_rtt.as_secs_f64());
            }
            metrics.set_timing(&secs_metric("profile_request"), profile_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("members_request"), members_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("roles_request"), roles_rtt.as_secs_f64());
            metrics.set_timing(
                &secs_metric("role_members_request"),
                role_members_rtt.as_secs_f64(),
            );
            metrics.set_timing(&secs_metric("titles_request"), titles_rtt.as_secs_f64());
            metrics.set("reported_member_count", profile.member_count);
            metrics.set("reported_role_count", profile.role_count);
            metrics.set("member_reply_count", reported_member_count);
            metrics.set("role_reply_count", reported_role_count);
            metrics.set(&count_metric("members_returned"), members_returned);
            metrics.set(&count_metric("roles_returned"), roles_returned);
            metrics.set(&count_metric("role_member_pairs"), pairs_returned);
            metrics.set(&count_metric("titles_returned"), titles_returned);
            metrics.set(&count_metric("selected_titles"), selected_returned);
            Ok(())
        })
    }
}
