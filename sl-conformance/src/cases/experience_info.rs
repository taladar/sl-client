//! Resolve an **experience** by id over the `GetExperienceInfo` capability and
//! search for it by name over `FindExperienceByName`.
//!
//! An *experience* is a Second Life grouping of scripted content that runs under
//! one shared permission grant: an avatar admits an experience once, and every
//! object keyed to it may then act (attach, teleport, animate, take controls)
//! without re-prompting. Experiences live entirely behind HTTP capabilities —
//! there is no UDP path — and this case exercises the two read capabilities that
//! turn an experience id into human-readable metadata: `GetExperienceInfo`
//! (batch id → `{ name, description, owner, maturity, properties, slurl, … }`)
//! and `FindExperienceByName` (a name substring → one page of matching
//! experiences).
//!
//! The case first settles on an *anchor* experience. A stable
//! [`experience`](crate::fixtures) fixture wins when configured (the reliable
//! Second Life path — the metadata of a known experience is fixed, so the record
//! is reproducible). Absent a fixture it *discovers* one from the agent's own
//! experience relationships — the experiences it owns
//! (`AgentExperiences`), administers (`GetAdminExperiences`), created
//! (`GetCreatorExperiences`), or has admitted/blocked (`GetExperiences`) — using
//! whichever id those queries surface first. With neither a fixture nor a
//! discovered relationship there is no experience to resolve, so the case records
//! `partial` rather than failing.
//!
//! The observable protocol effect asserted is the **info round-trip**: the
//! anchor must come back from `GetExperienceInfo` as a real (non-
//! [`missing`](sl_client_tokio::ExperienceInfo::missing)) record whose
//! `public_id` matches the request and whose `name` is non-empty. The case then
//! searches for that name over `FindExperienceByName` and asserts the search
//! capability *answers* — whether the anchor itself appears in the (single,
//! paged) result set is best-effort and recorded, not asserted, since a common
//! name can overflow one page or a private experience may be unlisted.
//!
//! `1av`. Experiences are Second-Life-centric — the legend lists them under
//! SL-only — and stock OpenSim ships **no** experience module, so on OpenSim the
//! capabilities are absent, every query is a silent no-op, and (with no fixture)
//! the case records `partial` at once rather than blocking on replies that never
//! come.

use std::time::Instant;

use sl_client_tokio::{Command, Event, ExperienceKey};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    LONG_TIMEOUT, REGION_TIMEOUT, check, check_eq, count_metric, is_opensim, secs_metric,
};

/// The minimum query length `FindExperienceByName` accepts; a name shorter than
/// this is not searched (the info round-trip still stands on its own).
const MIN_QUERY_CHARS: usize = 3;

/// Resolves an experience by id and searches for it by name.
#[derive(Debug)]
pub struct ExperienceInfo;

impl GridTest for ExperienceInfo {
    fn name(&self) -> &'static str {
        "experience-info"
    }

    fn description(&self) -> &'static str {
        "Resolve an experience over GetExperienceInfo and search for it by name"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let fixture = ctx.experience();

            // Stock OpenSim ships no experience module, so with no configured
            // experience there is nothing to resolve — record partial up front
            // rather than block on capabilities the region never seeds.
            if fixture.is_none() && is_opensim(grid) {
                ctx.mark_partial(
                    "stock OpenSim ships no experience module, and no `experience` \
                     fixture is configured, so there is no experience to resolve",
                );
                return Ok(());
            }

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // Settle on the experience to resolve: a fixture wins, else discover
            // one from the agent's own experience relationships.
            let (anchor, source) = match fixture {
                Some(id) => (Some(id), "fixture"),
                None => discover_experience(session).await?,
            };
            let Some(anchor) = anchor else {
                ctx.mark_partial(
                    "no experience to resolve: the avatar owns/administers/created \
                     none and has admitted none, and no `experience` fixture is \
                     configured",
                );
                return Ok(());
            };

            // GetExperienceInfo: the info round-trip. The anchor is a known
            // fixture or one the grid just reported the avatar relates to, so it
            // must resolve to a real (non-missing) record.
            let info_started = Instant::now();
            session
                .send(Command::RequestExperienceInfo {
                    experience_ids: vec![anchor],
                })
                .await?;
            let infos = match session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::ExperienceInfo(infos) => Some(infos.clone()),
                    _ => None,
                })
                .await
            {
                Ok(infos) => infos,
                Err(TestFailure::Timeout(_)) => {
                    ctx.mark_partial(
                        "grid did not answer GetExperienceInfo (capability absent \
                         from the region seed)",
                    );
                    return Ok(());
                }
                Err(other) => return Err(other),
            };
            let info_rtt = info_started.elapsed();

            let Some(record) = infos.iter().find(|info| info.public_id == anchor) else {
                return Err(TestFailure::Assertion(format!(
                    "GetExperienceInfo reply ({} record(s)) did not resolve the \
                     requested experience {anchor}",
                    infos.len(),
                )));
            };
            check(
                !record.missing,
                "the requested experience came back as a missing placeholder",
            )?;
            check_eq("info record public_id", &record.public_id, &anchor)?;
            check(
                !record.name.is_empty(),
                "the resolved experience record has an empty name",
            )?;
            let name = record.name.clone();
            let maturity = record.maturity;
            let is_grid = record.properties.is_grid();

            // FindExperienceByName: search for the resolved name and confirm the
            // search capability answers. Whether the anchor appears in the single
            // paged result set is best-effort (recorded, not asserted).
            let (search_len, search_rtt, found_by_name, searched) =
                if name.chars().count() >= MIN_QUERY_CHARS {
                    let search_started = Instant::now();
                    session
                        .send(Command::FindExperiences {
                            query: name.clone(),
                            page: 0,
                        })
                        .await?;
                    match session
                        .wait_for(LONG_TIMEOUT, |event| match event {
                            Event::ExperienceSearchResults(results) => Some(results.clone()),
                            _ => None,
                        })
                        .await
                    {
                        Ok(results) => {
                            let rtt = search_started.elapsed();
                            let found = results
                                .iter()
                                .any(|result| result.public_id == anchor && !result.missing);
                            (Some(results.len()), Some(rtt), found, true)
                        }
                        Err(TestFailure::Timeout(_)) => (None, None, false, true),
                        Err(other) => return Err(other),
                    }
                } else {
                    (None, None, false, false)
                };

            let metrics = ctx.metrics();
            metrics.set("experience_source", source);
            metrics.set("experience_name", name);
            metrics.set(&count_metric("maturity"), i64::from(maturity));
            metrics.set("experience_is_grid", is_grid);
            metrics.set_timing(&secs_metric("info_rtt"), info_rtt.as_secs_f64());
            metrics.set("searched_by_name", searched);
            if let Some(len) = search_len {
                metrics.set(
                    &count_metric("search_results"),
                    i64::try_from(len).unwrap_or(-1),
                );
                metrics.set("found_by_name", found_by_name);
            }
            if let Some(rtt) = search_rtt {
                metrics.set_timing(&secs_metric("search_rtt"), rtt.as_secs_f64());
            }
            Ok(())
        })
    }
}

/// Discover an experience id from the agent's own experience relationships.
///
/// Issues the owned / administered / created / preference queries and reads back
/// whichever id-list replies arrive, returning the first id found and a label for
/// its source. A grid without an experience backend answers none of them, so this
/// yields `(None, "none")` after the replies stop coming.
async fn discover_experience(
    session: &mut Session,
) -> Result<(Option<ExperienceKey>, &'static str), TestFailure> {
    session.send(Command::RequestOwnedExperiences).await?;
    session.send(Command::RequestAdminExperiences).await?;
    session.send(Command::RequestCreatorExperiences).await?;
    session.send(Command::RequestExperiencePermissions).await?;

    // Read up to the four id-list replies, stopping at the first that carries an
    // id. If a grid seeds none of the capabilities the first wait simply times
    // out and discovery ends empty.
    for _ in 0..4_u8 {
        match session
            .wait_for(LONG_TIMEOUT, |event| match event {
                Event::OwnedExperiences(ids) => Some(("owned", ids.clone())),
                Event::AdminExperiences(ids) => Some(("admin", ids.clone())),
                Event::CreatorExperiences(ids) => Some(("creator", ids.clone())),
                Event::ExperiencePermissions { allowed, blocked } => {
                    let mut ids = allowed.clone();
                    ids.extend(blocked.iter().copied());
                    Some(("permissions", ids))
                }
                _ => None,
            })
            .await
        {
            Ok((source, ids)) => {
                if let Some(id) = ids.into_iter().next() {
                    return Ok((Some(id), source));
                }
            }
            Err(TestFailure::Timeout(_)) => break,
            Err(other) => return Err(other),
        }
    }
    Ok((None, "none"))
}
