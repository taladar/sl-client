//! [`Route`]: the gestures a user makes to reach a node that is not on screen
//! until something is opened — a menu path, a combo's option, a pie slice.

use std::collections::VecDeque;

use bevy::prelude::*;
use sl_automation_proto::{ActionabilityCheck, Deadline, Locator, NodeState, Role, UiNode};
use sl_viewer_ui_pie_menu::pie_menu::PIE_MENU_NAME;

use crate::pursuit::{Intent, Progress, Pursuit, PursuitError, Target};

/// What the pointer does at a step's node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// A left click.
    Click,
    /// Moving the pointer over it — how a submenu opens.
    Hover,
}

impl Gesture {
    /// The checks the gesture's node must pass.
    const fn intent(self) -> Intent {
        match self {
            Self::Click => Intent::Click,
            Self::Hover => Intent::Hover,
        }
    }
}

/// One step of a route: a node, what to do to it, and a state in which the
/// step is already done.
#[derive(Debug, Clone)]
struct Step {
    /// The node.
    locator: Locator,
    /// What to do to it.
    gesture: Gesture,
    /// When the node is already in this state the gesture is skipped — a menu
    /// that is already open is not clicked shut.
    done_when: Option<NodeState>,
}

/// Where a [`Route`] stands after a frame.
#[derive(Debug, Clone, PartialEq)]
pub enum RouteProgress {
    /// Make this gesture at this target now, then poll again.
    Act {
        /// What to do.
        gesture: Gesture,
        /// Where.
        target: Target,
    },
    /// The current step's node is not actionable yet: the check it fails.
    Waiting(ActionabilityCheck),
    /// Every step has been made. The node of the last one, as it was acted on.
    Done(Option<Box<UiNode>>),
}

/// A sequence of gestures, each on the one node its locator names, each
/// waited for like an action ([`Pursuit`]).
///
/// The route does not move the pointer: it says what to do where
/// ([`RouteProgress::Act`]), and the caller does it through the viewer's input
/// path and polls again. So the gestures are the user's, and a disabled entry
/// or a covered option stops the route exactly where it would stop a person.
#[derive(Debug)]
pub struct Route {
    /// The steps not yet begun.
    steps: VecDeque<Step>,
    /// The step under way and its wait.
    current: Option<(Step, Pursuit)>,
    /// The deadline each step waits under.
    deadline: Deadline,
    /// The node the last gesture was made on.
    last: Option<Box<UiNode>>,
}

impl Route {
    /// A route of the given steps.
    fn of(steps: impl IntoIterator<Item = Step>) -> Self {
        Self {
            steps: steps.into_iter().collect(),
            current: None,
            deadline: Deadline::default(),
            last: None,
        }
    }

    /// Walk a menu path from the menu bar: open the bar menu whose Fluent key
    /// is the first of `keys`, hover each submenu line in turn, and click the
    /// last entry.
    ///
    /// Entries are named by key, not text, so the path reads the same in
    /// every locale. Only open menus exist, so an entry is looked for among
    /// them; two open entries sharing a key are ambiguous, as for any action.
    #[must_use]
    pub fn menu_path(keys: &[&str]) -> Self {
        let last = keys.len().saturating_sub(1);
        Self::of(keys.iter().enumerate().map(|(index, key)| {
            let entry = Locator::role(Role::MenuItem).name_key(*key);
            let opens_more = index < last;
            Step {
                // The first entry is a bar button; the rest are in the open
                // menus.
                locator: if index == 0 {
                    entry.within(Locator::role(Role::MenuBar))
                } else {
                    entry
                },
                // A bar menu opens on a click, a submenu on hover.
                gesture: if index == 0 || !opens_more {
                    Gesture::Click
                } else {
                    Gesture::Hover
                },
                done_when: opens_more.then_some(NodeState::Expanded),
            }
        }))
    }

    /// Pick `option` from the combo `combo` names: open it unless it is open,
    /// then click the option in its list. `option` is looked for inside the
    /// combo (its own scope, if it had one, is replaced).
    #[must_use]
    pub fn select_option(combo: Locator, option: Locator) -> Self {
        Self::of([
            Step {
                locator: combo.clone(),
                gesture: Gesture::Click,
                done_when: Some(NodeState::Expanded),
            },
            Step {
                locator: option.within(combo),
                gesture: Gesture::Click,
                done_when: None,
            },
        ])
    }

    /// Click the slice `slice` names on the open pie menu.
    #[must_use]
    pub fn pie_slice(slice: Locator) -> Self {
        Self::of([Step {
            locator: slice.within(Locator {
                role: Some(Role::Menu),
                ..Locator::test_id(PIE_MENU_NAME)
            }),
            gesture: Gesture::Click,
            done_when: None,
        }])
    }

    /// Wait for each step under `deadline`.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Deadline) -> Self {
        self.deadline = deadline;
        self
    }

    /// Look at the UI once, for the step under way.
    ///
    /// # Errors
    ///
    /// Whatever the step's [`Pursuit::poll`] fails with.
    pub fn poll(&mut self, world: &mut World) -> Result<RouteProgress, PursuitError> {
        loop {
            let (step, mut pursuit) = match self.current.take() {
                Some(current) => current,
                None => match self.steps.pop_front() {
                    Some(step) => {
                        let pursuit = Pursuit::new(step.locator.clone(), step.gesture.intent())
                            .with_deadline(self.deadline);
                        (step, pursuit)
                    }
                    None => return Ok(RouteProgress::Done(self.last.clone())),
                },
            };
            match pursuit.poll(world)? {
                Progress::Waiting(check) => {
                    self.current = Some((step, pursuit));
                    return Ok(RouteProgress::Waiting(check));
                }
                Progress::Ready(target) => {
                    if step
                        .done_when
                        .is_some_and(|state| target.node.has_state(state))
                    {
                        continue;
                    }
                    self.last = Some(target.node.clone());
                    return Ok(RouteProgress::Act {
                        gesture: step.gesture,
                        target,
                    });
                }
            }
        }
    }
}
