//! A census of what a simulator says about an object's *administrative* facts
//! — creator, owner, group, permissions, sale state, name, description — and
//! of who it says it to.
//!
//! Those facts travel in none of the object-update forms. A viewer learns them
//! two ways:
//!
//! - **`ObjectProperties`**, the answer to selecting an object
//!   (`ObjectSelect`, [`Command::RequestObjectProperties`]) — what the build
//!   floater shows;
//! - **`ObjectPropertiesFamily`**, the answer to a selection-free
//!   [`Command::RequestObjectPropertiesFamily`] — the condensed record behind
//!   the hover tip and the pay and report dialogs.
//!
//! The case rezzes a cube as the primary avatar and reads both, then asks
//! every question a build floater's behaviour rests on, each in a leg of its
//! own with a quiet window to hear the answer (or the silence) in:
//!
//! 1. **Our own object.** Every field of its record; what else a select
//!    brings (the object again, its physics record); whether a second select
//!    of something already selected is answered; a local id the region does
//!    not have.
//! 2. **Somebody else's object.** The secondary avatar selects the same cube.
//! 3. **The family record**, with and without request flags, of an object and
//!    of an id nothing has.
//! 4. **Who is told of a change.** Three times over — with both avatars
//!    holding the cube selected, with only the primary, and with nobody — the
//!    primary drops an inventory item into it, renames it, re-describes it,
//!    prices it and re-permissions it. After each the case notes whether the
//!    primary and the other avatar were sent the new record. The contents
//!    serial is a field of the record, and the only word a viewer gets that
//!    its listing of a prim's contents is stale.
//! 5. **A linkset.** A second cube linked under the first: what a select of
//!    the root alone answers, and what a select and a family request of the
//!    child do.
//!
//! What each live grid answered is stated as a [`Measured`] beside the leg
//! that observes it and written up in `book/src/gridspec/objects.md`
//! § Properties; every run, live or fake, is held to it. Objects the case
//! did not rez — a stranger's, a neighbouring region's, an avatar — are
//! [`super::object_select_scene`]'s.
//!
//! `2av`, `[both]`, offline on both fake flavours. The cubes are derezzed to
//! the Trash at the end, also when a leg fails.

use std::collections::HashSet;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sl_client_tokio::{
    AgentKey, AssetType, Command, DeRezDestination, Event, FolderType, InventoryFolder,
    InventoryFolderKey, LindenAmount, Object, ObjectKey, ObjectProperties as Properties,
    ObjectPropertiesFamily, ObjectUpdateForm, OwnerKey, PermissionField, Permissions, PrimShape,
    RegionLocalObjectId, SaleType, ScopedObjectId, TaskInventoryKey, TransactionId, Uuid, Vector,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::record::MetricValue;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    REGION_TIMEOUT, REPLY_TIMEOUT, check_eq, created_item_announcement, is_opensim, secs_metric,
    settle_scene_with_avatar, wait_for_own_new_object,
};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, where
/// this workspace's test object stands as the rez placement reference.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The overall budget for settling a session's initial scene.
const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// The idle gap that ends the settle.
const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// How long to wait for a step's confirming event.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a leg listens for what a request brings — or for the silence that
/// is its answer. Every reply measured on either grid arrived within a
/// second.
const QUIET: Duration = Duration::from_secs(4);

/// How long the session that was *not* waited on is then read for: its events
/// were queued the whole time, so this only has to empty the queue.
const DRAIN: Duration = Duration::from_millis(500);

/// [`QUIET`] on a fake grid.
const FAKE_QUIET: Duration = Duration::from_millis(400);

/// [`DRAIN`] on a fake grid.
const FAKE_DRAIN: Duration = Duration::from_millis(100);

/// [`SETTLE_IDLE`] on a fake grid.
const FAKE_SETTLE_IDLE: Duration = Duration::from_secs(1);

/// How far above the anchor the first cube is rezzed, in metres.
const REZ_LIFT_M: f32 = 1.0;

/// How far beside the first cube the second is rezzed, in metres: near enough
/// to link.
const CHILD_OFFSET_M: f32 = 1.5;

/// The name the child cube is given before it is linked, so a reply can be
/// told from the root's.
const CHILD_NAME: &str = "SLClientPropsChild";

/// `OBJECT_PAY_REQUEST`: the request flag the pay dialog sets on its family
/// request, which the reply echoes.
const OBJECT_PAY_REQUEST: u32 = 0x04;

/// The start of 2002, in seconds since the Unix epoch: no object of either
/// grid is older.
const FIRST_OBJECT_EPOCH: u64 = 1_009_843_200;

/// A local id no region hands out in a session's lifetime.
const UNKNOWN_LOCAL_ID: u32 = 0x7FFF_FFF0;

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/objects.md § Properties (object-properties, 2026-10-09)";

/// One measured answer: what a metric the census recorded reads on each grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    /// A text metric.
    Text(&'static str),
    /// A flag.
    Flag(bool),
    /// A count.
    Count(i64),
}

impl Answer {
    /// Whether the recorded `value` is this answer.
    fn is(self, value: &MetricValue) -> bool {
        match (self, value) {
            (Self::Text(expected), MetricValue::Text(actual)) => expected == actual,
            (Self::Flag(expected), MetricValue::Bool(actual)) => expected == *actual,
            (Self::Count(expected), MetricValue::Int(actual)) => expected == *actual,
            _ => false,
        }
    }
}

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

/// Every edit of a round, as [`joined`] writes them.
const EVERY_EDIT: Answer = Answer::Text("name+description+sale+permissions");

/// What both grids' selects bring: the record, and the physics record over
/// the event queue.
const RECORD_AND_PHYSICS: Answer = Answer::Text("properties+physics");

/// What each grid was measured to answer, by the metric the census records it
/// under. Every run — live or fake — is held to all of it at the end, so a
/// run that disagrees says everything it disagrees about at once.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    // A select.
    ("own_select_brings", both(RECORD_AND_PHYSICS)),
    ("own_select_records", both(Answer::Count(1))),
    ("reselect_while_selected_brings", both(RECORD_AND_PHYSICS)),
    ("select_after_deselect_brings", both(RECORD_AND_PHYSICS)),
    (
        "deselect_brings",
        each(Answer::Text("terse update"), Answer::Text("nothing")),
    ),
    ("unknown_select_records", both(Answer::Count(0))),
    (
        "other_select_brings",
        each(
            RECORD_AND_PHYSICS,
            Answer::Text("properties+full update+physics"),
        ),
    ),
    ("other_sees_the_same_record", both(Answer::Flag(true))),
    // The record of a prim nobody has edited.
    ("creation_date_unit", both(Answer::Text("microseconds"))),
    ("name", both(Answer::Text("Object"))),
    ("description", both(Answer::Text(""))),
    ("touch_name", both(Answer::Text(""))),
    ("sit_name", both(Answer::Text(""))),
    ("creator_is_the_rezzer", both(Answer::Flag(true))),
    ("owner_is_the_rezzer", both(Answer::Flag(true))),
    (
        "last_owner",
        each(Answer::Text("nil"), Answer::Text("the rezzer")),
    ),
    ("group_set", both(Answer::Flag(false))),
    (
        "mask_base",
        each(Answer::Text("0x7fffffff"), Answer::Text("0x0009e000")),
    ),
    (
        "mask_owner",
        each(Answer::Text("0x7fffffff"), Answer::Text("0x0009e000")),
    ),
    ("mask_group", both(Answer::Text("0x00000000"))),
    ("mask_everyone", both(Answer::Text("0x00000000"))),
    ("mask_next_owner", both(Answer::Text("0x00082000"))),
    ("ownership_cost", each(Answer::Count(10), Answer::Count(0))),
    ("sale_type", both(Answer::Count(0))),
    ("category", both(Answer::Count(0))),
    ("inventory_serial", both(Answer::Count(0))),
    ("item_id_nil", both(Answer::Flag(true))),
    ("folder_id_set", both(Answer::Flag(false))),
    ("from_task_id_set", both(Answer::Flag(false))),
    ("aggregate_perms", both(Answer::Text("0x00/0x00/0x00"))),
    // The family record.
    ("family_records", both(Answer::Count(1))),
    ("family_pay_records", both(Answer::Count(1))),
    ("family_agrees_with_properties", both(Answer::Flag(true))),
    ("other_family_records", both(Answer::Count(1))),
    ("unknown_family_records", both(Answer::Count(0))),
    // Who is told of a write into the contents.
    (
        "contents_write_with_both_selecting_told_writer",
        both(Answer::Flag(true)),
    ),
    (
        "contents_write_with_both_selecting_told_other",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "contents_write_with_editor_only_selecting_told_writer",
        both(Answer::Flag(true)),
    ),
    (
        "contents_write_with_editor_only_selecting_told_other",
        both(Answer::Flag(false)),
    ),
    (
        "contents_write_with_nobody_selecting_told_writer",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    (
        "contents_write_with_nobody_selecting_told_other",
        both(Answer::Flag(false)),
    ),
    // Who is told of an edit of the record.
    (
        "edit_with_both_selecting_told_editor",
        each(EVERY_EDIT, Answer::Text("sale+permissions")),
    ),
    (
        "edit_with_editor_only_selecting_told_editor",
        each(EVERY_EDIT, Answer::Text("sale+permissions")),
    ),
    (
        "edit_with_nobody_selecting_told_editor",
        each(EVERY_EDIT, Answer::Text("sale+permissions")),
    ),
    (
        "edit_with_both_selecting_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "edit_with_editor_only_selecting_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "edit_with_nobody_selecting_told_other",
        both(Answer::Text("nothing")),
    ),
    ("edits_with_nothing_selected_took", both(EVERY_EDIT)),
    // A linkset.
    (
        "link_brings",
        each(Answer::Text("child"), Answer::Text("root")),
    ),
    ("root_select_answers", both(Answer::Text("root"))),
    ("child_select_answers", both(Answer::Text("child"))),
    ("child_record_name_is_its_own", both(Answer::Flag(true))),
    ("child_record_masks_are_the_roots", both(Answer::Flag(true))),
    (
        "child_record_sale_is_the_roots",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    ("child_family_answers", both(Answer::Text("the root"))),
];

/// Holds what the census recorded to [`MEASURED`], naming every metric that
/// reads otherwise — or was never recorded.
fn hold_to_measured(grid: Grid, metrics: &Metrics) -> Result<(), TestFailure> {
    let failures: Vec<String> = MEASURED
        .iter()
        .filter_map(|(key, measured)| {
            let expected = measured.on(grid);
            match metrics.get(key) {
                Some(value) if expected.is(value) => None,
                Some(value) => Some(format!("{key} was {value:?}, measured {expected:?}")),
                None => Some(format!("{key} was not recorded, measured {expected:?}")),
            }
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

/// The four edits of the properties record the push leg makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Edit {
    /// `ObjectName`.
    Name,
    /// `ObjectDescription`.
    Description,
    /// `ObjectSaleInfo`.
    Sale,
    /// `ObjectPermissions`, the next owner's copy bit.
    Permissions,
}

impl Edit {
    /// Every edit, in the order a round makes them.
    const ALL: [Self; 4] = [Self::Name, Self::Description, Self::Sale, Self::Permissions];

    /// The word a metric names the edit by.
    const fn word(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Description => "description",
            Self::Sale => "sale",
            Self::Permissions => "permissions",
        }
    }
}

/// What one round of the push leg expects the record to read once its edits
/// are in.
#[derive(Debug, Clone)]
struct Round {
    /// The name the round sets.
    name: String,
    /// The description the round sets.
    description: String,
    /// The price the round asks.
    price: u64,
    /// Whether the next owner may copy once the round's permission edit is
    /// in.
    next_owner_copy: bool,
}

impl Round {
    /// The values round `index` sets, the copy bit flipped from `had_copy`.
    fn new(index: u8, had_copy: bool) -> Self {
        Self {
            name: format!("SLClientProps{index}"),
            description: format!("object-properties round {index}"),
            price: 10_u64.saturating_add(u64::from(index)),
            next_owner_copy: !had_copy,
        }
    }

    /// The command that makes `edit`.
    fn command(&self, edit: Edit, local_id: ScopedObjectId) -> Command {
        match edit {
            Edit::Name => Command::SetObjectName {
                local_id,
                name: self.name.clone(),
            },
            Edit::Description => Command::SetObjectDescription {
                local_id,
                description: self.description.clone(),
            },
            Edit::Sale => Command::SetObjectForSale {
                local_id,
                sale_type: SaleType::Copy,
                sale_price: Some(LindenAmount(self.price)),
            },
            Edit::Permissions => Command::SetObjectPermissions {
                local_ids: vec![local_id],
                field: PermissionField::NextOwner,
                set: self.next_owner_copy,
                mask: Permissions::COPY,
            },
        }
    }

    /// Whether `properties` reads as it does once `edit` is in.
    fn carried_by(&self, edit: Edit, properties: &Properties) -> bool {
        match edit {
            Edit::Name => properties.name == self.name,
            Edit::Description => properties.description == self.description,
            Edit::Sale => properties.sale_price == Some(LindenAmount(self.price)),
            Edit::Permissions => {
                properties
                    .permissions
                    .next_owner
                    .contains(Permissions::COPY)
                    == self.next_owner_copy
            }
        }
    }
}

/// What one session was sent during a window.
#[derive(Debug, Default)]
struct Heard {
    /// Every `ObjectProperties` record, in arrival order.
    properties: Vec<Properties>,
    /// How long after the window opened the first record came.
    first_after: Option<Duration>,
    /// Every `ObjectPropertiesFamily` record.
    families: Vec<ObjectPropertiesFamily>,
    /// The objects an `ObjectPhysicsProperties` event named.
    physics: Vec<ScopedObjectId>,
    /// The objects of the agent's region that were sent again, and the form
    /// each came in.
    updated: Vec<(RegionLocalObjectId, ObjectUpdateForm)>,
}

impl Heard {
    /// The records about `object`.
    fn about(&self, object: ObjectKey) -> impl Iterator<Item = &Properties> {
        self.properties
            .iter()
            .filter(move |properties| properties.object_id == object)
    }

    /// The last record about `object`.
    fn last_about(&self, object: ObjectKey) -> Option<&Properties> {
        self.about(object).last()
    }

    /// What the window held about one object, as the `+`-joined list a metric
    /// records: `properties`, the form of an update, `physics`, or `nothing`.
    fn summary(&self, object: ObjectKey, local_id: ScopedObjectId) -> String {
        let mut parts = Vec::new();
        if self.about(object).next().is_some() {
            parts.push("properties");
        }
        for (form, word) in [
            (ObjectUpdateForm::Full, "full update"),
            (ObjectUpdateForm::Compressed, "compressed update"),
            (ObjectUpdateForm::Terse, "terse update"),
        ] {
            if self.updated.contains(&(local_id.id(), form)) {
                parts.push(word);
            }
        }
        if self.physics.contains(&local_id) {
            parts.push("physics");
        }
        if parts.is_empty() {
            "nothing".to_owned()
        } else {
            parts.join("+")
        }
    }
}

/// Listens on `session` for `window`, keeping everything it is sent about
/// objects' properties.
async fn listen(session: &mut Session, window: Duration) -> Result<Heard, TestFailure> {
    let mut heard = Heard::default();
    let started = Instant::now();
    let outcome = session
        .wait_for(window, |event| {
            match event {
                Event::ObjectProperties(properties) => {
                    if heard.first_after.is_none() {
                        heard.first_after = Some(started.elapsed());
                    }
                    heard.properties.push((**properties).clone());
                }
                Event::ObjectPropertiesFamily { properties } => {
                    heard.families.push(properties.clone());
                }
                Event::ObjectPhysicsProperties(entries) => {
                    heard
                        .physics
                        .extend(entries.iter().map(|(local_id, _data)| *local_id));
                }
                Event::ObjectStreamBatch(batch) if !batch.child => {
                    heard.updated.extend(
                        batch
                            .entries
                            .iter()
                            .filter(|entry| entry.known)
                            .map(|entry| (entry.local_id, batch.form)),
                    );
                }
                _ => {}
            }
            None::<()>
        })
        .await;
    match outcome {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(heard),
        Err(other) => Err(other),
    }
}

/// The objects the case rezzed and has to take away again.
#[derive(Debug, Default)]
struct Rezzed {
    /// The roots still standing, by the primary's ids.
    roots: Vec<ScopedObjectId>,
}

/// A census of `ObjectProperties` and `ObjectPropertiesFamily`: what each
/// carries, what a select brings, and who is told of an edit.
#[derive(Debug)]
pub struct ObjectProperties;

impl GridTest for ObjectProperties {
    fn name(&self) -> &'static str {
        "object-properties"
    }

    fn description(&self) -> &'static str {
        "What a select and a family request answer, and who is sent an edited object's record"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn accounts(&self) -> u8 {
        2
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

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // The Trash folder comes with the login, before the region does.
            let folders = {
                let folders = ctx
                    .primary()
                    .wait_for(REPLY_TIMEOUT, |event| match event {
                        Event::InventorySkeleton(folders) => Some(folders.clone()),
                        _ => None,
                    })
                    .await?;
                Folders::of(&folders).ok_or_else(|| {
                    TestFailure::Assertion("inventory skeleton had no root folder".to_owned())
                })?
            };
            let mut rezzed = Rezzed::default();
            let mut metrics = Metrics::new();
            let outcome = census(ctx, folders, &mut rezzed, &mut metrics).await;
            ctx.metrics().merge(metrics);
            // Take the cubes away whatever became of the census.
            let cleanup = clean_up(ctx.primary(), &rezzed, folders.trash).await;
            outcome.and(cleanup)
        })
    }
}

/// The legs, in order. Everything rezzed is noted in `rezzed` as it appears,
/// so the caller can take it away when a leg fails.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of five legs, each a request and a window to hear its answer in"
)]
async fn census(
    ctx: &mut TestContext,
    folders: Folders,
    rezzed: &mut Rezzed,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let Some((primary, secondary)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "object-properties needs a secondary avatar".to_owned(),
        ));
    };
    let owner = agent_of(primary)?;
    // A loopback grid answers in well under a millisecond, and the case runs
    // against one on every `cargo test`.
    let (quiet, drain, settle_idle) = if grid.is_fake() {
        (FAKE_QUIET, FAKE_DRAIN, FAKE_SETTLE_IDLE)
    } else {
        (QUIET, DRAIN, SETTLE_IDLE)
    };

    // Both avatars to the spot, each with its scene settled.
    primary.wait_for_region(REGION_TIMEOUT).await?;
    let settled = settle_scene_with_avatar(
        primary,
        grid,
        build_position.clone(),
        SETTLE_WINDOW,
        settle_idle,
    )
    .await?;
    secondary.wait_for_region(REGION_TIMEOUT).await?;
    settle_scene_with_avatar(secondary, grid, build_position, SETTLE_WINDOW, settle_idle).await?;
    let anchor = settled.anchor.clone().ok_or_else(|| {
        TestFailure::Assertion("the arrival streamed nothing to place a cube against".to_owned())
    })?;
    let root_circuit = primary
        .circuit_id()
        .ok_or_else(|| TestFailure::State("the primary session has no root circuit".to_owned()))?;

    let mut seen = settled.seen.clone();

    // --- 1. Our own object. ---
    let position = Vector {
        x: anchor.x,
        y: anchor.y,
        z: anchor.z + REZ_LIFT_M,
    };
    let cube = rez_cube(primary, &seen, position.clone()).await?;
    let scoped = cube.scoped_id();
    let object_id = cube.full_id;
    rezzed.roots.push(scoped);
    seen.insert(scoped);
    metrics.set("object_id", object_id.to_string());
    // A rez is followed by more updates of the new object; they are not a
    // select's doing.
    listen(primary, quiet).await?;

    primary
        .send(Command::RequestObjectProperties {
            local_ids: vec![scoped],
        })
        .await?;
    let heard = listen(primary, quiet).await?;
    let own_select = heard.summary(object_id, scoped);
    metrics.set("own_select_brings", own_select.clone());
    metrics.set(
        "own_select_records",
        i64::try_from(heard.about(object_id).count()).unwrap_or(-1),
    );
    let baseline = heard.last_about(object_id).cloned().ok_or_else(|| {
        TestFailure::Assertion(format!(
            "a select of our own cube brought {own_select}, not its properties"
        ))
    })?;
    if let Some(after) = heard.first_after {
        metrics.set_timing(&secs_metric("properties_rtt"), after.as_secs_f64());
    }
    record_properties(metrics, &baseline, owner)?;

    // Selected again while it is selected.
    primary
        .send(Command::RequestObjectProperties {
            local_ids: vec![scoped],
        })
        .await?;
    let again = listen(primary, quiet).await?;
    metrics.set(
        "reselect_while_selected_brings",
        again.summary(object_id, scoped),
    );

    // Deselected and selected.
    primary
        .send(Command::DeselectObjects {
            local_ids: vec![scoped],
        })
        .await?;
    let on_deselect = listen(primary, quiet).await?;
    metrics.set("deselect_brings", on_deselect.summary(object_id, scoped));
    primary
        .send(Command::RequestObjectProperties {
            local_ids: vec![scoped],
        })
        .await?;
    let reselect = listen(primary, quiet).await?;
    metrics.set(
        "select_after_deselect_brings",
        reselect.summary(object_id, scoped),
    );

    // A local id the region does not have.
    let unknown = ScopedObjectId::new(root_circuit, RegionLocalObjectId(UNKNOWN_LOCAL_ID));
    primary
        .send(Command::RequestObjectProperties {
            local_ids: vec![unknown],
        })
        .await?;
    let of_unknown = listen(primary, quiet).await?;
    metrics.set(
        "unknown_select_records",
        i64::try_from(of_unknown.properties.len()).unwrap_or(-1),
    );
    primary
        .send(Command::DeselectObjects {
            local_ids: vec![unknown],
        })
        .await?;

    // --- 2. Somebody else's object: the secondary selects the cube. ---
    let theirs = find_object(secondary, object_id).await?;
    listen(secondary, drain).await?;
    secondary
        .send(Command::RequestObjectProperties {
            local_ids: vec![theirs],
        })
        .await?;
    let other = listen(secondary, quiet).await?;
    let other_select = other.summary(object_id, theirs);
    metrics.set("other_select_brings", other_select.clone());
    let seen_by_other = other.last_about(object_id).cloned().ok_or_else(|| {
        TestFailure::Assertion(format!(
            "another avatar's select of the cube brought {other_select}, not its properties"
        ))
    })?;
    metrics.set("other_sees_the_same_record", seen_by_other == baseline);

    // --- 3. The family record. ---
    for (what, flags) in [("family", 0), ("family_pay", OBJECT_PAY_REQUEST)] {
        primary
            .send(Command::RequestObjectPropertiesFamily {
                request_flags: flags,
                object_id,
            })
            .await?;
        let heard = listen(primary, quiet).await?;
        let family = heard
            .families
            .iter()
            .find(|family| family.object_id == object_id)
            .ok_or_else(|| {
                TestFailure::Assertion(format!(
                    "a family request with flags {flags:#x} of our own cube went unanswered"
                ))
            })?;
        check_eq(
            &format!("{what} request flags echoed"),
            &family.request_flags,
            &flags,
        )?;
        metrics.set(
            &format!("{what}_records"),
            i64::try_from(heard.families.len()).unwrap_or(-1),
        );
        if flags == 0 {
            metrics.set(
                "family_agrees_with_properties",
                family_agrees(family, &baseline),
            );
            metrics.set(
                "family_ownership_cost",
                i64::try_from(family.ownership_cost.0).unwrap_or(-1),
            );
        }
    }
    secondary
        .send(Command::RequestObjectPropertiesFamily {
            request_flags: 0,
            object_id,
        })
        .await?;
    let other_family = listen(secondary, quiet).await?;
    metrics.set(
        "other_family_records",
        i64::try_from(other_family.families.len()).unwrap_or(-1),
    );
    primary
        .send(Command::RequestObjectPropertiesFamily {
            request_flags: 0,
            object_id: ObjectKey::from(Uuid::from_u128(0x000B_1EC7_0000_DEAD)),
        })
        .await?;
    let of_nothing = listen(primary, quiet).await?;
    metrics.set(
        "unknown_family_records",
        i64::try_from(of_nothing.families.len()).unwrap_or(-1),
    );

    // --- 4. Who is told of a change. ---
    // A donor: a second cube, taken into the inventory, is the item dropped.
    let donor = rez_cube(
        primary,
        &seen,
        Vector {
            x: position.x,
            y: position.y + CHILD_OFFSET_M,
            z: position.z,
        },
    )
    .await?;
    let donor_scoped = donor.scoped_id();
    rezzed.roots.push(donor_scoped);
    seen.insert(donor_scoped);
    primary
        .send(Command::DerezObjects {
            local_ids: vec![donor_scoped],
            destination: DeRezDestination::TakeIntoAgentInventory(folders.objects),
            transaction_id: TransactionId::from(Uuid::new_v4()),
            group_id: None,
        })
        .await?;
    let (_announcement, item) =
        created_item_announcement(primary, STEP_TIMEOUT, AssetType::Object.to_code()).await?;
    rezzed.roots.retain(|root| *root != donor_scoped);
    listen(primary, drain).await?;
    listen(secondary, drain).await?;

    // Round 1: both hold it selected. Round 2: the editor alone. Round 3:
    // nobody. The primary is still selected from the leg above, and so is the
    // secondary.
    let mut had_copy = baseline.permissions.next_owner.contains(Permissions::COPY);
    let mut last_round = None;
    let mut contents_serial = baseline.inventory_serial;
    for (index, what) in [(1_u8, "both"), (2, "editor_only"), (3, "nobody")] {
        match index {
            2 => {
                secondary
                    .send(Command::DeselectObjects {
                        local_ids: vec![theirs],
                    })
                    .await?;
            }
            3 => {
                primary
                    .send(Command::DeselectObjects {
                        local_ids: vec![scoped],
                    })
                    .await?;
            }
            _ => {}
        }
        // Whatever the deselect itself brought is not an edit's doing.
        listen(primary, drain).await?;
        listen(secondary, drain).await?;
        // A write into the contents first: the same item, dropped again.
        primary
            .send(Command::UpdateTaskInventory {
                target: scoped,
                key: TaskInventoryKey::Item,
                item: Box::new(super::task_inventory::task_item(&item)),
            })
            .await?;
        let to_writer = listen(primary, quiet).await?;
        let to_other = listen(secondary, drain).await?;
        let advanced = |heard: &Heard| {
            heard
                .about(object_id)
                .map(|properties| properties.inventory_serial)
                .max()
                .filter(|serial| *serial > contents_serial)
        };
        metrics.set(
            &format!("contents_write_with_{what}_selecting_told_writer"),
            advanced(&to_writer).is_some(),
        );
        metrics.set(
            &format!("contents_write_with_{what}_selecting_told_other"),
            advanced(&to_other).is_some(),
        );
        if let Some(serial) = advanced(&to_writer) {
            contents_serial = serial;
        }
        let round = Round::new(index, had_copy);
        had_copy = round.next_owner_copy;
        let mut editor_told = Vec::new();
        let mut other_told = Vec::new();
        for edit in Edit::ALL {
            primary.send(round.command(edit, scoped)).await?;
            let to_editor = listen(primary, quiet).await?;
            let to_other = listen(secondary, drain).await?;
            if to_editor
                .about(object_id)
                .any(|properties| round.carried_by(edit, properties))
            {
                editor_told.push(edit.word());
            }
            if to_other
                .about(object_id)
                .any(|properties| round.carried_by(edit, properties))
            {
                other_told.push(edit.word());
            }
        }
        metrics.set(
            &format!("edit_with_{what}_selecting_told_editor"),
            joined(&editor_told),
        );
        metrics.set(
            &format!("edit_with_{what}_selecting_told_other"),
            joined(&other_told),
        );
        last_round = Some(round);
    }

    metrics.set(
        "contents_serial_after_three_writes",
        i64::from(contents_serial),
    );
    // What the record reads once all three rounds are in: an edit made with
    // nothing selected took as well, or it did not.
    primary
        .send(Command::RequestObjectProperties {
            local_ids: vec![scoped],
        })
        .await?;
    let edited = listen(primary, quiet)
        .await?
        .last_about(object_id)
        .cloned()
        .ok_or_else(|| {
            TestFailure::Assertion("the edited cube's properties were not read back".to_owned())
        })?;
    if let Some(round) = &last_round {
        let took: Vec<&str> = Edit::ALL
            .into_iter()
            .filter(|edit| round.carried_by(*edit, &edited))
            .map(Edit::word)
            .collect();
        metrics.set("edits_with_nothing_selected_took", joined(&took));
    }
    primary
        .send(Command::DeselectObjects {
            local_ids: vec![scoped],
        })
        .await?;

    // --- 5. A linkset. ---
    let child = rez_cube(
        primary,
        &seen,
        Vector {
            x: position.x + CHILD_OFFSET_M,
            y: position.y,
            z: position.z,
        },
    )
    .await?;
    let child_scoped = child.scoped_id();
    let child_id = child.full_id;
    rezzed.roots.push(child_scoped);
    primary
        .send(Command::SetObjectName {
            local_id: child_scoped,
            name: CHILD_NAME.to_owned(),
        })
        .await?;
    primary
        .send(Command::LinkObjects {
            local_ids: vec![scoped, child_scoped],
        })
        .await?;
    primary
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::ObjectUpdated(object)
                if object.scoped_id() == child_scoped
                    && object.parent_id != RegionLocalObjectId(0)
                    && object.scoped_parent_id() == scoped =>
            {
                Some(())
            }
            _ => None,
        })
        .await
        .map_err(|failure| match failure {
            TestFailure::Timeout(_) => {
                TestFailure::Assertion("the second cube never came under the first".to_owned())
            }
            other => other,
        })?;
    rezzed.roots.retain(|root| *root != child_scoped);
    let on_link = listen(primary, quiet).await?;
    metrics.set("link_brings", linkset_answer(&on_link, object_id, child_id));

    primary
        .send(Command::RequestObjectProperties {
            local_ids: vec![scoped],
        })
        .await?;
    let of_root = listen(primary, quiet).await?;
    metrics.set(
        "root_select_answers",
        linkset_answer(&of_root, object_id, child_id),
    );
    primary
        .send(Command::DeselectObjects {
            local_ids: vec![scoped],
        })
        .await?;
    listen(primary, drain).await?;

    primary
        .send(Command::RequestObjectProperties {
            local_ids: vec![child_scoped],
        })
        .await?;
    let of_child = listen(primary, quiet).await?;
    metrics.set(
        "child_select_answers",
        linkset_answer(&of_child, object_id, child_id),
    );
    if let Some(record) = of_child.last_about(child_id) {
        metrics.set("child_record_name_is_its_own", record.name == CHILD_NAME);
        metrics.set(
            "child_record_masks_are_the_roots",
            record.permissions == edited.permissions,
        );
        metrics.set(
            "child_record_sale_is_the_roots",
            record.sale_price == edited.sale_price,
        );
    }
    primary
        .send(Command::DeselectObjects {
            local_ids: vec![child_scoped],
        })
        .await?;

    primary
        .send(Command::RequestObjectPropertiesFamily {
            request_flags: 0,
            object_id: child_id,
        })
        .await?;
    let child_family = listen(primary, quiet).await?;
    let child_family_answer = child_family.families.first().map_or("nothing", |family| {
        if family.object_id == child_id {
            if family.name == CHILD_NAME {
                "the child"
            } else {
                "the child's id under another name"
            }
        } else if family.object_id == object_id {
            "the root"
        } else {
            "something else"
        }
    });
    metrics.set("child_family_answers", child_family_answer);

    hold_to_measured(grid, metrics)
}

/// Derezzes every root still standing to the Trash and waits for each to go.
async fn clean_up(
    session: &mut Session,
    rezzed: &Rezzed,
    trash: InventoryFolderKey,
) -> Result<(), TestFailure> {
    if rezzed.roots.is_empty() {
        return Ok(());
    }
    session
        .send(Command::DerezObjects {
            local_ids: rezzed.roots.clone(),
            destination: DeRezDestination::Trash(trash),
            transaction_id: TransactionId::from(Uuid::new_v4()),
            group_id: None,
        })
        .await?;
    let mut pending: HashSet<ScopedObjectId> = rezzed.roots.iter().copied().collect();
    let started = Instant::now();
    while !pending.is_empty() {
        let remaining = STEP_TIMEOUT.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(TestFailure::Assertion(format!(
                "{} object(s) were never removed on cleanup",
                pending.len()
            )));
        }
        match session
            .wait_for(remaining, |event| match event {
                Event::ObjectRemoved { local_id, .. } => Some(*local_id),
                _ => None,
            })
            .await
        {
            Ok(local_id) => {
                pending.remove(&local_id);
            }
            Err(TestFailure::Timeout(_)) => {}
            Err(other) => return Err(other),
        }
    }
    Ok(())
}

/// Rezzes a cube at `position` and returns it once the region has sent it.
async fn rez_cube(
    session: &mut Session,
    seen: &HashSet<ScopedObjectId>,
    position: Vector,
) -> Result<Object, TestFailure> {
    session
        .send(Command::RezObject {
            shape: PrimShape::cube(position),
            group_id: None,
        })
        .await?;
    wait_for_own_new_object(session, seen, STEP_TIMEOUT)
        .await?
        .map_err(|reason| {
            TestFailure::Assertion(format!("no new object appeared after RezObject: {reason}"))
        })
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
                "the other avatar was never sent the cube the first one rezzed".to_owned(),
            ),
            other => other,
        })
}

/// The agent a session logged in as.
fn agent_of(session: &Session) -> Result<AgentKey, TestFailure> {
    session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))
}

/// The two folders of a login skeleton the case files things into.
#[derive(Debug, Clone, Copy)]
struct Folders {
    /// Where a take puts the donor cube.
    objects: InventoryFolderKey,
    /// Where the cleanup puts what was rezzed.
    trash: InventoryFolderKey,
}

impl Folders {
    /// The Objects and Trash folders of a login skeleton, each falling back to
    /// its root.
    fn of(folders: &[InventoryFolder]) -> Option<Self> {
        let root = folders
            .iter()
            .find(|folder| folder.parent_id.is_none())
            .map(|folder| folder.folder_id);
        let of_type = |folder_type: FolderType| {
            folders
                .iter()
                .find(|folder| folder.folder_type == folder_type.to_code())
                .map(|folder| folder.folder_id)
                .or(root)
        };
        Some(Self {
            objects: of_type(FolderType::Object)?,
            trash: of_type(FolderType::Trash)?,
        })
    }
}

/// A `+`-joined list, or `nothing`.
fn joined(words: &[&str]) -> String {
    if words.is_empty() {
        "nothing".to_owned()
    } else {
        words.join("+")
    }
}

/// Which prims of a two-prim linkset a window's records were about.
fn linkset_answer(heard: &Heard, root: ObjectKey, child: ObjectKey) -> &'static str {
    let of_root = heard.about(root).next().is_some();
    let of_child = heard.about(child).next().is_some();
    match (of_root, of_child) {
        (true, true) => "root+child",
        (true, false) => "root",
        (false, true) => "child",
        (false, false) => "nothing",
    }
}

/// Whether a family record says what the full record says in every field the
/// two share.
fn family_agrees(family: &ObjectPropertiesFamily, properties: &Properties) -> bool {
    family.owner == properties.owner
        && family.group == properties.group
        && family.permissions == properties.permissions
        && family.ownership_cost == properties.ownership_cost
        && family.sale_type == properties.sale_type
        && family.sale_price == properties.sale_price
        && family.category == properties.category
        && family.last_owner_id == properties.last_owner_id
        && family.name == properties.name
        && family.description == properties.description
}

/// Records every field of a freshly rezzed cube's record.
fn record_properties(
    metrics: &mut Metrics,
    properties: &Properties,
    owner: AgentKey,
) -> Result<(), TestFailure> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| TestFailure::State(format!("the clock is before the epoch: {error}")))?
        .as_secs();
    // The creation date's unit, by which reading of it is a date an object
    // could have been made on: after Second Life opened and not in the future.
    let plausible =
        |seconds: u64| (FIRST_OBJECT_EPOCH..=now.saturating_add(86_400)).contains(&seconds);
    let unit = if plausible(properties.creation_date) {
        "seconds"
    } else if plausible(properties.creation_date / 1_000_000) {
        "microseconds"
    } else if properties.creation_date == 0 {
        "zero"
    } else {
        "neither"
    };
    metrics.set("creation_date_unit", unit);
    metrics.set("name", properties.name.clone());
    metrics.set("description", properties.description.clone());
    metrics.set("touch_name", properties.touch_name.clone());
    metrics.set("sit_name", properties.sit_name.clone());
    metrics.set("creator_is_the_rezzer", properties.creator_id == owner);
    metrics.set(
        "owner_is_the_rezzer",
        properties.owner == OwnerKey::Agent(owner),
    );
    metrics.set(
        "last_owner",
        if properties.last_owner_id.is_nil() {
            "nil"
        } else if properties.last_owner_id == owner.uuid() {
            "the rezzer"
        } else {
            "somebody else"
        },
    );
    metrics.set("group_set", properties.group.is_some());
    for (what, mask) in [
        ("base", properties.permissions.base),
        ("owner", properties.permissions.owner),
        ("group", properties.permissions.group),
        ("everyone", properties.permissions.everyone),
        ("next_owner", properties.permissions.next_owner),
    ] {
        metrics.set(&format!("mask_{what}"), format!("{:#010x}", mask.bits()));
    }
    metrics.set(
        "ownership_cost",
        i64::try_from(properties.ownership_cost.0).unwrap_or(-1),
    );
    metrics.set("sale_type", i64::from(properties.sale_type));
    metrics.set(
        "sale_price",
        properties
            .sale_price
            .as_ref()
            .map_or(-1, |price| i64::try_from(price.0).unwrap_or(-1)),
    );
    metrics.set("category", i64::from(properties.category));
    metrics.set("inventory_serial", i64::from(properties.inventory_serial));
    metrics.set("item_id_nil", properties.item_id.uuid().is_nil());
    metrics.set("folder_id_set", properties.folder_id.is_some());
    metrics.set("from_task_id_set", properties.from_task_id.is_some());
    metrics.set(
        "aggregate_perms",
        format!(
            "{:#04x}/{:#04x}/{:#04x}",
            properties.aggregate_perms,
            properties.aggregate_perm_textures,
            properties.aggregate_perm_textures_owner
        ),
    );
    metrics.set(
        "texture_ids",
        i64::try_from(properties.texture_ids.len()).unwrap_or(-1),
    );
    Ok(())
}
