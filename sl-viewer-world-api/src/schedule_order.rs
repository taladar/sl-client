//! Reading back the ordering a built schedule will actually enforce.
//!
//! A plugin that schedules a *pipeline* — stage N's output is stage N+1's input
//! — states that with `.chain()` and `.after()` edges, and the prose above it
//! says which stage feeds which. Neither the prose nor the order the systems
//! are written in proves anything: the multithreaded executor runs whatever the
//! dependency graph allows, whenever it allows it, so an unordered pair that
//! happens to be *stored* in the right sequence is still free to run in either.
//! What is under test is therefore the ordering **edges**, and that they reach
//! from each stage to the next.
//!
//! This module is the harness for that assertion. It lives here, in the crate
//! whose [`WorldPhase`](crate::phases::WorldPhase) vocabulary those edges are
//! written against, because every world crate schedules a pipeline of its own
//! and the alternative was a third copy of the graph walk below.
//!
//! It is an ordinary module rather than a feature-gated one. Everything in it
//! is generic over the systems handed to it and it pulls in no dependency of
//! its own, so an app that never calls it never has it codegen'd — which is not
//! worth doubling a `cargo hack` feature powerset for.
//!
//! ```ignore
//! use sl_viewer_world_api::schedule_order::ScheduleOrder;
//! use sl_viewer_world_api::stage;
//!
//! let order = ScheduleOrder::of(my_pipeline());
//! order.assert_pipeline(&[stage!(first), stage!(second), stage!(third)]);
//! ```

use core::any::TypeId;
use std::collections::{HashMap, HashSet};

use bevy::ecs::schedule::{NodeId, ScheduleGraph, SystemKey};
use bevy::ecs::system::ScheduleSystem;
use bevy::prelude::*;

/// One stage of a pipeline: the name to report it by, and the type the
/// scheduler knows it as.
///
/// The identity has to be the system's type rather than its name because
/// [`System::name`] is only a real name with Bevy's `debug` feature, which this
/// workspace does not enable — without it every system in a schedule answers
/// "Enable the debug feature to see the name".
#[derive(Debug, Clone, Copy)]
pub struct Stage {
    /// What to call the stage when an assertion about it fails.
    pub name: &'static str,
    /// The concrete system type Bevy boxed the function into.
    pub system_type: TypeId,
}

/// Name a pipeline stage by its system function.
///
/// Expands to a [`Stage`] carrying both the path as written — which is what an
/// assertion failure reports — and the type Bevy will know the system by.
#[macro_export]
macro_rules! stage {
    ($system:path) => {
        $crate::schedule_order::Stage {
            name: stringify!($system),
            system_type: $crate::schedule_order::system_type($system),
        }
    };
}

/// The type Bevy will know `system` by once it is boxed into a schedule.
#[must_use]
pub fn system_type<M, S: IntoSystem<(), (), M>>(system: S) -> TypeId {
    System::system_type(&IntoSystem::into_system(system))
}

/// A built schedule, reduced to what an ordering assertion needs.
#[derive(Debug)]
pub struct ScheduleOrder {
    /// Each system in the schedule, by the type it was boxed from.
    keys: HashMap<TypeId, SystemKey>,
    /// For each system, the systems it is ordered ahead of.
    before: HashMap<SystemKey, Vec<SystemKey>>,
}

impl ScheduleOrder {
    /// Build `systems` into a bare schedule and flatten what it orders.
    ///
    /// Every system an edge under test names has to be in the same call: an edge
    /// naming a system that is not in the schedule constrains nothing, and Bevy
    /// silently accepts it.
    ///
    /// # Panics
    ///
    /// Panics if the configuration does not build into a schedule at all (an
    /// ordering cycle, say), or if the built schedule cannot report its systems
    /// — either way there is no order to assert about.
    #[must_use]
    pub fn of<M>(systems: impl IntoScheduleConfigs<ScheduleSystem, M>) -> Self {
        let mut world = World::new();
        let mut schedule = Schedule::default();
        schedule.add_systems(systems);
        let failure = schedule.initialize(&mut world).err();
        assert!(failure.is_none(), "the schedule builds: {failure:?}");
        let systems = schedule.systems();
        assert!(
            systems.is_ok(),
            "an initialized schedule reports its systems"
        );
        let keys = systems
            .into_iter()
            .flatten()
            .map(|(key, system)| (System::system_type(&**system), key))
            .collect();
        Self {
            keys,
            before: before_edges(schedule.graph()),
        }
    }

    /// The schedule's key for `stage`, if it is in the schedule at all.
    fn key(&self, stage: &Stage) -> Option<SystemKey> {
        self.keys.get(&stage.system_type).copied()
    }

    /// Whether the schedule's ordering edges reach `later` from `earlier`.
    fn orders(&self, earlier: SystemKey, later: SystemKey) -> bool {
        let mut seen: HashSet<SystemKey> = HashSet::new();
        let mut pending = vec![earlier];
        while let Some(key) = pending.pop() {
            for &next in self.before.get(&key).into_iter().flatten() {
                if next == later {
                    return true;
                }
                if seen.insert(next) {
                    pending.push(next);
                }
            }
        }
        false
    }

    /// Assert the schedule orders `stages` one after another, each stage after
    /// the one before it.
    ///
    /// # Panics
    ///
    /// Panics naming the first stage that is missing from the schedule, or the
    /// first neighbouring pair nothing orders.
    #[track_caller]
    pub fn assert_pipeline(&self, stages: &[Stage]) {
        let mut previous: Option<(SystemKey, &Stage)> = None;
        for stage in stages {
            let key = self.key(stage);
            assert!(
                key.is_some(),
                "{} is not in the schedule at all",
                stage.name
            );
            if let (Some((earlier_key, earlier)), Some(key)) = (previous, key) {
                assert!(
                    self.orders(earlier_key, key),
                    "nothing orders {} before {}, so the scheduler may run them in \
                     either order",
                    earlier.name,
                    stage.name
                );
            }
            if let Some(key) = key {
                previous = Some((key, stage));
            }
        }
    }
}

/// The "runs before" relation the scheduler will enforce, from each system to
/// the systems it is ordered ahead of.
///
/// Bevy states ordering between *nodes*, and a node can be a set standing for
/// any number of systems, so an edge only constrains real systems once both of
/// its ends are expanded through the hierarchy — which is what the schedule
/// builder does before it sorts.
fn before_edges(graph: &ScheduleGraph) -> HashMap<SystemKey, Vec<SystemKey>> {
    let mut edges: HashMap<SystemKey, Vec<SystemKey>> = HashMap::new();
    for (from, to) in graph.dependency().graph().all_edges() {
        let mut earlier = Vec::new();
        systems_under(graph, from, &mut earlier);
        let mut later = Vec::new();
        systems_under(graph, to, &mut later);
        for key in earlier {
            edges.entry(key).or_default().extend(later.iter().copied());
        }
    }
    edges
}

/// Collect every system `node` stands for: itself, or — for a set — everything
/// anywhere beneath it in the hierarchy.
fn systems_under(graph: &ScheduleGraph, node: NodeId, into: &mut Vec<SystemKey>) {
    match node {
        NodeId::System(key) => into.push(key),
        NodeId::Set(_) => {
            for child in graph.hierarchy().graph().neighbors(node) {
                systems_under(graph, child, into);
            }
        }
    }
}
