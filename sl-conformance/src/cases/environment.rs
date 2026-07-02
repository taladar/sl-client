//! Request the region's Extended Environment (EEP) settings and record them.
//!
//! A modern viewer learns a region's (or parcel's) sky, water, and day-cycle
//! settings by GETting the `ExtEnvironment` capability. The reply is a *day
//! cycle*: a schedule of named sky/water *frames* over the length of a day, plus
//! the frame definitions the schedule references. This case drives it with
//! [`Command::RequestEnvironment`] (`parcel_id: None`, the whole region) and
//! asserts a decodable [`Event::Environment`] reply arrives carrying a real day
//! cycle.
//!
//! The one cross-grid invariant is that the reply describes a non-degenerate day
//! cycle: a positive [`EnvironmentSettings::day_length`] and at least one named
//! frame (an empty frame set would mean the capability answered but decoded to
//! nothing). Both grids serve a region default when no custom environment is set
//! — OpenSim's `EnvironmentModule` returns its built-in `WLDaycycle` (one water
//! frame plus sky frames) and Second Life its regional default — so the invariant
//! holds without any world setup. Records the reply latency, the day length and
//! offset, the reported environment version, and the sky/water frame and track
//! counts so the reporter can trend environment richness per grid. `1av`,
//! `[both]`. No new client code — the command/event/CAPS surface already existed.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, EnvironmentSettings, Event};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, count_metric, secs_metric};

/// How long to wait for the `ExtEnvironment` reply.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// Counts the named frames the day cycle defines: its sky frames plus its water
/// frames. A real environment defines at least one; a `0` count would mean the
/// reply decoded to an empty day cycle.
fn frame_count(env: &EnvironmentSettings) -> usize {
    env.day_cycle
        .sky_frames
        .len()
        .saturating_add(env.day_cycle.water_frames.len())
}

/// Requests the region's Extended Environment settings and records the day
/// cycle's length, version, and frame/track counts.
#[derive(Debug)]
pub struct Environment;

impl GridTest for Environment {
    fn name(&self) -> &'static str {
        "environment"
    }

    fn description(&self) -> &'static str {
        "Request the region's Extended Environment (EEP) settings and record them"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // `parcel_id: None` asks for the whole region's environment rather
            // than a single parcel's override.
            let start = Instant::now();
            session
                .send(Command::RequestEnvironment { parcel_id: None })
                .await?;
            let env = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::Environment(env) => Some(env.as_ref().clone()),
                    _ => None,
                })
                .await?;
            let elapsed = start.elapsed().as_secs_f64();

            check(
                env.day_length > 0,
                "expected the environment reply to carry a positive day length",
            )?;
            let frames = frame_count(&env);
            check(
                frames >= 1,
                "expected the environment reply to define at least one sky or water frame",
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("environment"), elapsed);
            metrics.set("day_length", i64::from(env.day_length));
            metrics.set("day_offset", i64::from(env.day_offset));
            metrics.set("env_version", i64::from(env.env_version));
            metrics.set(&count_metric("frames"), i64::try_from(frames).unwrap_or(-1));
            metrics.set(
                &count_metric("sky_frames"),
                i64::try_from(env.day_cycle.sky_frames.len()).unwrap_or(-1),
            );
            metrics.set(
                &count_metric("water_frames"),
                i64::try_from(env.day_cycle.water_frames.len()).unwrap_or(-1),
            );
            metrics.set(
                &count_metric("sky_tracks"),
                i64::try_from(env.day_cycle.sky_tracks.len()).unwrap_or(-1),
            );
            Ok(())
        })
    }
}
