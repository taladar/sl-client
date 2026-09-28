//! The errors a test matches on when a request cannot be carried out.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::locator::Locator;
use crate::message::WaitCondition;
use crate::snapshot::UiNode;
use crate::world::{WorldLocator, WorldNode};

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
    #[error(
        "{locator} matches {} UI nodes, an action needs exactly one: {}",
        candidates.len(),
        list_nodes(candidates)
    )]
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
    /// A world locator an action needs one thing for matches several.
    #[error(
        "{locator} matches {} things in the world, an action needs exactly one: {}",
        candidates.len(),
        list_nodes(candidates)
    )]
    WorldAmbiguous {
        /// The locator that matched too much.
        locator: WorldLocator,
        /// Every thing it matched, in the resolver's order.
        candidates: Vec<WorldNode>,
    },
    /// A wait on a world locator ran out of time: it matched nothing (when
    /// one match was wanted), or things it might match were still waiting
    /// for the names or owners the viewer asked the simulator for.
    #[error(
        "timed out after {frames} frames ({millis} ms) on {locator}{}",
        world_timeout_detail(unresolved, last_observed.len())
    )]
    WorldTimedOut {
        /// The locator being waited on.
        locator: WorldLocator,
        /// The things whose name or owner never arrived, so whether they
        /// match could not be told.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        unresolved: Vec<WorldNode>,
        /// The things the locator matched when time ran out.
        last_observed: Vec<WorldNode>,
        /// The frames waited.
        frames: u32,
        /// The wall-clock milliseconds waited.
        millis: u64,
    },
}

/// The nodes of an error message, one after the other.
fn list_nodes<T: fmt::Display>(nodes: &[T]) -> String {
    nodes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
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

/// The tail of a world timeout's message: what was still unresolved and how
/// much matched at the end.
fn world_timeout_detail(unresolved: &[WorldNode], observed: usize) -> String {
    let pending = if unresolved.is_empty() {
        String::new()
    } else {
        format!(
            ", still waiting for the name or owner of {}",
            list_nodes(unresolved)
        )
    };
    format!("{pending} ({observed} matching things at the end)")
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use std::collections::BTreeSet;

    use super::{ActionabilityCheck, AutomationError};
    use crate::locator::Locator;
    use crate::message::WaitCondition;
    use crate::snapshot::{Bounds, NodeId, NodeVisibility, Role, UiNode};
    use crate::world::{WorldKind, WorldLocator, WorldNode};

    /// A named OK button at `x`, with a test id.
    fn ok_at(x: f32, test_id: &str) -> UiNode {
        UiNode {
            id: NodeId(1),
            role: Role::Button,
            name: Some("OK".to_owned()),
            name_key: Some("button-ok".to_owned()),
            test_id: Some(test_id.to_owned()),
            states: BTreeSet::new(),
            value: None,
            level: None,
            accelerator: None,
            bounds: Bounds {
                x,
                y: 20.0,
                width: 120.0,
                height: 30.0,
            },
            visibility: NodeVisibility::Visible,
            children: Vec::new(),
        }
    }

    #[test]
    fn an_ambiguity_names_every_candidate() {
        let error = AutomationError::Ambiguous {
            locator: Locator::role(Role::Button).named("OK"),
            candidates: vec![ok_at(10.0, "prefs.ok"), ok_at(200.5, "profile.ok")],
        };
        assert_eq!(
            error.to_string(),
            r#"button name="OK" matches 2 UI nodes, an action needs exactly one: button "OK" key=button-ok #prefs.ok at 10,20 120x30; button "OK" key=button-ok #profile.ok at 200.5,20 120x30"#
        );
    }

    #[test]
    fn a_world_timeout_names_what_never_resolved() {
        let pending = WorldNode {
            kind: WorldKind::Object,
            own: false,
            full_id: uuid::Uuid::from_u128(2),
            local_id: Some(2),
            pcode: 9,
            name: None,
            description: None,
            owner: None,
            position: Some([1.0, 2.0, 3.0]),
            rotation: None,
            scale: None,
            parent: None,
            children: Vec::new(),
            attachment_point: None,
            worn_by: None,
            sitting_on: None,
            selected: false,
            hover_text: None,
            name_tag: None,
        };
        let error = AutomationError::WorldTimedOut {
            locator: WorldLocator::kind(WorldKind::Object).named("Door"),
            unresolved: vec![pending],
            last_observed: Vec::new(),
            frames: 60,
            millis: 1000,
        };
        assert_eq!(
            error.to_string(),
            r#"timed out after 60 frames (1000 ms) on object name="Door", still waiting for the name or owner of object #00000000-0000-0000-0000-000000000002 local=2 at <1,2,3> (0 matching things at the end)"#
        );
    }

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
