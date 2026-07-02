//! Request the region's estate configuration and covenant summary.
//!
//! The "Region/Estate" floater a viewer opens for an estate manager or owner is
//! populated from two round-trips over the estate-management channel:
//!
//! - `EstateOwnerMessage`/`getinfo` ([`Command::RequestEstateInfo`]) asks for the
//!   estate's configuration. The simulator replies with an `estateupdateinfo`
//!   [`Event::EstateInfo`] (name, owner, id, flags, sun, parent estate, covenant
//!   id + timestamp, abuse-report email) followed by one `setaccess`
//!   [`Event::EstateAccessList`] per access list (estate managers, allowed
//!   agents, allowed groups, banned agents). OpenSim gates `getinfo` behind
//!   `CanIssueEstateCommand`, so a non-manager gets *no* reply — the case must run
//!   as the **estate-owner** avatar (`--avatar estate-owner`), who owns the region
//!   on the local grid.
//! - `EstateCovenantRequest` ([`Command::RequestEstateCovenant`]) asks for the
//!   covenant summary. The simulator replies with an `EstateCovenantReply`
//!   [`Event::EstateCovenant`] (covenant notecard id + timestamp, estate name,
//!   estate owner). The covenant *text* is a notecard asset fetched separately by
//!   its id; this case only exercises the summary. Unlike `getinfo`, the covenant
//!   request is ungated, but the case is already logged in as the estate owner.
//!
//! The cross-grid invariants asserted are that the estate names a non-empty
//! estate, that both replies agree the estate owner is the logged-in (estate
//! owner) avatar, and that the covenant summary's estate name matches the
//! configuration's. The access lists that trail `getinfo` are drained and their
//! count / total membership recorded (the dedicated `estate-access` case exercises
//! mutating them); an estate with empty lists still emits one empty `setaccess`
//! per list, so their arrival is recorded but not asserted, to stay robust across
//! grids. `1av` (estate owner), `[both]`.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, EstateCovenant, EstateInfo, Event};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, secs_metric};

/// The quiet gap (no further `setaccess`) that marks the estate access lists that
/// trail an `estateupdateinfo` reply fully drained.
const ACCESS_DRAIN_QUIET: Duration = Duration::from_secs(3);

/// Requests the region's estate config + covenant and records both replies.
///
/// Named `…Case` rather than `EstateInfo` to avoid clashing with the
/// [`EstateInfo`] reply type this case decodes.
#[expect(
    clippy::module_name_repetitions,
    reason = "the bare `EstateInfo` name is the reply type; the case struct needs a distinct name"
)]
#[derive(Debug)]
pub struct EstateInfoCase;

impl GridTest for EstateInfoCase {
    fn name(&self) -> &'static str {
        "estate-info"
    }

    fn description(&self) -> &'static str {
        "Request the region's estate configuration and covenant summary"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            let agent = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

            // 1. Request the estate configuration (`getinfo`). OpenSim only
            //    answers a manager/owner, so a reply confirms our rights.
            let info_start = Instant::now();
            session.send(Command::RequestEstateInfo).await?;
            let info: EstateInfo = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::EstateInfo(info) => Some((**info).clone()),
                    _ => None,
                })
                .await?;
            let info_secs = info_start.elapsed().as_secs_f64();

            check(
                !info.estate_name.is_empty(),
                "expected the estate to have a non-empty name",
            )?;
            check_eq(
                "estate owner is the logged-in (estate-owner) avatar",
                &info.estate_owner,
                &agent.uuid(),
            )?;

            // The `estateupdateinfo` reply is trailed by one `setaccess` per
            // access list (managers, allowed agents, allowed groups, bans);
            // drain them so their arrival can be recorded. The list contents are
            // asserted by the dedicated `estate-access` case.
            let access = drain_access_lists(session, ACCESS_DRAIN_QUIET).await?;

            // 2. Request the covenant summary (`EstateCovenantRequest`). Both
            //    replies must agree on the estate name and owner.
            let covenant_start = Instant::now();
            session.send(Command::RequestEstateCovenant).await?;
            let covenant: EstateCovenant = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::EstateCovenant(covenant) => Some(covenant.clone()),
                    _ => None,
                })
                .await?;
            let covenant_secs = covenant_start.elapsed().as_secs_f64();

            check_eq(
                "covenant summary names the same estate as the configuration",
                &covenant.estate_name,
                &info.estate_name,
            )?;
            check_eq(
                "covenant summary names the logged-in (estate-owner) avatar as owner",
                &covenant.estate_owner_id,
                &agent.uuid(),
            )?;

            let member_total: usize = access.iter().sum();
            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("estate_info"), info_secs);
            metrics.set_timing(&secs_metric("estate_covenant"), covenant_secs);
            metrics.set("estate_name", info.estate_name.clone());
            metrics.set("estate_owner", info.estate_owner.to_string());
            metrics.set("estate_id", info.estate_id);
            metrics.set("estate_flags", info.estate_flags);
            metrics.set("parent_estate", info.parent_estate);
            metrics.set("abuse_email_present", !info.abuse_email.is_empty());
            metrics.set("has_covenant", info.covenant_id.is_some());
            metrics.set("covenant_timestamp", covenant.covenant_timestamp);
            metrics.set(
                "access_list_count",
                i64::try_from(access.len()).unwrap_or(-1),
            );
            metrics.set(
                "access_member_total",
                i64::try_from(member_total).unwrap_or(-1),
            );
            Ok(())
        })
    }
}

/// Drains the `setaccess` [`Event::EstateAccessList`] events that trail an
/// `estateupdateinfo` reply until none arrives for `quiet`, returning the member
/// count of each chunk (a large list may arrive split across several events).
///
/// # Errors
///
/// Propagates a [`Session::wait_for`] disconnect.
async fn drain_access_lists(
    session: &mut Session,
    quiet: Duration,
) -> Result<Vec<usize>, TestFailure> {
    let mut lists = Vec::new();
    loop {
        match session
            .wait_for(quiet, |event| match event {
                Event::EstateAccessList { members, .. } => Some(members.len()),
                _ => None,
            })
            .await
        {
            Ok(count) => lists.push(count),
            Err(TestFailure::Timeout(_)) => return Ok(lists),
            Err(other) => return Err(other),
        }
    }
}
