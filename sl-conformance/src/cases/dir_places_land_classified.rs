//! Drive the three dedicated directory searches — `DirPlacesQuery`,
//! `DirLandQuery` and `DirClassifiedQuery` — and record which reply, if any,
//! each grid answers with.

use std::time::Instant;

use sl_client_tokio::{
    ClassifiedCategory, Command, DirFindFlags, Event, LandSearchType, ParcelCategory, QueryId, Uuid,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, is_opensim};

/// The classifieds query reuses the shared `query_flags` field with its *own*
/// bit namespace (`CLASSIFIED_QUERY_INC_*`), distinct from the `DFQ_*` bits the
/// other directory queries use. These are the viewer's "include every maturity"
/// bits (`CLASSIFIED_QUERY_INC_PG | INC_MATURE | INC_ADULT` = `1<<2 | 1<<3 |
/// 1<<6`); the [`DirFindFlags`] newtype is only carrying the raw `u32` here.
const CLASSIFIED_INC_ALL: DirFindFlags = DirFindFlags::from_bits((1 << 2) | (1 << 3) | (1 << 6));

/// Sends one directory-query `command` and awaits the first reply the `extract`
/// closure accepts, mapping a reply-timeout to "this grid does not answer this
/// query" (`Ok(None)`) rather than a failure — OpenSim implements none of the
/// three (see [`DirPlacesLandClassified`]).
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

/// Runs the three dedicated directory searches and records which the grid
/// answers.
///
/// Alongside the unified [`DirFindQuery`](Command::DirFindQuery) (people /
/// groups / events), the viewer *Search* floater drives three dedicated
/// queries, each with its own reply message:
///
/// - [`DirPlacesQuery`](Command::DirPlacesQuery) → [`DirPlacesReply`](Event::DirPlacesReply):
///   named parcels. This case issues the viewer's *initial* places query
///   verbatim — empty text, [`ParcelCategory::Linden`], sorted by dwell — so the
///   anchor needs no baked search string and matches the grid's Linden hubs.
/// - [`DirLandQuery`](Command::DirLandQuery) → [`DirLandReply`](Event::DirLandReply):
///   land for sale, across every sale type with no price/area limit.
/// - [`DirClassifiedQuery`](Command::DirClassifiedQuery) →
///   [`DirClassifiedReply`](Event::DirClassifiedReply): classified ads, all
///   maturities (its flags are the classifieds bit namespace, see
///   `CLASSIFIED_INC_ALL`).
///
/// The grids diverge sharply and the case is grid-aware (`1av`):
///
/// - **OpenSim** answers **none** of the three: `BasicSearchModule` subscribes
///   only to `OnDirFindQuery`, and no core module handles `OnDirPlacesQuery`,
///   `OnDirLandQuery` or `OnDirClassifiedQuery`, so all three draw no reply.
///   Asserted: zero of the three reply types arrive (the absence is the
///   documented behaviour, not a failure).
/// - **Second Life (aditi)** answers from the grid search backend. The places
///   query is the anchor: the Linden-category search returns the grid's Linden
///   hubs. The case asserts a `DirPlacesReply` arrives; if the beta search
///   backend answers none over UDP, it marks the run partial rather than
///   failing. Land and classifieds are recorded but not asserted, because the
///   two diverge even on Second Life (confirmed live): the places search returns
///   the Linden hubs and the classifieds search replies (empty on the beta
///   grid), but the land-for-sale query draws **no** `DirLandReply` at all.
#[derive(Debug)]
pub struct DirPlacesLandClassified;

impl GridTest for DirPlacesLandClassified {
    fn name(&self) -> &'static str {
        "dir-places-land-classified"
    }

    fn description(&self) -> &'static str {
        "DirPlacesQuery / DirLandQuery / DirClassifiedQuery"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // Places: the viewer's initial query — every Linden location in
            // PG/Mature sims, any name, sorted by dwell.
            let places_id = QueryId::from(Uuid::new_v4());
            let places_start = Instant::now();
            let places = query(
                session,
                Command::DirPlacesQuery {
                    query_id: places_id,
                    query_text: String::new(),
                    flags: DirFindFlags::INC_PG
                        .union(DirFindFlags::INC_MATURE)
                        .union(DirFindFlags::DWELL_SORT),
                    category: ParcelCategory::Linden,
                    sim_name: String::new(),
                    query_start: 0,
                },
                |event| match event {
                    Event::DirPlacesReply {
                        query_id,
                        results,
                        status,
                    } if *query_id == places_id.get() => Some((results.len(), *status)),
                    _ => None,
                },
            )
            .await?;
            let places_secs = places_start.elapsed().as_secs_f64();

            // Land: all sale types, all maturities, no price/area limit.
            let land_id = QueryId::from(Uuid::new_v4());
            let land_start = Instant::now();
            let land = query(
                session,
                Command::DirLandQuery {
                    query_id: land_id,
                    flags: DirFindFlags::INC_PG
                        .union(DirFindFlags::INC_MATURE)
                        .union(DirFindFlags::INC_ADULT),
                    search_type: LandSearchType::ALL,
                    price: 0,
                    area: 0,
                    query_start: 0,
                },
                |event| match event {
                    Event::DirLandReply { query_id, results } if *query_id == land_id.get() => {
                        Some(results.len())
                    }
                    _ => None,
                },
            )
            .await?;
            let land_secs = land_start.elapsed().as_secs_f64();

            // Classifieds: any category, all maturities.
            let classified_id = QueryId::from(Uuid::new_v4());
            let classified_start = Instant::now();
            let classified = query(
                session,
                Command::DirClassifiedQuery {
                    query_id: classified_id,
                    query_text: String::new(),
                    flags: CLASSIFIED_INC_ALL,
                    category: ClassifiedCategory::AnyCategory,
                    query_start: 0,
                },
                |event| match event {
                    Event::DirClassifiedReply {
                        query_id,
                        results,
                        status,
                    } if *query_id == classified_id.get() => Some((results.len(), *status)),
                    _ => None,
                },
            )
            .await?;
            let classified_secs = classified_start.elapsed().as_secs_f64();

            let types_answered = [places.is_some(), land.is_some(), classified.is_some()]
                .into_iter()
                .filter(|replied| *replied)
                .count();

            // Record the per-type outcomes before the assertions so even a
            // failing run documents what each query type returned.
            let metrics = ctx.metrics();
            metrics.set("places_replied", places.is_some());
            if let Some((result_count, status)) = places {
                metrics.set(
                    "places_result_count",
                    i64::try_from(result_count).unwrap_or(i64::MAX),
                );
                metrics.set("places_status", i64::from(status));
                metrics.set_timing("places_secs", places_secs);
            }
            metrics.set("land_replied", land.is_some());
            if let Some(result_count) = land {
                metrics.set(
                    "land_result_count",
                    i64::try_from(result_count).unwrap_or(i64::MAX),
                );
                metrics.set_timing("land_secs", land_secs);
            }
            metrics.set("classified_replied", classified.is_some());
            if let Some((result_count, status)) = classified {
                metrics.set(
                    "classified_result_count",
                    i64::try_from(result_count).unwrap_or(i64::MAX),
                );
                metrics.set("classified_status", i64::from(status));
                metrics.set_timing("classified_secs", classified_secs);
            }
            metrics.set(
                "types_answered",
                i64::try_from(types_answered).unwrap_or(i64::MAX),
            );

            if is_opensim(grid) {
                // No core module subscribes to any of the three, so a compliant
                // OpenSim answers none of them.
                check(
                    types_answered == 0,
                    "expected OpenSim to answer none of the places / land / classified queries (BasicSearchModule handles only DirFindQuery)",
                )?;
            } else if places.is_none() {
                // The beta search backend did not answer the anchor places query
                // over UDP; record the divergence rather than failing.
                ctx.mark_partial(
                    "aditi answered no DirPlacesReply for the Linden-category places search",
                );
            }

            Ok(())
        })
    }
}
