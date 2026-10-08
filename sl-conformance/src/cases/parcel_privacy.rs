//! Make a parcel hide its avatars (`SeeAVs` off) and record who is still
//! sent whom as two residents step in and out of it.
//!
//! The parcel's owner chops a corner off the region's parcel and turns off
//! *avatars on other parcels can see and chat with avatars on this parcel*
//! for it. The two residents then stand, in turn:
//!
//! 1. the owner inside, the other outside;
//! 2. both inside;
//! 3. the owner outside, the other inside;
//! 4. both outside again.
//!
//! At each step the case records, for each of them, whether the other's
//! avatar is in its object stream (and how long after the move a
//! `KillObject` took it out), and whether its coarse-location feed lists the
//! other — the two are separate channels, and a grid that hides an avatar in
//! one need not hide it in the other.
//!
//! The corner is joined back into the region's parcel on every path, as
//! `parcel-divide-join` does it.
//!
//! OpenSim only: on aditi our avatars own no land
//! (`gridspec-aditi-test-land`). Run it as the avatar that owns the region's
//! parcel (`--avatar estate-owner`).

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event, ParcelInfo};

use super::avatar_presence::{Feed, Seen, agent_of, ran_out};
use crate::cases::parcel_divide_join::{RestoreOnDrop, restore_single_parcel};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check, secs_metric};
use crate::teleport_trace::request_teleport;

/// Where the measured answers are written down.
const SOURCE: &str = "book/src/gridspec/avatars.md § Parcel privacy (parcel-privacy, 2026-10-08)";

/// The start location of both avatars: the "Default Region" centre, on the
/// parcel the corner is chopped from.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The corner chopped off: the south-west 64 m square.
const CORNER_M: f32 = 64.0;

/// Where a resident stands inside the corner.
const INSIDE: (f32, f32, f32) = (32.0, 32.0, 30.0);

/// Where one stands outside it: back near where both logged in, well clear of
/// the line.
const OUTSIDE: (f32, f32, f32) = (120.0, 120.0, 30.0);

/// How long the parcel edits are given to take effect.
const EDIT_SETTLE: Duration = Duration::from_secs(2);

/// How long the scene is left to arrive before anything is measured.
const SETTLE: Duration = Duration::from_secs(15);

/// How long both listen after each move: past two of OpenSim's coarse
/// updates.
const STEP: Duration = Duration::from_secs(12);

/// The sequence id of this case's parcel queries.
const SEQUENCE_ID: i32 = 5480;

/// Whether a resident outside a parcel that hides its avatars is still sent
/// the avatar of one inside it.
const OUTSIDE_SEES_INSIDE: Measured<bool> = Measured {
    second_life: false,
    opensim: false,
    source: SOURCE,
};

/// Whether two residents inside the same hiding parcel are sent each other.
const INSIDE_SEES_INSIDE: Measured<bool> = Measured {
    second_life: true,
    opensim: true,
    source: SOURCE,
};

/// The two residents, each listening for the other.
struct Both<'a> {
    /// The parcel's owner.
    owner: &'a mut Session,
    /// The other resident.
    other: &'a mut Session,
    /// What the owner is sent about the other.
    owner_feed: Feed,
    /// What the other is sent about the owner.
    other_feed: Feed,
}

impl Both<'_> {
    /// Listens on both sessions for `window`.
    async fn watch(&mut self, window: Duration) -> Result<(), TestFailure> {
        let owner_feed = &mut self.owner_feed;
        let other_feed = &mut self.other_feed;
        let (owner, other) = tokio::join!(
            self.owner.wait_for(window, |event| {
                owner_feed.note(event);
                None::<()>
            }),
            self.other.wait_for(window, |event| {
                other_feed.note(event);
                None::<()>
            }),
        );
        ran_out(owner).and_then(|()| ran_out(other))
    }

    /// Records, under `step`, who is sent whom now, and what happened to each
    /// stream since `since`.
    fn record(&self, step: &str, since: f64, metrics: &mut Metrics) -> Sees {
        let sees = Sees {
            owner_sees_other: self.owner_feed.present,
            other_sees_owner: self.other_feed.present,
        };
        for (who, whom, feed) in [
            ("owner", "other", &self.owner_feed),
            ("other", "owner", &self.other_feed),
        ] {
            let key = |field: &str| format!("{step}_{who}_{field}_{whom}");
            metrics.set(&key("sees"), feed.present);
            if let Some(kill) = feed.first(Seen::Removed, true, since) {
                metrics.set_timing(&secs_metric(&key("kill_of")), kill - since);
            }
            if let Some(back) = feed.first(Seen::Object, true, since) {
                metrics.set_timing(&secs_metric(&key("object_of")), back - since);
            }
            metrics.set(
                &key("coarse_lists"),
                feed.root_coarse(since, feed.now())
                    .last()
                    .is_some_and(|coarse| coarse.subject.is_some()),
            );
        }
        sees
    }
}

/// Who is sent whom at one step.
#[derive(Debug, Clone, Copy)]
struct Sees {
    /// Whether the owner's stream holds the other's avatar.
    owner_sees_other: bool,
    /// Whether the other's stream holds the owner's.
    other_sees_owner: bool,
}

/// Teleports `session` to `to` within its region.
async fn step_to(session: &Session, to: (f32, f32, f32)) -> Result<(), TestFailure> {
    let region = session
        .region_handle()
        .ok_or_else(|| TestFailure::Assertion("login established no region handle".to_owned()))?;
    request_teleport(session, region, to, (1.0, 0.0, 0.0)).await
}

/// The parcel under the 4 m square whose south-west corner is `at`.
async fn parcel_at(session: &mut Session, at: (f32, f32)) -> Result<ParcelInfo, TestFailure> {
    let (west, south) = at;
    session
        .send(Command::RequestParcelProperties {
            west,
            south,
            east: west + 4.0,
            north: south + 4.0,
            sequence_id: SEQUENCE_ID,
            snap_selection: false,
        })
        .await?;
    session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ParcelProperties(parcel) if parcel.sequence_id == SEQUENCE_ID => {
                Some((**parcel).clone())
            }
            _ => None,
        })
        .await
}

/// The measurement itself, run under the case's cleanup guard.
async fn exercise(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let (owner, other) = ctx.primary_and_secondary().ok_or_else(|| {
        TestFailure::Assertion("this case needs a second avatar (--secondary)".to_owned())
    })?;
    owner.wait_for_region(REGION_TIMEOUT).await?;
    other.wait_for_region(REGION_TIMEOUT).await?;
    let (owner_id, other_id) = (agent_of(owner)?, agent_of(other)?);

    restore_single_parcel(owner).await?;
    let whole = parcel_at(owner, (OUTSIDE.0, OUTSIDE.1)).await?;
    check(
        whole.owner.uuid() == owner_id,
        "the primary avatar does not own the region's parcel (run with --avatar estate-owner)",
    )?;
    owner
        .send(Command::DivideParcel {
            west: 0.0,
            south: 0.0,
            east: CORNER_M,
            north: CORNER_M,
        })
        .await?;
    tokio::time::sleep(EDIT_SETTLE).await;
    let corner = parcel_at(owner, (INSIDE.0, INSIDE.1)).await?;
    check(
        corner.local_id != whole.local_id,
        "the divide made no new parcel of the corner",
    )?;
    metrics.set("see_avs_before", format!("{:?}", corner.see_avs));
    metrics.set(
        "edit_transport",
        if owner.cap("ParcelPropertiesUpdate").is_some() {
            "capability"
        } else {
            "udp"
        },
    );
    let mut hiding = corner.to_update();
    hiding.see_avs = Some(false);
    owner.send(Command::UpdateParcel(Box::new(hiding))).await?;
    tokio::time::sleep(EDIT_SETTLE).await;
    let hidden = parcel_at(owner, (INSIDE.0, INSIDE.1)).await?;
    metrics.set("see_avs_after", format!("{:?}", hidden.see_avs));

    let started = Instant::now();
    let mut both = Both {
        owner_feed: Feed::new(owner, owner_id, Some(other_id), started),
        other_feed: Feed::new(other, other_id, Some(owner_id), started),
        owner,
        other,
    };
    // Both outside to begin with, side by side.
    step_to(both.owner, OUTSIDE).await?;
    step_to(both.other, OUTSIDE).await?;
    both.watch(SETTLE).await?;
    let before = both.record("outside", 0.0, metrics);
    check(
        before.owner_sees_other && before.other_sees_owner,
        "the two residents were not sent each other before either stepped in",
    )?;

    let since = both.owner_feed.now();
    step_to(both.owner, INSIDE).await?;
    both.watch(STEP).await?;
    let owner_in = both.record("owner_inside", since, metrics);

    let since = both.owner_feed.now();
    step_to(both.other, INSIDE).await?;
    both.watch(STEP).await?;
    let both_in = both.record("both_inside", since, metrics);

    let since = both.owner_feed.now();
    step_to(both.owner, OUTSIDE).await?;
    both.watch(STEP).await?;
    let other_in = both.record("other_inside", since, metrics);

    let since = both.owner_feed.now();
    step_to(both.other, OUTSIDE).await?;
    both.watch(STEP).await?;
    let after = both.record("outside_again", since, metrics);

    OUTSIDE_SEES_INSIDE.check(
        "whether a resident outside a hiding parcel is sent the owner inside it",
        grid,
        &owner_in.other_sees_owner,
    )?;
    OUTSIDE_SEES_INSIDE.check(
        "whether the owner outside its hiding parcel is sent a resident inside it",
        grid,
        &other_in.owner_sees_other,
    )?;
    INSIDE_SEES_INSIDE.check(
        "whether two residents inside a hiding parcel are sent each other",
        grid,
        &(both_in.owner_sees_other && both_in.other_sees_owner),
    )?;
    check(
        after.owner_sees_other && after.other_sees_owner,
        "the two residents were not sent each other again once both had stepped out",
    )
}

/// Hides a parcel's avatars and records who is sent whom across its line.
#[derive(Debug)]
pub struct ParcelPrivacy;

impl GridTest for ParcelPrivacy {
    fn name(&self) -> &'static str {
        "parcel-privacy"
    }

    fn description(&self) -> &'static str {
        "Turn a parcel's SeeAVs off and record who is sent whom as two residents step in and out"
    }

    fn grids(&self) -> &'static [Grid] {
        // Not aditi: our avatars own no land there. Not the fake grid: its
        // residents are not shown to each other.
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn start_location(&self, _grid: Grid) -> &'static str {
        OPENSIM_START
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // The divide leaves the region split until it is joined back: the
            // awaited join for every path that returns, the drop guard for one
            // that never does.
            let mut guard = RestoreOnDrop {
                commander: ctx.primary().commander(),
                armed: true,
            };
            let mut metrics = Metrics::new();
            let outcome = exercise(ctx, &mut metrics).await;
            ctx.metrics().merge(metrics);
            let restored = restore_single_parcel(ctx.primary()).await;
            guard.armed = false;
            outcome.and(restored)
        })
    }
}
