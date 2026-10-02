//! [`ManipulatorDrag`]: a drag of one of the build tool's transform handles —
//! move, rotate or stretch the selection by a stated amount, on a stated side
//! of the snap guide, with the modifier keys a user would hold.
//!
//! The drag drives the synthetic input itself rather than handing back one
//! gesture, because the keys come first: a held `Ctrl` swaps the move rig for
//! the rotate rig and `Ctrl+Shift` for the stretch rig, so the handle to be
//! dragged only exists — and can only be planned against — once they are
//! down. A held `Shift` on a move handle leaves a copy behind.
//!
//! Where to press and which path to drag is the build tool's own answer
//! ([`ManipulatorProbes`]): its hit test puts the press on the handle, and its
//! drag math, run backwards, gives the path — so the drag does exactly what the
//! plan predicts, snapping included.

use std::time::{Duration, Instant};

use bevy::input::keyboard::Key;
use bevy::prelude::*;
use sl_automation_proto::{ActionabilityCheck, AutomationError, Deadline};
use sl_viewer_ui_core::synthetic_input::{
    ActionStatus, InputAction, InputActionId, InputStep, SyntheticInput,
};
use sl_viewer_world_api::{
    EditToolState, ManipulatorAmount, ManipulatorProbes, ManipulatorQuery, ManipulatorRefusal,
    ProbeTicket, SelectionSet,
};

use crate::pursuit::{DEFAULT_DEADLINE, DEFAULT_DEADLINE_FRAMES, PursuitError};
use crate::world_aim::{CameraStill, reach_the_world};

/// The longest single pointer step along a drag path, logical pixels — short
/// enough that every drag reader sees the motion as a drag, not a jump.
const PATH_STEP_PIXELS: f32 = 16.0;

/// The keys held down through a handle drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldKeys {
    /// None.
    None,
    /// `Shift`: on a move handle, leave a copy of the selection behind.
    Shift,
    /// `Ctrl`: the rotate rig, whatever the build floater's tool.
    Ctrl,
    /// `Ctrl+Shift`: the stretch rig, whatever the build floater's tool.
    CtrlShift,
}

impl HeldKeys {
    /// The keys, in the order they go down (and the reverse of the order they
    /// come up).
    fn keys(self) -> Vec<(KeyCode, Key)> {
        let ctrl = (KeyCode::ControlLeft, Key::Control);
        let shift = (KeyCode::ShiftLeft, Key::Shift);
        match self {
            Self::None => Vec::new(),
            Self::Shift => vec![shift],
            Self::Ctrl => vec![ctrl],
            Self::CtrlShift => vec![ctrl, shift],
        }
    }

    /// The gesture that puts them down.
    fn down(self) -> InputAction {
        InputAction::from_steps(
            self.keys()
                .into_iter()
                .map(|(key_code, logical)| InputStep::KeyDown {
                    key_code,
                    logical,
                    text: None,
                })
                .collect(),
        )
    }

    /// The steps that let them go.
    fn up_steps(self) -> Vec<InputStep> {
        self.keys()
            .into_iter()
            .rev()
            .map(|(key_code, logical)| InputStep::KeyUp { key_code, logical })
            .collect()
    }
}

/// What a [`ManipulatorDrag`] is doing while it is not done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragStage {
    /// Waiting for build mode with something selected.
    BuildMode,
    /// Putting the modifier keys down.
    Holding,
    /// Waiting for the camera to hold still.
    Settling,
    /// Waiting for the build tool's plan (or for its rig to show the handle).
    Planning,
    /// Playing the drag.
    Dragging,
}

impl DragStage {
    /// The check a timeout in this stage reports as still failing.
    #[must_use]
    pub const fn check(self) -> ActionabilityCheck {
        match self {
            Self::BuildMode => ActionabilityCheck::BuildMode,
            Self::Holding | Self::Settling | Self::Dragging => ActionabilityCheck::Stable,
            Self::Planning => ActionabilityCheck::Attached,
        }
    }
}

/// Where a [`ManipulatorDrag`] stands after a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragProgress {
    /// The drag has been played; what the build tool predicted it does.
    Done(ManipulatorAmount),
    /// Not yet: poll again next frame.
    Waiting(DragStage),
}

/// Where the drag stands, with its state.
#[derive(Debug, Clone, Copy)]
enum Stage {
    /// Not started.
    Start,
    /// The keys are going down.
    Hold(InputActionId),
    /// Waiting for a still camera.
    Settle,
    /// The plan is asked for.
    Plan(ProbeTicket),
    /// The drag is playing; it will do this.
    Drag(InputActionId, ManipulatorAmount),
}

/// A drag of one transform handle, polled once a frame.
///
/// Each [`poll`](Self::poll) moves it along: build mode with a selection →
/// the modifier keys down → a still camera → the build tool's plan (retried
/// while its rig does not show the handle yet) → a press the UI would not take
/// → the drag played through the synthetic input, the keys let go after the
/// release. The answer is the plan's prediction; the caller checks the
/// selection against it.
#[derive(Debug)]
pub struct ManipulatorDrag {
    /// What to drag, how far, on which side of the snap guide.
    query: ManipulatorQuery,
    /// The keys to hold.
    keys: HeldKeys,
    /// The most frames to wait.
    max_frames: u32,
    /// The most wall-clock time to wait.
    max_time: Duration,
    /// When the first poll ran.
    started: Option<Instant>,
    /// The polls so far.
    frames: u32,
    /// Where it stands.
    stage: Stage,
    /// The camera's stillness.
    still: CameraStill,
    /// Whether the keys are (or are going) down.
    holding: bool,
}

impl ManipulatorDrag {
    /// Drag as `query` says, holding `keys`, under the default deadline.
    #[must_use]
    pub fn new(query: ManipulatorQuery, keys: HeldKeys) -> Self {
        Self {
            query,
            keys,
            max_frames: DEFAULT_DEADLINE_FRAMES,
            max_time: DEFAULT_DEADLINE,
            started: None,
            frames: 0,
            stage: Stage::Start,
            still: CameraStill::default(),
            holding: false,
        }
    }

    /// Give up after `deadline`; a limit it leaves unset keeps the default.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Deadline) -> Self {
        if let Some(frames) = deadline.frames {
            self.max_frames = frames;
        }
        if let Some(millis) = deadline.millis {
            self.max_time = Duration::from_millis(millis);
        }
        self
    }

    /// Move the drag along by a frame.
    ///
    /// # Errors
    ///
    /// [`AutomationError::ManipulatorRefused`] when the build tool will not
    /// plan the drag (other than for a handle its rig does not show yet), or
    /// every press it offers is under a UI node;
    /// [`AutomationError::ManipulatorTimedOut`] when the deadline passes before
    /// the drag starts; and [`PursuitError::Model`] when the UI model cannot be
    /// read. Held keys are let go on any failure.
    pub fn poll(&mut self, world: &mut World) -> Result<DragProgress, PursuitError> {
        let started = *self.started.get_or_insert_with(Instant::now);
        self.frames = self.frames.saturating_add(1);
        let progress = match self.step(world) {
            Ok(progress) => progress,
            Err(error) => {
                self.give_up(world);
                return Err(error);
            }
        };
        let DragProgress::Waiting(stage) = progress else {
            return Ok(progress);
        };
        // Once playing, the drag finishes on its own.
        if stage == DragStage::Dragging {
            return Ok(progress);
        }
        let waited = started.elapsed();
        if self.frames >= self.max_frames || waited >= self.max_time {
            self.give_up(world);
            return Err(AutomationError::ManipulatorTimedOut {
                handle: self.query.handle.slug(),
                failed_check: stage.check(),
                frames: self.frames,
                millis: u64::try_from(waited.as_millis()).unwrap_or(u64::MAX),
            }
            .into());
        }
        Ok(progress)
    }

    /// One poll's work, deadline aside.
    fn step(&mut self, world: &mut World) -> Result<DragProgress, PursuitError> {
        match self.stage {
            Stage::Start => {
                let ready = world
                    .get_resource::<EditToolState>()
                    .is_some_and(|tool| tool.active)
                    && world
                        .get_resource::<SelectionSet>()
                        .is_some_and(|selection| !selection.is_empty());
                if !ready {
                    return Ok(DragProgress::Waiting(DragStage::BuildMode));
                }
                if self.keys == HeldKeys::None {
                    self.stage = Stage::Settle;
                    return Ok(DragProgress::Waiting(DragStage::Settling));
                }
                let id = enqueue(world, self.keys.down())?;
                self.holding = true;
                self.stage = Stage::Hold(id);
                Ok(DragProgress::Waiting(DragStage::Holding))
            }
            Stage::Hold(id) => {
                if finished(world, id) {
                    self.still.reset();
                    self.stage = Stage::Settle;
                    return Ok(DragProgress::Waiting(DragStage::Settling));
                }
                Ok(DragProgress::Waiting(DragStage::Holding))
            }
            Stage::Settle => {
                // The rig stands on the selection: where it lands on screen
                // is where the handles do.
                let selected: Vec<Vec3> = world
                    .get_resource::<SelectionSet>()
                    .map(|selection| {
                        selection
                            .iter()
                            .filter_map(|node| world.get::<GlobalTransform>(node.entity))
                            .map(GlobalTransform::translation)
                            .collect()
                    })
                    .unwrap_or_default();
                if !self.still.observe(world, &selected) {
                    return Ok(DragProgress::Waiting(DragStage::Settling));
                }
                let Some(mut probes) = world.get_resource_mut::<ManipulatorProbes>() else {
                    // No build tool in this app: nothing can plan, and the
                    // deadline will say so.
                    return Ok(DragProgress::Waiting(DragStage::Planning));
                };
                self.stage = Stage::Plan(probes.request(self.query));
                Ok(DragProgress::Waiting(DragStage::Planning))
            }
            Stage::Plan(ticket) => {
                let answer = world
                    .get_resource_mut::<ManipulatorProbes>()
                    .and_then(|mut probes| probes.take_answer(ticket));
                match answer {
                    None => Ok(DragProgress::Waiting(DragStage::Planning)),
                    Some(Err(ManipulatorRefusal::NoHandle)) => {
                        // The rig is not showing the handle yet (a held key's
                        // rig still being built): ask again.
                        self.stage = Stage::Settle;
                        Ok(DragProgress::Waiting(DragStage::Planning))
                    }
                    Some(Err(refusal)) => Err(self.refused(refusal.describe())),
                    Some(Ok(plans)) => {
                        let presses =
                            reach_the_world(world, plans.iter().map(|plan| plan.press).collect())?;
                        let Some(plan) =
                            plans.into_iter().find(|plan| presses.contains(&plan.press))
                        else {
                            return Err(
                                self.refused("every press on the handle is under a UI node")
                            );
                        };
                        let mut steps = vec![
                            InputStep::Move(plan.press),
                            InputStep::Press(MouseButton::Left),
                        ];
                        let mut at = plan.press;
                        for point in plan.path {
                            steps.extend(path_steps(at, point));
                            at = point;
                        }
                        steps.push(InputStep::Idle);
                        steps.push(InputStep::Release(MouseButton::Left));
                        steps.push(InputStep::Idle);
                        steps.extend(self.keys.up_steps());
                        let id = enqueue(world, InputAction::from_steps(steps))?;
                        self.holding = false;
                        self.stage = Stage::Drag(id, plan.predicted);
                        Ok(DragProgress::Waiting(DragStage::Dragging))
                    }
                }
            }
            Stage::Drag(id, predicted) => {
                if finished(world, id) {
                    return Ok(DragProgress::Done(predicted));
                }
                Ok(DragProgress::Waiting(DragStage::Dragging))
            }
        }
    }

    /// The refusal error for this drag.
    fn refused(&self, reason: &str) -> PursuitError {
        AutomationError::ManipulatorRefused {
            handle: self.query.handle.slug(),
            reason: reason.to_owned(),
        }
        .into()
    }

    /// Let go of the keys, if they are down, and drop an unanswered plan.
    fn give_up(&mut self, world: &mut World) {
        if let Stage::Plan(ticket) = self.stage
            && let Some(mut probes) = world.get_resource_mut::<ManipulatorProbes>()
        {
            probes.abandon(ticket);
        }
        if self.holding {
            self.holding = false;
            let _released = enqueue(world, InputAction::from_steps(self.keys.up_steps()));
        }
    }
}

/// The pointer moves from `from` to `to`, no step longer than
/// [`PATH_STEP_PIXELS`], ending exactly on `to`.
fn path_steps(from: Vec2, to: Vec2) -> Vec<InputStep> {
    let length = from.distance(to);
    let mut count = 1_u16;
    while f32::from(count) * PATH_STEP_PIXELS < length && count < u16::MAX {
        count = count.saturating_add(1);
    }
    (1..=count)
        .map(|step| InputStep::Move(from.lerp(to, f32::from(step) / f32::from(count))))
        .collect()
}

/// Queue `action` on the app's synthetic input.
///
/// # Errors
///
/// When the app has none, which a windowless viewer always installs.
fn enqueue(world: &mut World, action: InputAction) -> Result<InputActionId, PursuitError> {
    let Some(mut input) = world.get_resource_mut::<SyntheticInput>() else {
        return Err(AutomationError::ManipulatorRefused {
            handle: String::new(),
            reason: "the app has no synthetic input".to_owned(),
        }
        .into());
    };
    Ok(input.enqueue(action))
}

/// Whether the synthetic input has played `id` to its end.
fn finished(world: &World, id: InputActionId) -> bool {
    world.get_resource::<SyntheticInput>().is_some_and(|input| {
        matches!(
            input.status(id),
            ActionStatus::Done { .. } | ActionStatus::Expired | ActionStatus::Unknown
        )
    })
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_viewer_ui_core::synthetic_input::InputStep;

    use super::path_steps;

    #[test]
    fn a_path_is_cut_into_short_steps_ending_on_the_point() {
        let end = Vec2::new(40.0, 0.0);
        let points: Vec<Vec2> = path_steps(Vec2::ZERO, end)
            .into_iter()
            .filter_map(|step| match step {
                InputStep::Move(at) => Some(at),
                _other => None,
            })
            .collect();
        assert_eq!(points.len(), 3, "40 px in steps of at most 16: {points:?}");
        assert_eq!(
            points.last().copied(),
            Some(end),
            "the path ends on the point"
        );
        let mut at = Vec2::ZERO;
        for point in &points {
            assert!(at.distance(*point) <= super::PATH_STEP_PIXELS + 1.0e-3);
            at = *point;
        }
        assert_eq!(
            path_steps(Vec2::ZERO, Vec2::ZERO),
            vec![InputStep::Move(Vec2::ZERO)],
            "a standing path is one move"
        );
    }
}
