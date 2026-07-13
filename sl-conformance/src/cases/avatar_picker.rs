//! Autocomplete an avatar name and confirm the `AvatarPickerReply` results.

use sl_client_tokio::{Command, Event, QueryId, Uuid};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, is_opensim, send_then_wait};

/// Runs a name-autocomplete lookup and confirms the picker reply.
///
/// The viewer's avatar-name picker (the "Choose Resident" chooser behind a
/// calling-card offer, a friendship offer, an estate/parcel access grant, ...)
/// sends an `AvatarPickerRequest` carrying a partial name and a client-minted
/// `QueryID`; the simulator searches its directory and answers with an
/// `AvatarPickerReply` of matching residents, surfaced here as
/// [`Event::AvatarPickerReply`].
///
/// To supply a name the grid is guaranteed to know — without baking an avatar
/// name into the source or the record — the case searches for the agent's *own*
/// first name (from the login credentials, via
/// [`Session::avatar_first_name`](crate::context::Session::avatar_first_name)).
/// It records the raw and real (non-nil-keyed) match counts, how many carried a
/// legacy first name, and whether the querying agent itself appeared, but never
/// the names, keeping resident identities out of the record.
///
/// The grids diverge and the case is grid-aware (`1av`):
///
/// - **OpenSim** answers from `UserManagementModule.HandleAvatarPickerRequest`,
///   which searches the user-account service — a set that includes the
///   requester — so a self-name query must return the querying agent. Asserted:
///   at least one real (non-nil-keyed) match, and the agent's own id among them.
/// - **Second Life (aditi)** answers from the grid people-search. The beta
///   grid's people directory is sparse: as observed live, a self-name query
///   returns no real matches — only a nil-keyed, empty-named sentinel row (SL's
///   "no results" encoding). With nothing to observe, the case marks the run
///   partial rather than failing.
#[derive(Debug)]
pub struct AvatarPicker;

impl GridTest for AvatarPicker {
    fn name(&self) -> &'static str {
        "avatar-picker"
    }

    fn description(&self) -> &'static str {
        "avatar name-picker autocomplete"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // Search for the agent's own first name: a real, grid-present name
            // that needs no hardcoded identity.
            let query_text = session.avatar_first_name().to_owned();
            let self_id = session.agent_id();
            let query_id = QueryId::from(Uuid::new_v4());

            // Issue the `AvatarPickerRequest` and await the matching
            // `AvatarPickerReply` (gated on the echoed query id, so an unrelated
            // pending picker reply cannot satisfy this case). The predicate
            // returns only counts and whether our own id appears, so no resident
            // name is copied out of the event.
            let (result_count, real_count, self_present, named_count): (usize, usize, bool, usize) =
                send_then_wait(
                    session,
                    Command::AvatarPickerRequest {
                        query_id,
                        name: query_text,
                    },
                    REPLY_TIMEOUT,
                    |event| match event {
                        Event::AvatarPickerReply {
                            query_id: id,
                            results,
                        } if *id == query_id.get() => {
                            // SL encodes "no results" as a single nil-keyed,
                            // empty-named sentinel row, so count only real,
                            // non-nil-keyed matches for the assertions.
                            let real_count = results
                                .iter()
                                .filter(|r| !r.avatar_id.uuid().is_nil())
                                .count();
                            let self_present =
                                self_id.is_some_and(|id| results.iter().any(|r| r.avatar_id == id));
                            let named_count =
                                results.iter().filter(|r| !r.first_name.is_empty()).count();
                            Some((results.len(), real_count, self_present, named_count))
                        }
                        _ => None,
                    },
                )
                .await?;

            if is_opensim(grid) {
                // The account-service search includes the requester, so a search
                // for our own name must return us.
                check(
                    real_count > 0,
                    "expected the avatar picker to return at least one real match for the agent's own name",
                )?;
                check(
                    self_present,
                    "expected the OpenSim avatar-picker results to include the querying agent's own id",
                )?;
            } else if real_count == 0 {
                // The aditi people directory is sparse: a self-name query yields
                // only the "no results" sentinel, leaving nothing to observe.
                ctx.mark_partial(
                    "aditi people directory returned no real avatar-picker matches for the agent's own name",
                );
            }

            let metrics = ctx.metrics();
            metrics.set(
                "result_count",
                i64::try_from(result_count).unwrap_or(i64::MAX),
            );
            metrics.set("real_count", i64::try_from(real_count).unwrap_or(i64::MAX));
            metrics.set("self_present", self_present);
            metrics.set(
                "named_count",
                i64::try_from(named_count).unwrap_or(i64::MAX),
            );
            Ok(())
        })
    }
}
