//! What a grid makes of a link that joins prims of two owners, and of a link
//! and a delink of somebody else's prims.
//!
//! [`super::object_link_delink`] measures an avatar linking its own prims.
//! Here the **owner** (the primary) rezzes two cubes, `A` and `B`, and the
//! **stranger** (the secondary), who holds no right over them, rezzes a third
//! of its own, `X`. After each attempt the owner asks the grid for all three
//! again, so that whether the attempt took is read from the grid and not from
//! what either side was sent:
//!
//! 1. **Somebody else's prims.** The stranger links `A` and `B`.
//! 2. **A prim of each.** The stranger links `A` under its own `X`, and its
//!    own `X` under `A`; the owner does the same from its side.
//! 3. **Somebody else's set.** The owner links `A` and `B`; the stranger
//!    delinks the child, and then the whole set.
//!
//! What each live grid answered is stated in `MEASURED` and written up in
//! `book/src/gridspec/building.md` § Linking; every run is held to it. The
//! fake grid lets anybody edit anything
//! (`server-fake-grid-edit-permission-enforcement`), so the case is live
//! only.
//!
//! `2av`, `[both]`. Everything either avatar left standing is derezzed to its
//! Trash at the end, also when a leg fails.

use std::time::Duration;

use sl_client_tokio::{Command, Event, Object, ObjectKey, RegionLocalObjectId, Vector};

use crate::cases::object_edit::reread;
use crate::cases::object_rez_derez::{
    Answer, Folders, Heard, Pace, REZ_LIFT_M, Residue, Run, SETTLE_WINDOW, STEP_TIMEOUT, clean_up,
    joined, listen, rez_cube,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, is_opensim, settle_scene};

/// The OpenSim start location: the "Default Region" (1000,1000), centred.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// How far along the region's y axis the stranger's cube stands from the
/// owner's, in metres.
const STRANGERS_ROW_M: f32 = 3.0;

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/building.md § Linking (object-link-stranger, 2026-10-11)";

/// What each grid was measured to answer, by the metric the case records it
/// under. Every run is held to all of it at the end.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "owner_links_its_own_under_the_strangers",
        both(Answer::Text("nothing linked")),
    ),
    (
        "owner_links_its_own_under_the_strangers_alerts",
        each(
            Answer::Text(
                "Link failed because you do not have edit permission. [LinkFailedNoPermToEdit]",
            ),
            Answer::Text("none"),
        ),
    ),
    (
        "owner_links_the_strangers_under_its_own",
        both(Answer::Text("nothing linked")),
    ),
    (
        "owner_links_the_strangers_under_its_own_alerts",
        each(
            Answer::Text("Link failed -- owners differ [LinkFailedOwnersDiffer]"),
            Answer::Text("none"),
        ),
    ),
    ("owners_own_link", both(Answer::Text("B under A"))),
    ("owners_own_link_alerts", both(Answer::Text("none"))),
    (
        "stranger_links_its_own_under_the_owners",
        both(Answer::Text("nothing linked")),
    ),
    (
        "stranger_links_its_own_under_the_owners_alerts",
        each(
            Answer::Text(
                "Link failed because you do not have edit permission. [LinkFailedNoPermToEdit]",
            ),
            Answer::Text("none"),
        ),
    ),
    (
        "stranger_links_the_owners_under_its_own",
        both(Answer::Text("nothing linked")),
    ),
    (
        "stranger_links_the_owners_under_its_own_alerts",
        each(
            Answer::Text("Link failed -- owners differ [LinkFailedOwnersDiffer]"),
            Answer::Text("none"),
        ),
    ),
    (
        "strangers_delink_of_a_child",
        both(Answer::Text("B under A")),
    ),
    (
        "strangers_delink_of_a_child_alerts",
        each(
            Answer::Text("Unlink failed: no edit permissions."),
            Answer::Text("none"),
        ),
    ),
    ("strangers_delink_of_a_set", both(Answer::Text("B under A"))),
    (
        "strangers_delink_of_a_set_alerts",
        each(
            Answer::Text("Unlink failed: no edit permissions."),
            Answer::Text("none"),
        ),
    ),
    (
        "strangers_link_of_the_owners_two",
        both(Answer::Text("nothing linked")),
    ),
    (
        "strangers_link_of_the_owners_two_alerts",
        each(
            Answer::Text(
                "Link failed because you do not have edit permission. [LinkFailedNoPermToEdit]",
            ),
            Answer::Text("none"),
        ),
    ),
];

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

/// Holds what the case recorded to [`MEASURED`].
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
    Err(TestFailure::Assertion(format!(
        "{grid}: the grid no longer matches {SOURCE}: {}",
        failures.join("; ")
    )))
}

/// Measures what a link across two owners, and a link and a delink of
/// somebody else's prims, are answered with.
#[derive(Debug)]
pub struct ObjectLinkStranger;

impl GridTest for ObjectLinkStranger {
    fn name(&self) -> &'static str {
        "object-link-stranger"
    }

    fn description(&self) -> &'static str {
        "What a link of somebody else's prims, a link of a prim of each of two owners and a \
         delink of somebody else's set are answered with"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
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

    fn timeout(&self) -> Duration {
        Duration::from_secs(900)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let (owners, strangers) = {
                let Some((owner, stranger)) = ctx.primary_and_secondary() else {
                    return Err(TestFailure::Assertion(
                        "object-link-stranger needs a secondary avatar".to_owned(),
                    ));
                };
                (folders_of(owner).await?, folders_of(stranger).await?)
            };
            let mut left = Left::default();
            let mut metrics = Metrics::new();
            let outcome = measure(ctx, owners, strangers, &mut left, &mut metrics).await;
            let held = outcome.and_then(|()| hold_to_measured(grid, &metrics));
            ctx.metrics().merge(metrics);
            let mut cleanup = Ok(());
            if let Some((owner, stranger)) = ctx.primary_and_secondary() {
                cleanup = cleanup
                    .and(clean_up(stranger, &left.strangers, strangers.trash).await)
                    .and(clean_up(owner, &left.owners, owners.trash).await);
            }
            held.and(cleanup)
        })
    }
}

/// What the two avatars left behind.
#[derive(Debug, Default)]
struct Left {
    /// The owner's objects.
    owners: Residue,
    /// The stranger's.
    strangers: Residue,
}

/// The well-known folders of `session`'s login skeleton.
async fn folders_of(session: &mut Session) -> Result<Folders, TestFailure> {
    let folders = session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::InventorySkeleton(folders) => Some(folders.clone()),
            _ => None,
        })
        .await?;
    Folders::of(&folders)
        .ok_or_else(|| TestFailure::Assertion("inventory skeleton had no root folder".to_owned()))
}

/// Settles `session`'s scene and returns what it needs to rez and listen.
async fn arrive(
    session: &mut Session,
    grid: Grid,
    build_position: Option<Vector>,
    folders: Folders,
) -> Result<Run, TestFailure> {
    let pace = Pace::of(grid);
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
    Ok(Run {
        folders,
        owner,
        pace,
        position: Vector {
            x: anchor.x,
            y: anchor.y,
            z: anchor.z + REZ_LIFT_M,
        },
        seen,
    })
}

/// One of the three cubes, as each of the two avatars knows it: the
/// region-local id is the same, the circuit it is named down is not.
#[derive(Debug, Clone)]
struct Cube {
    /// The cube as the owner of `A` and `B` last read it — where the grid's
    /// own account of what hangs off what is read from.
    owners: Object,
    /// The cube as the stranger knows it.
    strangers: Object,
}

/// Who sends an attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum By {
    /// The owner of `A` and `B`.
    Owner,
    /// The owner of `X`.
    Stranger,
}

/// The legs, in order.
async fn measure(
    ctx: &mut TestContext,
    owners: Folders,
    strangers: Folders,
    left: &mut Left,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let Some((owner, stranger)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State("the secondary went away".to_owned()));
    };
    let mut owner_run = arrive(owner, grid, build_position.clone(), owners).await?;
    let mut stranger_run = arrive(stranger, grid, build_position, strangers).await?;
    stranger_run.position = Vector {
        y: owner_run.position.y + STRANGERS_ROW_M,
        ..owner_run.position.clone()
    };

    let mut cubes = Vec::with_capacity(3);
    for slot in 0..2 {
        let owners = rez_cube(owner, &mut owner_run, &mut left.owners, slot).await?;
        let strangers = find_object(stranger, owners.full_id).await?;
        cubes.push(Cube { owners, strangers });
    }
    let strangers = rez_cube(stranger, &mut stranger_run, &mut left.strangers, 0).await?;
    let owners = find_object(owner, strangers.full_id).await?;
    cubes.push(Cube { owners, strangers });
    let [a, b, x] = cubes.as_mut_slice() else {
        return Err(TestFailure::State("the case rezzes three cubes".to_owned()));
    };
    let mut sides = Sides {
        owner,
        stranger,
        owner_run: &owner_run,
        stranger_run: &stranger_run,
    };

    // --- 1. Somebody else's prims. ---
    attempt(
        &mut sides,
        metrics,
        "strangers_link_of_the_owners_two",
        By::Stranger,
        true,
        [a, b, x],
        [0, 1],
    )
    .await?;
    free(&mut sides, [a, b, x]).await?;

    // --- 2. A prim of each. ---
    for (key, by, order) in [
        (
            "stranger_links_the_owners_under_its_own",
            By::Stranger,
            [2, 0],
        ),
        (
            "stranger_links_its_own_under_the_owners",
            By::Stranger,
            [0, 2],
        ),
        ("owner_links_the_strangers_under_its_own", By::Owner, [0, 2]),
        ("owner_links_its_own_under_the_strangers", By::Owner, [2, 0]),
    ] {
        attempt(&mut sides, metrics, key, by, true, [a, b, x], order).await?;
        free(&mut sides, [a, b, x]).await?;
    }

    // --- 3. Somebody else's set. ---
    attempt(
        &mut sides,
        metrics,
        "owners_own_link",
        By::Owner,
        true,
        [a, b, x],
        [0, 1],
    )
    .await?;
    attempt(
        &mut sides,
        metrics,
        "strangers_delink_of_a_child",
        By::Stranger,
        false,
        [a, b, x],
        [1, 1],
    )
    .await?;
    attempt(
        &mut sides,
        metrics,
        "strangers_delink_of_a_set",
        By::Stranger,
        false,
        [a, b, x],
        [0, 1],
    )
    .await?;
    free(&mut sides, [a, b, x]).await
}

/// The two sessions and what each knows of the run.
struct Sides<'a> {
    /// The owner of `A` and `B`.
    owner: &'a mut Session,
    /// The owner of `X`.
    stranger: &'a mut Session,
    /// The owner's run.
    owner_run: &'a Run,
    /// The stranger's.
    stranger_run: &'a Run,
}

/// `by` links — or delinks — the two of `cubes` that `named` picks, the first
/// the root, after a while without a word; what it hears and what the grid
/// then says hangs off what are recorded under `key`.
async fn attempt(
    sides: &mut Sides<'_>,
    metrics: &mut Metrics,
    key: &str,
    by: By,
    link: bool,
    cubes: [&mut Cube; 3],
    named: [usize; 2],
) -> Result<(), TestFailure> {
    let (session, run) = match by {
        By::Owner => (&mut *sides.owner, sides.owner_run),
        By::Stranger => (&mut *sides.stranger, sides.stranger_run),
    };
    let mut local_ids = Vec::new();
    for index in named {
        let cube = cubes
            .get(index)
            .ok_or_else(|| TestFailure::State("the case rezzes three cubes".to_owned()))?;
        let id = match by {
            By::Owner => cube.owners.scoped_id(),
            By::Stranger => cube.strangers.scoped_id(),
        };
        if !local_ids.contains(&id) {
            local_ids.push(id);
        }
    }
    let watched: Vec<RegionLocalObjectId> = cubes.iter().map(|cube| cube.owners.local_id).collect();
    // Second Life does not say the same thing twice within a few seconds.
    let pause = run.pace.quiet.saturating_mul(5);
    listen(session, pause, pause, |_heard| false).await?;
    session
        .send(if link {
            Command::LinkObjects { local_ids }
        } else {
            Command::DelinkObjects { local_ids }
        })
        .await?;
    let heard = listen(
        session,
        run.pace.quiet.saturating_mul(2),
        run.pace.quiet.checked_div(2).unwrap_or(run.pace.quiet),
        |heard: &Heard| {
            !heard.alerts.is_empty()
                || heard
                    .updated
                    .iter()
                    .any(|(local_id, _form)| watched.contains(local_id))
        },
    )
    .await?;
    metrics.set(&format!("{key}_alerts"), joined(&heard.alerts));
    let [a, b, x] = cubes;
    for cube in [&mut *a, &mut *b, &mut *x] {
        reread(sides.owner, sides.owner_run, &mut cube.owners).await?;
    }
    metrics.set(key, hangs(&[("A", a), ("B", b), ("X", x)]));
    Ok(())
}

/// What hangs off what, as the owner read it: `B under A`, or `nothing
/// linked`.
fn hangs(cubes: &[(&str, &mut Cube)]) -> String {
    let linked: Vec<String> = cubes
        .iter()
        .filter_map(|(name, cube)| {
            let parent = cube.owners.parent_id;
            cubes
                .iter()
                .find(|(_root, other)| parent.0 != 0 && other.owners.local_id == parent)
                .map(|(root, _other)| format!("{name} under {root}"))
        })
        .collect();
    if linked.is_empty() {
        "nothing linked".to_owned()
    } else {
        linked.join(", ")
    }
}

/// Both avatars delink everything, so that the next attempt starts with
/// three free cubes whoever the grid let link what.
async fn free(sides: &mut Sides<'_>, cubes: [&mut Cube; 3]) -> Result<(), TestFailure> {
    if cubes.iter().all(|cube| cube.owners.parent_id.0 == 0) {
        return Ok(());
    }
    let owners: Vec<_> = cubes.iter().map(|cube| cube.owners.scoped_id()).collect();
    let strangers: Vec<_> = cubes
        .iter()
        .map(|cube| cube.strangers.scoped_id())
        .collect();
    sides
        .owner
        .send(Command::DelinkObjects { local_ids: owners })
        .await?;
    sides
        .stranger
        .send(Command::DelinkObjects {
            local_ids: strangers,
        })
        .await?;
    let quiet = sides.owner_run.pace.quiet;
    listen(sides.owner, quiet, quiet, |_heard| false).await?;
    listen(sides.stranger, quiet, quiet, |_heard| false).await?;
    for cube in cubes {
        reread(sides.owner, sides.owner_run, &mut cube.owners).await?;
    }
    Ok(())
}

/// The object `object_id` as `session` knows it, waiting for the region to
/// send it if it has not yet.
async fn find_object(session: &mut Session, object_id: ObjectKey) -> Result<Object, TestFailure> {
    session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                if object.full_id == object_id =>
            {
                Some((**object).clone())
            }
            _ => None,
        })
        .await
        .map_err(|failure| match failure {
            TestFailure::Timeout(_) => TestFailure::Assertion(
                "one avatar was never sent the cube the other rezzed".to_owned(),
            ),
            other => other,
        })
}
