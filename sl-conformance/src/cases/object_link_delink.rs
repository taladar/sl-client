//! A census of what a link and a delink of an avatar's own prims are answered
//! with, the order a grid keeps a linkset in, and the links it refuses.
//!
//! The case rezzes a handful of cubes — called `A`, `B`, `C` and `D` below and
//! in the metrics — and asks one question a leg, listening after each for
//! everything the grid says about it: the updates and the form each comes in,
//! the records, the alerts.
//!
//! 1. **A link.** Two cubes nobody has selected, linked and delinked; then all
//!    four, selected as a build floater holds them, linked in one message with
//!    `A` first and turned a quarter turn: what comes back for the root and
//!    for a child, whose record, in which order, and where a child reads as
//!    standing.
//! 2. **The order.** No message states a link number, but both grids promote
//!    link number 2 when a root is delinked alone, so delinking the root again
//!    and again reads the order out. It is read after a link of four at once,
//!    after a single prim was added to a set, and after two sets were joined.
//! 3. **A delink.** Of the root alone, of a child alone, and of everything at
//!    once; a link that names a child of another set, and a link of one prim.
//! 4. **A prim the agent may not modify.** The owner takes modify off its own
//!    owner mask and links the cube as a child and as a root.
//! 5. **Too far.** A second cube is moved straight up from the first and
//!    linked, the distance halved until the furthest that links is known to
//!    the metre — for cubes half a metre and ten metres across. Then three in
//!    a line, linked with the root at an end and in the middle.
//! 6. **Too many.** A linkset is copied and joined to its copy until it has
//!    256 prims, and one more is linked to it. On the way a set of 32 and one
//!    of 64 are made physical.
//!
//! What each live grid answered is stated in `MEASURED` and written up in
//! `book/src/gridspec/building.md` § Linking; every run, live or fake, is held
//! to it. What a grid makes of a link of somebody *else's* prims is
//! `object-link-stranger`'s.
//!
//! `1av`, `[both]`, offline on both fake flavours. Everything left standing
//! is derezzed to the Trash at the end, also when a leg fails, and the items
//! that makes are removed from the inventory.

use std::collections::HashMap;
use std::time::Duration;

use sl_client_tokio::{
    Command, Event, InventoryType, Object, ObjectFlagSettings, ObjectTransform, PermissionField,
    Permissions, RegionLocalObjectId, RestoreItem, RezScriptParams, ScopedObjectId, ScriptTarget,
    ScriptUploadLocation, Uuid, Vector,
};

use crate::cases::object_edit::{
    count, hex, lifted, moved, near, quarter_turn, record_of, resized,
};
use crate::cases::object_rez_derez::{
    Answer, Folders, Heard, Pace, REZ_LIFT_M, Residue, Run, SETTLE_WINDOW, STEP_TIMEOUT, clean_up,
    joined, listen, rez_cube,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, is_opensim, is_own, settle_scene};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, where
/// this workspace's test object stands as the rez placement reference.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The physical flag of an object update's flags.
const FLAGS_PHYSICAL: u32 = 1;

/// How far above where it was rezzed the distance leg holds its root, in
/// metres: clear of whatever stands on the ground once the cubes are large.
const REACH_LIFT_M: f32 = 30.0;

/// The furthest apart the distance leg puts two cubes, in metres.
const FURTHEST_M: f32 = 256.0;

/// The side of the large cubes of the distance leg, in metres.
const LARGE_SIDE_M: f32 = 10.0;

/// The spacing of the lattice the counting leg builds its linkset in, in
/// metres.
const LATTICE_M: f32 = 0.6;

/// How many times the counting leg doubles its linkset: to 256 prims.
const DOUBLINGS: u8 = 8;

/// What every cube's script says of the linkset it is in, at its start and
/// whenever its links change: its own link number, and the name of every prim
/// in the order of their link numbers. No message of the protocol states a
/// link number; a script is the only thing that can.
const REPORTER: &str = "report()
{
    integer prims = llGetNumberOfPrims();
    string said = \"LINKS \" + (string)llGetLinkNumber() + \":\";
    if (prims < 2)
    {
        said += \" \" + llGetObjectName();
    }
    else
    {
        integer link;
        for (link = 1; link <= prims; ++link)
        {
            said += \" \" + llGetLinkName(link);
        }
    }
    llOwnerSay(said);
}

default
{
    state_entry()
    {
        report();
    }

    changed(integer change)
    {
        if (change & CHANGED_LINK)
        {
            report();
        }
    }
}
";

/// What a line of [`REPORTER`]'s begins with.
const REPORT: &str = "LINKS ";

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/building.md § Linking (object-link-delink, 2026-10-11)";

/// What each grid was measured to answer, by the metric the census records it
/// under. Every run — live or fake — is held to all of it at the end.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("copy_of_a_set_of_four_prims", both(Answer::Count(4))),
    (
        "delink_all_brings_a_child",
        both(Answer::Text("full update")),
    ),
    (
        "delink_all_brings_the_root",
        both(Answer::Text("full update")),
    ),
    ("delink_all_records", both(Answer::Text("none"))),
    ("delink_all_took", both(Answer::Flag(true))),
    ("delink_child_alerts", both(Answer::Text("none"))),
    (
        "delink_child_brings_a_sibling",
        both(Answer::Text("full update")),
    ),
    (
        "delink_child_brings_the_child",
        both(Answer::Text("full update")),
    ),
    (
        "delink_child_brings_the_root",
        both(Answer::Text("full update")),
    ),
    (
        "delink_child_leaves_the_rest_linked",
        both(Answer::Flag(true)),
    ),
    ("delink_child_records", both(Answer::Text("none"))),
    ("delink_child_took", both(Answer::Flag(true))),
    ("delink_root_alerts", both(Answer::Text("none"))),
    (
        "delink_root_brings_a_child",
        both(Answer::Text("full update")),
    ),
    (
        "delink_root_brings_the_new_root",
        both(Answer::Text("full update")),
    ),
    (
        "delink_root_brings_the_old_root",
        both(Answer::Text("full update")),
    ),
    (
        "delink_root_promotes",
        each(Answer::Text("B"), Answer::Text("D")),
    ),
    ("delink_root_records", both(Answer::Text("none"))),
    ("delinked_child_reads", both(Answer::Text("where it stood"))),
    ("delinked_root_reads", both(Answer::Text("where it stood"))),
    ("joined_sets_took", both(Answer::Flag(true))),
    (
        "large_cubes_link_within",
        each(
            Answer::Text("64 m and not 65 m"),
            Answer::Text("as far as was tried, 256 m"),
        ),
    ),
    (
        "large_too_far_alerts",
        each(
            Answer::Text(
                "Link failed -- Unable to link any pieces - pieces are too far apart. [LinkFailed]",
            ),
            ABSENT,
        ),
    ),
    ("largest_set_linked", both(Answer::Count(256))),
    (
        "line_beyond_reach_from_an_end",
        each(Answer::Text("nothing links"), ABSENT),
    ),
    (
        "line_beyond_reach_from_an_end_alerts",
        each(
            Answer::Text(
                "Link failed -- Unable to link any pieces - pieces are too far apart. [LinkFailed]",
            ),
            ABSENT,
        ),
    ),
    (
        "line_beyond_reach_from_the_middle",
        each(Answer::Text("nothing links"), ABSENT),
    ),
    (
        "line_beyond_reach_from_the_middle_alerts",
        each(
            Answer::Text(
                "Link failed -- Unable to link any pieces - pieces are too far apart. [LinkFailed]",
            ),
            ABSENT,
        ),
    ),
    (
        "line_within_reach_from_an_end",
        each(Answer::Text("B and C under A"), ABSENT),
    ),
    (
        "line_within_reach_from_an_end_alerts",
        each(Answer::Text("none"), ABSENT),
    ),
    (
        "line_within_reach_from_the_middle",
        each(Answer::Text("A and C under B"), ABSENT),
    ),
    (
        "line_within_reach_from_the_middle_alerts",
        each(Answer::Text("none"), ABSENT),
    ),
    ("link_alerts", both(Answer::Text("none"))),
    (
        "link_arrival_order",
        each(Answer::Text("D C B A"), Answer::Text("A B C D")),
    ),
    ("link_brings_a_child", both(Answer::Text("full update"))),
    (
        "link_brings_the_root",
        both(Answer::Text("full update+properties")),
    ),
    (
        "link_naming_a_child",
        both(Answer::Text("the child's whole set joins")),
    ),
    ("link_naming_a_child_alerts", both(Answer::Text("none"))),
    ("link_of_one_alerts", both(Answer::Text("none"))),
    ("link_of_one_brings", both(Answer::Text("nothing"))),
    ("link_records", both(Answer::Text("A"))),
    ("link_took", both(Answer::Flag(true))),
    (
        "linked_child_reads",
        both(Answer::Text("its offset in the root's frame")),
    ),
    (
        "no_modify_child_alerts",
        each(
            Answer::Text(
                "Link failed because you do not have edit permission. [LinkFailedNoPermToEdit]",
            ),
            Answer::Text("none"),
        ),
    ),
    (
        "no_modify_child_links",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    (
        "no_modify_delink_alerts",
        each(
            Answer::Text("Unlink failed: no edit permissions."),
            Answer::Text("none"),
        ),
    ),
    (
        "no_modify_owner_mask",
        each(Answer::Text("0x7fffbfff"), Answer::Text("0x0009e000")),
    ),
    (
        "no_modify_root_alerts",
        each(
            Answer::Text(
                "Link failed because you do not have edit permission. [LinkFailedNoPermToEdit]",
            ),
            Answer::Text("none"),
        ),
    ),
    (
        "no_modify_root_links",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    (
        "one_more_alerts",
        each(
            Answer::Text(
                "Link failed -- Unable to link any pieces - too many pieces. [LinkFailed]",
            ),
            Answer::Text("none"),
        ),
    ),
    (
        "one_more_as_the_root_alerts",
        each(
            Answer::Text(
                "Link failed -- Unable to link any pieces - too many pieces. [LinkFailed]",
            ),
            ABSENT,
        ),
    ),
    (
        "one_more_as_the_root_links",
        each(Answer::Flag(false), ABSENT),
    ),
    (
        "one_more_brings",
        each(Answer::Text("nothing"), Answer::Text("full update")),
    ),
    (
        "one_more_links",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    ("order_linked_at_once", both(Answer::Text("A B C D"))),
    (
        "order_of_two_sets_joined",
        each(Answer::Text("A C D B"), Answer::Text("A D C B")),
    ),
    ("order_with_a_prim_added", both(Answer::Text("A C B"))),
    ("physical_set_of_32_alerts", both(Answer::Text("none"))),
    ("physical_set_of_32_took", both(Answer::Flag(true))),
    (
        "physical_set_of_64_alerts",
        each(
            Answer::Text(
                "Can't enable physics for object with physics resource cost greater than 32 [CantEnablePhysCostTooGreat]",
            ),
            Answer::Text("none"),
        ),
    ),
    (
        "physical_set_of_64_took",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    ("promoted_root_reads", both(Answer::Text("where it stood"))),
    (
        "small_cubes_link_within",
        each(
            Answer::Text("64 m and not 65 m"),
            Answer::Text("as far as was tried, 256 m"),
        ),
    ),
    (
        "small_too_far_alerts",
        each(
            Answer::Text(
                "Link failed -- Unable to link any pieces - pieces are too far apart. [LinkFailed]",
            ),
            ABSENT,
        ),
    ),
    ("unselected_delink_took", both(Answer::Flag(true))),
    (
        "unselected_link_records",
        each(Answer::Text("none"), Answer::Text("A")),
    ),
    ("unselected_link_took", both(Answer::Flag(true))),
];

/// What a script says of a linkset on each live grid. The fake grid's scripts
/// know nothing of link numbers (`server-world-link-sets`), so only a live
/// run is held to these.
const MEASURED_LIVE: &[(&str, Measured<Answer>)] = &[
    (
        "a_root_delinked_alone_promotes",
        each(
            Answer::Text("link number 2"),
            Answer::Text("link number 2, link number 3, link number 4"),
        ),
    ),
    ("a_single_prim_is_link_number", both(Answer::Text("0"))),
    ("numbers_linked_at_once", both(Answer::Text("A B C D"))),
    (
        "numbers_of_two_sets_joined",
        each(Answer::Text("A C D B"), Answer::Text("A B C D")),
    ),
    (
        "numbers_once_the_root_left",
        each(Answer::Text("B C D"), Answer::Text("D C B")),
    ),
    (
        "numbers_with_a_prim_added",
        each(Answer::Text("A C B"), Answer::Text("A B C")),
    ),
];

/// What a metric reads on a grid that never records it: a leg the grid's own
/// answer cut short.
const ABSENT: Answer = Answer::Text("not recorded");

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

/// Holds what the census recorded to [`MEASURED`], naming every metric that
/// reads otherwise — or was never recorded.
fn hold_to_measured(grid: Grid, metrics: &Metrics) -> Result<(), TestFailure> {
    let live: &[(&str, Measured<Answer>)] = if grid.is_fake() { &[] } else { MEASURED_LIVE };
    let failures: Vec<String> = MEASURED
        .iter()
        .chain(live)
        .filter_map(|(key, measured)| {
            let expected = measured.on(grid);
            match metrics.get(key) {
                None if *expected == ABSENT => None,
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

/// A census of what a link and a delink are answered with, the order a
/// linkset is kept in and the links a grid refuses.
#[derive(Debug)]
pub struct ObjectLinkDelink;

impl GridTest for ObjectLinkDelink {
    fn name(&self) -> &'static str {
        "object-link-delink"
    }

    fn description(&self) -> &'static str {
        "What a link and a delink of the agent's own prims are answered with, the order a \
         linkset is kept in, and the links a grid refuses: of a prim it may not modify, too far \
         apart and too many"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
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
            // The folders come with the login, before the region does.
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
            let grid = ctx.grid();
            let mut scene = Scene::default();
            let mut metrics = Metrics::new();
            let outcome = census(ctx, folders, &mut scene, &mut metrics).await;
            let held = outcome.and_then(|()| hold_to_measured(grid, &metrics));
            ctx.metrics().merge(metrics);
            // Take away what is left whatever became of the census: every
            // root, which is what a derez names a linkset by.
            let residue = Residue {
                roots: scene.roots(),
                items: Vec::new(),
            };
            let cleanup = clean_up(ctx.primary(), &residue, folders.trash).await;
            held.and(cleanup)
        })
    }
}

/// The prims the case rezzed, each as the grid last described it. A linkset
/// is derezzed by its root, so the cleanup has to know which prims are roots
/// whatever leg the census stopped in.
#[derive(Debug, Default)]
struct Scene {
    /// The case's own prims still in the world.
    objects: HashMap<RegionLocalObjectId, Object>,
    /// Whether the four cubes carry [`REPORTER`].
    scripted: bool,
}

impl Scene {
    /// Notes a prim the case rezzed.
    fn note(&mut self, object: &Object) {
        let _previous = self.objects.insert(object.local_id, object.clone());
    }

    /// Brings the prims up to what a window heard of them, and notes the
    /// agent's own that it brought anew.
    fn fold(&mut self, heard: &Heard, owner: sl_client_tokio::Uuid) {
        for object in &heard.added {
            if is_own(object, owner) || self.objects.contains_key(&object.local_id) {
                self.note(object);
            }
        }
        for object in &heard.changed {
            if self.objects.contains_key(&object.local_id) {
                self.note(object);
            }
        }
        for (_after, killed) in &heard.kills {
            for local_id in killed {
                let _gone = self.objects.remove(local_id);
            }
        }
    }

    /// `object` as the grid last described it.
    fn latest(&self, object: &Object) -> Object {
        self.objects
            .get(&object.local_id)
            .cloned()
            .unwrap_or_else(|| object.clone())
    }

    /// Whether `child` hangs off `root`.
    fn hangs_off(&self, child: &Object, root: &Object) -> bool {
        self.objects
            .get(&child.local_id)
            .is_some_and(|object| object.parent_id == root.local_id)
    }

    /// Whether `object` hangs off nothing.
    fn is_free(&self, object: &Object) -> bool {
        self.objects
            .get(&object.local_id)
            .is_some_and(|object| object.parent_id == RegionLocalObjectId(0))
    }

    /// How many prims hang off `root`, and `root` itself.
    fn prims_of(&self, root: &Object) -> usize {
        self.objects
            .values()
            .filter(|object| object.parent_id == root.local_id)
            .count()
            .saturating_add(1)
    }

    /// Every prim that hangs off nothing.
    fn roots(&self) -> Vec<ScopedObjectId> {
        self.objects
            .values()
            .filter(|object| object.parent_id == RegionLocalObjectId(0))
            .map(Object::scoped_id)
            .collect()
    }
}

/// The legs, in order.
async fn census(
    ctx: &mut TestContext,
    folders: Folders,
    scene: &mut Scene,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let pace = Pace::of(grid);
    let session = ctx.primary();
    let owner = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

    session.wait_for_region(REGION_TIMEOUT).await?;
    let (seen, anchor) = settle_scene(
        session,
        grid,
        build_position,
        SETTLE_WINDOW,
        pace.settle_idle,
    )
    .await?;
    let anchor = anchor.ok_or_else(|| {
        TestFailure::Assertion("the arrival streamed nothing to place a cube against".to_owned())
    })?;
    let mut run = Run {
        folders,
        owner,
        pace,
        position: Vector {
            x: anchor.x,
            y: anchor.y,
            z: anchor.z + REZ_LIFT_M,
        },
        seen,
    };

    let mut cubes = Vec::with_capacity(4);
    for slot in 0..4 {
        cubes.push(rez(session, &mut run, scene, slot).await?);
    }
    let stood: Vec<Vector> = cubes
        .iter()
        .map(|cube| cube.motion.position.clone())
        .collect();
    for cube in &cubes {
        ask(
            session,
            &run,
            scene,
            Command::SetObjectName {
                local_id: cube.scoped_id(),
                name: letter(&cubes, cube),
            },
            std::slice::from_ref(cube),
        )
        .await?;
    }
    // The fake grid's scripts know nothing of link numbers
    // (`server-world-link-sets`), so there the order is read from the delinks
    // alone.
    if !grid.is_fake() {
        let mut alone = Vec::new();
        for cube in &cubes {
            let heard = plant(session, &run, cube).await?;
            alone.push(
                reports(&heard)
                    .last()
                    .map_or_else(|| "nothing said".to_owned(), |(own, _names)| own.clone()),
            );
        }
        alone.sort();
        alone.dedup();
        metrics.set("a_single_prim_is_link_number", alone.join(", "));
        scene.scripted = true;
    }

    let at_once = links(session, &run, scene, metrics, &cubes, &stood).await?;
    orders(session, &run, scene, metrics, &cubes, at_once).await?;
    delinks(session, &run, scene, metrics, &cubes, &stood).await?;
    unmodifiable(session, &run, scene, metrics, &cubes).await?;
    reach(session, &run, scene, metrics, &cubes).await?;
    many(session, &mut run, scene, metrics).await
}

/// Rezzes a cube at `slot` and notes it.
async fn rez(
    session: &mut Session,
    run: &mut Run,
    scene: &mut Scene,
    slot: u8,
) -> Result<Object, TestFailure> {
    // The scene knows what stands; the residue of the shared helper is not
    // what the cleanup goes by.
    let mut residue = Residue::default();
    let cube = rez_cube(session, run, &mut residue, slot).await?;
    scene.note(&cube);
    Ok(cube)
}

/// Leg 1: a link of two unselected cubes, and of four selected ones.
async fn links(
    session: &mut Session,
    run: &Run,
    scene: &mut Scene,
    metrics: &mut Metrics,
    cubes: &[Object],
    stood: &[Vector],
) -> Result<Option<String>, TestFailure> {
    let [a, b, c, d] = cubes else {
        return Err(TestFailure::State(
            "the census rezzes four cubes".to_owned(),
        ));
    };

    // Nothing is selected yet: a link and a delink as no build floater sends
    // them.
    let heard = ask(session, run, scene, link(&[a, b]), &[a, b]).await?;
    metrics.set("unselected_link_took", scene.hangs_off(b, a));
    metrics.set("unselected_link_records", records(&heard, cubes));
    ask(session, run, scene, delink(&[a, b]), &[a, b]).await?;
    metrics.set("unselected_delink_took", scene.is_free(b));
    if !scene.is_free(b) {
        // Selected, the delink is what a viewer sends.
        select(session, run, &[a, b]).await?;
        ask(session, run, scene, delink(&[a, b]), &[a, b]).await?;
    }

    select(session, run, cubes).await?;
    ask(
        session,
        run,
        scene,
        Command::UpdateObject {
            local_id: a.scoped_id(),
            transform: ObjectTransform {
                position: None,
                rotation: Some(quarter_turn()),
                scale: None,
                group: true,
                uniform: false,
            },
        },
        &[a],
    )
    .await?;

    let heard = ask_told(session, run, scene, link(&[a, b, c, d]), cubes, 4).await?;
    let at_once = scene.scripted.then(|| numbers(&heard, 4));
    if let Some(said) = &at_once {
        metrics.set("numbers_linked_at_once", said.clone());
    }
    metrics.set(
        "link_took",
        [b, c, d].iter().all(|child| scene.hangs_off(child, a)),
    );
    metrics.set("link_brings_the_root", heard.brought(a));
    metrics.set("link_brings_a_child", heard.brought(b));
    metrics.set("link_records", records(&heard, cubes));
    metrics.set("link_alerts", joined(&heard.alerts));
    metrics.set("link_arrival_order", arrival_order(&heard, cubes));
    // A child's place is stated in its root's frame: `B` stands along the
    // region's x axis from `A`, which a quarter turn about z makes the root's
    // own −y.
    let (Some(root_stood), Some(child_stood)) = (stood.first(), stood.get(1)) else {
        return Err(TestFailure::State(
            "the census rezzes four cubes".to_owned(),
        ));
    };
    let offset = Vector {
        x: child_stood.y - root_stood.y,
        y: root_stood.x - child_stood.x,
        z: child_stood.z - root_stood.z,
    };
    let reads = scene.latest(b).motion.position;
    metrics.set(
        "linked_child_reads",
        if near(&reads, &offset) {
            "its offset in the root's frame".to_owned()
        } else if near(&reads, child_stood) {
            "where it stood in the region".to_owned()
        } else {
            format!("{:.2}, {:.2}, {:.2}", reads.x, reads.y, reads.z)
        },
    );
    Ok(at_once)
}

/// Leg 2: the order of a linkset, read out by delinking its root again and
/// again. Enters with all four linked under `A` and leaves them free.
async fn orders(
    session: &mut Session,
    run: &Run,
    scene: &mut Scene,
    metrics: &mut Metrics,
    cubes: &[Object],
    at_once: Option<String>,
) -> Result<(), TestFailure> {
    let [a, b, c, d] = cubes else {
        return Err(TestFailure::State(
            "the census rezzes four cubes".to_owned(),
        ));
    };
    // What the scripts said of each set, beside the order its roots were
    // promoted in.
    let mut said_and_promoted: Vec<(String, String)> = Vec::new();

    // Linked in one message, `A B C D`.
    let order = peel(session, run, scene, cubes, cubes).await?;
    metrics.set("order_linked_at_once", order.clone());
    said_and_promoted.extend(at_once.map(|said| (said, order)));

    // One prim added to a set: `A B`, then `A C`.
    ask(session, run, scene, link(&[a, b]), cubes).await?;
    let heard = ask_told(session, run, scene, link(&[a, c]), cubes, 3).await?;
    let said = scene.scripted.then(|| numbers(&heard, 3));
    let order = peel(
        session,
        run,
        scene,
        cubes,
        &[a.clone(), b.clone(), c.clone()],
    )
    .await?;
    metrics.set("order_with_a_prim_added", order.clone());
    if let Some(said) = said {
        metrics.set("numbers_with_a_prim_added", said.clone());
        said_and_promoted.push((said, order));
    }

    // Two sets joined: `A B` and `C D`, then `A C`.
    ask(session, run, scene, link(&[a, b]), cubes).await?;
    ask(session, run, scene, link(&[c, d]), cubes).await?;
    let heard = ask_told(session, run, scene, link(&[a, c]), cubes, 4).await?;
    let said = scene.scripted.then(|| numbers(&heard, 4));
    metrics.set(
        "joined_sets_took",
        [b, c, d].iter().all(|child| scene.hangs_off(child, a)),
    );
    let order = peel(session, run, scene, cubes, cubes).await?;
    metrics.set("order_of_two_sets_joined", order.clone());
    if let Some(said) = said {
        metrics.set("numbers_of_two_sets_joined", said.clone());
        said_and_promoted.push((said, order));
    }

    // Which link number a grid makes the root when the root is delinked
    // alone: where the prim promoted first stood in what the scripts said.
    if scene.scripted {
        let mut promoted: Vec<String> = said_and_promoted
            .iter()
            .map(|(said, order)| {
                order
                    .split(' ')
                    .nth(1)
                    .and_then(|second| said.split(' ').position(|name| name == second))
                    .map_or_else(
                        || "an unknown link number".to_owned(),
                        |index| format!("link number {}", index.saturating_add(1)),
                    )
            })
            .collect();
        promoted.sort();
        promoted.dedup();
        metrics.set("a_root_delinked_alone_promotes", promoted.join(", "));
    }
    Ok(())
}

/// Delinks the root of the set `members` are — its first — again and again,
/// and returns the order the prims were promoted in, as their letters: the
/// order of their link numbers, where a grid promotes number 2.
async fn peel(
    session: &mut Session,
    run: &Run,
    scene: &mut Scene,
    cubes: &[Object],
    members: &[Object],
) -> Result<String, TestFailure> {
    let Some((first, rest)) = members.split_first() else {
        return Ok("nothing".to_owned());
    };
    let mut root = first.clone();
    let mut rest: Vec<Object> = rest.to_vec();
    let mut order = vec![letter(cubes, &root)];
    while !rest.is_empty() {
        ask(session, run, scene, delink(&[&root]), members).await?;
        let free: Vec<Object> = rest
            .iter()
            .filter(|member| scene.is_free(member))
            .cloned()
            .collect();
        match free.as_slice() {
            [promoted] => {
                order.push(letter(cubes, promoted));
                root = promoted.clone();
                rest.retain(|member| member.local_id != root.local_id);
            }
            [] => {
                order.push("and the rest still linked".to_owned());
                break;
            }
            _ if free.len() == rest.len() => {
                order.push(format!("and {} all freed at once", free.len()));
                break;
            }
            _ => {
                order.push(format!("and {} of {} freed", free.len(), rest.len()));
                break;
            }
        }
    }
    // Whatever a grid made of it, the next leg starts with everything free.
    if members.iter().any(|member| !scene.is_free(member)) {
        let all: Vec<&Object> = members.iter().collect();
        ask(session, run, scene, delink(&all), members).await?;
    }
    Ok(order.join(" "))
}

/// Leg 3: a delink of the root alone, of a child alone and of everything; a
/// link that names a child of another set, and a link of one prim.
async fn delinks(
    session: &mut Session,
    run: &Run,
    scene: &mut Scene,
    metrics: &mut Metrics,
    cubes: &[Object],
    stood: &[Vector],
) -> Result<(), TestFailure> {
    let [a, b, c, d] = cubes else {
        return Err(TestFailure::State(
            "the census rezzes four cubes".to_owned(),
        ));
    };
    let place_of = |cube: &Object| {
        cubes
            .iter()
            .position(|other| other.local_id == cube.local_id)
            .and_then(|index| stood.get(index))
            .unwrap_or(&cube.motion.position)
            .clone()
    };
    let stands = |scene: &Scene, cube: &Object| {
        let reads = scene.latest(cube).motion.position;
        if near(&reads, &place_of(cube)) {
            "where it stood".to_owned()
        } else {
            format!("{:.2}, {:.2}, {:.2}", reads.x, reads.y, reads.z)
        }
    };

    // The root alone.
    ask(session, run, scene, link(&[a, b, c, d]), cubes).await?;
    let heard = ask_told(session, run, scene, delink(&[a]), cubes, 3).await?;
    if scene.scripted {
        metrics.set("numbers_once_the_root_left", numbers(&heard, 3));
    }
    let promoted = [b, c, d].into_iter().find(|cube| scene.is_free(cube));
    // Which prim that is depends on everything the cubes were linked into
    // before, on a grid that does not promote by link number.
    metrics.set(
        "delink_root_promotes",
        promoted.map_or_else(|| "nothing".to_owned(), |cube| letter(cubes, cube)),
    );
    metrics.set("delink_root_brings_the_old_root", heard.brought(a));
    metrics.set(
        "delink_root_brings_the_new_root",
        promoted.map_or_else(|| "no new root".to_owned(), |cube| heard.brought(cube)),
    );
    metrics.set(
        "delink_root_brings_a_child",
        [b, c, d]
            .into_iter()
            .find(|cube| !scene.is_free(cube))
            .map_or_else(|| "no child left".to_owned(), |cube| heard.brought(cube)),
    );
    metrics.set("delink_root_records", records(&heard, cubes));
    metrics.set("delink_root_alerts", joined(&heard.alerts));
    metrics.set("delinked_root_reads", stands(scene, a));
    metrics.set(
        "promoted_root_reads",
        promoted.map_or_else(|| "no new root".to_owned(), |cube| stands(scene, cube)),
    );
    let all: Vec<&Object> = cubes.iter().collect();
    ask(session, run, scene, delink(&all), cubes).await?;

    // A child alone.
    ask(session, run, scene, link(&[a, b, c, d]), cubes).await?;
    let heard = ask(session, run, scene, delink(&[c]), cubes).await?;
    metrics.set("delink_child_took", scene.is_free(c));
    metrics.set("delink_child_brings_the_child", heard.brought(c));
    metrics.set("delink_child_brings_the_root", heard.brought(a));
    metrics.set("delink_child_brings_a_sibling", heard.brought(b));
    metrics.set("delink_child_records", records(&heard, cubes));
    metrics.set("delink_child_alerts", joined(&heard.alerts));
    metrics.set("delinked_child_reads", stands(scene, c));
    metrics.set(
        "delink_child_leaves_the_rest_linked",
        scene.hangs_off(b, a) && scene.hangs_off(d, a),
    );

    // Everything at once, as a viewer unlinks a selected set.
    let heard = ask(session, run, scene, delink(&[a, b, d]), cubes).await?;
    metrics.set(
        "delink_all_took",
        cubes.iter().all(|cube| scene.is_free(cube)),
    );
    metrics.set("delink_all_brings_the_root", heard.brought(a));
    metrics.set("delink_all_brings_a_child", heard.brought(b));
    metrics.set("delink_all_records", records(&heard, cubes));

    // A link that names a child of another set, which no viewer sends.
    ask(session, run, scene, link(&[a, b]), cubes).await?;
    let heard = ask(session, run, scene, link(&[c, b]), cubes).await?;
    metrics.set(
        "link_naming_a_child",
        match (
            scene.hangs_off(b, c),
            scene.hangs_off(a, c),
            scene.hangs_off(b, a),
        ) {
            (true, true, _) => "the child's whole set joins",
            (true, false, _) => "the child alone joins",
            (false, _, true) => "nothing",
            (false, _, false) => "the child is freed",
        },
    );
    metrics.set("link_naming_a_child_alerts", joined(&heard.alerts));
    ask(session, run, scene, delink(&all), cubes).await?;

    // A link of one prim.
    let heard = ask(session, run, scene, link(&[a]), cubes).await?;
    metrics.set("link_of_one_brings", heard.brought(a));
    metrics.set("link_of_one_alerts", joined(&heard.alerts));
    Ok(())
}

/// Leg 4: a prim whose owner may not modify it, linked as a child and as a
/// root.
async fn unmodifiable(
    session: &mut Session,
    run: &Run,
    scene: &mut Scene,
    metrics: &mut Metrics,
    cubes: &[Object],
) -> Result<(), TestFailure> {
    let [a, _b, _c, d] = cubes else {
        return Err(TestFailure::State(
            "the census rezzes four cubes".to_owned(),
        ));
    };
    let modify = |set: bool| Command::SetObjectPermissions {
        local_ids: vec![d.scoped_id()],
        field: PermissionField::Owner,
        set,
        mask: Permissions::MODIFY,
    };
    ask(session, run, scene, modify(false), &[d]).await?;
    let record = record_of(session, run, d).await?;
    metrics.set("no_modify_owner_mask", hex(record.permissions.owner));

    let heard = ask(session, run, scene, link(&[a, d]), cubes).await?;
    metrics.set("no_modify_child_links", scene.hangs_off(d, a));
    metrics.set("no_modify_child_alerts", joined(&heard.alerts));
    // A delink that names it too, whether or not there is anything to
    // delink.
    let heard = ask(session, run, scene, delink(&[a, d]), cubes).await?;
    metrics.set("no_modify_delink_alerts", joined(&heard.alerts));

    rest(session, run).await?;
    let heard = ask(session, run, scene, link(&[d, a]), cubes).await?;
    metrics.set("no_modify_root_links", scene.hangs_off(a, d));
    metrics.set("no_modify_root_alerts", joined(&heard.alerts));
    ask(session, run, scene, delink(&[a, d]), cubes).await?;

    ask(session, run, scene, modify(true), &[d]).await?;
    Ok(())
}

/// Leg 5: how far apart two prims link, and three in a line.
async fn reach(
    session: &mut Session,
    run: &Run,
    scene: &mut Scene,
    metrics: &mut Metrics,
    cubes: &[Object],
) -> Result<(), TestFailure> {
    let [a, b, c, _d] = cubes else {
        return Err(TestFailure::State(
            "the census rezzes four cubes".to_owned(),
        ));
    };
    let base = lifted(&a.motion.position, REACH_LIFT_M);
    ask(session, run, scene, moved(a.scoped_id(), &base), &[a]).await?;

    let small = furthest(session, run, scene, metrics, "small", a, b, &base).await?;

    // Three in a line, `A` lowest, `B` in the middle and `C` highest: first
    // with the ends nearer than two prims link, then with the ends further
    // apart than that and the middle one within reach of both.
    if let Some(reach_m) = small {
        for (key, step) in [
            ("line_within_reach", reach_m * 0.4375),
            ("line_beyond_reach", reach_m * 0.75),
        ] {
            ask(
                session,
                run,
                scene,
                moved(b.scoped_id(), &lifted(&base, step)),
                &[b],
            )
            .await?;
            ask(
                session,
                run,
                scene,
                moved(c.scoped_id(), &lifted(&base, step * 2.0)),
                &[c],
            )
            .await?;
            for (from, root, second, third) in
                [("from_an_end", a, b, c), ("from_the_middle", b, a, c)]
            {
                rest(session, run).await?;
                let heard = ask(session, run, scene, link(&[root, second, third]), cubes).await?;
                let linked: Vec<String> = [second, third]
                    .into_iter()
                    .filter(|cube| scene.hangs_off(cube, root))
                    .map(|cube| letter(cubes, cube))
                    .collect();
                metrics.set(
                    &format!("{key}_{from}"),
                    if linked.is_empty() {
                        "nothing links".to_owned()
                    } else {
                        format!("{} under {}", linked.join(" and "), letter(cubes, root))
                    },
                );
                metrics.set(&format!("{key}_{from}_alerts"), joined(&heard.alerts));
                ask(session, run, scene, delink(&[a, b, c]), cubes).await?;
            }
        }
    }

    for cube in [a, b] {
        ask(
            session,
            run,
            scene,
            resized(cube.scoped_id(), LARGE_SIDE_M, LARGE_SIDE_M, LARGE_SIDE_M),
            &[cube],
        )
        .await?;
    }
    furthest(session, run, scene, metrics, "large", a, b, &base).await?;
    // Back to a size that is nobody's nuisance while the last leg runs.
    for cube in [a, b] {
        ask(
            session,
            run,
            scene,
            resized(cube.scoped_id(), 0.5, 0.5, 0.5),
            &[cube],
        )
        .await?;
    }
    Ok(())
}

/// Finds the furthest `child` links to `root` from, straight above `base`
/// where `root` stands, to the metre — and records it under `key`, with what
/// the first refusal was answered with. `None` where the cubes link at
/// [`FURTHEST_M`].
#[expect(
    clippy::too_many_arguments,
    reason = "a leg's helper: the session, the run and its books, and the two cubes"
)]
async fn furthest(
    session: &mut Session,
    run: &Run,
    scene: &mut Scene,
    metrics: &mut Metrics,
    key: &str,
    root: &Object,
    child: &Object,
    base: &Vector,
) -> Result<Option<f32>, TestFailure> {
    let pair = [root, child];
    let mut refusal: Option<Heard> = None;
    let mut trail: Vec<String> = Vec::new();
    let mut links_at = 0.0_f32;
    let mut refused_at = FURTHEST_M;
    let mut distance = FURTHEST_M;
    rest(session, run).await?;
    loop {
        ask(
            session,
            run,
            scene,
            moved(child.scoped_id(), &lifted(base, distance)),
            &[child],
        )
        .await?;
        let heard = ask(session, run, scene, link(&pair), &pair).await?;
        if scene.hangs_off(child, root) {
            trail.push(format!("{distance:.0} links"));
            links_at = distance;
            ask(session, run, scene, delink(&pair), &pair).await?;
            if (distance - FURTHEST_M).abs() < f32::EPSILON {
                break;
            }
        } else {
            trail.push(format!(
                "{distance:.0} {}",
                if heard.alerts.is_empty() {
                    "refused in silence"
                } else {
                    "refused aloud"
                }
            ));
            refused_at = distance;
            // The first refusal's: a grid may not say the same thing twice
            // in a row.
            let _first = refusal.get_or_insert(heard);
        }
        if refused_at - links_at <= 1.0 {
            break;
        }
        distance = f32::midpoint(links_at, refused_at).round();
    }
    metrics.set(&format!("{key}_cubes_tried"), trail.join(", "));
    let Some(refusal) = refusal else {
        metrics.set(
            &format!("{key}_cubes_link_within"),
            "as far as was tried, 256 m",
        );
        return Ok(None);
    };
    metrics.set(
        &format!("{key}_cubes_link_within"),
        format!("{links_at:.0} m and not {refused_at:.0} m"),
    );
    metrics.set(&format!("{key}_too_far_alerts"), joined(&refusal.alerts));
    Ok(Some(links_at))
}

/// Leg 6: a linkset doubled until it has 256 prims, one more linked to it,
/// and a set of 32 and of 64 made physical on the way.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow: a copy, a join and what each brought, eight times over"
)]
async fn many(
    session: &mut Session,
    run: &mut Run,
    scene: &mut Scene,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let owner = run.owner.uuid();
    let root = rez(session, run, scene, 5).await?;
    select(session, run, std::slice::from_ref(&root)).await?;
    let mut prims: usize = 1;
    for doubling in 0..DOUBLINGS {
        // The copies fill a lattice, so that the whole set stays a few metres
        // across.
        let stride = LATTICE_M * f32::from(1_u8 << (doubling / 3));
        let offset = match doubling % 3 {
            0 => Vector {
                x: stride,
                y: 0.0,
                z: 0.0,
            },
            1 => Vector {
                x: 0.0,
                y: stride,
                z: 0.0,
            },
            _ => Vector {
                x: 0.0,
                y: 0.0,
                z: stride,
            },
        };
        session
            .send(Command::DuplicateObjects {
                local_ids: vec![root.scoped_id()],
                offset,
                group_id: None,
            })
            .await?;
        let seen = run.seen.clone();
        let is_new =
            move |object: &Object| is_own(object, owner) && !seen.contains(&object.scoped_id());
        let heard = listen(session, STEP_TIMEOUT, run.pace.quiet, |heard| {
            heard.added.iter().filter(|object| is_new(object)).count() >= prims
        })
        .await?;
        scene.fold(&heard, owner);
        let copied: Vec<Object> = heard
            .added
            .iter()
            .filter(|object| is_new(object))
            .cloned()
            .collect();
        run.seen.extend(copied.iter().map(Object::scoped_id));
        if doubling == 2 {
            metrics.set("copy_of_a_set_of_four_prims", count(copied.len()));
        }
        let Some(copy) = copied
            .iter()
            .find(|object| object.parent_id == RegionLocalObjectId(0))
        else {
            return Err(TestFailure::Assertion(format!(
                "a duplicate of a linkset of {prims} brought no new root; alerts: {}",
                joined(&heard.alerts)
            )));
        };
        if copied.len() != prims {
            return Err(TestFailure::Assertion(format!(
                "a duplicate of a linkset of {prims} brought {} prims; alerts: {}",
                copied.len(),
                joined(&heard.alerts)
            )));
        }
        select(session, run, &copied).await?;

        session.send(link(&[&root, copy])).await?;
        let joined_prims = prims.saturating_mul(2);
        let before = scene.objects.clone();
        let heard = listen(session, STEP_TIMEOUT, run.pace.quiet, |heard| {
            if !heard.alerts.is_empty() {
                return true;
            }
            let mut counted = Scene {
                objects: before.clone(),
                scripted: false,
            };
            counted.fold(heard, owner);
            counted.prims_of(&root) >= joined_prims
        })
        .await?;
        scene.fold(&heard, owner);
        let linked = scene.prims_of(&root);
        if linked != joined_prims {
            metrics.set("largest_set_linked", count(prims));
            metrics.set(
                "doubling_refused_at",
                format!("{joined_prims} prims: {}", joined(&heard.alerts)),
            );
            return Ok(());
        }
        prims = joined_prims;

        if prims == 32 || prims == 64 {
            let physical = |on: bool| Command::SetObjectFlags {
                local_id: root.scoped_id(),
                flags: ObjectFlagSettings {
                    use_physics: on,
                    ..ObjectFlagSettings::default()
                },
            };
            let heard = ask(session, run, scene, physical(true), &[&root]).await?;
            metrics.set(
                &format!("physical_set_of_{prims}_took"),
                scene.latest(&root).update_flags & FLAGS_PHYSICAL != 0,
            );
            metrics.set(
                &format!("physical_set_of_{prims}_alerts"),
                joined(&heard.alerts),
            );
            ask(session, run, scene, physical(false), &[&root]).await?;
        }
    }
    metrics.set("largest_set_linked", count(prims));

    // One more than that.
    let extra = rez(session, run, scene, 4).await?;
    select(session, run, std::slice::from_ref(&extra)).await?;
    let heard = ask(
        session,
        run,
        scene,
        link(&[&root, &extra]),
        // The set's update is one of 257: what says whether the link took
        // is the new prim's.
        &[&extra],
    )
    .await?;
    metrics.set("one_more_links", scene.hangs_off(&extra, &root));
    metrics.set("one_more_brings", heard.brought(&extra));
    metrics.set("one_more_alerts", joined(&heard.alerts));
    if !scene.hangs_off(&extra, &root) {
        // The other way around: the single prim as the root.
        rest(session, run).await?;
        let heard = ask(session, run, scene, link(&[&extra, &root]), &[&root]).await?;
        metrics.set("one_more_as_the_root_links", scene.hangs_off(&root, &extra));
        metrics.set("one_more_as_the_root_alerts", joined(&heard.alerts));
    }
    Ok(())
}

/// Lets a while pass before a request whose refusal is to be heard: Second
/// Life does not say the same thing twice within a few seconds, so that of
/// refusals six seconds apart every other one came without its alert.
async fn rest(session: &mut Session, run: &Run) -> Result<(), TestFailure> {
    let pause = run.pace.quiet.saturating_mul(5);
    listen(session, pause, pause, |_heard| false).await?;
    Ok(())
}

/// An `ObjectLink` of `prims`, the first the root.
fn link(prims: &[&Object]) -> Command {
    Command::LinkObjects {
        local_ids: prims.iter().map(|prim| prim.scoped_id()).collect(),
    }
}

/// An `ObjectDelink` of `prims`.
fn delink(prims: &[&Object]) -> Command {
    Command::DelinkObjects {
        local_ids: prims.iter().map(|prim| prim.scoped_id()).collect(),
    }
}

/// Selects `prims` and leaves them selected, as a build floater holds what it
/// links.
async fn select<O: std::borrow::Borrow<Object> + Sync>(
    session: &mut Session,
    run: &Run,
    prims: &[O],
) -> Result<(), TestFailure> {
    session
        .send(Command::RequestObjectProperties {
            local_ids: prims.iter().map(|prim| prim.borrow().scoped_id()).collect(),
        })
        .await?;
    let wanted = prims.len();
    listen(
        session,
        STEP_TIMEOUT,
        run.pace.quiet.checked_div(3).unwrap_or(run.pace.quiet),
        |heard| heard.properties.len() >= wanted,
    )
    .await?;
    Ok(())
}

/// Sends `command` and listens for what it brings: until something is said of
/// one of `watched` — or an alert comes — and a little longer, or for as long
/// as an answer may take. Brings `scene` up to it.
async fn ask<O: std::borrow::Borrow<Object> + Sync>(
    session: &mut Session,
    run: &Run,
    scene: &mut Scene,
    command: Command,
    watched: &[O],
) -> Result<Heard, TestFailure> {
    session.send(command).await?;
    let watched: Vec<RegionLocalObjectId> = watched
        .iter()
        .map(|object| object.borrow().local_id)
        .collect();
    let heard = listen(
        session,
        run.pace.quiet.saturating_mul(2),
        run.pace.quiet.checked_div(2).unwrap_or(run.pace.quiet),
        |heard| {
            !heard.alerts.is_empty()
                || heard
                    .updated
                    .iter()
                    .any(|(local_id, _form)| watched.contains(local_id))
                || heard
                    .kills
                    .iter()
                    .any(|(_after, ids)| ids.iter().any(|id| watched.contains(id)))
        },
    )
    .await?;
    scene.fold(&heard, run.owner.uuid());
    Ok(heard)
}

/// [`ask`], listening on until a script has said what a linkset of `prims`
/// is numbered as — where the cubes carry scripts.
async fn ask_told(
    session: &mut Session,
    run: &Run,
    scene: &mut Scene,
    command: Command,
    watched: &[Object],
    prims: usize,
) -> Result<Heard, TestFailure> {
    if !scene.scripted {
        return ask(session, run, scene, command, watched).await;
    }
    session.send(command).await?;
    let heard = listen(session, STEP_TIMEOUT, run.pace.quiet, |heard| {
        !heard.alerts.is_empty()
            || reports(heard)
                .iter()
                .any(|(_own, names)| names.len() == prims)
    })
    .await?;
    scene.fold(&heard, run.owner.uuid());
    Ok(heard)
}

/// Puts [`REPORTER`] into `cube` and returns the window its first report came
/// in.
async fn plant(session: &mut Session, run: &Run, cube: &Object) -> Result<Heard, TestFailure> {
    session
        .send(Command::RezScript {
            target: cube.scoped_id(),
            params: Box::new(RezScriptParams {
                group_id: None,
                enabled: true,
                item: RestoreItem::new_script(
                    run.owner,
                    cube.full_id,
                    "New Script",
                    Uuid::new_v4(),
                ),
            }),
        })
        .await?;
    // The script lands in the contents in its own time.
    let started = std::time::Instant::now();
    let item_id = loop {
        session
            .send(Command::FetchTaskInventory {
                target: cube.scoped_id(),
                held_serial: None,
            })
            .await?;
        let found = crate::support::wait_for_task_listing(session, cube.full_id, STEP_TIMEOUT)
            .await?
            .iter()
            .find(|entry| entry.inv_type == InventoryType::Script)
            .map(|entry| entry.item_id);
        if let Some(item_id) = found {
            break item_id;
        }
        if started.elapsed() >= STEP_TIMEOUT {
            return Err(TestFailure::Assertion(
                "a new script did not appear in a cube's contents".to_owned(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    session
        .send(Command::UploadScript {
            location: ScriptUploadLocation::TaskInventory {
                task_id: cube.full_id,
                item_id,
                running: true,
                experience: None,
            },
            target: ScriptTarget::Mono,
            source: REPORTER.as_bytes().to_vec(),
        })
        .await?;
    // A grid may run the script before it says the upload is done.
    let mut early: Vec<(String, String)> = Vec::new();
    let compiled = session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::ScriptUploaded {
                compiled, errors, ..
            } => Some(Ok((*compiled, errors.clone()))),
            Event::AssetUploadFailed { reason } => Some(Err(reason.clone())),
            Event::ChatReceived(chat) => {
                early.push((chat.from_name.clone(), chat.message.clone()));
                None
            }
            _ => None,
        })
        .await?;
    match compiled {
        Ok((true, _errors)) => {}
        Ok((false, errors)) => {
            return Err(TestFailure::Assertion(format!(
                "the reporting script did not compile: {errors:?}"
            )));
        }
        Err(reason) => {
            return Err(TestFailure::Assertion(format!(
                "the reporting script's upload failed: {reason}"
            )));
        }
    }
    let said_early = early
        .iter()
        .any(|(_speaker, text)| text.starts_with(REPORT));
    let mut heard = listen(session, STEP_TIMEOUT, run.pace.quiet, |heard| {
        said_early || !reports(heard).is_empty()
    })
    .await?;
    early.append(&mut heard.chats);
    heard.chats = early;
    if reports(&heard).is_empty() {
        return Err(TestFailure::Assertion(format!(
            "the reporting script said nothing; chat: {:?}",
            heard.chats
        )));
    }
    Ok(heard)
}

/// Every report of [`REPORTER`]'s in a window: the link number the speaker
/// gave itself, and the names in the order of their link numbers.
fn reports(heard: &Heard) -> Vec<(String, Vec<String>)> {
    heard
        .chats
        .iter()
        .filter_map(|(_speaker, text)| text.strip_prefix(REPORT))
        .filter_map(|report| report.split_once(':'))
        .map(|(own, names)| {
            (
                own.trim().to_owned(),
                names.split_whitespace().map(str::to_owned).collect(),
            )
        })
        .collect()
}

/// The names of a linkset of `prims` in the order of their link numbers, as
/// the last script to speak of such a set in a window said them.
fn numbers(heard: &Heard, prims: usize) -> String {
    reports(heard)
        .into_iter()
        .rfind(|(_own, names)| names.len() == prims)
        .map_or_else(
            || "nothing said".to_owned(),
            |(_own, names)| names.join(" "),
        )
}

/// The letter the metrics call `cube` by: `A` for the first of `cubes`.
fn letter(cubes: &[Object], cube: &Object) -> String {
    cubes
        .iter()
        .position(|other| other.local_id == cube.local_id)
        .and_then(|index| u8::try_from(index).ok())
        .map_or_else(
            || "?".to_owned(),
            |index| char::from(b'A'.saturating_add(index)).to_string(),
        )
}

/// Whose records a window brought, as the cubes' letters — or `none`.
fn records(heard: &Heard, cubes: &[Object]) -> String {
    let told: Vec<String> = cubes
        .iter()
        .filter(|cube| {
            heard
                .properties
                .iter()
                .any(|properties| properties.object_id == cube.full_id)
        })
        .map(|cube| letter(cubes, cube))
        .collect();
    if told.is_empty() {
        "none".to_owned()
    } else {
        told.join(" ")
    }
}

/// The order a window first named each of `cubes` in, as their letters.
fn arrival_order(heard: &Heard, cubes: &[Object]) -> String {
    let mut order: Vec<String> = Vec::new();
    for (local_id, _form) in &heard.updated {
        if let Some(cube) = cubes.iter().find(|cube| cube.local_id == *local_id) {
            let name = letter(cubes, cube);
            if !order.contains(&name) {
                order.push(name);
            }
        }
    }
    if order.is_empty() {
        "nothing".to_owned()
    } else {
        order.join(" ")
    }
}
