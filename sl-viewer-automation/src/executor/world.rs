//! The requests on the world: finds and waits over a world locator, the
//! actions aimed through the viewer's own pick, a drag of a transform handle
//! and the rubber band.

use std::collections::BTreeSet;

use bevy::prelude::*;
use sl_automation_proto::{
    AutomationError, Deadline, DragAmount, DragModifiers, ResponseBody, SnapSide, WorldAction,
    WorldLocator, WorldNode, WorldWaitCondition,
};
use sl_client_bevy::Uuid;
use sl_viewer_ui_core::synthetic_input::InputActionId;
use sl_viewer_world_api::{
    ManipulatorAmount, ManipulatorAxis, ManipulatorHandle, ManipulatorQuery, SelectionSet,
    SnapRegime,
};

use super::{Clock, Started, Step, Task, automation_error, enqueue, finished};
use crate::manipulator_drag::{DragProgress, HeldKeys, ManipulatorDrag};
use crate::pursuit::{Intent, Progress, Pursuit};
use crate::world_aim::{AimProgress, WorldAim, WorldIntent, WorldTarget};
use crate::world_query::{WorldProgress, WorldQuery, WorldWant};
use crate::world_sweep::{SweepProgress, WorldSweep};

/// How many frames a rubber band waits, after its drag, for the selection to
/// be its targets before it reports what was selected instead.
const SWEEP_CONFIRM_FRAMES: u32 = 10;

/// Every in-world thing `locator` matches.
pub(super) fn find(locator: WorldLocator, deadline: Deadline) -> Started {
    running(WorldTask::Find(Box::new(
        WorldQuery::new(locator, WorldWant::All).with_deadline(deadline),
    )))
}

/// A wait until `locator`'s matches satisfy `condition`.
pub(super) fn wait(
    locator: WorldLocator,
    condition: WorldWaitCondition,
    deadline: Deadline,
) -> Started {
    running(WorldTask::Wait(Box::new(WorldWait {
        query: WorldQuery::new(locator.clone(), WorldWant::All).with_deadline(deadline),
        locator,
        condition,
        clock: Clock::new(deadline),
        last_observed: Vec::new(),
    })))
}

/// `action` on the one thing `locator` names.
pub(super) fn act(
    locator: WorldLocator,
    action: WorldAction,
    reveal: bool,
    deadline: Deadline,
) -> Started {
    let aim = |intent: WorldIntent| {
        let aim = WorldAim::new(locator.clone(), intent).with_deadline(deadline);
        if reveal { aim } else { aim.without_reveal() }
    };
    let stage = match action {
        WorldAction::Click => ActStage::Aim(Box::new(aim(WorldIntent::Click))),
        WorldAction::RightClick => ActStage::Aim(Box::new(aim(WorldIntent::RightClick))),
        WorldAction::Hover => ActStage::Aim(Box::new(aim(WorldIntent::Hover))),
        WorldAction::Select => ActStage::Aim(Box::new(aim(WorldIntent::Select))),
        WorldAction::DropFrom(source) => ActStage::Source {
            pursuit: Box::new(Pursuit::new(source, Intent::Click).with_deadline(deadline)),
        },
    };
    running(WorldTask::Act(Box::new(WorldAct {
        locator,
        reveal,
        deadline,
        stage,
    })))
}

/// A drag of the transform handle `handle` by `amount`.
pub(super) fn drag(
    handle: &str,
    amount: DragAmount,
    snap: SnapSide,
    modifiers: DragModifiers,
    deadline: Deadline,
) -> Started {
    let Some(handle) = parse_handle(handle) else {
        return Started::answered(Err(Box::new(AutomationError::InvalidRequest {
            reason: format!(
                "{handle:?} is not a transform handle (translate-x, translate-plane-z, rotate-y, \
                 scale-face-x-pos, scale-corner-pnp, …)"
            ),
        })));
    };
    let query = ManipulatorQuery {
        handle,
        amount: match amount {
            DragAmount::Distance(metres) => ManipulatorAmount::Distance(metres),
            DragAmount::Offset(metres) => ManipulatorAmount::Offset(metres),
            DragAmount::Angle(radians) => ManipulatorAmount::Angle(radians),
            DragAmount::Factor(factor) => ManipulatorAmount::Factor(factor),
        },
        regime: match snap {
            SnapSide::Free => SnapRegime::Free,
            SnapSide::Grid => SnapRegime::Grid,
        },
    };
    let keys = match modifiers {
        DragModifiers::None => HeldKeys::None,
        DragModifiers::Shift => HeldKeys::Shift,
        DragModifiers::Ctrl => HeldKeys::Ctrl,
        DragModifiers::CtrlShift => HeldKeys::CtrlShift,
    };
    running(WorldTask::Drag(Box::new(
        ManipulatorDrag::new(query, keys).with_deadline(deadline),
    )))
}

/// A rubber band that selects exactly the things `locator` names.
pub(super) fn sweep(locator: WorldLocator, deadline: Deadline) -> Started {
    running(WorldTask::Sweep(Box::new(Sweep {
        planning: WorldSweep::new(locator.clone()).with_deadline(deadline),
        locator,
        stage: SweepStage::Plan,
    })))
}

/// `task`, under way.
fn running(task: WorldTask) -> Started {
    Started::Running(Task::world(task))
}

/// The transform handle whose test address is `slug`.
fn parse_handle(slug: &str) -> Option<ManipulatorHandle> {
    let mut handles = Vec::new();
    for axis in [ManipulatorAxis::X, ManipulatorAxis::Y, ManipulatorAxis::Z] {
        handles.extend([
            ManipulatorHandle::Translate(axis),
            ManipulatorHandle::TranslatePlane(axis),
            ManipulatorHandle::Rotate(axis),
            ManipulatorHandle::StretchFace(axis, false),
            ManipulatorHandle::StretchFace(axis, true),
        ]);
    }
    for x in [false, true] {
        for y in [false, true] {
            for z in [false, true] {
                handles.push(ManipulatorHandle::StretchCorner([x, y, z]));
            }
        }
    }
    handles.into_iter().find(|handle| handle.slug() == slug)
}

/// A world request under way.
pub(super) enum WorldTask {
    /// Every match.
    Find(Box<WorldQuery>),
    /// A wait over the matches.
    Wait(Box<WorldWait>),
    /// An action on one thing.
    Act(Box<WorldAct>),
    /// A handle drag.
    Drag(Box<ManipulatorDrag>),
    /// A rubber band.
    Sweep(Box<Sweep>),
}

impl WorldTask {
    /// Whether it plays input.
    pub(super) const fn acts(&self) -> bool {
        matches!(self, Self::Act(_) | Self::Drag(_) | Self::Sweep(_))
    }

    /// Advance it by a frame.
    pub(super) fn poll(&mut self, world: &mut World) -> Step {
        match self {
            Self::Find(query) => match query.poll(world) {
                Ok(WorldProgress::Ready(nodes)) => Step::done(ResponseBody::FoundWorld { nodes }),
                Ok(WorldProgress::Waiting { .. }) => Step::Pending,
                Err(error) => Step::fail(automation_error(error)),
            },
            Self::Wait(wait) => wait.poll(world),
            Self::Act(act) => act.poll(world),
            Self::Drag(drag) => match drag.poll(world) {
                Ok(DragProgress::Done(predicted)) => Step::done(ResponseBody::Dragged {
                    predicted: match predicted {
                        ManipulatorAmount::Distance(metres) => DragAmount::Distance(metres),
                        ManipulatorAmount::Offset(metres) => DragAmount::Offset(metres),
                        ManipulatorAmount::Angle(radians) => DragAmount::Angle(radians),
                        ManipulatorAmount::Factor(factor) => DragAmount::Factor(factor),
                    },
                }),
                Ok(DragProgress::Waiting(_stage)) => Step::Pending,
                Err(error) => Step::fail(automation_error(error)),
            },
            Self::Sweep(sweep) => sweep.poll(world),
        }
    }
}

/// A wait over the things a world locator matches.
pub(super) struct WorldWait {
    /// The resolution, polled each frame.
    query: WorldQuery,
    /// The things.
    locator: WorldLocator,
    /// What they must come to satisfy.
    condition: WorldWaitCondition,
    /// Its deadline.
    clock: Clock,
    /// The matches in the last frame they could be told.
    last_observed: Vec<WorldNode>,
}

impl WorldWait {
    /// Advance it by a frame.
    fn poll(&mut self, world: &mut World) -> Step {
        self.clock.tick();
        match self.query.poll(world) {
            Ok(WorldProgress::Ready(nodes)) => {
                let holds = match self.condition {
                    WorldWaitCondition::Attached => !nodes.is_empty(),
                    WorldWaitCondition::Detached => nodes.is_empty(),
                };
                if holds {
                    return Step::done(ResponseBody::WorldSatisfied { nodes });
                }
                self.last_observed = nodes;
            }
            Ok(WorldProgress::Waiting { .. }) => {}
            Err(error) => return Step::fail(automation_error(error)),
        }
        match self.clock.expired() {
            Some((frames, millis)) => Step::fail(AutomationError::WorldTimedOut {
                locator: self.locator.clone(),
                failed_check: None,
                unresolved: Vec::new(),
                last_observed: core::mem::take(&mut self.last_observed),
                frames,
                millis,
            }),
            None => Step::Pending,
        }
    }
}

/// Where a world action stands.
enum ActStage {
    /// A drop waiting for the UI node it drags from.
    Source {
        /// The wait for it.
        pursuit: Box<Pursuit>,
    },
    /// Aiming at the thing.
    Aim(Box<WorldAim>),
    /// Playing the gesture on the thing.
    Play(InputActionId, Box<WorldTarget>),
}

/// An action on the one thing a world locator names.
pub(super) struct WorldAct {
    /// The thing.
    locator: WorldLocator,
    /// Whether the camera may frame it.
    reveal: bool,
    /// The deadline each wait runs under.
    deadline: Deadline,
    /// Where it stands.
    stage: ActStage,
}

impl WorldAct {
    /// Advance it by a frame.
    fn poll(&mut self, world: &mut World) -> Step {
        match &mut self.stage {
            ActStage::Source { pursuit } => match pursuit.poll(world) {
                Ok(Progress::Ready(target)) => {
                    let aim =
                        WorldAim::new(self.locator.clone(), WorldIntent::DropFrom(target.aim))
                            .with_deadline(self.deadline);
                    self.stage = ActStage::Aim(Box::new(if self.reveal {
                        aim
                    } else {
                        aim.without_reveal()
                    }));
                    Step::Pending
                }
                Ok(Progress::Waiting(_check)) => Step::Pending,
                Err(error) => Step::fail(automation_error(error)),
            },
            ActStage::Aim(aim) => match aim.poll(world) {
                Ok(AimProgress::Ready(target)) => match enqueue(world, target.input()) {
                    Ok(id) => {
                        self.stage = ActStage::Play(id, Box::new(target));
                        Step::Pending
                    }
                    Err(error) => Step::fail(error),
                },
                Ok(AimProgress::Waiting(_stage)) => Step::Pending,
                Err(error) => Step::fail(automation_error(error)),
            },
            ActStage::Play(id, target) => {
                if !finished(world, *id) {
                    return Step::Pending;
                }
                Step::done(ResponseBody::WorldDone {
                    node: target.node.clone(),
                    hit_point: target.hit_point,
                })
            }
        }
    }
}

/// Where a rubber band stands.
enum SweepStage {
    /// Planning the band.
    Plan,
    /// Drawing it; it selects these.
    Draw(InputActionId, Vec<WorldNode>),
    /// Waiting for the selection to be these, for this many frames so far.
    Confirm(Vec<WorldNode>, u32),
}

/// A rubber band over the things a world locator names.
pub(super) struct Sweep {
    /// The band's planning.
    planning: WorldSweep,
    /// The things.
    locator: WorldLocator,
    /// Where it stands.
    stage: SweepStage,
}

impl Sweep {
    /// Advance it by a frame.
    fn poll(&mut self, world: &mut World) -> Step {
        match &mut self.stage {
            SweepStage::Plan => match self.planning.poll(world) {
                Ok(SweepProgress::Ready(band)) => match enqueue(world, band.input()) {
                    Ok(id) => {
                        self.stage = SweepStage::Draw(id, band.nodes);
                        Step::Pending
                    }
                    Err(error) => Step::fail(error),
                },
                Ok(SweepProgress::Waiting(_stage)) => Step::Pending,
                Err(error) => Step::fail(automation_error(error)),
            },
            SweepStage::Draw(id, nodes) => {
                if finished(world, *id) {
                    self.stage = SweepStage::Confirm(core::mem::take(nodes), 0);
                }
                Step::Pending
            }
            SweepStage::Confirm(nodes, frames) => {
                let wanted: BTreeSet<Uuid> = nodes.iter().map(|node| node.full_id).collect();
                let got: BTreeSet<Uuid> = world
                    .get_resource::<SelectionSet>()
                    .map(|selection| selection.iter().map(|node| node.full.uuid()).collect())
                    .unwrap_or_default();
                if got == wanted {
                    return Step::done(ResponseBody::Swept {
                        nodes: core::mem::take(nodes),
                    });
                }
                *frames = frames.saturating_add(1);
                if *frames < SWEEP_CONFIRM_FRAMES {
                    return Step::Pending;
                }
                Step::fail(AutomationError::SweepInexact {
                    locator: self.locator.clone(),
                    missing: wanted.difference(&got).copied().collect(),
                    extra: got.difference(&wanted).copied().collect(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_viewer_world_api::{ManipulatorAxis, ManipulatorHandle};

    use super::parse_handle;

    #[test]
    fn every_handle_is_found_by_its_test_address() {
        let axes = [ManipulatorAxis::X, ManipulatorAxis::Y, ManipulatorAxis::Z];
        let mut handles: Vec<ManipulatorHandle> = axes
            .iter()
            .flat_map(|axis| {
                [
                    ManipulatorHandle::Translate(*axis),
                    ManipulatorHandle::TranslatePlane(*axis),
                    ManipulatorHandle::Rotate(*axis),
                    ManipulatorHandle::StretchFace(*axis, false),
                    ManipulatorHandle::StretchFace(*axis, true),
                ]
            })
            .collect();
        handles.push(ManipulatorHandle::StretchCorner([true, false, true]));
        for handle in handles {
            assert_eq!(parse_handle(&handle.slug()), Some(handle));
        }
        assert_eq!(
            parse_handle("scale-corner-pnp"),
            Some(ManipulatorHandle::StretchCorner([true, false, true]))
        );
        assert_eq!(parse_handle("translate-w"), None);
    }
}
