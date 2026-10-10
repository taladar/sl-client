//! What the land has to say about a rez: a parcel that does not let the
//! avatar build, an owner who sends an object back, and a parcel that returns
//! what is left on it.
//!
//! [`super::object_rez_derez`] measures an avatar handling its own objects on
//! land that lets it. This case measures the three ways land gets in between,
//! from the side of the avatar it happens to — **the builder**:
//!
//! 1. **No-build land.** The builder makes an object item where it may build,
//!    then, on land that does not let it, sends an `ObjectAdd` and a
//!    `RezObject` of that item: does anything appear, what is it told, and
//!    does the item survive.
//! 2. **Returned by the owner.** The builder rezzes a cube and the land's
//!    owner derezzes it with `DRD_RETURN_TO_OWNER`: what each of them hears,
//!    and where the builder's item lands.
//! 3. **Auto-return.** The owner sets the parcel to return other people's
//!    objects after a minute; the builder rezzes a cube and waits.
//!
//! Legs 2 and 3, and the way leg 1 comes by its no-build land, need an avatar
//! with rights over the land: the **primary**, with the builder as the
//! **secondary** (on the local OpenSim `--avatar estate-owner --secondary
//! resident`; the builder must hold no estate or land rights, or the land
//! does not refuse it). The owner takes "everyone may build" off the parcel
//! the builder stands on and puts it back. On Second Life we hold no land
//! (`gridspec-aditi-test-land`), so there the case runs with one avatar: the
//! builder looks through its login region for a parcel that is not open to
//! building, flies there, tries, and flies back.
//!
//! What each grid answered is stated in `MEASURED` and written up in
//! `book/src/gridspec/building.md` § Land that says no; every run is held to
//! it. The fake grid refuses a rez as its flavour's grid does and does not
//! return anything (`server-fake-grid-object-return`), so legs 2 and 3 are
//! live only.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    Camera, Command, DeRezDestination, Event, InventoryItem, InventoryKey, Object, ObjectKey,
    OwnerKey, ParcelFlags, ParcelInfo, PrimShape, RegionLocalObjectId, ScopedObjectId,
    ScopedParcelId, TransactionId, Uuid, Vector,
};

use crate::cases::object_rez_derez::{
    Answer, FAKE_QUIET, FAKE_SETTLE_IDLE, Folders, Heard, QUIET, SETTLE_IDLE, SETTLE_WINDOW,
    STEP_TIMEOUT, joined, listen, rez_params, wait_for_notice,
};
use crate::context::{Commander, Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    ANNOUNCED_LEGACY, LONG_TIMEOUT, REGION_TIMEOUT, REPLY_TIMEOUT, is_aditi, is_opensim, is_own,
    secs_metric, settle_scene_with_avatar, wait_for_own_new_object, walk_within_region,
};

/// The OpenSim start location: the "Default Region" (1000,1000), centred.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// How far above the anchor the cubes are rezzed, in metres.
const REZ_LIFT_M: f32 = 1.0;

/// How far apart along the region's x axis the cubes stand, in metres.
const SPACING_M: f32 = 1.5;

/// The name the donor cube is given.
const CUBE_NAME: &str = "SLClientRezLand";

/// How long a parcel edit is given to take effect.
const EDIT_SETTLE: Duration = Duration::from_secs(2);

/// [`EDIT_SETTLE`] on a fake grid.
const FAKE_EDIT_SETTLE: Duration = Duration::from_millis(200);

/// The auto-return time the owner sets: the shortest there is.
const AUTO_RETURN: Duration = Duration::from_secs(60);

/// How long the builder waits for an auto-return.
const AUTO_RETURN_WINDOW: Duration = Duration::from_secs(600);

/// How far apart the points of the search for a no-build parcel are, in
/// metres.
const SCAN_STEP_M: u16 = 32;

/// The side of a region, in metres.
const REGION_SIDE_M: u16 = 256;

/// How long the search waits for the parcels it asked about.
const SCAN_WINDOW: Duration = Duration::from_secs(20);

/// The first sequence id of this case's parcel queries.
const SEQUENCE_BASE: i32 = 6120;

/// How far behind and above a spot the camera looks at it from.
const CAMERA_BACK_M: f32 = 6.0;

/// Where the measurements below are written down.
const SOURCE: &str =
    "book/src/gridspec/building.md § Land that says no (object-rez-land, 2026-10-10)";

/// An answer both grids give.
const fn both(answer: Answer) -> Measured<Answer> {
    Measured {
        second_life: answer,
        opensim: answer,
        source: SOURCE,
    }
}

/// An answer the grids differ in.
const fn each(second_life: Answer, opensim: Answer) -> Measured<Answer> {
    Measured {
        second_life,
        opensim,
        source: SOURCE,
    }
}

/// What each grid was measured to answer on no-build land, by the metric the
/// case records it under. Every run, live or fake, is held to it.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("refused_add_object_appears", both(Answer::Flag(false))),
    (
        "refused_add_alerts",
        each(
            Answer::Text(
                "You cannot create objects here.  The owner of this land does not allow it.  \
                 Use the land tool to see land ownership. [CantCreateObjectParcelPerms]",
            ),
            Answer::Text("You cannot create objects here."),
        ),
    ),
    ("refused_add_messages", both(Answer::Text("none"))),
    ("refused_rez_object_appears", both(Answer::Flag(false))),
    (
        "refused_rez_says",
        each(
            Answer::Text("the owner of this land does not allow it"),
            Answer::Text("nothing"),
        ),
    ),
    ("refused_rez_messages", both(Answer::Text("none"))),
    ("refused_rez_removes_the_item", both(Answer::Flag(false))),
    ("refused_rez_announcement", both(Answer::Text("none"))),
];

/// What OpenSim was measured to answer in the legs that need land of our own,
/// which only it has run.
const MEASURED_ON_OPENSIM: &[(&str, Answer)] = &[
    ("parcel_owner_is_the_builder", Answer::Flag(false)),
    ("owner_return_kills_for_the_owner", Answer::Flag(true)),
    ("owner_return_owner_announcement", Answer::Text("none")),
    ("owner_return_owner_derez_ack", Answer::Text("none")),
    ("owner_return_owner_alerts", Answer::Text("none")),
    ("owner_return_kills_the_object", Answer::Flag(true)),
    ("owner_return_announcement", Answer::Text(ANNOUNCED_LEGACY)),
    ("owner_return_item_lands_in", Answer::Text("lost and found")),
    ("owner_return_told_by", Answer::Text("an instant message")),
    ("owner_return_notice_from", Answer::Text("Server")),
    (
        "owner_return_notice_reason",
        Answer::Text("parcel owner return"),
    ),
    ("auto_return_minutes_set", Answer::Count(1)),
    ("auto_return_kills_the_object", Answer::Flag(true)),
    ("auto_return_announcement", Answer::Text(ANNOUNCED_LEGACY)),
    ("auto_return_item_lands_in", Answer::Text("lost and found")),
    ("auto_return_told_by", Answer::Text("an instant message")),
    ("auto_return_notice_from", Answer::Text("Server")),
    (
        "auto_return_notice_reason",
        Answer::Text("parcel autoreturn"),
    ),
];

/// Holds what the case recorded to [`MEASURED`] — and, on the one grid that
/// ran them, the owner's legs to [`MEASURED_ON_OPENSIM`].
fn hold_to_measured(grid: Grid, metrics: &Metrics) -> Result<(), TestFailure> {
    let owner_legs: &[(&str, Answer)] = if is_opensim(grid) {
        MEASURED_ON_OPENSIM
    } else {
        &[]
    };
    let failures: Vec<String> = MEASURED
        .iter()
        .map(|(key, measured)| (*key, *measured.on(grid)))
        .chain(owner_legs.iter().copied())
        .filter_map(|(key, expected)| match metrics.get(key) {
            Some(value) if expected.is(value) => None,
            Some(value) => Some(format!("{key} was {value:?}, measured {expected:?}")),
            None => Some(format!("{key} was not recorded, measured {expected:?}")),
        })
        .collect();
    if failures.is_empty() {
        return Ok(());
    }
    let verdict = if grid.is_fake() {
        "the fake grid no longer imitates"
    } else {
        "the grid no longer matches"
    };
    Err(TestFailure::Assertion(format!(
        "{grid}: {verdict} {SOURCE}: {}",
        failures.join("; ")
    )))
}

/// What the case left behind and has to put right.
#[derive(Debug, Default)]
struct Residue {
    /// The builder's objects still standing.
    roots: Vec<ScopedObjectId>,
    /// The inventory items the grid announced to the builder.
    items: Vec<InventoryKey>,
}

/// Puts a parcel back as it was found when dropped, for a path that never
/// reaches the awaited restore.
struct RestoreParcel {
    /// The owner's command channel.
    commander: Commander,
    /// The parcel as it was found, and its id on the owner's circuit.
    found: Option<(ScopedParcelId, Box<ParcelInfo>)>,
}

impl RestoreParcel {
    /// The commands that put the parcel back.
    fn commands(parcel: ScopedParcelId, found: &ParcelInfo) -> [Command; 2] {
        [
            Command::UpdateParcel(Box::new(found.to_update())),
            Command::SetParcelOtherCleanTime {
                local_id: parcel,
                clean_time: Duration::from_secs(
                    u64::try_from(found.other_clean_time)
                        .unwrap_or(0)
                        .saturating_mul(60),
                ),
            },
        ]
    }
}

impl Drop for RestoreParcel {
    fn drop(&mut self) {
        if let Some((parcel, found)) = self.found.take() {
            for command in Self::commands(parcel, &found) {
                let _queued = self.commander.try_send(command);
            }
        }
    }
}

/// Measures a rez on land that does not let the avatar build, an owner's
/// return and a parcel's auto-return.
#[derive(Debug)]
pub struct ObjectRezLand;

impl GridTest for ObjectRezLand {
    fn name(&self) -> &'static str {
        "object-rez-land"
    }

    fn description(&self) -> &'static str {
        "What a rez on no-build land is refused with, and what an owner's return and a \
         parcel's auto-return send the builder"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn accounts_on(&self, grid: Grid) -> u8 {
        // We hold no land on Second Life for a second avatar to own.
        if is_aditi(grid) { 1 } else { 2 }
    }

    fn rezzes_objects(&self) -> bool {
        true
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1500)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            // The builder's folders come with its login, before its region.
            let folders = {
                let folders = builder(ctx)
                    .wait_for(REPLY_TIMEOUT, |event| match event {
                        Event::InventorySkeleton(folders) => Some(folders.clone()),
                        _ => None,
                    })
                    .await?;
                Folders::of(&folders).ok_or_else(|| {
                    TestFailure::Assertion("inventory skeleton had no root folder".to_owned())
                })?
            };
            let mut residue = Residue::default();
            let mut metrics = Metrics::new();
            let mut guard = RestoreParcel {
                commander: ctx.primary().commander(),
                found: None,
            };
            let outcome = measure(ctx, folders, &mut residue, &mut guard, &mut metrics).await;
            // The parcel first, whatever became of the legs.
            let restored = match guard.found.take() {
                Some((parcel, found)) => {
                    let mut sent = Ok(());
                    for command in RestoreParcel::commands(parcel, &found) {
                        sent = sent.and(ctx.primary().send(command).await);
                    }
                    sent
                }
                None => Ok(()),
            };
            let held = outcome.and_then(|()| hold_to_measured(grid, &metrics));
            ctx.metrics().merge(metrics);
            let cleanup = clean_up(builder(ctx), &residue, folders).await;
            held.and(restored).and(cleanup)
        })
    }
}

/// The session that builds: the secondary, or the only avatar there is.
const fn builder(ctx: &mut TestContext) -> &mut Session {
    if ctx.secondary().is_some() {
        // Asked twice because a borrow of the `Some` cannot outlive the test
        // for it.
        #[expect(
            clippy::unwrap_used,
            reason = "the line above saw the secondary session"
        )]
        return ctx.secondary().unwrap();
    }
    ctx.primary()
}

/// The durations a run listens for, which a loopback grid shortens.
#[derive(Debug, Clone, Copy)]
struct Pace {
    /// How long a leg goes on listening once it has its answer.
    quiet: Duration,
    /// The idle gap that ends a scene's settle.
    settle_idle: Duration,
    /// How long a parcel edit is given to take effect.
    edit_settle: Duration,
}

impl Pace {
    /// The pace of a run on `grid`.
    const fn of(grid: Grid) -> Self {
        if grid.is_fake() {
            Self {
                quiet: FAKE_QUIET,
                settle_idle: FAKE_SETTLE_IDLE,
                edit_settle: FAKE_EDIT_SETTLE,
            }
        } else {
            Self {
                quiet: QUIET,
                settle_idle: SETTLE_IDLE,
                edit_settle: EDIT_SETTLE,
            }
        }
    }
}

/// What the builder knows of the run.
#[derive(Debug)]
struct Building {
    /// The builder's folders.
    folders: Folders,
    /// The builder.
    agent: Uuid,
    /// How long to listen.
    pace: Pace,
    /// Where the first cube stands.
    position: Vector,
    /// Every object the builder has sighted.
    seen: HashSet<ScopedObjectId>,
}

impl Building {
    /// The spot `slot` places along from the first cube's.
    fn spot(&self, slot: u8) -> Vector {
        Vector {
            x: f32::from(slot).mul_add(SPACING_M, self.position.x),
            y: self.position.y,
            z: self.position.z,
        }
    }

    /// Whether `object` is a root of the builder's that it has not sighted.
    fn is_new_root(&self, object: &Object) -> bool {
        is_own(object, self.agent)
            && object.parent_id == RegionLocalObjectId(0)
            && !self.seen.contains(&object.scoped_id())
    }
}

/// The legs, in order.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of three legs, each a request and a window to hear its answer in"
)]
async fn measure(
    ctx: &mut TestContext,
    folders: Folders,
    residue: &mut Residue,
    guard: &mut RestoreParcel,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let pace = Pace::of(grid);
    let derez_pace = crate::cases::object_rez_derez::Pace::of(grid);
    let build_position = ctx.build_position();
    let owned = ctx.secondary().is_some();
    if owned {
        ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
    }
    let owner_circuit = ctx.primary().circuit_id();

    let session = builder(ctx);
    let agent = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    session.wait_for_region(REGION_TIMEOUT).await?;
    let settled = settle_scene_with_avatar(
        session,
        grid,
        build_position,
        SETTLE_WINDOW,
        pace.settle_idle,
    )
    .await?;
    let stands = settled.avatar.clone().ok_or_else(|| {
        TestFailure::Assertion(
            "the builder's avatar never appeared in the object stream".to_owned(),
        )
    })?;
    // Beside the builder, whose own parcel the owner's edit is about.
    let mut building = Building {
        folders,
        agent,
        pace,
        position: Vector {
            x: stands.x + SPACING_M,
            y: stands.y,
            z: stands.z + REZ_LIFT_M,
        },
        seen: settled.seen,
    };

    // The item a rez from inventory needs, made where building is allowed.
    let donor = rez_cube(session, &mut building, residue, 0).await?;
    session
        .send(Command::SetObjectName {
            local_id: donor.scoped_id(),
            name: CUBE_NAME.to_owned(),
        })
        .await?;
    listen(session, pace.quiet, pace.quiet, |_heard| false).await?;
    let taken = derez(
        session,
        &building,
        donor.scoped_id(),
        DeRezDestination::TakeIntoAgentInventory(folders.objects),
    )
    .await?;
    if taken.killed(donor.scoped_id()) {
        residue.roots.retain(|root| *root != donor.scoped_id());
    }
    residue.items.extend(taken.items().map(|item| item.item_id));
    let item = taken.items().next().cloned().ok_or_else(|| {
        TestFailure::Assertion("a take was answered with no inventory item to rez".to_owned())
    })?;

    // --- 1. No-build land. ---
    let refused_at = if owned {
        // The owner takes "everyone may build" off the parcel under the cubes.
        let owner = ctx.primary();
        let found = read_parcel(owner, &building.position, SEQUENCE_BASE).await?;
        let circuit = owner_circuit
            .ok_or_else(|| TestFailure::Assertion("the owner has no root circuit id".to_owned()))?;
        let parcel = ScopedParcelId::new(circuit, found.local_id);
        metrics.set(
            "parcel_owner_is_the_builder",
            found.owner == OwnerKey::Agent(agent.into()),
        );
        guard.found = Some((parcel, Box::new(found.clone())));
        let mut closed = found.to_update();
        closed.parcel_flags = closed
            .parcel_flags
            .difference(ParcelFlags::CREATE_OBJECTS)
            .difference(ParcelFlags::CREATE_GROUP_OBJECTS);
        owner.send(Command::UpdateParcel(Box::new(closed))).await?;
        tokio::time::sleep(pace.edit_settle).await;
        let now = read_parcel(owner, &building.position, SEQUENCE_BASE.saturating_add(1)).await?;
        if now.flags().contains(ParcelFlags::CREATE_OBJECTS) {
            return Err(TestFailure::Assertion(
                "the owner's edit did not take \"everyone may build\" off the parcel".to_owned(),
            ));
        }
        metrics.set("no_build_land", "the owner's edit");
        Some(building.spot(1))
    } else {
        let session = builder(ctx);
        match find_no_build_parcel(session, agent, &stands).await? {
            Some(target) => {
                metrics.set("no_build_land", "found in the login region");
                let arrived =
                    walk_within_region(session, stands.clone(), &target, |_event| {}).await?;
                look_at(session, &arrived).await?;
                listen(session, pace.quiet, pace.quiet, |_heard| false).await?;
                let under = read_parcel(session, &arrived, SEQUENCE_BASE.saturating_add(2)).await?;
                if under.flags().contains(ParcelFlags::CREATE_OBJECTS) {
                    metrics.set(
                        "no_build_land",
                        "the flight ended on land that lets us build",
                    );
                    None
                } else {
                    Some(arrived)
                }
            }
            None => {
                metrics.set("no_build_land", "none in the login region");
                None
            }
        }
    };
    let Some(refused_at) = refused_at else {
        ctx.mark_partial("found no land that refuses a rez");
        return Ok(());
    };

    let session = builder(ctx);
    session
        .send(Command::RezObject {
            shape: PrimShape::cube(refused_at.clone()),
            group_id: None,
        })
        .await?;
    let heard = listen(session, pace.quiet.saturating_mul(2), pace.quiet, |heard| {
        !heard.alerts.is_empty()
            || heard
                .added
                .iter()
                .any(|object| building.is_new_root(object))
    })
    .await?;
    record_refusal(metrics, "refused_add", &building, residue, &heard);
    building
        .seen
        .extend(heard.added.iter().map(Object::scoped_id));

    session
        .send(Command::RezObjectFromInventory {
            params: Box::new(rez_params(&item, &refused_at)),
        })
        .await?;
    let heard = listen(session, pace.quiet.saturating_mul(2), pace.quiet, |heard| {
        !heard.alerts.is_empty()
            || heard
                .added
                .iter()
                .any(|object| building.is_new_root(object))
    })
    .await?;
    record_refusal(metrics, "refused_rez", &building, residue, &heard);
    // The alert names the object, the spot, the parcel and the region, so it
    // is held by what it says and not letter for letter.
    metrics.set(
        "refused_rez_says",
        match heard.alerts.first() {
            None => "nothing",
            Some(alert)
                if alert.starts_with(&format!("Can't rez object '{}' at ", item.name))
                    && alert.contains("because the owner of this land does not allow it") =>
            {
                "the owner of this land does not allow it"
            }
            Some(_other) => "something else",
        },
    );
    building
        .seen
        .extend(heard.added.iter().map(Object::scoped_id));
    metrics.set(
        "refused_rez_removes_the_item",
        heard.removed_items.contains(&item.item_id),
    );
    metrics.set("refused_rez_announcement", heard.announcement());

    if !owned {
        // Back to where building is allowed, so the next login starts there.
        let session = builder(ctx);
        walk_within_region(session, refused_at, &stands, |_event| {}).await?;
        return Ok(());
    }

    // Building is allowed again for the owner's legs.
    let (parcel, found) = guard
        .found
        .clone()
        .ok_or_else(|| TestFailure::State("the parcel was never read".to_owned()))?;
    ctx.primary()
        .send(Command::UpdateParcel(Box::new(found.to_update())))
        .await?;
    tokio::time::sleep(pace.edit_settle).await;
    if grid.is_fake() {
        return Ok(());
    }

    // --- 2. Returned by the owner. ---
    let session = builder(ctx);
    let theirs = rez_cube(session, &mut building, residue, 2).await?;
    let owner = ctx.primary();
    let seen_by_owner = find_object(owner, theirs.full_id).await?;
    let transaction = TransactionId::from(Uuid::new_v4());
    owner
        .send(Command::DerezObjects {
            local_ids: vec![seen_by_owner],
            destination: DeRezDestination::ReturnToOwner,
            transaction_id: transaction,
            group_id: None,
        })
        .await?;
    let owner_heard = listen(owner, STEP_TIMEOUT, pace.quiet, |heard| {
        heard.killed(seen_by_owner)
    })
    .await?;
    metrics.set(
        "owner_return_kills_for_the_owner",
        owner_heard.killed(seen_by_owner),
    );
    metrics.set(
        "owner_return_owner_announcement",
        owner_heard.announcement(),
    );
    metrics.set(
        "owner_return_owner_derez_ack",
        match owner_heard.derez_acks.first() {
            Some((_transaction, true)) => "success",
            Some((_transaction, false)) => "failure",
            None => "none",
        },
    );
    metrics.set("owner_return_owner_alerts", joined(&owner_heard.alerts));
    let session = builder(ctx);
    let mut heard = listen(session, pace.quiet.saturating_mul(2), pace.quiet, |heard| {
        heard.killed(theirs.scoped_id()) && !heard.announced.is_empty()
    })
    .await?;
    wait_for_notice(session, &mut heard, derez_pace).await?;
    record_return(metrics, "owner_return", &building, residue, &heard, &theirs);

    // --- 3. Auto-return. ---
    ctx.primary()
        .send(Command::SetParcelOtherCleanTime {
            local_id: parcel,
            clean_time: AUTO_RETURN,
        })
        .await?;
    tokio::time::sleep(pace.edit_settle).await;
    let set = read_parcel(
        ctx.primary(),
        &building.position,
        SEQUENCE_BASE.saturating_add(3),
    )
    .await?;
    metrics.set("auto_return_minutes_set", i64::from(set.other_clean_time));
    let session = builder(ctx);
    let left = rez_cube(session, &mut building, residue, 3).await?;
    let mut heard = listen(session, AUTO_RETURN_WINDOW, pace.quiet, |heard| {
        heard.killed(left.scoped_id()) && !heard.announced.is_empty()
    })
    .await?;
    wait_for_notice(session, &mut heard, derez_pace).await?;
    if let Some(after) = heard.killed_after(left.scoped_id()) {
        // The cube stood for the quiet window of its rez before the watch began.
        metrics.set_timing(
            &secs_metric("auto_return_after"),
            after.saturating_add(pace.quiet).as_secs_f64(),
        );
    }
    record_return(metrics, "auto_return", &building, residue, &heard, &left);
    Ok(())
}

/// Records what the builder heard of a rez the land was to refuse.
fn record_refusal(
    metrics: &mut Metrics,
    prefix: &str,
    building: &Building,
    residue: &mut Residue,
    heard: &Heard,
) {
    let appeared: Vec<&Object> = heard
        .added
        .iter()
        .filter(|object| building.is_new_root(object))
        .collect();
    metrics.set(&format!("{prefix}_object_appears"), !appeared.is_empty());
    residue
        .roots
        .extend(appeared.iter().map(|object| object.scoped_id()));
    metrics.set(&format!("{prefix}_alerts"), joined(&heard.alerts));
    metrics.set(&format!("{prefix}_messages"), joined(&heard.messages));
}

/// Records what the builder heard when `object` was sent back to it.
fn record_return(
    metrics: &mut Metrics,
    prefix: &str,
    building: &Building,
    residue: &mut Residue,
    heard: &Heard,
    object: &Object,
) {
    let gone = heard.killed(object.scoped_id());
    metrics.set(&format!("{prefix}_kills_the_object"), gone);
    if gone {
        residue.roots.retain(|root| *root != object.scoped_id());
    }
    metrics.set(&format!("{prefix}_announcement"), heard.announcement());
    metrics.set(
        &format!("{prefix}_item_lands_in"),
        heard
            .items()
            .next()
            .map_or("nowhere", |item| building.folders.word(item.folder_id)),
    );
    residue.items.extend(heard.items().map(|item| item.item_id));
    metrics.set(&format!("{prefix}_alerts"), joined(&heard.alerts));
    metrics.set(&format!("{prefix}_messages"), joined(&heard.messages));
    metrics.set(&format!("{prefix}_told_by"), heard.told_by());
    heard.record_notice(metrics, prefix);
    metrics.set(
        &format!("{prefix}_notice_reason"),
        heard
            .messages
            .first()
            .and_then(|message| message.rsplit_once(" due to "))
            .map_or("none", |(_before, reason)| reason)
            .to_owned(),
    );
}

/// Rezzes a cube with `ObjectAdd` at `slot` and waits for it.
async fn rez_cube(
    session: &mut Session,
    building: &mut Building,
    residue: &mut Residue,
    slot: u8,
) -> Result<Object, TestFailure> {
    session
        .send(Command::RezObject {
            shape: PrimShape::cube(building.spot(slot)),
            group_id: None,
        })
        .await?;
    let cube = wait_for_own_new_object(session, &building.seen, STEP_TIMEOUT)
        .await?
        .map_err(|reason| {
            TestFailure::Assertion(format!("no new object appeared after RezObject: {reason}"))
        })?;
    building.seen.insert(cube.scoped_id());
    residue.roots.push(cube.scoped_id());
    listen(
        session,
        building.pace.quiet,
        building.pace.quiet,
        |_heard| false,
    )
    .await?;
    Ok(cube)
}

/// Derezzes `object` to `destination` and listens until it is gone and an
/// item has been announced.
async fn derez(
    session: &mut Session,
    building: &Building,
    object: ScopedObjectId,
    destination: DeRezDestination,
) -> Result<Heard, TestFailure> {
    session
        .send(Command::DerezObjects {
            local_ids: vec![object],
            destination,
            transaction_id: TransactionId::from(Uuid::new_v4()),
            group_id: None,
        })
        .await?;
    listen(session, STEP_TIMEOUT, building.pace.quiet, |heard| {
        heard.killed(object) && !heard.announced.is_empty()
    })
    .await
}

/// The id `session` knows the object `object_id` by, waiting for the region
/// to send it if it has not yet.
async fn find_object(
    session: &mut Session,
    object_id: ObjectKey,
) -> Result<ScopedObjectId, TestFailure> {
    session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                if object.full_id == object_id =>
            {
                Some(object.scoped_id())
            }
            _ => None,
        })
        .await
        .map_err(|failure| match failure {
            TestFailure::Timeout(_) => TestFailure::Assertion(
                "the owner was never sent the cube the builder rezzed".to_owned(),
            ),
            other => other,
        })
}

/// Reads the parcel under the 4 m square containing `point`.
async fn read_parcel(
    session: &mut Session,
    point: &Vector,
    sequence_id: i32,
) -> Result<ParcelInfo, TestFailure> {
    let west = (point.x / 4.0).floor() * 4.0;
    let south = (point.y / 4.0).floor() * 4.0;
    session
        .send(Command::RequestParcelProperties {
            west,
            south,
            east: west + 4.0,
            north: south + 4.0,
            sequence_id,
            snap_selection: false,
        })
        .await?;
    session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ParcelProperties(parcel) if parcel.sequence_id == sequence_id => {
                Some((**parcel).clone())
            }
            _ => None,
        })
        .await
}

/// Points the camera at `spot`, so that the grid streams what is there.
async fn look_at(session: &Session, spot: &Vector) -> Result<(), TestFailure> {
    session
        .send(Command::SetCamera(Camera::looking_at(
            Vector {
                x: spot.x - CAMERA_BACK_M,
                y: spot.y,
                z: spot.z + CAMERA_BACK_M,
            },
            spot.clone(),
        )))
        .await
}

/// Looks through the region for the nearest parcel that neither lets `agent`
/// build nor keeps avatars out, and returns a point on it.
///
/// A parcel with an access list or an access group is passed over: land that
/// turns an avatar back is [`super::parcel_ban_line`]'s to measure, and some
/// of it sends an intruder home.
async fn find_no_build_parcel(
    session: &mut Session,
    agent: Uuid,
    from: &Vector,
) -> Result<Option<Vector>, TestFailure> {
    let step = usize::from(SCAN_STEP_M);
    let points: Vec<(f32, f32)> = (0..REGION_SIDE_M)
        .step_by(step)
        .flat_map(|x| {
            (0..REGION_SIDE_M).step_by(step).map(move |y| {
                (
                    f32::from(x.saturating_add(SCAN_STEP_M / 2)),
                    f32::from(y.saturating_add(SCAN_STEP_M / 2)),
                )
            })
        })
        .collect();
    let first = SEQUENCE_BASE.saturating_add(100);
    let mut asked: Vec<(i32, (f32, f32))> = Vec::new();
    for (index, (x, y)) in points.iter().enumerate() {
        let sequence_id = first.saturating_add(i32::try_from(index).unwrap_or(0));
        let west = (x / 4.0).floor() * 4.0;
        let south = (y / 4.0).floor() * 4.0;
        session
            .send(Command::RequestParcelProperties {
                west,
                south,
                east: west + 4.0,
                north: south + 4.0,
                sequence_id,
                snap_selection: false,
            })
            .await?;
        asked.push((sequence_id, (*x, *y)));
    }
    let mut closed: Vec<(f32, f32)> = Vec::new();
    let mut answered = 0_usize;
    let started = Instant::now();
    while answered < asked.len() {
        let remaining = SCAN_WINDOW.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        let parcel = match session
            .wait_for(remaining, |event| match event {
                Event::ParcelProperties(parcel)
                    if asked.iter().any(|(id, _point)| *id == parcel.sequence_id) =>
                {
                    Some((**parcel).clone())
                }
                _ => None,
            })
            .await
        {
            Ok(parcel) => parcel,
            Err(TestFailure::Timeout(_)) => break,
            Err(other) => return Err(other),
        };
        answered = answered.saturating_add(1);
        let flags = parcel.flags();
        let open = flags.contains(ParcelFlags::CREATE_OBJECTS)
            || parcel.owner == OwnerKey::Agent(agent.into());
        let keeps_out = flags.contains(ParcelFlags::USE_ACCESS_LIST)
            || flags.contains(ParcelFlags::USE_ACCESS_GROUP);
        if parcel.request_result.has_data() && !open && !keeps_out {
            closed.extend(
                asked
                    .iter()
                    .filter(|(id, _point)| *id == parcel.sequence_id)
                    .map(|(_id, point)| *point),
            );
        }
    }
    let distance = |(x, y): &(f32, f32)| (x - from.x).hypot(y - from.y);
    Ok(closed
        .iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .map(|(x, y)| Vector {
            x: *x,
            y: *y,
            z: from.z,
        }))
}

/// Derezzes whatever the builder left standing to the Trash and removes the
/// items the legs made from its inventory.
async fn clean_up(
    session: &mut Session,
    residue: &Residue,
    folders: Folders,
) -> Result<(), TestFailure> {
    let mut items = residue.items.clone();
    let outcome = if residue.roots.is_empty() {
        Ok(())
    } else {
        session
            .send(Command::DerezObjects {
                local_ids: residue.roots.clone(),
                destination: DeRezDestination::Trash(folders.trash),
                transaction_id: TransactionId::from(Uuid::new_v4()),
                group_id: None,
            })
            .await?;
        let pending: HashSet<ScopedObjectId> = residue.roots.iter().copied().collect();
        let heard = listen(session, STEP_TIMEOUT, Duration::from_secs(1), |heard| {
            pending.iter().all(|root| heard.killed(*root))
        })
        .await?;
        items.extend(heard.items().map(|item: &InventoryItem| item.item_id));
        let left = pending.iter().filter(|root| !heard.killed(**root)).count();
        if left == 0 {
            Ok(())
        } else {
            Err(TestFailure::Assertion(format!(
                "{left} object(s) were never removed on cleanup"
            )))
        }
    };
    if !items.is_empty() {
        session.send(Command::RemoveInventoryItems(items)).await?;
    }
    outcome
}
