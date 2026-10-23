//! Accept a lure into a region rated above the accepter's maturity preference.

use sl_client_tokio::{Command, RegionHandle};

use super::teleport_access_refused::{
    ADITI_RATED_REGION, REFUSAL_ALERT, set_preference, stated_preference,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::lure::{
    bucket_text, notice_window, offer_and_receive, offered_id, record_im, record_notices,
    watch_notices,
};
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, check_eq};
use crate::teleport_trace::{region_named, request_teleport, watch_teleport};

/// Where the accepter waits for the offer: a region rated General, some way
/// from the rated one.
const GENERAL_REGION: &str = "Ahern";

/// Where the accepter asks to land there.
const GENERAL_SPOT: (f32, f32, f32) = (128.0, 128.0, 40.0);

/// Where the offerer asks to land in the rated region on a fake grid, whose
/// avatars all log in at its General start region.
const RATED_SPOT: (f32, f32, f32) = (128.0, 128.0, 30.0);

/// Offers a teleport from a region rated Adult to an avatar whose maturity
/// preference is General, and has it accept.
///
/// An offer says how its destination is rated — the last field of its binary
/// bucket — so a viewer can warn before the Teleport button is pressed, and the
/// reference does: it asks whether to raise the preference, or declines for an
/// account that may not. This case presses the button without raising
/// anything, which is what a client that ignores the field would do, and
/// records what the grid says then.
///
/// Both avatars log in at the rated region. The accepter teleports out to a
/// General one, lowers its preference there, and accepts the offerer's lure
/// back. Its preference is put back afterwards whatever happened.
///
/// `2av`. Second Life only: OpenSim does not check the preference for any
/// kind of teleport (`teleport-access-refused`), and its offers carry no
/// rating. The Second Life fake flavour plays it with its regions: the offerer
/// teleports to its Moderate one, and the accepter waits where both logged in.
#[derive(Debug)]
pub struct TeleportLureRated;

impl GridTest for TeleportLureRated {
    fn name(&self) -> &'static str {
        "teleport-lure-rated"
    }

    fn description(&self) -> &'static str {
        "Accept a lure into a region rated above the accepter's maturity preference"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Aditi, Grid::FakeSl]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if grid.is_fake() {
            "last"
        } else {
            "uri:Mauve&128&128&30"
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let (rated_region, general_region) = if grid.is_fake() {
                (crate::fake::FAR_REGION, crate::fake::START_REGION)
            } else {
                (ADITI_RATED_REGION, GENERAL_REGION)
            };
            let (primary, secondary) = ctx.primary_and_secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            primary.wait_for_region(REGION_TIMEOUT).await?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;
            let rated = region_named(primary, rated_region).await?;
            let rated_handle = RegionHandle::from(rated.grid_coordinates);
            go_to(primary, rated_handle, RATED_SPOT, "the offerer").await?;
            check_eq(
                "the offerer's region",
                &primary.region_handle(),
                &Some(rated_handle),
            )?;
            let primary_id = crate::lure::agent_id(primary, "offerer")?;

            // The accepter leaves for a General region and lowers its
            // preference there.
            let general = region_named(secondary, general_region).await?;
            let general_handle = RegionHandle::from(general.grid_coordinates);
            go_to(secondary, general_handle, GENERAL_SPOT, "the accepter").await?;
            let before = stated_preference(secondary);
            let lowered = set_preference(secondary, "PG").await?;
            check_eq("lowered_preference", &lowered.as_deref(), &Some("PG"))?;

            let message = format!("sl-conformance teleport-lure-rated {primary_id}");
            let offer = offer_and_receive(primary, secondary, &message).await;
            let trace = match &offer {
                Ok(offer) => {
                    secondary
                        .send(Command::AcceptTeleportLure {
                            lure_id: offered_id(offer),
                        })
                        .await?;
                    Some(watch_teleport(secondary, REGION_TIMEOUT).await)
                }
                Err(_unoffered) => None,
            };
            let accepter_after = watch_notices(secondary, notice_window(grid)).await;

            // The preference goes back whatever happened.
            let restore_to = before.as_deref().unwrap_or("M");
            let _restored = set_preference(secondary, restore_to).await?;
            let offer = offer?;
            let trace = trace.ok_or_else(|| {
                TestFailure::Assertion("an offer arrived and no acceptance was sent".to_owned())
            })??;
            let accepter_after = accepter_after?;
            let now_in = secondary.region_handle();

            let metrics = ctx.metrics();
            record_im("offer_", &offer, metrics);
            trace.record("", metrics);
            record_notices("accepter_after", &accepter_after, metrics);
            metrics.set("region_rating", format!("{:?}", rated.maturity));
            metrics.set(
                "preference_before",
                before.unwrap_or_else(|| "none".to_owned()),
            );

            // The offer names its destination's rating.
            let bucket = bucket_text(&offer);
            let rating = rated.maturity.to_login_access().unwrap_or("none");
            check(
                bucket.trim_end().ends_with(&format!("|{rating}")),
                &format!("the offer's bucket does not rate its destination {rating}: {bucket:?}"),
            )?;
            // And the grid refuses the accepter as it refuses a teleport to the
            // same place asked for by location.
            let failure = trace.failure.as_ref().ok_or_else(|| {
                TestFailure::Assertion(format!(
                    "a lure into a region above the preference ended as [{}]",
                    trace.sequence()
                ))
            })?;
            check(failure.from_grid, "the grid did not answer the acceptance")?;
            // Unlike a location teleport, which is started and narrated before
            // it is refused, a lure is refused with the failure alone.
            check_eq(
                "the messages of the refusal",
                &trace.sequence().as_str(),
                &"failed",
            )?;
            check_eq(
                "refusal_alert",
                &failure.alert.as_ref().map(|alert| alert.message.as_str()),
                &Some(REFUSAL_ALERT),
            )?;
            check_eq("region after the refusal", &now_in, &Some(general_handle))
        })
    }
}

/// Teleports `session` to `spot` in the region `handle`, unless it stands in
/// that region already.
async fn go_to(
    session: &mut Session,
    handle: RegionHandle,
    spot: (f32, f32, f32),
    who: &str,
) -> Result<(), TestFailure> {
    if session.region_handle() == Some(handle) {
        return Ok(());
    }
    request_teleport(session, handle, spot, (1.0, 0.0, 0.0)).await?;
    let trace = watch_teleport(session, REGION_TIMEOUT).await?;
    check(
        trace.arrival.is_some(),
        &format!("{who} did not reach the region it was sent to"),
    )
}
