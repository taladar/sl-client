//! Drive a `DirFindQuery` for each of the three search types (people, groups,
//! events) and confirm each answers with its correctly-typed, query-id-correlated
//! reply.

use std::time::Instant;

use sl_client_tokio::{Command, DirFindFlags, Event, QueryId, Uuid};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, is_opensim};

/// Issues one `DirFindQuery` and awaits the first reply the `extract` closure
/// accepts, treating a reply-timeout as "this grid does not answer this query
/// type" (`Ok(None)`) rather than a failure — the reply types diverge sharply
/// by grid (see [`DirFindPeopleGroupsEvents`]).
///
/// # Errors
///
/// Propagates a send failure or a non-timeout wait error (a closed channel or an
/// intervening disconnect); a reply timeout maps to `Ok(None)`.
async fn find<T>(
    session: &mut Session,
    query_id: QueryId,
    query_text: String,
    flags: DirFindFlags,
    mut extract: impl FnMut(&Event) -> Option<T>,
) -> Result<Option<T>, TestFailure> {
    session
        .send(Command::DirFindQuery {
            query_id,
            query_text,
            flags,
            query_start: 0,
        })
        .await?;
    match session
        .wait_for(REPLY_TIMEOUT, |event| extract(event))
        .await
    {
        Ok(value) => Ok(Some(value)),
        Err(TestFailure::Timeout(_)) => Ok(None),
        Err(other) => Err(other),
    }
}

/// Runs the unified directory search across all three `DirFindQuery` types and
/// records which reply types the grid answers.
///
/// `DirFindQuery` is the viewer *Search* floater's one query for people, groups
/// and events; the [`DirFindFlags`] select the type, and the simulator answers
/// with the matching, query-id-correlated
/// [`DirPeopleReply`](Event::DirPeopleReply),
/// [`DirGroupsReply`](Event::DirGroupsReply) or
/// [`DirEventsReply`](Event::DirEventsReply). This case runs one of each and
/// records, per type, whether a reply arrived and how many results it carried.
///
/// To supply a name the grid is guaranteed to know — without baking an avatar
/// name into the source or the record — every sub-query searches for the agent's
/// *own* first name (from the login credentials, via
/// [`Session::avatar_first_name`](crate::context::Session::avatar_first_name)),
/// exactly as the [`avatar-picker`](super::avatar_picker) case does. A groups /
/// events search for that string legitimately matches nothing; the case records
/// counts but asserts only on the people reply, which is the one anchor a grid
/// can be relied on to answer.
///
/// The grids diverge and the case is grid-aware (`1av`):
///
/// - **OpenSim** answers from `BasicSearchModule.OnDirFindQuery`: a people
///   search hits the user-account service — a set that includes the requester —
///   so a self-name query must return the querying agent; a groups search
///   replies (empty unless a group matches) when the groups module is loaded;
///   **events are unimplemented**, so an events query draws no reply (recorded
///   as not answered, not a failure). Asserted: a people reply arrives with at
///   least one real (non-nil-keyed) match, and the agent's own id among them.
/// - **Second Life (aditi)** answers all three from the grid search backend, but
///   the beta people directory is sparse: as with `avatar-picker`, a self-name
///   query may return no real match. With nothing to observe on the anchor
///   query, the case marks the run partial rather than failing, and never
///   asserts the agent's own presence (the SL index need not carry the beta
///   account).
#[derive(Debug)]
pub struct DirFindPeopleGroupsEvents;

impl GridTest for DirFindPeopleGroupsEvents {
    fn name(&self) -> &'static str {
        "dir-find-people-groups-events"
    }

    fn description(&self) -> &'static str {
        "DirFindQuery across people / groups / events"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let self_id = ctx.primary().agent_id();
            let query_text = ctx.primary().avatar_first_name().to_owned();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // People: search the agent's own first name. The predicate returns
            // only counts and whether our own id appears, so no resident name is
            // copied out of the event.
            let people_id = QueryId::from(Uuid::new_v4());
            let people_start = Instant::now();
            let people = find(
                session,
                people_id,
                query_text.clone(),
                DirFindFlags::PEOPLE,
                |event| match event {
                    Event::DirPeopleReply { query_id, results } if *query_id == people_id.get() => {
                        // SL encodes "no results" as a single nil-keyed sentinel
                        // row, so count only real, non-nil-keyed matches.
                        let real = results
                            .iter()
                            .filter(|r| !r.agent_id.uuid().is_nil())
                            .count();
                        let self_present =
                            self_id.is_some_and(|id| results.iter().any(|r| r.agent_id == id));
                        Some((results.len(), real, self_present))
                    }
                    _ => None,
                },
            )
            .await?;
            let people_secs = people_start.elapsed().as_secs_f64();

            // Groups: same self-name string; matches nothing here, so only the
            // reply's arrival and count are observed.
            let groups_id = QueryId::from(Uuid::new_v4());
            let groups_start = Instant::now();
            let groups = find(
                session,
                groups_id,
                query_text.clone(),
                DirFindFlags::GROUPS,
                |event| match event {
                    Event::DirGroupsReply { query_id, results } if *query_id == groups_id.get() => {
                        Some(results.len())
                    }
                    _ => None,
                },
            )
            .await?;
            let groups_secs = groups_start.elapsed().as_secs_f64();

            // Events: OpenSim never answers this branch; SL does. Record the
            // reply's arrival, count and search-status when it comes.
            let events_id = QueryId::from(Uuid::new_v4());
            let events_start = Instant::now();
            let events = find(
                session,
                events_id,
                query_text,
                DirFindFlags::EVENTS,
                |event| match event {
                    Event::DirEventsReply {
                        query_id,
                        results,
                        status,
                    } if *query_id == events_id.get() => Some((results.len(), *status)),
                    _ => None,
                },
            )
            .await?;
            let events_secs = events_start.elapsed().as_secs_f64();

            let types_answered = [people.is_some(), groups.is_some(), events.is_some()]
                .into_iter()
                .filter(|replied| *replied)
                .count();

            // Record the per-type outcomes before the assertions so even a
            // failing run documents what each query type returned.
            let metrics = ctx.metrics();
            metrics.set("people_replied", people.is_some());
            if let Some((result_count, real_count, self_present)) = people {
                metrics.set(
                    "people_result_count",
                    i64::try_from(result_count).unwrap_or(i64::MAX),
                );
                metrics.set(
                    "people_real_count",
                    i64::try_from(real_count).unwrap_or(i64::MAX),
                );
                metrics.set("people_self_present", self_present);
                metrics.set_timing("people_secs", people_secs);
            }
            metrics.set("groups_replied", groups.is_some());
            if let Some(result_count) = groups {
                metrics.set(
                    "groups_result_count",
                    i64::try_from(result_count).unwrap_or(i64::MAX),
                );
                metrics.set_timing("groups_secs", groups_secs);
            }
            metrics.set("events_replied", events.is_some());
            if let Some((result_count, status)) = events {
                metrics.set(
                    "events_result_count",
                    i64::try_from(result_count).unwrap_or(i64::MAX),
                );
                metrics.set("events_status", i64::from(status));
                metrics.set_timing("events_secs", events_secs);
            }
            metrics.set(
                "types_answered",
                i64::try_from(types_answered).unwrap_or(i64::MAX),
            );

            if is_opensim(grid) {
                // The account-service people search includes the requester, so a
                // search for our own name must return us. Events are unimplemented
                // on OpenSim, so their absence is expected, not asserted.
                let (_, real_count, self_present) = people.ok_or_else(|| {
                    TestFailure::Assertion(
                        "expected a DirPeopleReply for an OpenSim people search".to_owned(),
                    )
                })?;
                check(
                    real_count > 0,
                    "expected the people search to return at least one real match for the agent's own name",
                )?;
                check(
                    self_present,
                    "expected the OpenSim people results to include the querying agent's own id",
                )?;
            } else if !matches!(people, Some((_, real, _)) if real > 0) {
                // The aditi people directory is sparse: a self-name query yields
                // no real match, leaving the anchor query nothing to observe.
                ctx.mark_partial(
                    "aditi people directory returned no real DirFindQuery matches for the agent's own name",
                );
            }

            Ok(())
        })
    }
}
