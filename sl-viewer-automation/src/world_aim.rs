//! [`WorldAim`]: a world action's wait for the one thing it acts on — resolve
//! it, find a point where a click lands on *it*, and frame it with the camera
//! when there is none.
//!
//! A click on an object must reach that object, not whatever stands in front
//! of it, and a test must not need to know where the camera is. So the aim
//! never trusts geometry alone: it projects the thing's bounding box, picks
//! candidate points on the faces that face the camera, and asks the viewer's
//! own pick resolver — the one every real click goes through — what a click
//! at each would hit ([`PickProbes`]). The first point that lands on the
//! thing is the aim. A point under a UI node that would take the click is
//! never a candidate.
//!
//! When no point on screen lands on it (it is off screen, behind the camera,
//! or covered), the aim **reveals** it once: it asks the camera to frame the
//! thing ([`FrameObject`]), waits for the camera to settle, and aims again. The
//! camera is left where the reveal put it. Only then does it fail, naming what
//! a click at the thing's centre would have hit.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use bevy::ecs::system::SystemState;
use bevy::input::keyboard::Key;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use sl_automation_proto::{
    ActionabilityCheck, AutomationError, Deadline, WorldKind, WorldLocator, WorldNode,
};
use sl_client_bevy::{AgentKey, ObjectKey, ScopedObjectId, SlIdentity, Uuid};
use sl_viewer_kit::coords::{bevy_to_sl_vec, region_offset_bevy};
use sl_viewer_ui_core::synthetic_input::{InputAction, InputStep};
use sl_viewer_world_api::{
    AvatarState, EditTool, EditToolState, FrameObject, ObjectState, PickProbes, PressOutcome,
    ProbeId, ProbeTarget, ProbeTicket, SelectionAnswer, SelectionProbes, SelectionQuery,
    ViewerCamera,
};

use crate::pursuit::{DEFAULT_DEADLINE, DEFAULT_DEADLINE_FRAMES, PursuitError};
use crate::ui_model::UiModel;
use crate::world_query::{WorldProgress, WorldQuery, WorldWant};

/// How many consecutive polls the camera and the target must hold still
/// before the aim trusts a projection — the UI's stability rule.
const STABLE_FRAMES: u32 = 2;

/// The polls after a reveal before the camera's pose is judged at all: the
/// request is answered in the next update, and a camera that has not started
/// moving yet looks exactly like one that has stopped.
pub(crate) const REVEAL_SETTLE_FRAMES: u32 = 3;

/// The most points probed per aim: the projected centre, the centre of each
/// face towards the camera, and a lattice on those faces.
const MAX_CANDIDATES: usize = 28;

/// Where the lattice of candidate points sits on a face, in the face's own
/// coordinates (the face spans −0.5 … 0.5): the centre, and 60 % of the way
/// across either way — clear of the edges, where a pick is least certain.
const LATTICE: [f32; 3] = [-0.3, 0.0, 0.3];

/// Candidate points nearer each other than this, logical pixels, are one.
const DEDUPE_PIXELS: f32 = 2.0;

/// The least a corner of the target's box may move on screen between polls
/// and the target still be still, logical pixels: under the one-pixel pick
/// the probes ask for, so a target a few pixels across must hold still.
const PIXEL_TOLERANCE: f32 = 0.5;

/// How far a corner of the target's box may move on screen, as a fraction of
/// the smaller side of its projected box, and the aim points still land on
/// it: every candidate point sits at least a fifth of a face from its edges
/// (the [`LATTICE`] at ±0.3 of a face spanning ±0.5), so a quarter of that
/// keeps a probed point well inside the target.
const EDGE_FRACTION: f32 = 0.05;

/// How far, logical pixels, a point a pursuit acts on may move on screen
/// between two polls with the camera still counted as holding: a click lands
/// within it, and a ground pick is accepted anywhere in a footprint three
/// times as wide.
const CAMERA_PIXEL_TOLERANCE: f32 = 1.0;

/// How far the target may drift between polls and still be still, metres.
const TARGET_TOLERANCE: f32 = 0.01;

/// The frames a drop's pointer takes from the drag's start to the target.
const DROP_STEPS: u32 = 4;

/// The frames a drop rests on the target before it lets go: the world keeps a
/// drag's pick fresh at ~15 Hz and answers it frames later, so a release the
/// instant the pointer arrives resolves where the pointer *was*.
const DROP_REST_FRAMES: usize = 24;

/// What a world action is going to do to its target.
///
/// No grab-drag of the object itself: the viewer has no press-drag-release
/// grab for one to drive (the Move tool of [[viewer-build-tool-row-parity]]).
/// The build tool's handle drags are a `ManipulatorDrag`, and its rubber band a
/// `WorldSweep`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WorldIntent {
    /// A left click: a touch outside build mode.
    Click,
    /// Two left clicks: with the double-click action set to teleport, a
    /// teleport to where they land. Judged by the pick resolver, as a click
    /// is.
    DoubleClick,
    /// A right click: the target's pie menu.
    RightClick,
    /// The pointer over it: its hover tip.
    Hover,
    /// A left click in build mode, which selects. Waits for the build tool to
    /// be active on a tool that selects; outside it the same click would
    /// touch, and with the Create tool it would rez. Judged by the
    /// selection gesture's own object picker, and a point the transform rig
    /// would take is not on the target.
    Select,
    /// [`Select`](Self::Select) with `Shift` held, which toggles the target in
    /// the selection and keeps the rest of it. Waits and is judged as a select
    /// is.
    ShiftSelect,
    /// A left click with the build tool's Create tool, which rezzes the picked
    /// shape where it lands. Waits for the Create tool; with any other tool the
    /// same click would select. Judged by the pick resolver, whose hit is the
    /// surface the placer's own ray strikes.
    Place,
    /// Drop what a drag started at this point (logical pixels — an inventory
    /// row, found with a UI locator) carries onto the target: press there,
    /// move onto the target, rest while the drag's world pick catches up,
    /// release. Judged by the pick the drop itself reads.
    DropFrom(Vec2),
}

impl WorldIntent {
    /// Whether it is a build-mode selection click, judged by the selection
    /// gesture's picker and waiting for a tool that selects.
    const fn selects(self) -> bool {
        matches!(self, Self::Select | Self::ShiftSelect)
    }
}

/// What a [`WorldAim`] is doing while it is not ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AimStage {
    /// Finding the one thing the locator names.
    Resolving,
    /// Waiting for the camera and the target to hold still.
    Settling,
    /// Waiting for the build tool on a tool that selects (a
    /// [`WorldIntent::Select`], a [`WorldIntent::ShiftSelect`] or a sweep).
    BuildMode,
    /// Waiting for the build tool's Create tool (a [`WorldIntent::Place`]).
    CreateTool,
    /// Asking the pick resolver about candidate points.
    Probing,
    /// Framing the target with the camera, because it failed this check.
    Revealing(ActionabilityCheck),
}

impl AimStage {
    /// The check a timeout in this stage reports as still failing.
    #[must_use]
    pub const fn check(self) -> ActionabilityCheck {
        match self {
            Self::Resolving => ActionabilityCheck::Attached,
            Self::Settling => ActionabilityCheck::Stable,
            Self::BuildMode => ActionabilityCheck::BuildMode,
            Self::CreateTool => ActionabilityCheck::CreateTool,
            Self::Probing => ActionabilityCheck::ReceivesEvents,
            Self::Revealing(check) => check,
        }
    }
}

/// Where a thing's bounding box lands on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenProjection {
    /// The box of every corner in front of the camera, logical pixels. It
    /// may reach past the viewport.
    pub bounds: Rect,
    /// Whether a point of the thing facing the camera lies in the viewport.
    pub on_screen: bool,
}

/// The thing a world action may now be applied to, and where to aim.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldTarget {
    /// The thing, as resolved.
    pub node: Box<WorldNode>,
    /// The point a click lands on it, logical pixels.
    pub aim: Vec2,
    /// Where on it the pick resolver says that click lands, region-local
    /// metres — absent for a select, whose picker says which object only.
    pub hit_point: Option<[f32; 3]>,
    /// Its projected bounding box.
    pub bounds: Rect,
    /// What will be done to it.
    intent: WorldIntent,
}

impl WorldTarget {
    /// The gesture the intent is, aimed at this target, for the synthetic
    /// input to play.
    #[must_use]
    pub fn input(&self) -> InputAction {
        gesture(self.intent, self.aim)
    }
}

/// The gesture `intent` is, aimed at `aim` (logical pixels).
pub(crate) fn gesture(intent: WorldIntent, aim: Vec2) -> InputAction {
    match intent {
        WorldIntent::Click | WorldIntent::Select | WorldIntent::Place => {
            InputAction::click(aim, MouseButton::Left)
        }
        WorldIntent::DoubleClick => InputAction::double_click(aim, MouseButton::Left),
        WorldIntent::ShiftSelect => shifted_click(aim),
        WorldIntent::RightClick => InputAction::click(aim, MouseButton::Right),
        WorldIntent::Hover => InputAction::move_to(aim),
        WorldIntent::DropFrom(from) => drop_gesture(from, aim, DROP_REST_FRAMES),
    }
}

/// A left click at `at` with `Shift` down a frame before the press and up a
/// frame after the release, so the selection gesture reads it held at press.
fn shifted_click(at: Vec2) -> InputAction {
    let (key_code, logical) = (KeyCode::ShiftLeft, Key::Shift);
    InputAction::from_steps(vec![
        InputStep::KeyDown {
            key_code,
            logical: logical.clone(),
            text: None,
        },
        InputStep::Move(at),
        InputStep::Press(MouseButton::Left),
        InputStep::Release(MouseButton::Left),
        InputStep::Idle,
        InputStep::KeyUp { key_code, logical },
        InputStep::Idle,
    ])
}

/// A left-button drag from `from` onto `to`: press at `from`, the pointer
/// across in [`DROP_STEPS`] frames, `rest` frames over `to`, release, settle.
pub(crate) fn drop_gesture(from: Vec2, to: Vec2, rest: usize) -> InputAction {
    let mut steps = vec![InputStep::Move(from), InputStep::Press(MouseButton::Left)];
    steps.extend((1..=DROP_STEPS).map(|step| {
        let fraction = f32::from(u16::try_from(step).unwrap_or(u16::MAX))
            / f32::from(u16::try_from(DROP_STEPS).unwrap_or(u16::MAX));
        InputStep::Move(from.lerp(to, fraction))
    }));
    steps.extend(core::iter::repeat_n(InputStep::Idle, rest));
    steps.push(InputStep::Release(MouseButton::Left));
    steps.push(InputStep::Idle);
    InputAction::from_steps(steps)
}

/// Where a [`WorldAim`] stands after a frame.
#[derive(Debug, Clone, PartialEq)]
pub enum AimProgress {
    /// The target and a verified aim point.
    Ready(WorldTarget),
    /// Not yet: poll again next frame.
    Waiting(AimStage),
}

/// Which pick answers count as the target.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Subject {
    /// An avatar's own body.
    Avatar(AgentKey),
    /// A prim and its link children; for an attachment, also the rigged
    /// submeshes the avatar wears of it.
    Prims {
        /// The prim and its link children.
        prims: Vec<ScopedObjectId>,
        /// The attachment, when the prim is one.
        worn: Option<ScopedObjectId>,
    },
}

impl Subject {
    /// Whether a selection press that picks `scoped` selects this subject.
    fn is_selected_by(&self, scoped: ScopedObjectId) -> bool {
        matches!(self, Self::Prims { prims, .. } if prims.contains(&scoped))
    }

    /// Whether a pick that found `target` found this subject.
    fn is_hit(&self, target: &ProbeTarget) -> bool {
        match (self, target) {
            (Self::Avatar(agent), ProbeTarget::Avatar { agent: hit, worn }) => {
                hit == agent && worn.is_none()
            }
            (Self::Prims { prims, .. }, ProbeTarget::Object { scoped, .. }) => {
                prims.contains(scoped)
            }
            (
                Self::Prims {
                    worn: Some(worn), ..
                },
                ProbeTarget::Avatar {
                    worn: Some(hit), ..
                },
            ) => hit == worn,
            _ => false,
        }
    }
}

/// Where the target is in one poll: its centre in the world, and where each
/// corner of its box lands on screen (`None` for one behind the camera).
///
/// Stability is judged on screen, as the UI judges a node by its bounds: an
/// aim point is a screen point on the target, so a pose is the same while no
/// corner of the target moves on screen by more than [`EDGE_FRACTION`] of the
/// target's projected size (never less than [`PIXEL_TOLERANCE`]) — a probed
/// point still lands on it. The eye itself may drift: the follow camera holds
/// the own avatar's animated head, which sways a couple of centimetres with
/// the idle animation, and at a few frames a second (a loaded machine) that is
/// more than a pixel between polls; an eye tolerance, or a fixed sub-pixel
/// one, restarts the probing for ever.
#[derive(Debug, Clone, Copy)]
struct Pose {
    /// The box's corners on screen, logical pixels.
    corners: [Option<Vec2>; 8],
    /// The target's centre.
    target: Vec3,
}

impl Pose {
    /// The pose of the box `boxed` (the unit cube's image) under `project`,
    /// which puts a world point on screen.
    fn of(boxed: &Affine3A, project: impl Fn(Vec3) -> Option<Vec2>) -> Self {
        Self {
            corners: box_corners(boxed).map(project),
            target: boxed.transform_point3(Vec3::ZERO),
        }
    }

    /// How far a corner may move on screen and the pose still be this one:
    /// [`EDGE_FRACTION`] of the smaller side of the corners' bounds, at least
    /// [`PIXEL_TOLERANCE`].
    fn tolerance(&self) -> f32 {
        let mut on_screen = self.corners.iter().flatten();
        let Some(first) = on_screen.next() else {
            return PIXEL_TOLERANCE;
        };
        let bounds = on_screen.fold(Rect::from_corners(*first, *first), |rect, corner| {
            rect.union_point(*corner)
        });
        (bounds.width().min(bounds.height()) * EDGE_FRACTION).max(PIXEL_TOLERANCE)
    }

    /// Whether `other` is this pose, to the tolerances.
    fn same(&self, other: &Self) -> bool {
        let tolerance = self.tolerance();
        self.target.distance(other.target) < TARGET_TOLERANCE
            && self
                .corners
                .iter()
                .zip(&other.corners)
                .all(|pair| match pair {
                    (Some(this), Some(that)) => this.distance(*that) < tolerance,
                    (None, None) => true,
                    (Some(_), None) | (None, Some(_)) => false,
                })
    }
}

/// Whether every one of `points` lands within [`CAMERA_PIXEL_TOLERANCE`] of
/// where it did, `before` and `after` putting a world point on screen — or
/// stays off screen.
fn points_hold(
    points: &[Vec3],
    before: impl Fn(Vec3) -> Option<Vec2>,
    after: impl Fn(Vec3) -> Option<Vec2>,
) -> bool {
    points
        .iter()
        .all(|point| match (before(*point), after(*point)) {
            (Some(was), Some(is)) => {
                let moved = was.distance(is);
                if moved >= CAMERA_PIXEL_TOLERANCE {
                    debug!(?point, moved, "camera still: a point moved on screen");
                }
                moved < CAMERA_PIXEL_TOLERANCE
            }
            (None, None) => true,
            (Some(_), None) | (None, Some(_)) => false,
        })
}

/// Whether `eye` is inside the box `boxed` (the unit cube's image).
fn eye_inside(boxed: &Affine3A, eye: Vec3) -> bool {
    let local = boxed.inverse().transform_point3(eye);
    local.abs().max_element() < 0.5
}

/// The eight corners of the box `boxed` (the unit cube's image).
fn box_corners(boxed: &Affine3A) -> [Vec3; 8] {
    let mut corners = [Vec3::ZERO; 8];
    for (index, corner) in corners.iter_mut().enumerate() {
        let side = |mask: usize| if index & mask == 0 { -0.5 } else { 0.5 };
        *corner = boxed.transform_point3(Vec3::new(side(1), side(2), side(4)));
    }
    corners
}

/// The corners of `node`'s box in the world, while it is tracked as a full
/// object.
pub(crate) fn node_corners(world: &World, node: &WorldNode) -> Option<[Vec3; 8]> {
    let (_subject, geometry, _prims) = locate(world, node)?;
    Some(box_corners(
        &world.get::<GlobalTransform>(geometry)?.affine(),
    ))
}

/// Whether the camera holds still, judged a poll at a time — the stability
/// rule every world pursuit shares: the points the pursuit will act on land
/// within [`CAMERA_PIXEL_TOLERANCE`] of where they did, for [`STABLE_FRAMES`]
/// polls in a row.
///
/// Judged on screen, not by the eye: the follow camera holds the own avatar's
/// head, which an idle animation or an AO sways 0.5–2 mm a frame (measured on
/// aditi), so an eye tolerance below that waits for good — and above it is
/// arbitrary, since what a click needs is that the point under it stays put.
#[derive(Debug, Default)]
pub(crate) struct CameraStill {
    /// The last poll's camera.
    last: Option<(Camera, GlobalTransform)>,
    /// For how many polls in a row the points have held.
    streak: u32,
}

impl CameraStill {
    /// Record this poll's camera and say whether it has now held still for
    /// `points` (Bevy world positions). With no points nothing can move on
    /// screen, and the camera holds.
    pub(crate) fn observe(&mut self, world: &mut World, points: &[Vec3]) -> bool {
        let Some(now) = view(world) else {
            self.reset();
            return false;
        };
        let same = self.last.as_ref().is_some_and(|last| {
            last.0.logical_viewport_size() == now.0.logical_viewport_size()
                && points_hold(
                    points,
                    |point| last.0.world_to_viewport(&last.1, point).ok(),
                    |point| now.0.world_to_viewport(&now.1, point).ok(),
                )
        });
        self.streak = if same {
            self.streak.saturating_add(1)
        } else {
            1
        };
        self.last = Some(now);
        self.streak >= STABLE_FRAMES
    }

    /// Forget: the next poll starts a new streak.
    pub(crate) const fn reset(&mut self) {
        self.last = None;
        self.streak = 0;
    }
}

/// Everything one poll reads about the target and the view.
struct Sight {
    /// Which answers count as the target.
    subject: Subject,
    /// The target's bounding box: the unit cube's image.
    boxed: Affine3A,
    /// The target's prims, which the camera passes through when it frames
    /// the target.
    prims: Vec<Entity>,
    /// The camera and its placement.
    camera: (Camera, GlobalTransform),
    /// The camera's eye.
    eye: Vec3,
    /// The pose, for stability.
    pose: Pose,
}

impl Sight {
    /// The centre of the target's box and the radius of a sphere holding it.
    fn sphere(&self) -> (Vec3, f32) {
        let centre = self.boxed.transform_point3(Vec3::ZERO);
        let corner = self.boxed.transform_point3(Vec3::splat(0.5));
        (centre, centre.distance(corner))
    }
}

/// One candidate's question, put to the resolver that judges the intent.
#[derive(Debug, Clone, Copy)]
enum Asked {
    /// The world pick: every intent but a select.
    Pick(ProbeId),
    /// The selection gesture's press classification: a select.
    Press(ProbeTicket),
}

/// What one candidate's answer came to.
#[derive(Debug, Clone, Copy)]
enum Verdict {
    /// On the target — with the world point, when the resolver gives one.
    OnTarget(Option<Vec3>),
    /// Elsewhere — on this object or avatar, when it is one.
    Elsewhere(Option<Uuid>),
}

impl Asked {
    /// Put the question at `point` to the resolver `intent` is judged by;
    /// `None` when that resolver is not in the app.
    fn ask(world: &mut World, intent: WorldIntent, point: Vec2) -> Option<Self> {
        if intent.selects() {
            let mut probes = world.get_resource_mut::<SelectionProbes>()?;
            Some(Self::Press(probes.request(SelectionQuery::Press(point))))
        } else {
            let mut probes = world.get_resource_mut::<PickProbes>()?;
            Some(Self::Pick(probes.request(point)))
        }
    }

    /// The answer, once there is one, judged against `subject`.
    fn read(self, world: &mut World, subject: &Subject) -> Option<Verdict> {
        match self {
            Self::Pick(id) => {
                let answer = world.get_resource_mut::<PickProbes>()?.take_answer(id)?;
                Some(match answer {
                    Some(hit) if subject.is_hit(&hit.target) => {
                        Verdict::OnTarget(Some(hit.world_point))
                    }
                    other => {
                        Verdict::Elsewhere(other.and_then(|hit| full_id_of(world, &hit.target)))
                    }
                })
            }
            Self::Press(ticket) => {
                let answer = world
                    .get_resource_mut::<SelectionProbes>()?
                    .take_answer(ticket)?;
                Some(match answer {
                    SelectionAnswer::Press(PressOutcome::Object(scoped))
                        if subject.is_selected_by(scoped) =>
                    {
                        Verdict::OnTarget(None)
                    }
                    SelectionAnswer::Press(PressOutcome::Object(scoped)) => {
                        Verdict::Elsewhere(object_full_id(world, &scoped))
                    }
                    SelectionAnswer::Press(PressOutcome::Handle | PressOutcome::EmptyWorld)
                    | SelectionAnswer::Sweep(_) => Verdict::Elsewhere(None),
                })
            }
        }
    }

    /// Give the question up.
    fn abandon(self, world: &mut World) {
        match self {
            Self::Pick(id) => {
                if let Some(mut probes) = world.get_resource_mut::<PickProbes>() {
                    probes.abandon(id);
                }
            }
            Self::Press(ticket) => {
                if let Some(mut probes) = world.get_resource_mut::<SelectionProbes>() {
                    probes.abandon(ticket);
                }
            }
        }
    }
}

/// The probing in flight: the candidates asked about, answered in order.
#[derive(Debug)]
struct Probing {
    /// The pose the candidates were projected in; a change invalidates them.
    pose: Pose,
    /// The projected bounding box.
    bounds: Rect,
    /// The questions still to be read, first candidate first.
    pending: VecDeque<(Asked, Vec2)>,
    /// Whether the first candidate's answer has been read.
    read_first: bool,
    /// What the first candidate (the projected centre) landed on instead:
    /// the failure report's "covered by".
    covered_by: Option<Uuid>,
}

/// The stage a [`WorldAim`] is in, with its state.
#[derive(Debug)]
enum Stage {
    /// Waiting for the locator to resolve.
    Resolve,
    /// Waiting for a still camera and target.
    Settle,
    /// Reading probe answers.
    Probe(Probing),
}

/// A world action's wait for its target, polled once a frame.
///
/// Each [`poll`](Self::poll) moves it along: the locator resolves strictly
/// (a [`WorldQuery`] for one thing, which waits for names and fails on
/// ambiguity); the camera and the target must hold still; candidate points on
/// the target's box are projected and probed through the viewer's own pick
/// resolver, one a frame; and the first that lands on the target is the aim.
/// With none on screen, or none landing, the camera frames the target once
/// and the aim starts over — unless [`without_reveal`](Self::without_reveal)
/// said not to.
///
/// A probe in flight when the aim is dropped is still answered, into nothing.
#[derive(Debug)]
pub struct WorldAim {
    /// What is wanted.
    locator: WorldLocator,
    /// What will be done to it.
    intent: WorldIntent,
    /// Whether the camera may frame the target.
    reveal: bool,
    /// The locator's resolution.
    query: WorldQuery,
    /// The most frames to wait.
    max_frames: u32,
    /// The most wall-clock time to wait.
    max_time: Duration,
    /// When the first poll ran.
    started: Option<Instant>,
    /// The polls so far.
    frames: u32,
    /// The resolved target.
    node: Option<WorldNode>,
    /// Where the aim stands.
    stage: Stage,
    /// Whether the camera has framed the target already.
    revealed: bool,
    /// No pose is judged before this poll.
    settle_from: u32,
    /// The last poll's pose, and for how many polls in a row it has held.
    stability: Option<(Pose, u32)>,
}

impl WorldAim {
    /// Aim at the one thing `locator` names, for `intent`, under the default
    /// deadline.
    #[must_use]
    pub fn new(locator: WorldLocator, intent: WorldIntent) -> Self {
        Self {
            query: WorldQuery::new(locator.clone(), WorldWant::One),
            locator,
            intent,
            reveal: true,
            max_frames: DEFAULT_DEADLINE_FRAMES,
            max_time: DEFAULT_DEADLINE,
            started: None,
            frames: 0,
            node: None,
            stage: Stage::Resolve,
            revealed: false,
            settle_from: 0,
            stability: None,
        }
    }

    /// Give up after `deadline`; a limit it leaves unset keeps the default.
    #[must_use]
    pub fn with_deadline(mut self, deadline: Deadline) -> Self {
        if let Some(frames) = deadline.frames {
            self.max_frames = frames;
        }
        if let Some(millis) = deadline.millis {
            self.max_time = Duration::from_millis(millis);
        }
        self.query = WorldQuery::new(self.locator.clone(), WorldWant::One).with_deadline(deadline);
        self
    }

    /// Never move the camera: fail where a reveal would have framed the
    /// target.
    #[must_use]
    pub const fn without_reveal(mut self) -> Self {
        self.reveal = false;
        self
    }

    /// Look at the world once — a frame's worth of the wait. May ask the
    /// simulator for object properties, queue pick probes, or ask the camera
    /// to frame the target on the way.
    ///
    /// # Errors
    ///
    /// The resolution's errors ([`AutomationError::WorldAmbiguous`], and
    /// [`AutomationError::WorldTimedOut`] while nothing resolves);
    /// [`AutomationError::WorldNotActionable`] when no point of the target
    /// takes a click and no reveal was allowed; [`AutomationError::WorldTimedOut`] with the failing
    /// check when the deadline passes — also for a target a reveal never
    /// brings to a point that takes a click; and [`PursuitError::Model`] when the models
    /// cannot be read.
    pub fn poll(&mut self, world: &mut World) -> Result<AimProgress, PursuitError> {
        let started = *self.started.get_or_insert_with(Instant::now);
        self.frames = self.frames.saturating_add(1);
        let progress = match self.step(world) {
            Ok(progress) => progress,
            Err(error) => {
                self.abandon(world);
                return Err(error);
            }
        };
        let AimProgress::Waiting(stage) = &progress else {
            return Ok(progress);
        };
        // The resolution keeps its own deadline, and says more when it passes.
        if *stage == AimStage::Resolving {
            return Ok(progress);
        }
        let waited = started.elapsed();
        if self.frames >= self.max_frames || waited >= self.max_time {
            self.abandon(world);
            return Err(AutomationError::WorldTimedOut {
                locator: self.locator.clone(),
                failed_check: Some(stage.check()),
                unresolved: Vec::new(),
                last_observed: self.node.iter().cloned().collect(),
                frames: self.frames,
                millis: u64::try_from(waited.as_millis()).unwrap_or(u64::MAX),
            }
            .into());
        }
        Ok(progress)
    }

    /// One poll's work, deadline aside.
    fn step(&mut self, world: &mut World) -> Result<AimProgress, PursuitError> {
        match core::mem::replace(&mut self.stage, Stage::Resolve) {
            Stage::Resolve => self.resolve(world),
            Stage::Settle => self.settle(world),
            Stage::Probe(probing) => self.probe(world, probing),
        }
    }

    /// Poll the resolution; once it answers, start settling.
    fn resolve(&mut self, world: &mut World) -> Result<AimProgress, PursuitError> {
        match self.query.poll(world)? {
            WorldProgress::Ready(nodes) => {
                self.node = nodes.into_iter().next();
                self.stage = Stage::Settle;
                self.stability = None;
                Ok(AimProgress::Waiting(AimStage::Settling))
            }
            WorldProgress::Waiting { .. } => Ok(AimProgress::Waiting(AimStage::Resolving)),
        }
    }

    /// The target went away: resolve it again, within what is left of the
    /// deadline.
    fn lost(&mut self) -> AimProgress {
        let remaining_frames = self.max_frames.saturating_sub(self.frames);
        let remaining_time = self.max_time.saturating_sub(
            self.started
                .map_or(Duration::ZERO, |started| started.elapsed()),
        );
        self.query =
            WorldQuery::new(self.locator.clone(), WorldWant::One).with_deadline(Deadline {
                frames: Some(remaining_frames),
                millis: Some(u64::try_from(remaining_time.as_millis()).unwrap_or(u64::MAX)),
            });
        self.node = None;
        self.stage = Stage::Resolve;
        AimProgress::Waiting(AimStage::Resolving)
    }

    /// Wait for a still camera and target, then project and queue the probes.
    fn settle(&mut self, world: &mut World) -> Result<AimProgress, PursuitError> {
        self.stage = Stage::Settle;
        let Some(node) = self.node.clone() else {
            return Ok(self.lost());
        };
        let Some(sight) = sight(world, &node) else {
            return Ok(self.lost());
        };
        if self.intent.selects()
            && !world
                .get_resource::<EditToolState>()
                .is_some_and(|tool| tool.active && tool.tool.selects_objects())
        {
            self.stability = None;
            return Ok(AimProgress::Waiting(AimStage::BuildMode));
        }
        if self.intent == WorldIntent::Place
            && !world
                .get_resource::<EditToolState>()
                .is_some_and(|tool| tool.active && tool.tool == EditTool::Create)
        {
            self.stability = None;
            return Ok(AimProgress::Waiting(AimStage::CreateTool));
        }
        if !self.observe(sight.pose) || self.frames < self.settle_from {
            return Ok(AimProgress::Waiting(AimStage::Settling));
        }
        // A camera inside the target sees through its faces, so no point on
        // it can take a click; after a reveal that is the glide on its way
        // (seen on aditi, the eye passing into a fresh cube and out again),
        // not an answer. Before one, it is a room the camera is meant to be in.
        if self.revealed && eye_inside(&sight.boxed, sight.eye) {
            self.stability = None;
            return Ok(AimProgress::Waiting(AimStage::Settling));
        }
        let (camera, transform) = &sight.camera;
        let Some(viewport) = camera.logical_viewport_size() else {
            return Ok(AimProgress::Waiting(AimStage::Settling));
        };
        let candidates = candidates(&sight.boxed, sight.eye, viewport, |point| {
            camera.world_to_viewport(transform, point).ok()
        });
        let points = reach_the_world(world, candidates.points)?;
        let (Some(bounds), false) = (candidates.bounds, points.is_empty()) else {
            let check = if candidates.on_screen {
                ActionabilityCheck::ReceivesEvents
            } else {
                ActionabilityCheck::InViewport
            };
            return self.fail(world, &node, &sight, check, None);
        };
        let mut pending = VecDeque::new();
        for point in points {
            let Some(asked) = Asked::ask(world, self.intent, point) else {
                // No resolver for this intent in the app: nothing can say
                // where a click lands, and the deadline will report the
                // check as failing.
                return Ok(AimProgress::Waiting(AimStage::Probing));
            };
            pending.push_back((asked, point));
        }
        self.stage = Stage::Probe(Probing {
            pose: sight.pose,
            bounds,
            pending,
            read_first: false,
            covered_by: None,
        });
        Ok(AimProgress::Waiting(AimStage::Probing))
    }

    /// Read the probe answers in candidate order; the first that lands on the
    /// target is the aim.
    fn probe(
        &mut self,
        world: &mut World,
        mut probing: Probing,
    ) -> Result<AimProgress, PursuitError> {
        let Some(node) = self.node.clone() else {
            abandon_all(world, &mut probing);
            return Ok(self.lost());
        };
        let Some(sight) = sight(world, &node) else {
            abandon_all(world, &mut probing);
            return Ok(self.lost());
        };
        if !sight.pose.same(&probing.pose) {
            // Something moved since the points were projected: they no
            // longer say where the target is.
            abandon_all(world, &mut probing);
            self.stability = None;
            self.stage = Stage::Settle;
            return Ok(AimProgress::Waiting(AimStage::Settling));
        }
        while let Some(&(asked, point)) = probing.pending.front() {
            let Some(verdict) = asked.read(world, &sight.subject) else {
                self.stage = Stage::Probe(probing);
                return Ok(AimProgress::Waiting(AimStage::Probing));
            };
            let _read = probing.pending.pop_front();
            match verdict {
                Verdict::OnTarget(world_point) => {
                    abandon_all(world, &mut probing);
                    return Ok(self.ready(world, node, point, world_point, probing.bounds));
                }
                Verdict::Elsewhere(other) => {
                    if !probing.read_first {
                        probing.read_first = true;
                        probing.covered_by = other;
                    }
                }
            }
        }
        let covered_by = probing.covered_by;
        self.fail(
            world,
            &node,
            &sight,
            ActionabilityCheck::ReceivesEvents,
            covered_by,
        )
    }

    /// The aim is found: the target, with where the click lands on it.
    fn ready(
        &self,
        world: &World,
        node: WorldNode,
        aim: Vec2,
        world_point: Option<Vec3>,
        bounds: Rect,
    ) -> AimProgress {
        let offset = region_offset(world);
        let hit_point = world_point.map(|point| {
            let local = bevy_to_sl_vec(Vec3::new(
                point.x - offset.x,
                point.y - offset.y,
                point.z - offset.z,
            ));
            [local.x, local.y, local.z]
        });
        AimProgress::Ready(WorldTarget {
            node: Box::new(node),
            aim,
            hit_point,
            bounds,
            intent: self.intent,
        })
    }

    /// No point takes a click: frame the target once and start over, or fail.
    fn fail(
        &mut self,
        world: &mut World,
        node: &WorldNode,
        sight: &Sight,
        check: ActionabilityCheck,
        covered_by: Option<Uuid>,
    ) -> Result<AimProgress, PursuitError> {
        if self.reveal && !self.revealed && world.contains_resource::<Messages<FrameObject>>() {
            let (center, radius) = sight.sphere();
            world.write_message(FrameObject {
                center,
                radius,
                passes_through: sight.prims.clone(),
            });
            self.revealed = true;
            self.settle_from = self.frames.saturating_add(REVEAL_SETTLE_FRAMES);
            self.stability = None;
            self.stage = Stage::Settle;
            return Ok(AimProgress::Waiting(AimStage::Revealing(check)));
        }
        if self.reveal && self.revealed {
            // The camera eases into a framing, and a glide's last stretch is
            // slow enough to pass for still: a target the reveal is still
            // bringing on screen is looked at again until the deadline, which
            // reports the check if it never passes. (A viewer process at ten
            // frames a second found its prim off screen at the first look.)
            self.stability = None;
            self.stage = Stage::Settle;
            return Ok(AimProgress::Waiting(AimStage::Revealing(check)));
        }
        Err(AutomationError::WorldNotActionable {
            locator: self.locator.clone(),
            check,
            node: Box::new(node.clone()),
            covered_by,
        }
        .into())
    }

    /// Record this poll's pose and say whether it has now held for
    /// [`STABLE_FRAMES`] polls in a row.
    fn observe(&mut self, pose: Pose) -> bool {
        let streak = match self.stability {
            Some((last, streak)) if last.same(&pose) => streak.saturating_add(1),
            _ => 1,
        };
        self.stability = Some((pose, streak));
        streak >= STABLE_FRAMES
    }

    /// Give up the probes in flight.
    fn abandon(&mut self, world: &mut World) {
        if let Stage::Probe(probing) = &mut self.stage {
            abandon_all(world, probing);
        }
    }
}

/// Give up every question `probing` still waits for.
fn abandon_all(world: &mut World, probing: &mut Probing) {
    for (asked, _point) in probing.pending.drain(..) {
        asked.abandon(world);
    }
}

/// Where `node`'s bounding box lands on screen. `None` when it is not
/// tracked as a full object, no camera stands, or no corner of it is in front
/// of the camera.
#[must_use]
pub fn screen_projection(world: &mut World, node: &WorldNode) -> Option<ScreenProjection> {
    let sight = sight(world, node)?;
    let (camera, transform) = &sight.camera;
    let viewport = camera.logical_viewport_size()?;
    let candidates = candidates(&sight.boxed, sight.eye, viewport, |point| {
        camera.world_to_viewport(transform, point).ok()
    });
    Some(ScreenProjection {
        bounds: candidates.bounds?,
        on_screen: candidates.on_screen,
    })
}

/// The main viewer camera, cloned out of the world.
pub(crate) fn view(world: &mut World) -> Option<(Camera, GlobalTransform)> {
    let mut cameras = world.query_filtered::<(&Camera, &GlobalTransform), With<ViewerCamera>>();
    cameras
        .single(world)
        .ok()
        .map(|(camera, transform)| (camera.clone(), *transform))
}

/// What the aim needs to know about `node` and the view this poll: its
/// subject, box and pose. `None` while it is not a tracked full object (a
/// coarse avatar, an object not rezzed yet) or no camera stands.
fn sight(world: &mut World, node: &WorldNode) -> Option<Sight> {
    let (subject, geometry, prims) = locate(world, node)?;
    let boxed = world.get::<GlobalTransform>(geometry)?.affine();
    let camera = view(world)?;
    let eye = camera.1.translation();
    let pose = Pose::of(&boxed, |point| {
        camera.0.world_to_viewport(&camera.1, point).ok()
    });
    Some(Sight {
        subject,
        boxed,
        prims,
        camera,
        eye,
        pose,
    })
}

/// The pick answers that count as `node`, the entity whose transform is its
/// bounding box (the geometry holder, which carries the object's scale), and
/// the entities of its prims.
fn locate(world: &World, node: &WorldNode) -> Option<(Subject, Entity, Vec<Entity>)> {
    let objects = world.get_resource::<ObjectState>()?;
    match node.kind {
        WorldKind::Avatar => {
            let agent = AgentKey::from(node.full_id);
            let avatars = world.get_resource::<AvatarState>()?;
            let (scoped, _agent) = avatars
                .by_scoped
                .iter()
                .find(|(_scoped, known)| **known == agent)?;
            let tracked = objects.objects().get(scoped)?;
            Some((
                Subject::Avatar(agent),
                tracked.geometry,
                vec![tracked.entity],
            ))
        }
        WorldKind::Object | WorldKind::Attachment => {
            let key = ObjectKey::from(node.full_id);
            let (scoped, tracked) = objects
                .objects()
                .iter()
                .find(|(_scoped, tracked)| tracked.full_key == key)?;
            let mut prims = vec![*scoped];
            if tracked.is_root {
                prims.extend_from_slice(objects.children_of(scoped));
            }
            let worn = (node.kind == WorldKind::Attachment).then_some(*scoped);
            let entities = prims
                .iter()
                .filter_map(|prim| objects.entity_by_scoped(prim))
                .collect();
            Some((Subject::Prims { prims, worn }, tracked.geometry, entities))
        }
    }
}

/// The full id of what a pick found, for a failure's "covered by": an
/// avatar's, or an object's. Land and water have none.
pub(crate) fn full_id_of(world: &World, target: &ProbeTarget) -> Option<Uuid> {
    match target {
        ProbeTarget::Avatar { agent, worn: None } => Some(agent.uuid()),
        ProbeTarget::Avatar {
            worn: Some(scoped), ..
        }
        | ProbeTarget::Object { scoped, .. } => object_full_id(world, scoped),
        ProbeTarget::Ground | ProbeTarget::Water => None,
    }
}

/// The full id of the tracked object `scoped`.
pub(crate) fn object_full_id(world: &World, scoped: &ScopedObjectId) -> Option<Uuid> {
    world
        .get_resource::<ObjectState>()
        .and_then(|objects| objects.objects().get(scoped))
        .map(|tracked| tracked.full_key.uuid())
}

/// The Bevy-space offset of the agent's region from the scene origin.
pub(crate) fn region_offset(world: &World) -> Vec3 {
    let handle = world
        .get_resource::<SlIdentity>()
        .and_then(|identity| identity.region_handle);
    let origin = world
        .get_resource::<ObjectState>()
        .and_then(|objects| objects.origin);
    handle.map_or(Vec3::ZERO, |handle| region_offset_bevy(handle, origin))
}

/// Of `points` (logical pixels), those a click at would reach the world
/// rather than a UI node.
///
/// # Errors
///
/// When the UI model cannot be read.
pub(crate) fn reach_the_world(
    world: &mut World,
    points: Vec<Vec2>,
) -> Result<Vec<Vec2>, PursuitError> {
    let scale = {
        let mut windows = world.query_filtered::<&Window, With<PrimaryWindow>>();
        windows.single(world).map_or(1.0, Window::scale_factor)
    };
    let mut state = SystemState::<UiModel<'_, '_>>::new(world);
    let model = state.get(world)?;
    Ok(points
        .into_iter()
        .filter(|point| !model.takes_click_at(Vec2::new(point.x * scale, point.y * scale)))
        .collect())
}

/// The candidate points of one projection.
#[derive(Debug, Clone, PartialEq)]
struct Candidates {
    /// The box of the box's corners in front of the camera; `None` when none
    /// is.
    bounds: Option<Rect>,
    /// Whether any point of a face towards the camera is in the viewport.
    on_screen: bool,
    /// The points to probe, best first, all in the viewport.
    points: Vec<Vec2>,
}

/// One of a box's three axes.
#[derive(Debug, Clone, Copy)]
enum Axis {
    /// Local x.
    X,
    /// Local y.
    Y,
    /// Local z.
    Z,
}

/// A point of the unit cube: `side` along `axis`, `u` and `v` along the other
/// two.
const fn face_point(axis: Axis, side: f32, u: f32, v: f32) -> Vec3 {
    match axis {
        Axis::X => Vec3::new(side, u, v),
        Axis::Y => Vec3::new(u, side, v),
        Axis::Z => Vec3::new(u, v, side),
    }
}

/// `a − b`, component-wise (glam's operators are linted).
const fn vsub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Whether `point` lies in a viewport of `size`, logical pixels.
fn in_viewport(point: Vec2, size: Vec2) -> bool {
    point.x >= 0.0 && point.y >= 0.0 && point.x < size.x && point.y < size.y
}

/// The points a click could aim at on the box `boxed` (the image of the unit
/// cube), seen from `eye` through `project`, best first: the projection of
/// the box's centre, then the centre of each face towards the camera, then a
/// lattice on those faces — each group nearest the projected centre first,
/// only points inside `viewport`, near-duplicates dropped, at most
/// [`MAX_CANDIDATES`].
fn candidates(
    boxed: &Affine3A,
    eye: Vec3,
    viewport: Vec2,
    project: impl Fn(Vec3) -> Option<Vec2>,
) -> Candidates {
    let corners: Vec<Vec2> = [-0.5_f32, 0.5]
        .into_iter()
        .flat_map(|x| [-0.5_f32, 0.5].into_iter().map(move |y| (x, y)))
        .flat_map(|(x, y)| [-0.5_f32, 0.5].into_iter().map(move |z| Vec3::new(x, y, z)))
        .filter_map(|corner| project(boxed.transform_point3(corner)))
        .collect();
    let bounds = corners.first().map(|first| {
        corners
            .iter()
            .fold(Rect::from_corners(*first, *first), |rect, corner| {
                rect.union_point(*corner)
            })
    });
    let centre = boxed.transform_point3(Vec3::ZERO);
    let projected_centre = project(centre);
    let anchor = projected_centre
        .or_else(|| bounds.map(|bounds| bounds.center()))
        .unwrap_or(Vec2::ZERO);
    let mut face_centres = Vec::new();
    let mut lattice = Vec::new();
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        for side in [-0.5_f32, 0.5] {
            let face_centre = boxed.transform_point3(face_point(axis, side, 0.0, 0.0));
            let facing = vsub(face_centre, centre).dot(vsub(eye, face_centre)) > 0.0;
            if !facing {
                continue;
            }
            for u in LATTICE {
                for v in LATTICE {
                    let Some(point) = project(boxed.transform_point3(face_point(axis, side, u, v)))
                    else {
                        continue;
                    };
                    if u == 0.0 && v == 0.0 {
                        face_centres.push(point);
                    } else {
                        lattice.push(point);
                    }
                }
            }
        }
    }
    let on_screen = face_centres
        .iter()
        .chain(&lattice)
        .any(|point| in_viewport(*point, viewport));
    let by_distance = |a: &Vec2, b: &Vec2| a.distance(anchor).total_cmp(&b.distance(anchor));
    face_centres.sort_by(by_distance);
    lattice.sort_by(by_distance);
    let mut points: Vec<Vec2> = Vec::new();
    for point in projected_centre
        .into_iter()
        .chain(face_centres)
        .chain(lattice)
        .filter(|point| in_viewport(*point, viewport))
    {
        if points.len() >= MAX_CANDIDATES {
            break;
        }
        if points
            .iter()
            .all(|kept| kept.distance(point) >= DEDUPE_PIXELS)
        {
            points.push(point);
        }
    }
    Candidates {
        bounds,
        on_screen,
        points,
    }
}

#[cfg(test)]
mod tests;
