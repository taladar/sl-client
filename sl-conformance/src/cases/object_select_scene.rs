//! Select what the region already holds: objects the agent does not own, in
//! its own region and in a neighbouring one, and an avatar.
//!
//! [`super::object_properties`] reads the record of a cube it rezzed. A build
//! floater is as often pointed at something else — a stranger's object, one
//! across a region border, and (by a script or a careless click) something
//! that is no object at all. This case watches a region arrive, keeps what it
//! streamed, and selects a sample of each kind in one `ObjectSelect`
//! ([`Command::RequestObjectProperties`]), counting the records that come
//! back:
//!
//! - root prims of the agent's own region;
//! - child prims of the agent's own region;
//! - root prims a neighbouring region streamed down its child circuit, asked
//!   of that region;
//! - the agent's own avatar.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/objects.md` § Properties.
//!
//! `1av`, `[both]`, offline on both fake flavours. Nothing is rezzed or
//! changed.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, CircuitId, Command, Event, Object, ObjectKey, ObjectProperties, OwnerKey,
    RegionLocalObjectId, ScopedObjectId, pcode,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, count_metric};

/// The OpenSim start location: the middle of the "Default Region", whose three
/// neighbours each hold an object or two.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// How long the arrival is watched for: long enough for the neighbours' child
/// circuits to open and stream.
const ARRIVAL_WINDOW: Duration = Duration::from_secs(30);

/// The idle gap that ends the watch early.
const ARRIVAL_IDLE: Duration = Duration::from_secs(8);

/// How long a select is listened to. Every record measured on either grid
/// came within a second.
const QUIET: Duration = Duration::from_secs(5);

/// [`ARRIVAL_IDLE`] and [`QUIET`] on a fake grid, which answers over loopback
/// in well under a millisecond and is run against on every `cargo test`.
const FAKE_IDLE: Duration = Duration::from_secs(1);

/// How many objects of each kind are selected.
const SAMPLE: usize = 8;

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/objects.md § Properties (object-select-scene, 2026-10-09)";

/// Whether every sampled root prim of the agent's own region is answered.
const ANSWERS_EVERY_ROOT: Measured<bool> = Measured {
    second_life: true,
    opensim: true,
    source: SOURCE,
};

/// Whether every sampled child prim of the agent's own region is answered.
const ANSWERS_EVERY_CHILD: Measured<bool> = Measured {
    second_life: true,
    opensim: true,
    source: SOURCE,
};

/// Whether every sampled root prim of a neighbouring region is answered by
/// that region.
const ANSWERS_EVERY_NEIGHBOURS: Measured<bool> = Measured {
    second_life: true,
    opensim: true,
    source: SOURCE,
};

/// How many records a select of the agent's own avatar brings.
const AVATAR_RECORDS: Measured<usize> = Measured {
    second_life: 0,
    opensim: 0,
    source: SOURCE,
};

/// Selects a sample of the objects a region streamed at arrival — roots,
/// children, a neighbour's, an avatar — and counts the records that answer.
#[derive(Debug)]
pub struct ObjectSelectScene;

impl GridTest for ObjectSelectScene {
    fn name(&self) -> &'static str {
        "object-select-scene"
    }

    fn description(&self) -> &'static str {
        "Select objects the agent does not own, a neighbouring region's, and an avatar"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        match grid {
            Grid::Aditi => super::teleport_cross_region::ADITI_START,
            Grid::Opensim => OPENSIM_START,
            Grid::FakeSl | Grid::FakeOpensim => "last",
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            let agent = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
            session.wait_for_region(REGION_TIMEOUT).await?;
            let (idle, quiet) = if grid.is_fake() {
                (FAKE_IDLE, FAKE_IDLE)
            } else {
                (ARRIVAL_IDLE, QUIET)
            };
            let scene = watch_arrival(session, idle).await?;
            let root_circuit = session
                .circuit_id()
                .ok_or_else(|| TestFailure::State("the session has no root circuit".to_owned()))?;

            let samples = samples(&scene, root_circuit, agent);

            let mut answers = Vec::new();
            for chosen in &samples {
                answers.push((chosen.len(), select(session, chosen, quiet).await?));
            }
            let metrics = ctx.metrics();
            for (what, (selected, answer)) in ["root", "child", "neighbour", "avatar"]
                .into_iter()
                .zip(&answers)
            {
                metrics.set(&count_metric(&format!("{what}_selected")), count(*selected));
                metrics.set(
                    &count_metric(&format!("{what}_answered")),
                    count(answer.answered.len()),
                );
                metrics.set(
                    &count_metric(&format!("{what}_answered_of_strangers")),
                    count(
                        answer
                            .records
                            .iter()
                            .filter(|record| record.owner != OwnerKey::Agent(agent))
                            .count(),
                    ),
                );
            }

            let [root, child, neighbour, own_avatar] = answers.as_slice() else {
                return Err(TestFailure::State("four samples were selected".to_owned()));
            };
            check(
                root.0 > 0,
                "the arrival streamed no root prim of the agent's region to select",
            )?;
            ANSWERS_EVERY_ROOT.check(
                "whether every selected root prim was answered",
                grid,
                &(root.1.answered.len() == root.0),
            )?;
            if child.0 > 0 {
                ANSWERS_EVERY_CHILD.check(
                    "whether every selected child prim was answered",
                    grid,
                    &(child.1.answered.len() == child.0),
                )?;
            }
            if neighbour.0 > 0 {
                ANSWERS_EVERY_NEIGHBOURS.check(
                    "whether every selected prim of a neighbouring region was answered",
                    grid,
                    &(neighbour.1.answered.len() == neighbour.0),
                )?;
            } else if !grid.is_fake() {
                ctx.mark_partial("no neighbouring region streamed a prim to select");
            }
            if own_avatar.0 > 0 {
                AVATAR_RECORDS.check(
                    "how many records a select of the agent's own avatar brought",
                    grid,
                    &own_avatar.1.records.len(),
                )?;
            }
            Ok(())
        })
    }
}

/// The four samples of a scene, in the order the case reports them: root
/// prims and child prims of the agent's own region, root prims of a
/// neighbouring one, and the agent's own avatar.
fn samples(
    scene: &[Object],
    root_circuit: CircuitId,
    agent: AgentKey,
) -> [Vec<(ScopedObjectId, ObjectKey)>; 4] {
    let is_prim = |object: &&Object| object.pcode == pcode::PRIMITIVE;
    let is_root = |object: &&Object| object.parent_id == RegionLocalObjectId(0);
    let here = |object: &&Object| object.circuit == root_circuit;
    [
        sample(scene.iter().filter(is_prim).filter(is_root).filter(here)),
        sample(
            scene
                .iter()
                .filter(is_prim)
                .filter(|object| !is_root(object))
                .filter(here),
        ),
        sample(
            scene
                .iter()
                .filter(is_prim)
                .filter(is_root)
                .filter(|object| !here(object)),
        ),
        sample(
            scene
                .iter()
                .filter(|object| object.full_id.uuid() == agent.uuid()),
        ),
    ]
}

/// A count as a metric value.
fn count(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(-1)
}

/// Up to [`SAMPLE`] of `objects`, lowest local id first — a region's oldest
/// objects, which are the ones that are there on the next run too.
fn sample<'a>(objects: impl Iterator<Item = &'a Object>) -> Vec<(ScopedObjectId, ObjectKey)> {
    let mut chosen: Vec<(ScopedObjectId, ObjectKey)> = objects
        .map(|object| (object.scoped_id(), object.full_id))
        .collect();
    chosen.sort_unstable();
    chosen.dedup();
    chosen.truncate(SAMPLE);
    chosen
}

/// Everything the arrival streamed, until [`ARRIVAL_WINDOW`] has passed — or
/// sooner, once a neighbouring region has streamed something and nothing new
/// has come for `idle`. A neighbour's child circuit opens after the
/// agent's own region has gone quiet, so an idle gap alone ends the watch
/// before there is a neighbour to select from.
async fn watch_arrival(session: &mut Session, idle: Duration) -> Result<Vec<Object>, TestFailure> {
    let mut scene: Vec<Object> = Vec::new();
    let started = Instant::now();
    loop {
        let remaining = ARRIVAL_WINDOW.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(scene);
        }
        match session
            .wait_for(remaining.min(idle), |event| match event {
                Event::ObjectAdded(object) => Some((**object).clone()),
                _ => None,
            })
            .await
        {
            Ok(object) => scene.push(object),
            Err(TestFailure::Timeout(_)) => {
                let root = session.circuit_id();
                if scene.iter().any(|object| Some(object.circuit) != root) {
                    return Ok(scene);
                }
            }
            Err(other) => return Err(other),
        }
    }
}

/// What one select was answered with.
#[derive(Debug, Default)]
struct Answer {
    /// Every record that came, in order.
    records: Vec<ObjectProperties>,
    /// The selected objects a record came for.
    answered: BTreeSet<ObjectKey>,
}

/// Selects `chosen` in one message, listens for `quiet`, and deselects.
async fn select(
    session: &mut Session,
    chosen: &[(ScopedObjectId, ObjectKey)],
    quiet: Duration,
) -> Result<Answer, TestFailure> {
    let mut answer = Answer::default();
    if chosen.is_empty() {
        return Ok(answer);
    }
    let local_ids: Vec<ScopedObjectId> = chosen.iter().map(|(local_id, _key)| *local_id).collect();
    session
        .send(Command::RequestObjectProperties {
            local_ids: local_ids.clone(),
        })
        .await?;
    let mut records = Vec::new();
    let outcome = session
        .wait_for(quiet, |event| {
            if let Event::ObjectProperties(properties) = event {
                records.push((**properties).clone());
            }
            None::<()>
        })
        .await;
    match outcome {
        Ok(()) | Err(TestFailure::Timeout(_)) => {}
        Err(other) => return Err(other),
    }
    session.send(Command::DeselectObjects { local_ids }).await?;
    answer.answered = records
        .iter()
        .map(|record| record.object_id)
        .filter(|object_id| chosen.iter().any(|(_local_id, key)| key == object_id))
        .collect();
    answer.records = records;
    Ok(answer)
}
