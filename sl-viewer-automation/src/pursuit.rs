//! [`Pursuit`]: one action's wait for its node, polled once a frame — resolve
//! the locator strictly, make the node actionable where a user would, and
//! check that it is.

use std::time::{Duration, Instant};

use bevy::ecs::system::{SystemParamValidationError, SystemState};
use bevy::prelude::*;
use sl_automation_proto::{
    ActionabilityCheck, AutomationError, Bounds, Deadline, Locator, NodeId, NodeState,
    NodeVisibility, Role, UiNode,
};

use crate::locate::{find_all, find_one, shallow};
use crate::reveal::{ListSearch, scroll_into_view};
use crate::ui_model::{UiModel, entity_of};

/// The frames an action waits for its node when its [`Deadline`] does not say.
pub const DEFAULT_DEADLINE_FRAMES: u32 = 600;

/// The wall-clock time an action waits for its node when its [`Deadline`] does
/// not say.
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(10);

/// How many consecutive frames a node's bounds must agree before it counts as
/// stable — two, as a browser driver counts animation frames: a node that
/// moved between the last two frames is still moving.
const STABLE_FRAMES: u32 = 2;

/// What an action is going to do to its node, which decides the checks it
/// must pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Press and release on it: attached, visible, in the viewport, stable,
    /// enabled and receiving events.
    Click,
    /// Put the pointer over it: as a click, but a disabled node may be
    /// hovered.
    Hover,
    /// Type into it: as a click, and it must be a text field that is not
    /// read-only.
    Fill,
}

impl Intent {
    /// Whether the node must be enabled.
    const fn needs_enabled(self) -> bool {
        matches!(self, Self::Click | Self::Fill)
    }

    /// Whether the node must accept text.
    const fn needs_editable(self) -> bool {
        matches!(self, Self::Fill)
    }
}

/// The node an action may now be applied to, and where to aim at it.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// The node, as it was in the frame it became actionable, without its
    /// children.
    pub node: Box<UiNode>,
    /// The centre of its visible part, in logical pixels — where the pointer
    /// goes.
    pub aim: Vec2,
}

/// Where a [`Pursuit`] stands after a frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    /// Exactly one node matches and it passes every check.
    Ready(Target),
    /// Not yet: the first check that failed this frame. Poll again next frame.
    Waiting(ActionabilityCheck),
}

/// Why a [`Pursuit`] gave up.
#[derive(Debug, thiserror::Error)]
pub enum PursuitError {
    /// The action cannot be carried out: nothing matched in time, several
    /// nodes matched, or the one match can never pass a check.
    #[error(transparent)]
    Automation(#[from] Box<AutomationError>),
    /// The semantic model could not be read from the world at all.
    #[error("the semantic model could not be read: {0}")]
    Model(#[from] SystemParamValidationError),
}

impl From<AutomationError> for PursuitError {
    fn from(error: AutomationError) -> Self {
        Self::Automation(Box::new(error))
    }
}

/// One action's wait for its node, polled once a frame.
///
/// Each [`poll`](Self::poll) snapshots the UI and resolves the locator
/// **strictly**: several matches fail at once with every candidate listed.
/// With one match, it is made actionable where a user would make it so —
/// scrolled into view when a scroll area hides it, and a virtual list paged
/// until the row is bound when nothing matches yet — and the checks are made
/// in [`ActionabilityCheck`] order. The first failure is reported and the
/// caller polls again next frame, until the node is ready or the deadline
/// passes.
///
/// A virtual list shows only the rows in its window, so strictness is judged
/// over the rows it has bound: a second match far down an unscrolled list is
/// not seen, as a browser driver does not see a row a virtualised table has
/// not rendered.
#[derive(Debug)]
pub struct Pursuit {
    /// The node wanted.
    locator: Locator,
    /// What will be done to it.
    intent: Intent,
    /// The most frames to wait.
    max_frames: u32,
    /// The most wall-clock time to wait.
    max_time: Duration,
    /// When the first poll ran.
    started: Option<Instant>,
    /// The polls so far.
    frames: u32,
    /// The node's bounds in the last poll, and for how many polls in a row
    /// they have been those.
    stability: Option<(NodeId, Bounds, u32)>,
    /// The virtual-list paging, while nothing matches.
    search: ListSearch,
    /// The nodes the locator matched in the last poll, for a timeout's report.
    last_observed: Vec<UiNode>,
}

/// What one poll decided, before the world is touched.
enum Decision {
    /// Ready.
    Ready(Target),
    /// Waiting on a check.
    Wait(ActionabilityCheck),
    /// Waiting on the visible check, and this node should be scrolled to.
    ScrollTo(Entity),
    /// Nothing matches: page the virtual lists under this scope (all, when
    /// `None`).
    Search(Option<Entity>),
}

impl Pursuit {
    /// Wait for the one node `locator` names to be actionable for `intent`,
    /// under the default deadline.
    #[must_use]
    pub fn new(locator: Locator, intent: Intent) -> Self {
        Self {
            locator,
            intent,
            max_frames: DEFAULT_DEADLINE_FRAMES,
            max_time: DEFAULT_DEADLINE,
            started: None,
            frames: 0,
            stability: None,
            search: ListSearch::default(),
            last_observed: Vec::new(),
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

    /// The locator being pursued.
    #[must_use]
    pub const fn locator(&self) -> &Locator {
        &self.locator
    }

    /// Look at the UI once — a frame's worth of the wait. May scroll a scroll
    /// area or a virtual list on the way.
    ///
    /// # Errors
    ///
    /// [`AutomationError::Ambiguous`] as soon as several nodes match (or the
    /// scope does); [`AutomationError::NotActionable`] when the one match can
    /// never pass a check (a `Fill` aimed at something that is not a text
    /// field); [`AutomationError::TimedOut`] once the deadline passes with a
    /// check still failing, carrying it and the last nodes seen; and
    /// [`PursuitError::Model`] when the model cannot be read at all.
    pub fn poll(&mut self, world: &mut World) -> Result<Progress, PursuitError> {
        let started = *self.started.get_or_insert_with(Instant::now);
        self.frames = self.frames.saturating_add(1);
        let decision = {
            let mut state = SystemState::<UiModel<'_, '_>>::new(world);
            let model = state.get(world)?;
            self.decide(&model)?
        };
        let check = match decision {
            Decision::Ready(target) => {
                self.search.found();
                return Ok(Progress::Ready(target));
            }
            Decision::Wait(check) => check,
            Decision::ScrollTo(entity) => {
                self.search.found();
                scroll_into_view(world, entity);
                ActionabilityCheck::Visible
            }
            Decision::Search(scope) => {
                self.search.step(world, scope);
                ActionabilityCheck::Attached
            }
        };
        let waited = started.elapsed();
        if self.frames >= self.max_frames || waited >= self.max_time {
            return Err(AutomationError::TimedOut {
                locator: self.locator.clone(),
                condition: None,
                failed_check: (self.last_observed.len() == 1).then_some(check),
                last_observed: core::mem::take(&mut self.last_observed),
                frames: self.frames,
                millis: u64::try_from(waited.as_millis()).unwrap_or(u64::MAX),
            }
            .into());
        }
        Ok(Progress::Waiting(check))
    }

    /// Resolve the locator in the model and judge the match, without touching
    /// the world.
    fn decide(&mut self, model: &UiModel<'_, '_>) -> Result<Decision, Box<AutomationError>> {
        let roots = model.snapshot();
        let found = match find_all(&roots, &self.locator) {
            Ok(found) => found,
            // The scope is not there (yet): a floater still opening, a panel
            // not built. Nothing inside it can match.
            Err(error) if matches!(*error, AutomationError::NotFound { .. }) => {
                self.forget();
                return Ok(Decision::Wait(ActionabilityCheck::Attached));
            }
            Err(error) => return Err(error),
        };
        let node = match found.as_slice() {
            [] => {
                self.forget();
                let scope = match &self.locator.within {
                    Some(scope) => Some(find_one(&roots, scope)?.id),
                    None => None,
                };
                return Ok(Decision::Search(scope.and_then(entity_of)));
            }
            [node] => *node,
            several => {
                return Err(Box::new(AutomationError::Ambiguous {
                    locator: self.locator.clone(),
                    candidates: several.iter().map(|node| shallow(node)).collect(),
                }));
            }
        };
        self.last_observed = vec![shallow(node)];
        let stable = self.observe_bounds(node);
        let entity = entity_of(node.id);
        match node.visibility {
            NodeVisibility::Hidden => return Ok(Decision::Wait(ActionabilityCheck::Visible)),
            NodeVisibility::Clipped => {
                return Ok(
                    entity.map_or(Decision::Wait(ActionabilityCheck::Visible), |entity| {
                        Decision::ScrollTo(entity)
                    }),
                );
            }
            NodeVisibility::OffScreen => {
                return Ok(Decision::Wait(ActionabilityCheck::InViewport));
            }
            NodeVisibility::Visible | NodeVisibility::Covered => {}
        }
        if !stable {
            return Ok(Decision::Wait(ActionabilityCheck::Stable));
        }
        if self.intent.needs_enabled() && node.has_state(NodeState::Disabled) {
            return Ok(Decision::Wait(ActionabilityCheck::Enabled));
        }
        if self.intent.needs_editable() {
            // A spin button is a number field: it is typed into like a text box.
            if !matches!(node.role, Role::Textbox | Role::SpinButton) {
                return Err(Box::new(AutomationError::NotActionable {
                    locator: self.locator.clone(),
                    check: ActionabilityCheck::Editable,
                    node: shallow(node),
                }));
            }
            if node.has_state(NodeState::ReadOnly) {
                return Ok(Decision::Wait(ActionabilityCheck::Editable));
            }
        }
        if node.visibility == NodeVisibility::Covered {
            return Ok(Decision::Wait(ActionabilityCheck::ReceivesEvents));
        }
        let Some(aim) = entity.and_then(|entity| model.aim_point(entity)) else {
            return Ok(Decision::Wait(ActionabilityCheck::Visible));
        };
        Ok(Decision::Ready(Target {
            node: Box::new(shallow(node)),
            aim,
        }))
    }

    /// Record `node`'s bounds for this poll and say whether they have now held
    /// still for [`STABLE_FRAMES`] polls in a row.
    fn observe_bounds(&mut self, node: &UiNode) -> bool {
        let streak = match self.stability {
            Some((id, bounds, streak)) if id == node.id && same_bounds(bounds, node.bounds) => {
                streak.saturating_add(1)
            }
            _ => 1,
        };
        self.stability = Some((node.id, node.bounds, streak));
        streak >= STABLE_FRAMES
    }

    /// No single node matched this poll: nothing is being watched for
    /// stability, and nothing was observed.
    fn forget(&mut self) {
        self.stability = None;
        self.last_observed.clear();
    }
}

/// Whether two boxes are the same to well under a logical pixel.
fn same_bounds(a: Bounds, b: Bounds) -> bool {
    const TOLERANCE: f32 = 0.01;
    (a.x - b.x).abs() < TOLERANCE
        && (a.y - b.y).abs() < TOLERANCE
        && (a.width - b.width).abs() < TOLERANCE
        && (a.height - b.height).abs() < TOLERANCE
}

#[cfg(test)]
mod tests;
