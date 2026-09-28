//! The errors a test matches on when a request cannot be carried out.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::locator::Locator;
use crate::message::WaitCondition;
use crate::snapshot::UiNode;

/// One of the checks a node must pass before an action is applied to it, in
/// the order they are made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionabilityCheck {
    /// The node still exists.
    Attached,
    /// The node is drawn and not scrolled out of its scroll area.
    Visible,
    /// The node lies inside the viewport.
    InViewport,
    /// The node's bounds have stopped changing (it is not still animating or
    /// being laid out).
    Stable,
    /// Neither the node nor any ancestor is disabled.
    Enabled,
    /// The node accepts text: it is a text field and not read-only. Checked
    /// only by actions that type.
    Editable,
    /// A hit test at the aim point lands on the node or a descendant — so
    /// nothing covers it.
    ReceivesEvents,
}

impl ActionabilityCheck {
    /// The check's serialized spelling, used for display too.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Attached => "attached",
            Self::Visible => "visible",
            Self::InViewport => "in_viewport",
            Self::Stable => "stable",
            Self::Enabled => "enabled",
            Self::Editable => "editable",
            Self::ReceivesEvents => "receives_events",
        }
    }
}

impl fmt::Display for ActionabilityCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a request could not be carried out.
///
/// Each kind carries what a failure report needs to explain itself: the
/// locator, and the nodes it did (or did not) find.
#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationError {
    /// Nothing matches the locator.
    #[error("no UI node matches {locator}")]
    NotFound {
        /// The locator that matched nothing.
        locator: Locator,
    },
    /// An action's locator matches several nodes; an action needs exactly
    /// one.
    #[error("{locator} matches {} UI nodes, an action needs exactly one", candidates.len())]
    Ambiguous {
        /// The locator that matched too much.
        locator: Locator,
        /// Every node it matched, in reading order.
        candidates: Vec<UiNode>,
    },
    /// The one matching node failed an actionability check the action cannot
    /// wait out.
    #[error("{locator} is not actionable: it fails the {check} check")]
    NotActionable {
        /// The locator of the node.
        locator: Locator,
        /// The first check the node failed.
        check: ActionabilityCheck,
        /// The node as it was when it failed.
        node: UiNode,
    },
    /// A wait, or an action's wait for actionability, ran out of time.
    #[error(
        "timed out after {frames} frames ({millis} ms) on {locator}{}",
        timeout_detail(*condition, *failed_check, last_observed.len())
    )]
    TimedOut {
        /// The locator being waited on.
        locator: Locator,
        /// The condition a wait was waiting for; absent when an action was
        /// waiting for its node to become actionable.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<WaitCondition>,
        /// The check the node was still failing when an action gave up;
        /// absent for a wait, or when there was no single node to check.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        failed_check: Option<ActionabilityCheck>,
        /// The nodes the locator matched when time ran out.
        last_observed: Vec<UiNode>,
        /// The frames waited.
        frames: u32,
        /// The wall-clock milliseconds waited.
        millis: u64,
    },
}

/// The tail of a timeout's message: what was awaited and what was seen last.
fn timeout_detail(
    condition: Option<WaitCondition>,
    failed_check: Option<ActionabilityCheck>,
    observed: usize,
) -> String {
    let awaited = match (condition, failed_check) {
        (Some(condition), _) => format!(", waiting for it to be {condition}"),
        (None, Some(check)) => format!(", still failing the {check} check"),
        (None, None) => String::new(),
    };
    format!("{awaited} ({observed} matching nodes at the end)")
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{ActionabilityCheck, AutomationError};
    use crate::locator::Locator;
    use crate::message::WaitCondition;
    use crate::snapshot::Role;

    #[test]
    fn timeout_messages_say_what_was_awaited() {
        let locator = Locator::role(Role::Button).named("OK");
        let wait = AutomationError::TimedOut {
            locator: locator.clone(),
            condition: Some(WaitCondition::Visible),
            failed_check: None,
            last_observed: Vec::new(),
            frames: 120,
            millis: 2000,
        };
        assert_eq!(
            wait.to_string(),
            r#"timed out after 120 frames (2000 ms) on button name="OK", waiting for it to be visible (0 matching nodes at the end)"#
        );
        let action = AutomationError::TimedOut {
            locator,
            condition: None,
            failed_check: Some(ActionabilityCheck::ReceivesEvents),
            last_observed: Vec::new(),
            frames: 3,
            millis: 50,
        };
        assert_eq!(
            action.to_string(),
            r#"timed out after 3 frames (50 ms) on button name="OK", still failing the receives_events check (0 matching nodes at the end)"#
        );
    }
}
