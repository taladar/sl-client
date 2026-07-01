//! Create, read back, edit, and delete the agent's own picks and classifieds.
//!
//! The profile "Picks" and "Classifieds" tabs are CRUD over two small lists the
//! profile service keeps per account. A viewer creates or edits a pick with
//! [`Command::UpdatePick`] (`PickInfoUpdate`) and a classified with
//! [`Command::UpdateClassified`] (`ClassifiedInfoUpdate`); it fetches one's full
//! record with [`Command::RequestClassifiedInfo`] (`ClassifiedInfoRequest`), and
//! removes one with [`Command::DeletePick`] / [`Command::DeleteClassified`].
//!
//! This case drives both round-trips on the agent's own profile (`1av`).
//!
//! **Picks** use a fresh random id per run and are driven entirely through the
//! *replies the simulator volunteers after every edit*: a `PickInfoUpdate` draws
//! back both an `AvatarPicksReply` (the whole list, [`Event::AvatarPicks`]) and a
//! `PickInfoReply` (the full record, [`Event::PickInfo`]), and a `PickDelete`
//! draws a fresh list. The case creates a marker pick and asserts the volunteered
//! detail, sweeps away any marker pick a prior interrupted run left behind (read
//! from that same list), edits the description and confirms the new detail, then
//! deletes it and confirms it left the list — leaving the profile as found. This
//! deliberately avoids the `avatarpicksrequest` list *query* (a `GenericMessage`),
//! which stock OpenSim does not answer for the agent's own empty profile.
//!
//! **Classifieds** get no volunteered reply after an edit, so they are read back
//! with the typed `ClassifiedInfoRequest`. They use a *fixed* id (so re-runs edit
//! one record rather than piling up) and toggle the description across two
//! markers so each edit is a real, detectable change. A classified listing costs
//! L$ on Second Life (this case lists at L$0, which OpenSim accepts and SL does
//! not), so when the created classified never reads back the classified half is
//! recorded `partial` rather than failed. Deleting a classified is best-effort:
//! stock OpenSim's `classified_delete` hits a data-layer error and leaves the
//! record, which the fixed id keeps bounded — the outcome is recorded, not
//! asserted.
//!
//! `1av`, `[both]`. OpenSim needs the UserProfiles module enabled (see the setup
//! memory); the aditi record is batched with the rest of the deferred Aditi runs.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, AvatarPick, ClassifiedCategory, ClassifiedInfo, ClassifiedKey, ClassifiedUpdate,
    Command, Event, PickInfo, PickKey, PickUpdate, Throttle, Uuid,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check_eq, is_aditi, secs_metric};

/// The fixed name both the marker pick and the marker classified are created
/// under, so a leftover entry from an interrupted run is recognisable.
const MARKER_NAME: &str = "sl-conformance picks-classifieds marker";
/// The description written when an entry is first created.
const DESC_CREATED: &str = "sl-conformance created description";
/// The description written by the edit step (distinct from [`DESC_CREATED`] so
/// the edit is a real, detectable change on read-back).
const DESC_EDITED: &str = "sl-conformance edited description";

/// The fixed id the marker classified is created under. Classified deletion is
/// unreliable on stock OpenSim, so reusing one id keeps a leftover bounded to a
/// single record that the next run simply edits in place.
const CLASSIFIED_ID: &str = "5c110029-c1a5-51f1-ed00-000000000001";

/// How long to keep polling a classified read-back for a written change.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(20);
/// How long to wait between classified read-back polls.
const VERIFY_POLL_INTERVAL: Duration = Duration::from_secs(1);
/// A short grace for a volunteered reply (a swept pick's list, a deletion's
/// confirmation) whose absence is tolerated rather than failed.
const SHORT_GRACE: Duration = Duration::from_secs(5);

/// Drives the full create/read/edit/delete round-trip over the agent's own
/// picks and classifieds.
#[derive(Debug)]
pub struct PicksClassifieds;

impl GridTest for PicksClassifieds {
    fn name(&self) -> &'static str {
        "picks-classifieds"
    }

    fn description(&self) -> &'static str {
        "Create, read back, edit, and delete the agent's own picks and classifieds"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let own = session.agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login reported no agent id".to_owned())
            })?;

            // The profile reply messages are Low-priority; a bare login sends no
            // AgentThrottle, so raise one first, as a real viewer does.
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            let pick = run_pick_roundtrip(session, own).await?;

            let classified = run_classified_roundtrip(session, own, grid).await?;
            if !classified.tested {
                ctx.mark_partial(
                    "classified create never read back (the grid likely requires a paid \
                     listing); classifieds round-trip untested",
                );
            }

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("pick_create"), pick.create_rtt.as_secs_f64());
            metrics.set("pick_listed", pick.listed);
            metrics.set("pick_leftovers_swept", pick.leftovers_swept);
            metrics.set("pick_edit_reflected", pick.edit_reflected);
            metrics.set("pick_deleted", pick.deleted);
            metrics.set("classifieds_tested", classified.tested);
            if let Some(create_rtt) = classified.create_rtt {
                metrics.set_timing(&secs_metric("classified_create"), create_rtt.as_secs_f64());
            }
            metrics.set("classified_edit_reflected", classified.edit_reflected);
            metrics.set("classified_deleted", classified.deleted);
            Ok(())
        })
    }
}

/// The outcome of the picks round-trip, for recording.
struct PickOutcome {
    /// The create round-trip time (update to the volunteered detail reply).
    create_rtt: Duration,
    /// Whether the new pick appeared in the list the create update volunteered.
    listed: bool,
    /// How many leftover marker picks an interrupted prior run left behind.
    leftovers_swept: u32,
    /// Whether the edited description was observed on the volunteered detail.
    edit_reflected: bool,
    /// Whether the pick was gone from the list after the delete.
    deleted: bool,
}

/// Creates a marker pick, asserts the volunteered detail, sweeps leftovers, edits
/// it, and deletes it — leaving the profile as it was found.
async fn run_pick_roundtrip(
    session: &mut Session,
    own: AgentKey,
) -> Result<PickOutcome, TestFailure> {
    let pick_id = PickKey::from(Uuid::new_v4());

    // Create the marker pick. The update volunteers both the whole list and this
    // pick's full record; collect both regardless of the order they arrive.
    let started = Instant::now();
    session
        .send(Command::UpdatePick(pick_update(pick_id, DESC_CREATED)))
        .await?;
    let (list, detail) = collect_pick_update_replies(session, own, pick_id, REPLY_TIMEOUT).await?;
    let detail = detail.ok_or_else(|| {
        TestFailure::Assertion("no PickInfoReply volunteered after creating the pick".to_owned())
    })?;
    let create_rtt = started.elapsed();
    check_eq("pick_info pick_id", &detail.pick_id, &pick_id)?;
    check_eq("pick_info creator_id", &detail.creator_id, &own)?;
    check_eq("pick_info name", &detail.name, &MARKER_NAME.to_owned())?;
    check_eq(
        "pick_info description",
        &detail.description,
        &DESC_CREATED.to_owned(),
    )?;

    // Confirm the new pick is in the volunteered list, and note any leftover
    // marker picks a prior interrupted run left behind.
    let mut listed = false;
    let mut leftovers: Vec<PickKey> = Vec::new();
    if let Some(picks) = list {
        listed = picks.iter().any(|pick| pick.pick_id == pick_id);
        leftovers = picks
            .iter()
            .filter(|pick| pick.name == MARKER_NAME && pick.pick_id != pick_id)
            .map(|pick| pick.pick_id)
            .collect();
    }

    // Sweep leftovers (best-effort): delete each and drain its volunteered list.
    let mut leftovers_swept = 0_u32;
    for stale in leftovers {
        session.send(Command::DeletePick(stale)).await?;
        let _drained = wait_for_picks(session, own, SHORT_GRACE).await;
        leftovers_swept = leftovers_swept.saturating_add(1);
    }

    // Edit the description; the update volunteers a fresh PickInfoReply.
    session
        .send(Command::UpdatePick(pick_update(pick_id, DESC_EDITED)))
        .await?;
    let edit_reflected = session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::PickInfo(info) if info.pick_id == pick_id && info.description == DESC_EDITED => {
                Some(())
            }
            _ => None,
        })
        .await
        .is_ok();

    // Delete the pick and confirm it leaves the volunteered list.
    session.send(Command::DeletePick(pick_id)).await?;
    let deleted = poll_pick_absent(session, own, pick_id).await;

    Ok(PickOutcome {
        create_rtt,
        listed,
        leftovers_swept,
        edit_reflected,
        deleted,
    })
}

/// Builds a [`PickUpdate`] for the marker pick with the given description.
fn pick_update(pick_id: PickKey, description: &str) -> PickUpdate {
    PickUpdate {
        pick_id,
        name: MARKER_NAME.to_owned(),
        description: description.to_owned(),
        ..PickUpdate::default()
    }
}

/// One of the two replies the simulator volunteers after a `PickInfoUpdate`.
enum PickReply {
    /// The whole picks list (`AvatarPicksReply`).
    List(Vec<AvatarPick>),
    /// One pick's full record (`PickInfoReply`).
    Detail(Box<PickInfo>),
}

/// Collects both the list and the detail reply the simulator volunteers after a
/// `PickInfoUpdate`, in whichever order they arrive, up to `timeout`. Either may
/// be absent if it does not arrive in time.
async fn collect_pick_update_replies(
    session: &mut Session,
    own: AgentKey,
    pick_id: PickKey,
    timeout: Duration,
) -> Result<(Option<Vec<AvatarPick>>, Option<PickInfo>), TestFailure> {
    let start = Instant::now();
    let mut list = None;
    let mut detail = None;
    while list.is_none() || detail.is_none() {
        let remaining = timeout
            .checked_sub(start.elapsed())
            .unwrap_or(Duration::ZERO);
        if remaining.is_zero() {
            break;
        }
        match session
            .wait_for(remaining, |event| match event {
                Event::AvatarPicks { target_id, picks } if *target_id == own.uuid() => {
                    Some(PickReply::List(picks.clone()))
                }
                Event::PickInfo(info) if info.pick_id == pick_id => {
                    Some(PickReply::Detail(info.clone()))
                }
                _ => None,
            })
            .await
        {
            Ok(PickReply::List(picks)) => list = Some(picks),
            Ok(PickReply::Detail(info)) => detail = Some(*info),
            Err(TestFailure::Timeout(_)) => break,
            Err(other) => return Err(other),
        }
    }
    Ok((list, detail))
}

/// Waits up to `timeout` for the next volunteered picks list for `own`.
async fn wait_for_picks(
    session: &mut Session,
    own: AgentKey,
    timeout: Duration,
) -> Result<Vec<AvatarPick>, TestFailure> {
    session
        .wait_for(timeout, |event| match event {
            Event::AvatarPicks { target_id, picks } if *target_id == own.uuid() => {
                Some(picks.clone())
            }
            _ => None,
        })
        .await
}

/// Polls the volunteered picks lists until one omits `pick_id` (the delete took),
/// or the timeout elapses. A stale list from the preceding edit may still carry
/// the pick, so keep reading until it is gone.
async fn poll_pick_absent(session: &mut Session, own: AgentKey, pick_id: PickKey) -> bool {
    let start = Instant::now();
    loop {
        let remaining = REPLY_TIMEOUT
            .checked_sub(start.elapsed())
            .unwrap_or(Duration::ZERO);
        if remaining.is_zero() {
            return false;
        }
        match wait_for_picks(session, own, remaining).await {
            Ok(picks) if picks.iter().all(|pick| pick.pick_id != pick_id) => return true,
            Ok(_still_present) => {}
            Err(_timeout_or_drop) => return false,
        }
    }
}

/// The outcome of the classifieds round-trip, for recording.
struct ClassifiedOutcome {
    /// Whether the round-trip ran (the grid accepted the L$0 listing).
    tested: bool,
    /// The create round-trip time, present only when the create read back.
    create_rtt: Option<Duration>,
    /// Whether the edited description was observed on read-back.
    edit_reflected: bool,
    /// Whether the classified was gone after the (best-effort) delete.
    deleted: bool,
}

/// Creates a marker classified under a fixed id, reads it back, edits it, and
/// deletes it (best-effort). When the grid declines the L$0 listing (Second Life
/// requires a paid one) the created record never reads back; that is reported as
/// `tested = false` rather than failing, since the picks half stands on its own.
async fn run_classified_roundtrip(
    session: &mut Session,
    own: AgentKey,
    grid: Grid,
) -> Result<ClassifiedOutcome, TestFailure> {
    let classified_id = ClassifiedKey::from(parse_uuid(CLASSIFIED_ID)?);

    // Create (or re-create) the marker classified, then read it back by id.
    let started = Instant::now();
    session
        .send(Command::UpdateClassified(classified_update(
            classified_id,
            DESC_CREATED,
        )))
        .await?;
    let created = poll_classified_until(session, classified_id, |info| {
        info.description == DESC_CREATED
    })
    .await;
    let info = match created {
        Some(info) => info,
        None if is_aditi(grid) => {
            return Ok(ClassifiedOutcome {
                tested: false,
                create_rtt: None,
                edit_reflected: false,
                deleted: false,
            });
        }
        None => {
            return Err(TestFailure::Assertion(
                "created classified never read back".to_owned(),
            ));
        }
    };
    let create_rtt = started.elapsed();
    check_eq(
        "classified_info classified_id",
        &info.classified_id,
        &classified_id,
    )?;
    check_eq("classified_info creator_id", &info.creator_id, &own)?;
    check_eq("classified_info name", &info.name, &MARKER_NAME.to_owned())?;
    check_eq(
        "classified_info category",
        &info.category,
        &ClassifiedCategory::Shopping,
    )?;

    // Edit the description and confirm a fresh read reflects it.
    session
        .send(Command::UpdateClassified(classified_update(
            classified_id,
            DESC_EDITED,
        )))
        .await?;
    let edit_reflected = poll_classified_until(session, classified_id, |info| {
        info.description == DESC_EDITED
    })
    .await
    .is_some();

    // Delete (best-effort): stock OpenSim's classified_delete errors in the data
    // layer and leaves the record, so record the outcome rather than asserting.
    session
        .send(Command::DeleteClassified(classified_id))
        .await?;
    let deleted = classified_gone(session, classified_id).await;

    Ok(ClassifiedOutcome {
        tested: true,
        create_rtt: Some(create_rtt),
        edit_reflected,
        deleted,
    })
}

/// Builds a [`ClassifiedUpdate`] for the marker classified with the given
/// description, listed at L$0 in the Shopping category.
fn classified_update(classified_id: ClassifiedKey, description: &str) -> ClassifiedUpdate {
    ClassifiedUpdate {
        classified_id,
        category: ClassifiedCategory::Shopping,
        name: MARKER_NAME.to_owned(),
        description: description.to_owned(),
        ..ClassifiedUpdate::default()
    }
}

/// Requests one classified's full record and returns it, if the reply arrives.
async fn read_classified(
    session: &mut Session,
    classified_id: ClassifiedKey,
    timeout: Duration,
) -> Option<ClassifiedInfo> {
    if session
        .send(Command::RequestClassifiedInfo(classified_id))
        .await
        .is_err()
    {
        return None;
    }
    session
        .wait_for(timeout, |event| match event {
            Event::ClassifiedInfo(info) if info.classified_id == classified_id => {
                Some((**info).clone())
            }
            _ => None,
        })
        .await
        .ok()
}

/// Re-reads one classified until `predicate` holds, returning it, or `None` after
/// [`VERIFY_TIMEOUT`].
async fn poll_classified_until<P>(
    session: &mut Session,
    classified_id: ClassifiedKey,
    mut predicate: P,
) -> Option<ClassifiedInfo>
where
    P: FnMut(&ClassifiedInfo) -> bool,
{
    let start = Instant::now();
    loop {
        if let Some(info) = read_classified(session, classified_id, REPLY_TIMEOUT).await
            && predicate(&info)
        {
            return Some(info);
        }
        if start.elapsed() >= VERIFY_TIMEOUT {
            return None;
        }
        tokio::time::sleep(VERIFY_POLL_INTERVAL).await;
    }
}

/// Whether the classified is gone: a fresh detail request draws no reply within
/// the grace window (a deleted classified is not answered).
async fn classified_gone(session: &mut Session, classified_id: ClassifiedKey) -> bool {
    read_classified(session, classified_id, SHORT_GRACE)
        .await
        .is_none()
}

/// Parse a well-known UUID literal, failing the test on a malformed value.
fn parse_uuid(literal: &str) -> Result<Uuid, TestFailure> {
    literal
        .parse()
        .map_err(|_invalid| TestFailure::Assertion(format!("bad fixture uuid: {literal}")))
}
