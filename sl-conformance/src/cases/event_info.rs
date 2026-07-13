//! Discover an in-world event through the events directory, fetch its full
//! detail with `EventInfoRequest`, and exercise the reminder add/remove pair
//! (`EventNotificationAddRequest` / `EventNotificationRemoveRequest`).

use std::time::Instant;

use sl_client_tokio::{Command, DirFindFlags, Event, QueryId, Uuid};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, is_opensim};

/// The events directory's *upcoming* query text: the viewer encodes the events
/// search as `"<date>|<category>|<name>"`, where `date` is either `"u"` for
/// current/ongoing events or a day offset. This is the *Search → Events* floater
/// on its default settings — current events (`u`), every category (`0`), no name
/// filter — so the anchor needs no baked search string (see
/// `LLPanelDirEvents::performQuery`).
const UPCOMING_EVENTS_QUERY: &str = "u|0|";

/// Sends one directory `command` and awaits the first reply the `extract`
/// closure accepts, mapping a reply-timeout to "this grid does not answer this
/// query" (`Ok(None)`) rather than a failure — OpenSim implements no events
/// directory (see [`EventInfo`]).
///
/// # Errors
///
/// Propagates a send failure or a non-timeout wait error (a closed channel or an
/// intervening disconnect); a reply timeout maps to `Ok(None)`.
async fn query<T>(
    session: &mut Session,
    command: Command,
    mut extract: impl FnMut(&Event) -> Option<T>,
) -> Result<Option<T>, TestFailure> {
    session.send(command).await?;
    match session
        .wait_for(REPLY_TIMEOUT, |event| extract(event))
        .await
    {
        Ok(value) => Ok(Some(value)),
        Err(TestFailure::Timeout(_)) => Ok(None),
        Err(other) => Err(other),
    }
}

/// Drives the events directory → `EventInfoRequest` → reminder add/remove flow.
///
/// The events directory is a three-step flow the viewer's *Search → Events*
/// floater drives: a [`DirFindQuery`](Command::DirFindQuery) with
/// [`DirFindFlags::EVENTS`] answers with a
/// [`DirEventsReply`](Event::DirEventsReply) of
/// [`DirEventResult`](sl_client_tokio::Event)s (each carrying an `EventId`); a
/// following [`EventInfoRequest`](Command::EventInfoRequest) for one of those ids
/// draws an [`EventInfoReply`](Event::EventInfoReply) with the full listing; and
/// the reminder pair
/// [`EventNotificationAddRequest`](Command::EventNotificationAddRequest) /
/// [`EventNotificationRemoveRequest`](Command::EventNotificationRemoveRequest)
/// subscribes/unsubscribes to a start-time reminder (neither draws a reply).
///
/// This case runs the upcoming-events query, and — when the grid returns at
/// least one event — fetches its detail and exercises the reminder pair against
/// that same id.
///
/// The grids diverge and the case is grid-aware (`1av`):
///
/// - **OpenSim** has **no events directory**: `BasicSearchModule` subscribes to
///   `OnDirFindQuery` but ignores the events branch, and no core module handles
///   events, so the query draws no `DirEventsReply` at all. Asserted: the events
///   query goes unanswered (the absence is the documented behaviour, not a
///   failure), so there is no id to look up and the info / reminder steps are
///   skipped.
/// - **Second Life (aditi)** answers from the grid search backend. When the beta
///   directory lists an upcoming event, the case asserts the `EventInfoReply`
///   arrives and echoes the requested id, then sends the reminder add/remove
///   pair (fire-and-forget; success is the session surviving the exchange). When
///   the beta directory lists no upcoming event — common on the sparsely
///   populated beta grid — there is nothing to look up, so the run is marked
///   partial rather than failing.
#[derive(Debug)]
pub struct EventInfo;

impl GridTest for EventInfo {
    fn name(&self) -> &'static str {
        "event-info"
    }

    fn description(&self) -> &'static str {
        "EventInfoRequest + notification add/remove via the events directory"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // Step 1: the upcoming-events directory query. Include every
            // maturity so a match is not filtered out by rating.
            let events_id = QueryId::from(Uuid::new_v4());
            let events_start = Instant::now();
            let events = query(
                session,
                Command::DirFindQuery {
                    query_id: events_id,
                    query_text: UPCOMING_EVENTS_QUERY.to_owned(),
                    flags: DirFindFlags::DATE_EVENTS
                        .union(DirFindFlags::INC_PG)
                        .union(DirFindFlags::INC_MATURE)
                        .union(DirFindFlags::INC_ADULT),
                    query_start: 0,
                },
                |event| match event {
                    Event::DirEventsReply {
                        query_id,
                        results,
                        status,
                    } if *query_id == events_id.get() => {
                        // Take the first real (non-zero-id) event to look up.
                        let first = results
                            .iter()
                            .map(|result| result.event_id)
                            .find(|id| id.get() != 0);
                        Some((results.len(), first, *status))
                    }
                    _ => None,
                },
            )
            .await?;
            let events_secs = events_start.elapsed().as_secs_f64();

            // Step 2: fetch the chosen event's full detail. Only reachable when
            // the directory returned a usable id (never on OpenSim).
            let event_id = events.and_then(|(_, first, _)| first);
            let mut info_reply = None;
            let mut info_secs = 0.0;
            let mut notifications_exercised = false;
            if let Some(event_id) = event_id {
                let info_start = Instant::now();
                let info =
                    query(
                        session,
                        Command::EventInfoRequest { event_id },
                        |event| match event {
                            Event::EventInfoReply { info } if info.event_id == event_id => Some((
                                info.event_id.get(),
                                info.name.len(),
                                info.description.len(),
                                info.duration,
                                info.amount.is_some(),
                            )),
                            _ => None,
                        },
                    )
                    .await?;
                info_secs = info_start.elapsed().as_secs_f64();
                info_reply = info;

                // Step 3: the reminder pair — subscribe then unsubscribe. Neither
                // draws a reply; the assertion is that the session survives the
                // exchange (a closed circuit surfaces as a send error).
                session
                    .send(Command::EventNotificationAddRequest { event_id })
                    .await?;
                session
                    .send(Command::EventNotificationRemoveRequest { event_id })
                    .await?;
                notifications_exercised = true;
            }

            // Record the per-step outcomes before the assertions so even a
            // failing run documents what each step returned.
            let metrics = ctx.metrics();
            metrics.set("events_replied", events.is_some());
            if let Some((result_count, _, status)) = events {
                metrics.set(
                    "events_result_count",
                    i64::try_from(result_count).unwrap_or(i64::MAX),
                );
                metrics.set("events_status", i64::from(status));
                metrics.set_timing("events_secs", events_secs);
            }
            metrics.set("event_found", event_id.is_some());
            metrics.set("event_info_replied", info_reply.is_some());
            if let Some((reply_id, name_len, description_len, duration, has_cover)) = info_reply {
                metrics.set("event_info_id", i64::from(reply_id));
                metrics.set(
                    "event_name_len",
                    i64::try_from(name_len).unwrap_or(i64::MAX),
                );
                metrics.set(
                    "event_description_len",
                    i64::try_from(description_len).unwrap_or(i64::MAX),
                );
                metrics.set("event_duration_mins", i64::from(duration));
                metrics.set("event_has_cover", has_cover);
                metrics.set_timing("event_info_secs", info_secs);
            }
            metrics.set("notifications_exercised", notifications_exercised);

            if is_opensim(grid) {
                // No core module answers the events branch, so a compliant
                // OpenSim draws no DirEventsReply and there is nothing to look up.
                check(
                    events.is_none(),
                    "expected OpenSim to answer no DirEventsReply (BasicSearchModule ignores the events branch)",
                )?;
            } else if event_id.is_some() {
                // The beta directory listed an event, so its detail must come
                // back with the requested id.
                check(
                    info_reply.is_some(),
                    "expected an EventInfoReply echoing the requested event id from the aditi events directory",
                )?;
            } else {
                // The beta directory listed no upcoming event, leaving nothing to
                // look up; record the divergence rather than failing.
                ctx.mark_partial(
                    "aditi events directory listed no upcoming event to fetch detail for",
                );
            }

            Ok(())
        })
    }
}
