//! Create a throwaway group, then drive the active-group selection: clear it,
//! then activate the new group, confirming each transition.
//!
//! Where `group-session-message` proves a group's *messaging* works, this proves
//! the group **lifecycle entry point** a viewer uses: creating a group and making
//! it the agent's active group. A new group is created with
//! [`Command::CreateGroup`] (the primary becomes founder/owner); the active group
//! is then set with [`Command::ActivateGroup`], whose `Option<GroupKey>` mirrors
//! the read side ([`Event::ActiveGroupChanged`] carries an
//! `Option` `active_group_id`): `Some(group)` activates it, `None` clears it.
//!
//! OpenSim auto-activates a group at creation time (the founder's principal
//! record is stamped with the new group as active in `GroupsService.CreateGroup`),
//! so a bare "activate then check active == group" would not actually exercise the
//! `ActivateGroup` command — creation already left the group active. To make the
//! activation a genuine, observable transition, the case first *clears* the active
//! group (`ActivateGroup(None)`) and confirms the grid reports no active group,
//! then activates the new group and confirms it is reported active with the
//! founder's non-zero powers and the group's name. This also exercises the new
//! `None`-clears path end to end on a live grid.
//!
//! `1av`. Needs Groups V2 enabled on OpenSim (see the setup memory). Listed
//! `[both]`: a single avatar can run it on Aditi too, but the Aditi run is
//! batched with the rest of the deferred Aditi records.

use std::time::{Instant, SystemTime};

use sl_client_tokio::{Command, CreateGroupParams, Event, LindenAmount};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, secs_metric};

/// Creates a throwaway group, clears the active group, then activates the new
/// group, asserting each active-group transition the grid reports.
#[derive(Debug)]
pub struct GroupCreateActivate;

impl GridTest for GroupCreateActivate {
    fn name(&self) -> &'static str {
        "group-create-activate"
    }

    fn description(&self) -> &'static str {
        "Create a group, clear the active group, then activate the new group and confirm it is active"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // Create a throwaway open-enrollment group. The name carries a
            // wall-clock suffix so repeated runs do not collide on the grid's
            // unique-name constraint. The primary becomes the founder/owner.
            let unique = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |since| since.as_millis());
            let group_name = format!("sl-client group-create {unique}");
            let created_at = Instant::now();
            session
                .send(Command::CreateGroup(CreateGroupParams {
                    name: group_name.clone(),
                    charter: "throwaway group for the group-create-activate conformance case"
                        .to_owned(),
                    show_in_list: false,
                    insignia_id: None,
                    membership_fee: LindenAmount(0),
                    open_enrollment: true,
                    allow_publish: false,
                    mature_publish: false,
                }))
                .await?;
            let (group_id, create_ok, create_message) = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::CreateGroupResult {
                        group_id,
                        success,
                        message,
                    } => Some((*group_id, *success, message.clone())),
                    _ => None,
                })
                .await?;
            let create_rtt = created_at.elapsed();
            check(
                create_ok,
                &format!("group creation failed: {create_message}"),
            )?;

            // Clear the active group first, so the subsequent activation is a real,
            // observable transition rather than a no-op on a group creation already
            // left active. The grid confirms with an `ActiveGroupChanged` reporting
            // no active group (`active_group_id == None`). This also drives the new
            // `ActivateGroup(None)` clear path against a live grid.
            let cleared_at = Instant::now();
            session.send(Command::ActivateGroup(None)).await?;
            session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::ActiveGroupChanged(active) if active.active_group_id.is_none() => {
                        Some(())
                    }
                    _ => None,
                })
                .await?;
            let clear_rtt = cleared_at.elapsed();

            // Now activate the new group. The grid confirms with an
            // `ActiveGroupChanged` naming it as the active group, carrying the
            // founder's powers and the group's name.
            let activated_at = Instant::now();
            session.send(Command::ActivateGroup(Some(group_id))).await?;
            let active = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::ActiveGroupChanged(active)
                        if active.active_group_id == Some(group_id) =>
                    {
                        Some((*active).clone())
                    }
                    _ => None,
                })
                .await?;
            let activate_rtt = activated_at.elapsed();

            // The activated group is reported with its name and the founder's
            // (non-zero) powers — proving the activation took effect on the grid,
            // not just locally.
            check_eq("active group name", &active.group_name, &group_name)?;
            check(
                active.group_powers != 0,
                "founder's active group reported zero powers",
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("group_create"), create_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("clear_active"), clear_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("activate"), activate_rtt.as_secs_f64());
            metrics.set("group_powers_hex", format!("{:#018x}", active.group_powers));
            metrics.set("group_powers_nonzero", active.group_powers != 0);
            Ok(())
        })
    }
}
