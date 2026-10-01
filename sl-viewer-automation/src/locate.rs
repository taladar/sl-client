//! Resolving a [`Locator`] against a snapshot: which nodes it names, and —
//! for an action — the one node it names.

use sl_automation_proto::{AutomationError, Locator, UiNode};

/// Every node of `roots` that `locator` matches, in reading order (the
/// snapshot's depth-first order, roots back to front).
///
/// [`Locator::within`] is resolved first and **strictly**: the scope must be
/// exactly one node, and only its descendants (not the scope itself) are
/// searched. [`Locator::nth`] then picks one match out of the rest, so the
/// result has at most one node when it is set.
///
/// # Errors
///
/// When the scope matches nothing ([`AutomationError::NotFound`], naming the
/// scope) or more than one node ([`AutomationError::Ambiguous`], likewise). A
/// target that matches nothing is not an error here: the result is empty.
pub fn find_all<'a>(
    roots: &'a [UiNode],
    locator: &Locator,
) -> Result<Vec<&'a UiNode>, Box<AutomationError>> {
    let space = match &locator.within {
        Some(scope) => find_one(roots, scope)?.children.as_slice(),
        None => roots,
    };
    let mut matches = Vec::new();
    collect_matches(space, locator, &mut matches);
    Ok(match locator.nth {
        Some(index) => usize::try_from(index)
            .ok()
            .and_then(|index| matches.get(index).copied())
            .into_iter()
            .collect(),
        None => matches,
    })
}

/// The one node of `roots` that `locator` matches — the resolution every
/// action makes.
///
/// # Errors
///
/// [`AutomationError::NotFound`] when nothing matches, and
/// [`AutomationError::Ambiguous`] listing every match when more than one
/// does: an action is never applied to "the first". A scope that does not
/// resolve fails the same way, naming the scope.
pub fn find_one<'a>(
    roots: &'a [UiNode],
    locator: &Locator,
) -> Result<&'a UiNode, Box<AutomationError>> {
    let matches = find_all(roots, locator)?;
    match matches.as_slice() {
        [] => Err(Box::new(AutomationError::NotFound {
            locator: locator.clone(),
        })),
        [only] => Ok(only),
        several => Err(Box::new(AutomationError::Ambiguous {
            locator: locator.clone(),
            candidates: several.iter().map(|node| shallow(node)).collect(),
        })),
    }
}

/// `node` without its children — how a node is reported on its own (a
/// candidate, a found node, the node an action was applied to).
#[must_use]
pub fn shallow(node: &UiNode) -> UiNode {
    UiNode {
        children: Vec::new(),
        ..node.clone()
    }
}

/// Append every node of `nodes` and their descendants that `locator` matches
/// on its own criteria, depth first.
fn collect_matches<'a>(nodes: &'a [UiNode], locator: &Locator, out: &mut Vec<&'a UiNode>) {
    for node in nodes {
        if locator.matches_node(node) {
            out.push(node);
        }
        collect_matches(&node.children, locator, out);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use pretty_assertions::assert_eq;
    use sl_automation_proto::{
        AutomationError, Bounds, Locator, NodeId, NodeVisibility, Role, UiNode,
    };

    use super::{find_all, find_one};

    /// A visible node with a role, a name and a test id.
    fn node(id: u64, role: Role, name: &str, children: Vec<UiNode>) -> UiNode {
        UiNode {
            id: NodeId(id),
            role,
            name: Some(name.to_owned()),
            name_key: None,
            test_id: Some(format!("node-{id}")),
            states: BTreeSet::new(),
            value: None,
            color: None,
            level: None,
            accelerator: None,
            bounds: Bounds::default(),
            visibility: NodeVisibility::Visible,
            children,
        }
    }

    /// Two windows, each with an OK and a Cancel button; the second also has
    /// an OK inside a group.
    fn two_windows() -> Vec<UiNode> {
        vec![
            node(
                1,
                Role::Window,
                "Preferences",
                vec![
                    node(2, Role::Button, "OK", Vec::new()),
                    node(3, Role::Button, "Cancel", Vec::new()),
                ],
            ),
            node(
                4,
                Role::Window,
                "Profile",
                vec![
                    node(5, Role::Button, "OK", Vec::new()),
                    node(
                        6,
                        Role::Group,
                        "Footer",
                        vec![node(7, Role::Button, "OK", Vec::new())],
                    ),
                ],
            ),
        ]
    }

    /// The ids of `nodes`.
    fn ids(nodes: &[&UiNode]) -> Vec<u64> {
        nodes.iter().map(|node| node.id.0).collect()
    }

    #[test]
    fn matches_come_in_reading_order() -> Result<(), Box<AutomationError>> {
        let roots = two_windows();
        let ok = Locator::role(Role::Button).named("OK");
        assert_eq!(ids(&find_all(&roots, &ok)?), vec![2, 5, 7]);
        Ok(())
    }

    #[test]
    fn within_searches_only_below_its_one_scope() -> Result<(), Box<AutomationError>> {
        let roots = two_windows();
        let ok = Locator::role(Role::Button).named("OK");
        let in_profile = ok
            .clone()
            .within(Locator::role(Role::Window).named("Profile"));
        assert_eq!(ids(&find_all(&roots, &in_profile)?), vec![5, 7]);
        let in_footer = ok.within(
            Locator::role(Role::Group).within(Locator::role(Role::Window).named("Profile")),
        );
        assert_eq!(find_one(&roots, &in_footer)?.id, NodeId(7), "scopes nest");
        let scope_itself =
            Locator::role(Role::Window).within(Locator::role(Role::Window).named("Profile"));
        assert!(
            find_all(&roots, &scope_itself)?.is_empty(),
            "the scope is not inside itself"
        );
        Ok(())
    }

    #[test]
    fn nth_picks_one_of_the_matches() -> Result<(), Box<AutomationError>> {
        let roots = two_windows();
        let ok = Locator::role(Role::Button).named("OK");
        assert_eq!(find_one(&roots, &ok.clone().nth(1))?.id, NodeId(5));
        assert!(
            find_all(&roots, &ok.nth(3))?.is_empty(),
            "an index past the matches matches nothing"
        );
        Ok(())
    }

    #[test]
    fn an_ambiguous_locator_lists_every_candidate_without_children() -> Result<(), String> {
        let roots = two_windows();
        let window = Locator::role(Role::Window);
        let Err(error) = find_one(&roots, &window) else {
            return Err("two windows are ambiguous".to_owned());
        };
        let AutomationError::Ambiguous {
            locator,
            candidates,
        } = *error
        else {
            return Err(format!("an ambiguity, not {error}"));
        };
        assert_eq!(locator, window, "the error names the locator");
        assert_eq!(
            candidates
                .iter()
                .map(|node| (node.id.0, node.children.len()))
                .collect::<Vec<_>>(),
            vec![(1, 0), (4, 0)],
            "both windows, each on its own"
        );
        Ok(())
    }

    #[test]
    fn a_scope_must_resolve_to_exactly_one_node() {
        let roots = two_windows();
        let any_window = Locator::role(Role::Window);
        let ok = Locator::role(Role::Button).within(any_window.clone());
        assert!(
            matches!(
                find_all(&roots, &ok).map_err(|error| *error),
                Err(AutomationError::Ambiguous { ref locator, .. }) if *locator == any_window
            ),
            "an ambiguous scope is the scope's error"
        );
        let nowhere = Locator::test_id("nowhere");
        assert!(
            matches!(
                find_all(&roots, &Locator::role(Role::Button).within(nowhere.clone()))
                    .map_err(|error| *error),
                Err(AutomationError::NotFound { ref locator }) if *locator == nowhere
            ),
            "a missing scope is the scope's error"
        );
    }

    #[test]
    fn nothing_found_is_empty_for_a_query_and_an_error_for_an_action() {
        let roots = two_windows();
        let missing = Locator::role(Role::Checkbox);
        assert!(
            find_all(&roots, &missing).is_ok_and(|found| found.is_empty()),
            "a query finds nothing"
        );
        assert!(
            matches!(
                find_one(&roots, &missing).map_err(|error| *error),
                Err(AutomationError::NotFound { .. })
            ),
            "an action has nothing to act on"
        );
    }
}
