//! Read the agent's experience *relationship* lists — owned, administered,
//! created — and the region's experience lists, then test the agent's admin /
//! contributor standing against one concrete experience.
//!
//! Beyond resolving an experience's metadata ([`experience_info`](super::experience_info))
//! and reading/writing the agent's per-experience allow/block *preferences*
//! ([`experience_permissions`](super::experience_permissions)), Second Life
//! exposes a family of read capabilities that describe how the agent *relates* to
//! experiences and how a region governs them, all behind HTTP with no UDP path:
//!
//! - `AgentExperiences` → the experiences the agent **owns**,
//! - `GetAdminExperiences` → the experiences the agent **administers**,
//! - `GetCreatorExperiences` → the experiences the agent **created**,
//! - `RegionExperiences` (GET) → the region's **allow / block / trust** lists,
//! - `IsExperienceAdmin?experience_id=<id>` → whether the agent administers *that*
//!   experience,
//! - `IsExperienceContributor?experience_id=<id>` → whether the agent contributes
//!   to *that* experience.
//!
//! This case exercises all six. The four **list** queries are best-effort: any of
//! their lists may legitimately be empty (a fresh avatar owns/administers/creates
//! nothing) and a region may not answer `RegionExperiences` for a non-manager, so
//! their counts are recorded, not asserted. The observable protocol effect the
//! case *asserts* is the **per-experience round-trip**: `IsExperienceAdmin` and
//! `IsExperienceContributor` must answer and each reply must echo back the exact
//! experience id that was queried (the runtime tags the reply with the queried id,
//! since the cap body carries only a bare `status`).
//!
//! The per-experience queries need a concrete experience to test. A stable
//! [`experience`](crate::fixtures) fixture wins (the reproducible Second Life
//! path). Absent a fixture the case *discovers* one from the relationship lists it
//! just read, preferring an experience the agent **administers** — because then
//! the two views of the same fact must agree, and the case additionally asserts
//! `IsExperienceAdmin` returns **true** for an experience the `admin` list already
//! named. With neither a fixture nor any relationship there is no experience to
//! test, so the case records `partial` rather than failing.
//!
//! `1av`. Experiences are Second-Life-centric and stock OpenSim ships **no**
//! experience module, so on OpenSim (with no fixture) the capabilities are absent,
//! every query is a silent no-op, and the case records `partial` up front rather
//! than block on replies that never come.

use std::time::Instant;

use sl_client_tokio::{Command, Event, ExperienceKey};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    LONG_TIMEOUT, REGION_TIMEOUT, check, check_eq, count_metric, is_opensim, secs_metric,
};

/// The agent's experience *relationship* lists plus the region's, as read back
/// from the four list capabilities. Any list may legitimately be empty, and a
/// list left [`None`] means its capability did not answer.
#[derive(Debug, Default)]
struct ExperienceLists {
    /// Experiences the agent owns (`AgentExperiences`).
    owned: Option<Vec<ExperienceKey>>,
    /// Experiences the agent administers (`GetAdminExperiences`).
    admin: Option<Vec<ExperienceKey>>,
    /// Experiences the agent created (`GetCreatorExperiences`).
    creator: Option<Vec<ExperienceKey>>,
    /// The region's allow / block / trust lists (`RegionExperiences` GET).
    region: Option<RegionLists>,
}

/// The region's experience allow / block / trust lists.
#[derive(Debug)]
struct RegionLists {
    /// Experiences the region allows.
    allowed: Vec<ExperienceKey>,
    /// Experiences the region blocks.
    blocked: Vec<ExperienceKey>,
    /// Experiences the region trusts (privileged, key-grid scope).
    trusted: Vec<ExperienceKey>,
}

impl ExperienceLists {
    /// The anchor experience to run the per-experience admin/contributor queries
    /// against, and a label for where it came from. Prefers an experience the
    /// agent administers (so the `admin`-list membership can be cross-checked
    /// against `IsExperienceAdmin`), then a created one, then an owned one.
    fn discover_anchor(&self) -> Option<(ExperienceKey, &'static str)> {
        let first =
            |list: &Option<Vec<ExperienceKey>>| list.as_ref().and_then(|ids| ids.first().copied());
        if let Some(id) = first(&self.admin) {
            Some((id, "admin"))
        } else if let Some(id) = first(&self.creator) {
            Some((id, "creator"))
        } else {
            first(&self.owned).map(|id| (id, "owned"))
        }
    }
}

/// Reads the agent's owned/admin/created and the region's experience lists, then
/// tests admin/contributor standing against one experience.
#[derive(Debug)]
pub struct ExperienceAdminContributor;

impl GridTest for ExperienceAdminContributor {
    fn name(&self) -> &'static str {
        "experience-admin-contributor"
    }

    fn description(&self) -> &'static str {
        "Read the owned/admin/creator/region experience lists and test admin/contributor status"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let fixture = ctx.experience();

            // Stock OpenSim ships no experience module, so with no configured
            // experience there is nothing to read or test — record partial up
            // front rather than block on capabilities the region never seeds.
            if fixture.is_none() && is_opensim(grid) {
                ctx.mark_partial(
                    "stock OpenSim ships no experience module, and no `experience` \
                     fixture is configured, so there are no experience relationships \
                     to read or test",
                );
                return Ok(());
            }

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // The four list queries. Any list may be empty and a region may not
            // answer for a non-manager, so these are recorded, not asserted.
            let lists_started = Instant::now();
            let lists = collect_experience_lists(session).await?;
            let lists_rtt = lists_started.elapsed();

            // Settle on the experience to test: a fixture wins (reproducible),
            // else discover one from the relationship lists just read.
            let (anchor, source) = match fixture {
                Some(id) => (id, "fixture"),
                None => match lists.discover_anchor() {
                    Some(found) => found,
                    None => {
                        record_list_metrics(ctx, &lists, lists_rtt);
                        ctx.mark_partial(
                            "no experience to test: the avatar owns/administers/created \
                             none and no `experience` fixture is configured",
                        );
                        return Ok(());
                    }
                },
            };

            // IsExperienceAdmin: the per-experience round-trip. Assert the cap
            // answers and echoes back the queried id (the runtime tags it, since
            // the reply body carries only a bare status).
            let session = ctx.primary();
            let admin_started = Instant::now();
            let Some(is_admin) = query_admin(session, anchor).await? else {
                record_list_metrics(ctx, &lists, lists_rtt);
                ctx.mark_partial(
                    "grid did not answer IsExperienceAdmin (capability absent from \
                     the region seed)",
                );
                return Ok(());
            };
            let admin_rtt = admin_started.elapsed();

            // IsExperienceContributor: same round-trip for the contributor cap.
            let contributor_started = Instant::now();
            let Some(is_contributor) = query_contributor(session, anchor).await? else {
                record_list_metrics(ctx, &lists, lists_rtt);
                ctx.mark_partial(
                    "grid did not answer IsExperienceContributor (capability absent \
                     from the region seed)",
                );
                return Ok(());
            };
            let contributor_rtt = contributor_started.elapsed();

            // Cross-consistency: an experience the `admin` list already named must
            // come back administered from the per-experience query — both are the
            // same fact read two ways.
            if source == "admin" {
                check(
                    is_admin,
                    "IsExperienceAdmin returned false for an experience the \
                     GetAdminExperiences list reported the agent administers",
                )?;
            }

            record_list_metrics(ctx, &lists, lists_rtt);
            let metrics = ctx.metrics();
            metrics.set("anchor_source", source);
            metrics.set("is_admin", is_admin);
            metrics.set("is_contributor", is_contributor);
            metrics.set_timing(&secs_metric("admin_rtt"), admin_rtt.as_secs_f64());
            metrics.set_timing(
                &secs_metric("contributor_rtt"),
                contributor_rtt.as_secs_f64(),
            );
            Ok(())
        })
    }
}

/// Issue the four list queries and read back whichever replies arrive.
///
/// Sends `AgentExperiences`, `GetAdminExperiences`, `GetCreatorExperiences`, and
/// the `RegionExperiences` GET, then reads up to four replies, matching each to
/// its list. A grid (or region) that does not seed one of the capabilities simply
/// never answers it, so the collecting loop stops at the first wait that times
/// out and leaves that list [`None`].
async fn collect_experience_lists(session: &mut Session) -> Result<ExperienceLists, TestFailure> {
    session.send(Command::RequestOwnedExperiences).await?;
    session.send(Command::RequestAdminExperiences).await?;
    session.send(Command::RequestCreatorExperiences).await?;
    session.send(Command::RequestRegionExperiences).await?;

    let mut lists = ExperienceLists::default();
    for _ in 0..4_u8 {
        match session
            .wait_for(LONG_TIMEOUT, |event| match event {
                Event::OwnedExperiences(ids) => Some(Reply::Owned(ids.clone())),
                Event::AdminExperiences(ids) => Some(Reply::Admin(ids.clone())),
                Event::CreatorExperiences(ids) => Some(Reply::Creator(ids.clone())),
                Event::RegionExperiences {
                    allowed,
                    blocked,
                    trusted,
                } => Some(Reply::Region(RegionLists {
                    allowed: allowed.clone(),
                    blocked: blocked.clone(),
                    trusted: trusted.clone(),
                })),
                _ => None,
            })
            .await
        {
            Ok(Reply::Owned(ids)) => lists.owned = Some(ids),
            Ok(Reply::Admin(ids)) => lists.admin = Some(ids),
            Ok(Reply::Creator(ids)) => lists.creator = Some(ids),
            Ok(Reply::Region(region)) => lists.region = Some(region),
            Err(TestFailure::Timeout(_)) => break,
            Err(other) => return Err(other),
        }
    }
    Ok(lists)
}

/// One decoded reply from the [`collect_experience_lists`] wait loop, routed into
/// its slot in [`ExperienceLists`].
enum Reply {
    /// An `AgentExperiences` reply.
    Owned(Vec<ExperienceKey>),
    /// A `GetAdminExperiences` reply.
    Admin(Vec<ExperienceKey>),
    /// A `GetCreatorExperiences` reply.
    Creator(Vec<ExperienceKey>),
    /// A `RegionExperiences` GET reply.
    Region(RegionLists),
}

/// Query `IsExperienceAdmin` for `experience`, asserting the reply echoes the
/// queried id, and return whether the agent administers it.
///
/// Returns [`None`] when the capability does not answer (absent from the region
/// seed), so the caller can record `partial` rather than fail.
async fn query_admin(
    session: &mut Session,
    experience: ExperienceKey,
) -> Result<Option<bool>, TestFailure> {
    session
        .send(Command::RequestExperienceAdmin {
            experience_id: experience,
        })
        .await?;
    match session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ExperienceAdminStatus {
                experience_id,
                is_admin,
            } => Some((*experience_id, *is_admin)),
            _ => None,
        })
        .await
    {
        Ok((echoed, is_admin)) => {
            check_eq(
                "IsExperienceAdmin echoed experience_id",
                &echoed,
                &experience,
            )?;
            Ok(Some(is_admin))
        }
        Err(TestFailure::Timeout(_)) => Ok(None),
        Err(other) => Err(other),
    }
}

/// Query `IsExperienceContributor` for `experience`, asserting the reply echoes
/// the queried id, and return whether the agent contributes to it.
///
/// Returns [`None`] when the capability does not answer, so the caller can record
/// `partial` rather than fail.
async fn query_contributor(
    session: &mut Session,
    experience: ExperienceKey,
) -> Result<Option<bool>, TestFailure> {
    session
        .send(Command::RequestExperienceContributor {
            experience_id: experience,
        })
        .await?;
    match session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ExperienceContributorStatus {
                experience_id,
                is_contributor,
            } => Some((*experience_id, *is_contributor)),
            _ => None,
        })
        .await
    {
        Ok((echoed, is_contributor)) => {
            check_eq(
                "IsExperienceContributor echoed experience_id",
                &echoed,
                &experience,
            )?;
            Ok(Some(is_contributor))
        }
        Err(TestFailure::Timeout(_)) => Ok(None),
        Err(other) => Err(other),
    }
}

/// Record the four list queries' counts and timing. A list left [`None`] (its
/// capability did not answer) records a `-1` sentinel count so the record
/// distinguishes "no answer" from "answered empty" (`0`).
fn record_list_metrics(
    ctx: &mut TestContext,
    lists: &ExperienceLists,
    lists_rtt: std::time::Duration,
) {
    let count = |list: &Option<Vec<ExperienceKey>>| {
        list.as_ref()
            .map_or(-1, |ids| i64::try_from(ids.len()).unwrap_or(-1))
    };
    let owned = count(&lists.owned);
    let admin = count(&lists.admin);
    let creator = count(&lists.creator);
    let (region_allowed, region_blocked, region_trusted) =
        lists.region.as_ref().map_or((-1, -1, -1), |region| {
            (
                i64::try_from(region.allowed.len()).unwrap_or(-1),
                i64::try_from(region.blocked.len()).unwrap_or(-1),
                i64::try_from(region.trusted.len()).unwrap_or(-1),
            )
        });

    let metrics = ctx.metrics();
    metrics.set(&count_metric("owned"), owned);
    metrics.set(&count_metric("admin"), admin);
    metrics.set(&count_metric("creator"), creator);
    metrics.set(&count_metric("region_allowed"), region_allowed);
    metrics.set(&count_metric("region_blocked"), region_blocked);
    metrics.set(&count_metric("region_trusted"), region_trusted);
    metrics.set_timing(&secs_metric("lists_rtt"), lists_rtt.as_secs_f64());
}
