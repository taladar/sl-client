//! The requests on the UI: snapshots and finds, the actions (click, hover,
//! fill, a key press, the routes through menus, combos and pies), opening a
//! floater, and the waits over a locator's matches.

use bevy::input::keyboard::Key;
use bevy::prelude::*;
use sl_automation_proto::{
    AutomationError, Deadline, Locator, NameMatcher, NodeState, NodeValue, NodeVisibility,
    PointerButton, ResponseBody, Role, UiNode, WaitCondition,
};
use sl_viewer_ui_core::synthetic_input::{InputAction, InputActionId};

use super::{Answer, Clock, Started, Step, automation_error, enqueue, finished, keys::parse_keys};
use crate::locate::{find_all, find_one, shallow};
use crate::pursuit::{Intent, Progress, Pursuit};
use crate::reveal::open_floater;
use crate::route::{Gesture, Route, RouteProgress};
use crate::ui_model::snapshot;
use crate::world_aim::drop_gesture;

/// How many frames a fill waits, after its typing, for the field to hold the
/// text before it reports what the field holds instead.
const FILL_CONFIRM_FRAMES: u32 = 10;

/// The frames a UI drag rests over its target before it lets go, so the
/// target is the hovered node when the button comes up.
const DRAG_REST_FRAMES: usize = 2;

/// Open the floater `floater`, naming its window.
pub(super) fn open(world: &mut World, floater: &str) -> Answer {
    open_floater(world, floater)
        .map(|window| ResponseBody::Opened { window })
        .map_err(|error| Box::new(automation_error(error)))
}

/// A key press: `keys` parsed, or why it cannot be.
pub(super) fn press(keys: &str, hold_frames: u32) -> Started {
    match parse_keys(keys, hold_frames) {
        Ok(action) => Started::Running(super::Task::ui(UiTask::Press(Play::Ready(action)))),
        Err(reason) => Started::answered(Err(Box::new(AutomationError::InvalidRequest {
            reason: format!("the keys {keys:?}: {reason}"),
        }))),
    }
}

/// A menu path by the entries' Fluent keys.
pub(super) fn menu_path(path: &[String], deadline: Deadline) -> Started {
    if path.is_empty() {
        return Started::answered(Err(Box::new(AutomationError::InvalidRequest {
            reason: "an empty menu path".to_owned(),
        })));
    }
    let keys: Vec<&str> = path.iter().map(String::as_str).collect();
    route(Route::menu_path(&keys).with_deadline(deadline))
}

/// A wait until `locator`'s matches satisfy `condition`.
pub(super) fn wait(locator: Locator, condition: WaitCondition, deadline: Deadline) -> Started {
    Started::Running(super::Task::ui(UiTask::Wait(UiWait {
        locator,
        condition,
        clock: Clock::new(deadline),
        last_observed: Vec::new(),
    })))
}

/// The semantic tree, whole or under the one node `within` resolves to.
pub(super) fn read_snapshot(world: &mut World, within: Option<Locator>) -> Answer {
    let roots = model(world)?;
    let roots = match within {
        Some(scope) => vec![find_one(&roots, &scope)?.clone()],
        None => roots,
    };
    Ok(ResponseBody::Snapshot { roots })
}

/// Every node `locator` matches.
pub(super) fn find(world: &mut World, locator: &Locator) -> Answer {
    let roots = model(world)?;
    let nodes = find_all(&roots, locator)?
        .into_iter()
        .map(shallow)
        .collect();
    Ok(ResponseBody::Found { nodes })
}

/// The semantic model, or why it cannot be read.
fn model(world: &mut World) -> Result<Vec<UiNode>, Box<AutomationError>> {
    snapshot(world).map_err(|error| {
        Box::new(AutomationError::Unavailable {
            what: format!("readable semantic model ({error})"),
        })
    })
}

/// An action on the one node `locator` names.
pub(super) fn act(locator: Locator, kind: UiActKind, deadline: Deadline) -> Started {
    let intent = match kind {
        UiActKind::Click { .. } => Intent::Click,
        UiActKind::Hover => Intent::Hover,
        UiActKind::Fill(_) => Intent::Fill,
    };
    Started::Running(super::Task::ui(UiTask::Act(UiAct {
        pursuit: Pursuit::new(locator.clone(), intent).with_deadline(deadline),
        locator,
        kind,
        stage: ActStage::Pursue,
    })))
}

/// A drag of the one node `source` names onto the one `target` names.
pub(super) fn drag_to(source: Locator, target: Locator, deadline: Deadline) -> Started {
    drag(source, DragEnd::Onto(target), deadline)
}

/// A drag of the one node `source` names by `offset` logical pixels.
pub(super) fn drag_by(source: Locator, offset: [f32; 2], deadline: Deadline) -> Started {
    drag(source, DragEnd::By(Vec2::from(offset)), deadline)
}

/// A drag of the one node `source` names to `end`.
fn drag(source: Locator, end: DragEnd, deadline: Deadline) -> Started {
    Started::Running(super::Task::ui(UiTask::Drag(Box::new(UiDrag {
        pursuit: Pursuit::new(source, Intent::Click).with_deadline(deadline),
        end,
        deadline,
        stage: DragStage::Source,
    }))))
}

/// A route of gestures.
pub(super) fn route(route: Route) -> Started {
    Started::Running(super::Task::ui(UiTask::Route(RouteTask {
        route,
        playing: None,
    })))
}

/// A UI request under way.
pub(super) enum UiTask {
    /// An action on one node.
    Act(UiAct),
    /// A drag of one node onto another, or by an offset.
    Drag(Box<UiDrag>),
    /// A key press.
    Press(Play),
    /// A route of gestures.
    Route(RouteTask),
    /// A wait over a locator's matches.
    Wait(UiWait),
}

impl UiTask {
    /// Whether it plays input.
    pub(super) const fn acts(&self) -> bool {
        !matches!(self, Self::Wait(_))
    }

    /// Advance it by a frame.
    pub(super) fn poll(&mut self, world: &mut World) -> Step {
        match self {
            Self::Act(act) => act.poll(world),
            Self::Drag(drag) => drag.poll(world),
            Self::Press(play) => match play.poll(world) {
                Ok(true) => Step::done(ResponseBody::Pressed),
                Ok(false) => Step::Pending,
                Err(error) => Step::fail(error),
            },
            Self::Route(route) => route.poll(world),
            Self::Wait(wait) => wait.poll(world),
        }
    }
}

/// Input to play, then wait out.
pub(super) enum Play {
    /// Not queued yet.
    Ready(InputAction),
    /// Queued; done when the injector has played it.
    Queued(InputActionId),
}

impl Play {
    /// Queue the input on the first poll; `true` once it has been played.
    fn poll(&mut self, world: &mut World) -> Result<bool, Box<AutomationError>> {
        match self {
            Self::Ready(action) => {
                let action = core::mem::replace(action, InputAction::from_steps(Vec::new()));
                *self = Self::Queued(enqueue(world, action)?);
                Ok(false)
            }
            Self::Queued(id) => Ok(finished(world, *id)),
        }
    }
}

/// What a UI action does to its node.
#[derive(Debug)]
pub(super) enum UiActKind {
    /// A click.
    Click {
        /// With which button.
        button: PointerButton,
        /// Twice, as a double click.
        double: bool,
    },
    /// The pointer onto it.
    Hover,
    /// Its text replaced by typing.
    Fill(String),
}

/// Where a UI action stands.
enum ActStage {
    /// Waiting for the node to become actionable.
    Pursue,
    /// Playing the gesture on the node as it was then.
    Play(InputActionId, Box<UiNode>),
    /// A fill waiting for the field to hold the text, for this many frames
    /// so far.
    Confirm(Box<UiNode>, u32),
}

/// An action on the one node a locator names.
pub(super) struct UiAct {
    /// The node.
    locator: Locator,
    /// What to do to it.
    kind: UiActKind,
    /// The wait for it.
    pursuit: Pursuit,
    /// Where it stands.
    stage: ActStage,
}

impl UiAct {
    /// Advance it by a frame.
    fn poll(&mut self, world: &mut World) -> Step {
        match &mut self.stage {
            ActStage::Pursue => {
                let target = match self.pursuit.poll(world) {
                    Ok(Progress::Ready(target)) => target,
                    Ok(Progress::Waiting(_check)) => return Step::Pending,
                    Err(error) => return Step::fail(automation_error(error)),
                };
                let gesture = match &self.kind {
                    UiActKind::Click { button, double } => {
                        let button = match button {
                            PointerButton::Left => MouseButton::Left,
                            PointerButton::Right => MouseButton::Right,
                        };
                        if *double {
                            InputAction::double_click(target.aim, button)
                        } else {
                            InputAction::click(target.aim, button)
                        }
                    }
                    UiActKind::Hover => InputAction::move_to(target.aim),
                    UiActKind::Fill(text) => InputAction::click(target.aim, MouseButton::Left)
                        .then(InputAction::chord(
                            &[(KeyCode::ControlLeft, Key::Control)],
                            KeyCode::KeyA,
                            Key::Character("a".into()),
                        ))
                        .then(InputAction::tap(KeyCode::Backspace, Key::Backspace))
                        .then(InputAction::type_text(text)),
                };
                match enqueue(world, gesture) {
                    Ok(id) => {
                        self.stage = ActStage::Play(id, target.node);
                        Step::Pending
                    }
                    Err(error) => Step::fail(error),
                }
            }
            ActStage::Play(id, node) => {
                if !finished(world, *id) {
                    return Step::Pending;
                }
                if matches!(self.kind, UiActKind::Fill(_)) {
                    self.stage = ActStage::Confirm(node.clone(), 0);
                    return Step::Pending;
                }
                Step::done(ResponseBody::Done {
                    node: (**node).clone(),
                })
            }
            ActStage::Confirm(node, frames) => {
                let UiActKind::Fill(text) = &self.kind else {
                    return Step::Pending;
                };
                let now = snapshot(world)
                    .ok()
                    .and_then(|roots| find_by_id(&roots, node.id).map(shallow));
                if let Some(now) = &now
                    && now.value.as_ref().is_some_and(|value| value.holds(text))
                {
                    return Step::done(ResponseBody::Done { node: now.clone() });
                }
                *frames = frames.saturating_add(1);
                if *frames < FILL_CONFIRM_FRAMES {
                    return Step::Pending;
                }
                Step::fail(AutomationError::FillMismatch {
                    locator: self.locator.clone(),
                    text: text.clone(),
                    node: now.unwrap_or_else(|| (**node).clone()),
                })
            }
        }
    }
}

/// Where a drag stands.
enum DragStage {
    /// Waiting for the source to become actionable.
    Source,
    /// Waiting for the target, the drag to start at this point.
    Target(Vec2, Box<Pursuit>),
    /// Playing the drag onto the target as it was then.
    Play(InputActionId, Box<UiNode>),
}

/// Where a drag ends.
enum DragEnd {
    /// Over the one node a locator names.
    Onto(Locator),
    /// This far from where it starts, in logical pixels.
    By(Vec2),
}

/// A drag of the one node a locator names onto the one another names, or by
/// an offset.
pub(super) struct UiDrag {
    /// The wait for the source.
    pursuit: Pursuit,
    /// Where it ends.
    end: DragEnd,
    /// When to give up waiting for the target.
    deadline: Deadline,
    /// Where it stands.
    stage: DragStage,
}

impl UiDrag {
    /// Advance it by a frame.
    fn poll(&mut self, world: &mut World) -> Step {
        match &mut self.stage {
            DragStage::Source => match self.pursuit.poll(world) {
                Ok(Progress::Ready(source)) => match &self.end {
                    DragEnd::Onto(target) => {
                        // The target need only be where the pointer can reach
                        // it: a drop target may be disabled for clicks.
                        let pursuit = Box::new(
                            Pursuit::new(target.clone(), Intent::Hover)
                                .with_deadline(self.deadline),
                        );
                        self.stage = DragStage::Target(source.aim, pursuit);
                        Step::Pending
                    }
                    DragEnd::By(offset) => {
                        let to = Vec2::new(source.aim.x + offset.x, source.aim.y + offset.y);
                        let gesture = drop_gesture(source.aim, to, DRAG_REST_FRAMES);
                        self.play(world, gesture, source.node)
                    }
                },
                Ok(Progress::Waiting(_check)) => Step::Pending,
                Err(error) => Step::fail(automation_error(error)),
            },
            DragStage::Target(from, pursuit) => match pursuit.poll(world) {
                Ok(Progress::Ready(target)) => {
                    let gesture = drop_gesture(*from, target.aim, DRAG_REST_FRAMES);
                    self.play(world, gesture, target.node)
                }
                Ok(Progress::Waiting(_check)) => Step::Pending,
                Err(error) => Step::fail(automation_error(error)),
            },
            DragStage::Play(id, node) => {
                if finished(world, *id) {
                    Step::done(ResponseBody::Done {
                        node: (**node).clone(),
                    })
                } else {
                    Step::Pending
                }
            }
        }
    }

    /// Queue `gesture`, the drag itself, answering with `node` once played.
    fn play(&mut self, world: &mut World, gesture: InputAction, node: Box<UiNode>) -> Step {
        match enqueue(world, gesture) {
            Ok(id) => {
                self.stage = DragStage::Play(id, node);
                Step::Pending
            }
            Err(error) => Step::fail(error),
        }
    }
}

/// The node of `roots` with id `id`, searched depth first.
fn find_by_id(roots: &[UiNode], id: sl_automation_proto::NodeId) -> Option<&UiNode> {
    roots.iter().find_map(|node| {
        if node.id == id {
            Some(node)
        } else {
            find_by_id(&node.children, id)
        }
    })
}

/// A route of gestures under way.
pub(super) struct RouteTask {
    /// The route.
    route: Route,
    /// The gesture being played, when one is.
    playing: Option<InputActionId>,
}

impl RouteTask {
    /// Advance it by a frame.
    fn poll(&mut self, world: &mut World) -> Step {
        if let Some(id) = self.playing {
            if !finished(world, id) {
                return Step::Pending;
            }
            self.playing = None;
        }
        match self.route.poll(world) {
            Ok(RouteProgress::Act { gesture, target }) => {
                let input = match gesture {
                    Gesture::Click => InputAction::click(target.aim, MouseButton::Left),
                    Gesture::Hover => InputAction::move_to(target.aim),
                };
                match enqueue(world, input) {
                    Ok(id) => {
                        self.playing = Some(id);
                        Step::Pending
                    }
                    Err(error) => Step::fail(error),
                }
            }
            Ok(RouteProgress::Waiting(_check)) => Step::Pending,
            Ok(RouteProgress::Done(Some(node))) => Step::done(ResponseBody::Done { node: *node }),
            // Every step was already done (an open menu path's last entry is
            // always clicked, so this is a route of no steps).
            Ok(RouteProgress::Done(None)) => Step::fail(AutomationError::InvalidRequest {
                reason: "the route made no gesture".to_owned(),
            }),
            Err(error) => Step::fail(automation_error(error)),
        }
    }
}

/// A wait over the nodes a locator matches.
pub(super) struct UiWait {
    /// The nodes.
    locator: Locator,
    /// What they must come to satisfy.
    condition: WaitCondition,
    /// Its deadline.
    clock: Clock,
    /// The matches in the last frame.
    last_observed: Vec<UiNode>,
}

impl UiWait {
    /// Advance it by a frame.
    fn poll(&mut self, world: &mut World) -> Step {
        self.clock.tick();
        let roots = match model(world) {
            Ok(roots) => roots,
            Err(error) => return Step::fail(error),
        };
        let matches: Vec<UiNode> = match find_all(&roots, &self.locator) {
            Ok(found) => found.into_iter().map(shallow).collect(),
            // A scope that is not there (yet) holds nothing.
            Err(error) if matches!(*error, AutomationError::NotFound { .. }) => Vec::new(),
            Err(error) => return Step::fail(error),
        };
        if holds(&self.condition, &matches) {
            return Step::done(ResponseBody::Satisfied { nodes: matches });
        }
        self.last_observed = matches;
        match self.clock.expired() {
            Some((frames, millis)) => Step::fail(AutomationError::TimedOut {
                locator: self.locator.clone(),
                condition: Some(self.condition.clone()),
                failed_check: None,
                last_observed: core::mem::take(&mut self.last_observed),
                frames,
                millis,
            }),
            None => Step::Pending,
        }
    }
}

/// Whether `condition` holds over `matches`.
///
/// A covered node counts as visible: it is drawn and on screen, only
/// something lies over its centre.
fn holds(condition: &WaitCondition, matches: &[UiNode]) -> bool {
    let visible = |node: &UiNode| {
        matches!(
            node.visibility,
            NodeVisibility::Visible | NodeVisibility::Covered
        )
    };
    let disabled = |node: &UiNode| node.has_state(NodeState::Disabled);
    match condition {
        WaitCondition::Attached => !matches.is_empty(),
        WaitCondition::Detached => matches.is_empty(),
        WaitCondition::Visible => matches.iter().any(visible),
        WaitCondition::Hidden => !matches.iter().any(visible),
        WaitCondition::Enabled => !matches.is_empty() && !matches.iter().any(disabled),
        WaitCondition::Disabled => !matches.is_empty() && matches.iter().all(disabled),
        WaitCondition::Text(matcher) => matches.iter().any(|node| match &node.value {
            // A spin button's text is its number: an exact wait matches the
            // number it spells (`1.250` waits for 1.25), a partial one its
            // shortest spelling.
            Some(value @ NodeValue::Number(number)) if node.role == Role::SpinButton => {
                match matcher {
                    NameMatcher::Exact(text) => value.holds(text),
                    NameMatcher::Contains(_) => matcher.matches(&number.to_string()),
                }
            }
            Some(NodeValue::Text(text) | NodeValue::Color(text)) => matcher.matches(text),
            Some(NodeValue::Number(_)) | None => node
                .name
                .as_deref()
                .is_some_and(|name| matcher.matches(name)),
        }),
    }
}
