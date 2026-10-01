//! Locators: the semantic query that names which UI node a test means.

use serde::{Deserialize, Serialize};

use crate::snapshot::{NodeState, Role, UiNode};

/// How a locator's name is compared with a node's resolved accessible name.
///
/// Both comparisons are case-sensitive. The resolved name depends on the
/// viewer's locale; prefer [`Locator::name_key`] where the node has one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameMatcher {
    /// The whole name, exactly.
    Exact(String),
    /// Any part of the name.
    Contains(String),
}

impl NameMatcher {
    /// Whether `name` satisfies the matcher.
    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        match self {
            Self::Exact(wanted) => name == wanted,
            Self::Contains(part) => name.contains(part.as_str()),
        }
    }
}

/// A semantic query for UI nodes: a role plus an accessible name, a Fluent
/// key or a test id, scoped with [`within`](Self::within), narrowed by state
/// filters and picked with [`nth`](Self::nth).
///
/// Every criterion that is set must hold; an unset one matches anything, so
/// [`Locator::default`] matches every node. An action on a locator that
/// matches more than one node is refused as ambiguous rather than applied to
/// the first — pin it down, or pick one with `nth`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Locator {
    /// The node's role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    /// A comparison against the node's resolved accessible name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<NameMatcher>,
    /// The Fluent key the node's name was translated from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_key: Option<String>,
    /// The viewer's own identifier for the node (in sl-client, the entity's
    /// `Name`, the address space of the UI contract registry).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_id: Option<String>,
    /// Only nodes inside the one node this locator resolves to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within: Option<Box<Self>>,
    /// Of the nodes that match everything else, only the one at this
    /// zero-based index in reading order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nth: Option<u32>,
    /// Enabled (`true`) or disabled (`false`), counting a disabled ancestor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Checked or not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    /// Selected or not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
    /// Expanded or not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    /// Focused or not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused: Option<bool>,
}

impl Locator {
    /// Nodes of `role`.
    #[must_use]
    pub fn role(role: Role) -> Self {
        Self {
            role: Some(role),
            ..Self::default()
        }
    }

    /// The node whose test id is `test_id`.
    #[must_use]
    pub fn test_id(test_id: impl Into<String>) -> Self {
        Self {
            test_id: Some(test_id.into()),
            ..Self::default()
        }
    }

    /// Also require the accessible name to be exactly `name`.
    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(NameMatcher::Exact(name.into()));
        self
    }

    /// Also require the accessible name to contain `part`.
    #[must_use]
    pub fn name_containing(mut self, part: impl Into<String>) -> Self {
        self.name = Some(NameMatcher::Contains(part.into()));
        self
    }

    /// Also require the name to have been translated from the Fluent key
    /// `key`.
    #[must_use]
    pub fn name_key(mut self, key: impl Into<String>) -> Self {
        self.name_key = Some(key.into());
        self
    }

    /// Only look inside the node `scope` resolves to.
    #[must_use]
    pub fn within(mut self, scope: Self) -> Self {
        self.within = Some(Box::new(scope));
        self
    }

    /// Pick the match at zero-based `index` in reading order.
    #[must_use]
    pub const fn nth(mut self, index: u32) -> Self {
        self.nth = Some(index);
        self
    }

    /// Also require the node to be enabled (`true`) or disabled (`false`).
    #[must_use]
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    /// Also require the node to be checked (`true`) or not (`false`).
    #[must_use]
    pub const fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// Also require the node to be selected (`true`) or not (`false`).
    #[must_use]
    pub const fn selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    /// Also require the node to be expanded (`true`) or not (`false`).
    #[must_use]
    pub const fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }

    /// Also require the node to hold the focus (`true`) or not (`false`).
    #[must_use]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = Some(focused);
        self
    }

    /// Whether `node` itself satisfies the criteria that concern a single
    /// node: role, name, name key, test id and the state filters.
    ///
    /// [`within`](Self::within) and [`nth`](Self::nth) are not consulted —
    /// they depend on the node's place in the tree and among the other
    /// matches, which only the resolver that walks the tree knows.
    #[must_use]
    pub fn matches_node(&self, node: &UiNode) -> bool {
        self.role.is_none_or(|role| role == node.role)
            && self.name.as_ref().is_none_or(|matcher| {
                node.name
                    .as_deref()
                    .is_some_and(|name| matcher.matches(name))
            })
            && self
                .name_key
                .as_ref()
                .is_none_or(|key| node.name_key.as_ref() == Some(key))
            && self
                .test_id
                .as_ref()
                .is_none_or(|id| node.test_id.as_ref() == Some(id))
            && self
                .enabled
                .is_none_or(|enabled| enabled != node.has_state(NodeState::Disabled))
            && state_filter_holds(self.checked, node, NodeState::Checked)
            && state_filter_holds(self.selected, node, NodeState::Selected)
            && state_filter_holds(self.expanded, node, NodeState::Expanded)
            && state_filter_holds(self.focused, node, NodeState::Focused)
    }
}

/// Whether a node satisfies one tri-state filter over `state`: an unset
/// filter always holds, a set one must agree with the node.
fn state_filter_holds(filter: Option<bool>, node: &UiNode, state: NodeState) -> bool {
    filter.is_none_or(|wanted| wanted == node.has_state(state))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use pretty_assertions::assert_eq;

    use super::{Locator, NameMatcher};
    use crate::snapshot::{Bounds, NodeId, NodeState, NodeVisibility, Role, UiNode};

    /// A visible OK button with the given states.
    fn ok_button(states: &[NodeState]) -> UiNode {
        UiNode {
            id: NodeId(7),
            role: Role::Button,
            name: Some("OK".to_owned()),
            name_key: Some("button-ok".to_owned()),
            test_id: Some("prefs.ok".to_owned()),
            states: states.iter().copied().collect::<BTreeSet<_>>(),
            value: None,
            color: None,
            level: None,
            accelerator: None,
            bounds: Bounds::default(),
            visibility: NodeVisibility::Visible,
            children: Vec::new(),
        }
    }

    #[test]
    fn default_locator_matches_any_node() {
        assert!(
            Locator::default().matches_node(&ok_button(&[])),
            "an empty locator constrains nothing"
        );
    }

    #[test]
    fn each_identity_criterion_can_reject() {
        let node = ok_button(&[]);
        let cases = [
            (Locator::role(Role::Button), true),
            (Locator::role(Role::Checkbox), false),
            (Locator::default().named("OK"), true),
            (Locator::default().named("OK!"), false),
            (Locator::default().name_containing("O"), true),
            (Locator::default().name_containing("o"), false),
            (Locator::default().name_key("button-ok"), true),
            (Locator::default().name_key("button-cancel"), false),
            (Locator::test_id("prefs.ok"), true),
            (Locator::test_id("prefs.cancel"), false),
        ];
        for (locator, expected) in cases {
            assert_eq!(locator.matches_node(&node), expected, "{locator}");
        }
    }

    #[test]
    fn a_name_criterion_rejects_an_unnamed_node() {
        let mut node = ok_button(&[]);
        node.name = None;
        assert!(
            !Locator::default().name_containing("").matches_node(&node),
            "even an empty substring needs a name to be in"
        );
    }

    #[test]
    fn state_filters_follow_the_node_states() {
        let plain = ok_button(&[]);
        let all = ok_button(&[
            NodeState::Disabled,
            NodeState::Checked,
            NodeState::Selected,
            NodeState::Expanded,
            NodeState::Focused,
        ]);
        let filters = [
            Locator::default().enabled(false),
            Locator::default().checked(true),
            Locator::default().selected(true),
            Locator::default().expanded(true),
            Locator::default().focused(true),
        ];
        for filter in filters {
            assert!(filter.matches_node(&all), "{filter} on the stateful node");
            assert!(!filter.matches_node(&plain), "{filter} on the plain node");
        }
        assert!(
            Locator::default().enabled(true).matches_node(&plain),
            "a node without the disabled state is enabled"
        );
        assert!(
            !Locator::default().enabled(true).matches_node(&all),
            "a disabled node is not enabled"
        );
    }

    #[test]
    fn scope_and_index_are_left_to_the_resolver() {
        let scoped = Locator::role(Role::Button)
            .within(Locator::test_id("somewhere.else"))
            .nth(3);
        assert!(
            scoped.matches_node(&ok_button(&[])),
            "within and nth need the tree, so a single node ignores them"
        );
    }

    #[test]
    fn name_matchers_are_case_sensitive() {
        assert!(NameMatcher::Exact("OK".to_owned()).matches("OK"), "exact");
        assert!(!NameMatcher::Exact("OK".to_owned()).matches("ok"), "case");
        assert!(
            NameMatcher::Contains("Apply".to_owned()).matches("Apply all"),
            "substring"
        );
    }
}
