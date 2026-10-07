//! Teleport into a region rated above the agent's maturity preference and
//! hold the grid to whether, and how, it refuses.

use std::time::Duration;

use sl_client_tokio::{AgentPreferences, Command, Event, Maturity, RegionHandle, TeleportFlags};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, check_eq};
use crate::teleport_trace::{region_named, request_teleport, watch_teleport};

/// Where each answer below is written down.
const SOURCE: &str = "book/src/gridspec/teleport.md (teleport-access-refused, 2026-10-07)";

/// Where the agent asks to land.
const DESTINATION: (f32, f32, f32) = (128.0, 128.0, 30.0);

/// The region on aditi the case teleports into: the sandbox the test avatars
/// build in, which is rated Adult.
const ADITI_RATED_REGION: &str = "Mauve";

/// How a location teleport into a region rated above the agent's preference
/// ends: refused on Second Life; OpenSim has no such check.
const OUTCOME: Measured<&str> = Measured {
    second_life: "failed",
    opensim: "moved",
    source: SOURCE,
};

/// The alert key Second Life refuses with.
const REFUSAL_ALERT: &str = "RegionTPAccessBlocked";

/// The LLSD key, in the alert's parameters, naming the refusing region's
/// rating.
const REFUSAL_PARAMETER: &str = "_region_access";

/// How long one attempt at setting the preference waits for its echo.
const PREFERENCE_ATTEMPT: Duration = Duration::from_secs(5);

/// How many times the preference is set before the case gives up on an echo.
const PREFERENCE_ATTEMPTS: u8 = 6;

/// Sets the agent's maturity preference and waits for the grid's echo of the
/// stored set, returning the preference it now reports.
///
/// The request is repeated until it is answered: straight after an arrival the
/// new region's capabilities are still being fetched, and a preference set in
/// that gap has no capability to go to.
async fn set_preference(
    session: &mut Session,
    max_access: &str,
) -> Result<Option<String>, TestFailure> {
    let mut attempts_left = PREFERENCE_ATTEMPTS;
    loop {
        session
            .send(Command::SetAgentPreferences(Box::new(AgentPreferences {
                max_access_pref: Some(max_access.to_owned()),
                ..AgentPreferences::default()
            })))
            .await?;
        let echoed = session
            .wait_for(PREFERENCE_ATTEMPT, |event| match event {
                Event::AgentPreferences(preferences) => Some(preferences.max_access_pref.clone()),
                _ => None,
            })
            .await;
        attempts_left = attempts_left.saturating_sub(1);
        match echoed {
            Err(TestFailure::Timeout(_)) if attempts_left > 0 => {}
            answered => return answered,
        }
    }
}

/// Lowers the agent's maturity preference to General and teleports into a
/// region rated above it.
///
/// **Second Life** runs the whole teleport up to the point of sending the
/// agent — the start, `resolving`, `Sending to destination.` — and then fails
/// it over the event queue: the reason a paragraph about maturity ratings, the
/// alert `RegionTPAccessBlocked`, and the alert's parameters an LLSD map
/// naming the region's rating (`_region_access`), which is what the reference
/// viewer builds its "change your preference and try again?" dialog from.
/// **OpenSim** does not consult the preference at all, and the agent arrives.
///
/// The case restores the preference the login stated. It runs on aditi and both fake
/// flavours; the local OpenSim has no region rated above General to try, so
/// its answer was measured by hand (a region set Mature for one probe) and is
/// held here on the OpenSim-flavoured fake grid.
#[derive(Debug)]
pub struct TeleportAccessRefused;

impl GridTest for TeleportAccessRefused {
    fn name(&self) -> &'static str {
        "teleport-access-refused"
    }

    fn description(&self) -> &'static str {
        "Teleport into a region rated above the maturity preference; hold the grid to its answer"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        match grid {
            Grid::Aditi => super::teleport_cross_region::ADITI_START,
            Grid::Opensim | Grid::FakeSl | Grid::FakeOpensim => "last",
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let rated_region = if grid.is_fake() {
                crate::fake::FAR_REGION
            } else {
                ADITI_RATED_REGION
            };
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let origin_handle = session.region_handle();

            let target = region_named(session, rated_region).await?;
            check(
                matches!(target.maturity, Maturity::Mature | Maturity::Adult),
                &format!(
                    "{rated_region} is rated {:?}, so it is not above a General preference",
                    target.maturity
                ),
            )?;
            let target_handle = RegionHandle::from(target.grid_coordinates);

            // What the agent had asked for before, to put back afterwards: the
            // preference the login response stated, or the ceiling it is
            // entitled to on a grid that states no preference. (Asking the
            // capability would be the direct way, and Second Life does not
            // answer a request that sets nothing.)
            let before = session.login_success().and_then(|login| {
                login
                    .agent_region_access
                    .clone()
                    .or_else(|| login.agent_access_max.clone())
            });
            let lowered = set_preference(session, "PG").await?;
            check_eq("lowered_preference", &lowered.as_deref(), &Some("PG"))?;

            request_teleport(session, target_handle, DESTINATION, (1.0, 0.0, 0.0)).await?;
            let trace = watch_teleport(session, REGION_TIMEOUT).await;

            // Put the preference back whatever happened, on whichever region
            // the agent is in now — or at Moderate, the stored default of a
            // grid whose login says nothing of it.
            let restore_to = before.as_deref().unwrap_or("M");
            let _restored = set_preference(session, restore_to).await?;
            let trace = trace?;
            let now_in = session.region_handle();

            let metrics = ctx.metrics();
            trace.record("", metrics);
            metrics.set("region_rating", format!("{:?}", target.maturity));
            metrics.set(
                "preference_before",
                before.unwrap_or_else(|| "none".to_owned()),
            );

            OUTCOME.check(
                "how a teleport into a region above the maturity preference ends",
                grid,
                &trace.outcome(),
            )?;
            let Some(failure) = &trace.failure else {
                // The grid without the check: the agent is in the rated region.
                return check(
                    now_in == Some(target_handle),
                    "the teleport was not refused, yet the session is not in the rated region",
                );
            };
            check(
                now_in == origin_handle,
                "a refused teleport left the session naming another region",
            )?;
            check_eq(
                "start_flags",
                &trace.starts.as_slice(),
                &[TeleportFlags::VIA_LOCATION].as_slice(),
            )?;
            super::teleport_cross_region::PROGRESS_LINES.check(
                "the progress lines ahead of a maturity refusal",
                grid,
                &trace.lines().as_slice(),
            )?;
            let alert = failure.alert.as_ref().ok_or_else(|| {
                TestFailure::Assertion(format!(
                    "the refusal ({:?}) carried no alert",
                    failure.reason
                ))
            })?;
            check_eq("refusal_alert", &alert.message.as_str(), &REFUSAL_ALERT)?;
            check(
                alert.extra_params.contains(REFUSAL_PARAMETER),
                &format!(
                    "the refusal's alert parameters do not name {REFUSAL_PARAMETER}: {:?}",
                    alert.extra_params
                ),
            )?;
            check(
                failure.reason.contains("maturity"),
                &format!(
                    "the refusal's reason says nothing of maturity: {:?}",
                    failure.reason
                ),
            )
        })
    }
}
