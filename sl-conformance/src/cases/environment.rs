//! Request the region's and a parcel's Extended Environment (EEP) settings and
//! record them — the discovery half of `gridspec-environment`.
//!
//! A modern viewer learns a region's (or parcel's) sky, water, and day-cycle
//! settings by GETting the `ExtEnvironment` capability. The reply is a *day
//! cycle*: a schedule of named sky/water *frames* over the length of a day, plus
//! the frame definitions the schedule references.
//!
//! 1. The **region** environment ([`Command::RequestEnvironment`] with
//!    `parcel_id: None`): a decodable [`Event::Environment`] carrying a real day
//!    cycle — a positive day length and at least one frame. Both grids serve a
//!    default when nobody set one. The whole shape is recorded: length, offset,
//!    flags, version, track altitudes, the names, and every track's keyframes.
//! 2. The environment of the **parcel** at the region centre, which nobody has
//!    overridden: what a grid answers for land that only inherits
//!    (`parcel_*` metrics, and whether its day is the region's).
//! 3. Where the agent owns that parcel (OpenSim as the estate owner): **set** a
//!    parcel environment with another day length, read it back, **reset** it,
//!    and read the inherited one back again. The parcel is left as it was found
//!    even when a step fails. OpenSim only serves a parcel's environment where
//!    the estate allows parcels one, so the case first records what a set does
//!    while it is not allowed (`set_while_disallowed`), allows it for the
//!    measurement, and puts the estate back. Where it does not (aditi; OpenSim as anyone
//!    else): attempt the set anyway and record the refusal (`refused_set`).
//!
//! The raw replies, every key and its wire type, are in the trace log
//! (`RUST_LOG=sl_client_tokio::wire=trace`). `1av`, `[both, fake]`.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, DayCycleFrame, EnvironmentSettings, EnvironmentUpdate, EstateFlags, EstateInfo,
    EstateInfoUpdate, Event, ParcelInfo,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check, check_eq, count_metric, secs_metric};

/// The western/southern edge of the square the parcel is read from, in region
/// metres: the 4×4 m square at the region centre the other land cases use.
const SQUARE_WEST_SOUTH: f32 = 124.0;

/// The eastern/northern edge of that square.
const SQUARE_EAST_NORTH: f32 = 128.0;

/// A distinctive sequence id for the parcel query.
const SEQUENCE_ID: i32 = 5420;

/// Whether a parcel that has no environment of its own is answered with one
/// that has no day in it (`is_default`, no `day_cycle`) rather than with the
/// region's: both grids do, and a viewer shows such a parcel the region's
/// environment itself.
const INHERITING_PARCEL_IS_DAYLESS: Measured<bool> = Measured {
    second_life: true,
    opensim: true,
    source: "environment on aditi and OpenSim (2026-10-05, book/src/gridspec/environment.md)",
};

/// How long the region is given to take an estate change.
const ESTATE_SETTLE: Duration = Duration::from_secs(2);

/// The day length the parcel override sets, in seconds: two hours, which is
/// neither grid's default.
const OVERRIDE_DAY_LENGTH: i32 = 7_200;

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
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // `parcel_id: None` asks for the whole region's environment rather
            // than a single parcel's override.
            let start = Instant::now();
            let env = request(session, None).await?;
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
            for (key, value) in shape(&env) {
                metrics.set(&format!("region_{key}"), value);
            }

            // 2. The parcel at the region centre, which only inherits.
            let session = ctx.primary();
            let agent = session.agent_id();
            let parcel = read_parcel(session).await?;
            if !parcel.request_result.has_data() {
                ctx.mark_partial("the region centre has no parcel to ask about");
                return Ok(());
            }
            let parcel_id = parcel.local_id.0;
            let inherited = request(session, Some(parcel_id)).await?;
            INHERITING_PARCEL_IS_DAYLESS.check(
                "a parcel with no environment of its own is answered without a day",
                ctx.grid(),
                &(inherited.day_length == 0 && inherited.day_cycle.sky_frames.is_empty()),
            )?;
            check_eq(
                "the parcel the inheriting answer names",
                &inherited.parcel_id,
                &parcel_id,
            )?;
            let metrics = ctx.metrics();
            metrics.set(
                "parcel_environment_version",
                i64::from(parcel.parcel_environment_version),
            );
            metrics.set(
                "region_allow_environment_override",
                parcel.region_allow_environment_override,
            );
            metrics.set(
                "parcel_day_is_the_regions",
                inherited.day_cycle == env.day_cycle,
            );
            for (key, value) in shape(&inherited) {
                metrics.set(&format!("parcel_{key}"), value);
            }

            // 3. An override, where the land is ours to set one on — and where
            //    it is not, what the grid says to the attempt.
            //    A fake grid enforces no land rights, so there every avatar is
            //    measured as the owner.
            let owns = agent.map(|agent| agent.uuid()) == Some(parcel.owner.uuid());
            if !owns && !ctx.grid().is_fake() {
                let session = ctx.primary();
                let refusal = refused_set(session, parcel_id).await?;
                let took = matches!(refusal, SetAnswer::Stored(_));
                if took {
                    session
                        .send(Command::ResetEnvironment {
                            parcel_id: Some(parcel_id),
                            track_no: None,
                        })
                        .await?;
                    let _reset = reply(session).await?;
                }
                ctx.metrics().set("refused_set", refusal.describe());
                check(
                    !matches!(refusal, SetAnswer::Silent),
                    "the grid gave a set it would not take no answer the client could read",
                )?;
                if took {
                    ctx.mark_partial("the grid took the set: the avatar may edit this land");
                }
                return Ok(());
            }
            // A parcel's environment only counts where the estate lets parcels
            // have one. Where it does not, record what a set does anyway, then
            // allow it for the length of the measurement.
            let session = ctx.primary();
            session.send(Command::RequestEstateInfo).await?;
            let estate = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::EstateInfo(info) => Some((**info).clone()),
                    _ => None,
                })
                .await?;
            let allowed = estate.estate_flags & EstateFlags::ALLOW_ENVIRONMENT_OVERRIDE.bits() != 0;
            let mut while_disallowed = None;
            if !allowed {
                let answer = set_parcel(session, parcel_id, &env).await?;
                while_disallowed = Some(if answer.day_length == OVERRIDE_DAY_LENGTH {
                    "stored and served"
                } else {
                    "accepted, and the parcel still inherits"
                });
                reset_parcel(session, parcel_id).await?;
                set_estate_flags(
                    session,
                    &estate,
                    estate.estate_flags | EstateFlags::ALLOW_ENVIRONMENT_OVERRIDE.bits(),
                )
                .await?;
            }
            let outcome = override_parcel(session, parcel_id, &env).await;
            // Whatever happened above, the parcel goes back to inheriting and
            // the estate to what it allowed.
            let reset = reset_parcel(session, parcel_id).await;
            if !allowed {
                set_estate_flags(session, &estate, estate.estate_flags).await?;
            }
            let (overridden, reset) = (outcome?, reset?);
            let reread = request(session, Some(parcel_id)).await?;
            check_eq(
                "the parcel's day length after the reset",
                &reread.day_length,
                &inherited.day_length,
            )?;
            let metrics = ctx.metrics();
            metrics.set("estate_allows_parcel_environments", allowed);
            if let Some(answer) = while_disallowed {
                metrics.set("set_while_disallowed", answer);
            }
            for (key, value) in shape(&overridden) {
                metrics.set(&format!("override_{key}"), value);
            }
            for (key, value) in shape(&reset) {
                metrics.set(&format!("reset_{key}"), value);
            }
            Ok(())
        })
    }
}

/// Sets a parcel environment that is the region's day (`region`) at another
/// length, checks the grid answered with it and serves it, and returns what it
/// serves.
///
/// # Errors
///
/// Returns a [`TestFailure`] when a send or wait fails, or the stored length is
/// not the one set.
async fn override_parcel(
    session: &mut Session,
    parcel_id: i32,
    region: &EnvironmentSettings,
) -> Result<EnvironmentSettings, TestFailure> {
    let stored = set_parcel(session, parcel_id, region).await?;
    check_eq(
        "the day length the set answered with",
        &stored.day_length,
        &OVERRIDE_DAY_LENGTH,
    )?;
    let reread = request(session, Some(parcel_id)).await?;
    check_eq(
        "the parcel's day length after the set",
        &reread.day_length,
        &OVERRIDE_DAY_LENGTH,
    )?;
    Ok(reread)
}

/// Sets a parcel environment that is the region's day (`region`) at
/// [`OVERRIDE_DAY_LENGTH`] — a parcel that only inherits has no day of its own
/// to send back — and returns the grid's answer: the stored settings, which the
/// client fetches itself where the grid's reply leaves them out.
///
/// # Errors
///
/// Propagates the send and wait failures.
async fn set_parcel(
    session: &mut Session,
    parcel_id: i32,
    region: &EnvironmentSettings,
) -> Result<EnvironmentSettings, TestFailure> {
    session
        .send(Command::SetEnvironment {
            parcel_id: Some(parcel_id),
            track_no: None,
            update: Box::new(EnvironmentUpdate {
                day_length: Some(OVERRIDE_DAY_LENGTH),
                day_offset: Some(region.day_offset),
                day_cycle: Some(region.day_cycle.clone()),
                ..EnvironmentUpdate::default()
            }),
        })
        .await?;
    reply(session).await
}

/// Drops a parcel's environment and returns what it inherits.
///
/// # Errors
///
/// Propagates the send and wait failures.
async fn reset_parcel(
    session: &mut Session,
    parcel_id: i32,
) -> Result<EnvironmentSettings, TestFailure> {
    session
        .send(Command::ResetEnvironment {
            parcel_id: Some(parcel_id),
            track_no: None,
        })
        .await?;
    reply(session).await
}

/// Writes the estate's flags and gives the region [`ESTATE_SETTLE`] to take
/// them.
///
/// # Errors
///
/// Propagates the send's failure.
async fn set_estate_flags(
    session: &Session,
    estate: &EstateInfo,
    flags: u32,
) -> Result<(), TestFailure> {
    session
        .send(Command::SetEstateInfo(EstateInfoUpdate {
            estate_name: estate.estate_name.clone(),
            flags,
            sun_hour: 0.0,
        }))
        .await?;
    tokio::time::sleep(ESTATE_SETTLE).await;
    Ok(())
}

/// What a grid answered a set with.
enum SetAnswer {
    /// It stored the environment and sent it back.
    Stored(Box<EnvironmentSettings>),
    /// It refused, with this reason.
    Refused(String),
    /// Nothing the client could read came back in time.
    Silent,
}

impl SetAnswer {
    /// The answer as the record shows it.
    fn describe(&self) -> String {
        match self {
            Self::Stored(stored) => format!("stored (day_length {})", stored.day_length),
            Self::Refused(message) => format!("refused: {message}"),
            Self::Silent => "no readable answer".to_owned(),
        }
    }
}

/// Sets a day length on a parcel the agent has no rights over and returns the
/// grid's answer.
///
/// # Errors
///
/// Propagates the send's failure and the wait's, other than its timeout.
async fn refused_set(session: &mut Session, parcel_id: i32) -> Result<SetAnswer, TestFailure> {
    session
        .send(Command::SetEnvironment {
            parcel_id: Some(parcel_id),
            track_no: None,
            update: Box::new(EnvironmentUpdate {
                day_length: Some(OVERRIDE_DAY_LENGTH),
                ..EnvironmentUpdate::default()
            }),
        })
        .await?;
    match session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::Environment(env) => Some(SetAnswer::Stored(env.clone())),
            Event::EnvironmentChangeRefused { message } => {
                Some(SetAnswer::Refused(message.clone()))
            }
            _ => None,
        })
        .await
    {
        Ok(answer) => Ok(answer),
        Err(TestFailure::Timeout(_)) => Ok(SetAnswer::Silent),
        Err(other) => Err(other),
    }
}

/// Asks for the environment of the region (`None`) or a parcel, and awaits it.
///
/// # Errors
///
/// Propagates the send and wait failures.
async fn request(
    session: &mut Session,
    parcel_id: Option<i32>,
) -> Result<EnvironmentSettings, TestFailure> {
    session
        .send(Command::RequestEnvironment { parcel_id })
        .await?;
    reply(session).await
}

/// Awaits the next environment the grid sends.
///
/// # Errors
///
/// Propagates the wait's failures.
async fn reply(session: &mut Session) -> Result<EnvironmentSettings, TestFailure> {
    session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::Environment(env) => Some(env.as_ref().clone()),
            _ => None,
        })
        .await
}

/// Reads the parcel at the region centre.
///
/// # Errors
///
/// Propagates the send and wait failures.
async fn read_parcel(session: &mut Session) -> Result<ParcelInfo, TestFailure> {
    session
        .send(Command::RequestParcelProperties {
            west: SQUARE_WEST_SOUTH,
            south: SQUARE_WEST_SOUTH,
            east: SQUARE_EAST_NORTH,
            north: SQUARE_EAST_NORTH,
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

/// One track's keyframes as the record shows them: `time=frame` pairs.
fn track(frames: &[DayCycleFrame]) -> String {
    frames
        .iter()
        .map(|frame| format!("{:.3}={}", frame.keyframe, frame.name))
        .collect::<Vec<_>>()
        .join(" ")
}

/// An environment's shape, as named metrics: everything but the frames' own
/// settings, which the trace log carries.
fn shape(env: &EnvironmentSettings) -> Vec<(&'static str, String)> {
    let mut shape = vec![
        ("parcel_id", env.parcel_id.to_string()),
        ("day_length", env.day_length.to_string()),
        ("day_offset", env.day_offset.to_string()),
        ("flags", env.flags.to_string()),
        ("env_version", env.env_version.to_string()),
        ("track_altitudes", format!("{:?}", env.track_altitudes)),
        ("day_asset", format!("{:?}", env.day_asset)),
        ("day_names", format!("{:?}", env.day_names)),
        ("cycle_name", env.day_cycle.name.clone()),
        ("water_track", track(&env.day_cycle.water_track)),
        (
            "sky_frame_names",
            env.day_cycle
                .sky_frames
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
        ),
        (
            "water_frame_names",
            env.day_cycle
                .water_frames
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
        ),
    ];
    for (name, frames) in ["sky_track_1", "sky_track_2", "sky_track_3", "sky_track_4"]
        .into_iter()
        .zip(&env.day_cycle.sky_tracks)
    {
        shape.push((name, track(frames)));
    }
    shape
}
