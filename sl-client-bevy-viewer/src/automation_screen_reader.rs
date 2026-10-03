//! The **screen-reader tree against the semantic model**, on every registered
//! floater: what AccessKit would be sent (`sl_viewer_automation::tree_nodes`)
//! is the model, node for node — the same roles, names, states and children,
//! and nothing the model calls hidden. The mapping's own teeth live in
//! `sl-viewer-automation`; this is the claim that the viewer's real floaters
//! come through it whole.

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;

    use accesskit::{Node, NodeId as AkNodeId, Toggled};
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_automation_proto::{NodeState, NodeVisibility, Role, UiNode};
    use sl_viewer_automation::{accesskit_role, snapshot, tree_nodes};

    use crate::floater_chrome::floater_app;
    use crate::floaters::FLOATERS;
    use crate::ui_contract::install_element_hosting;
    use crate::ui_element::ElementCx;
    use crate::ui_elements::ELEMENTS;
    use crate::ui_test::interact::InteractionTest;
    use crate::ui_test::{settle, spawn_element_into};

    /// The window's id in these trees; no entity has these bits.
    const ROOT: AkNodeId = AkNodeId(u64::MAX);

    /// Why `node` and its subtree do not match what was emitted for them, if
    /// they do not; each visible model node is checked, each hidden one must
    /// be absent.
    fn mismatch(node: &UiNode, emitted: &HashMap<AkNodeId, Node>) -> Option<String> {
        let id = AkNodeId(node.id.0);
        let what = format!("{:?} {:?} ({:?})", node.role, node.name, node.test_id);
        if node.visibility == NodeVisibility::Hidden {
            return emitted
                .contains_key(&id)
                .then(|| format!("{what} is hidden but in the tree"));
        }
        let Some(ak) = emitted.get(&id) else {
            return Some(format!("{what} is missing from the tree"));
        };
        let name = node.name.as_deref().filter(|name| !name.is_empty());
        if ak.role() != accesskit_role(node.role, name.is_some()) {
            return Some(format!("{what} was emitted as {:?}", ak.role()));
        }
        let spoken = if node.role == Role::Text {
            ak.value()
        } else {
            ak.label()
        };
        if spoken != name {
            return Some(format!("{what} is read as {spoken:?}"));
        }
        if ak.is_disabled() != node.has_state(NodeState::Disabled) {
            return Some(format!("{what}: disabled is {}", ak.is_disabled()));
        }
        if matches!(node.role, Role::Checkbox | Role::Radio)
            && ak.toggled() != Some(Toggled::from(node.has_state(NodeState::Checked)))
        {
            return Some(format!("{what}: toggled is {:?}", ak.toggled()));
        }
        let children: Vec<AkNodeId> = node
            .children
            .iter()
            .filter(|child| child.visibility != NodeVisibility::Hidden)
            .map(|child| AkNodeId(child.id.0))
            .collect();
        if ak.children() != children.as_slice() {
            return Some(format!("{what}: children differ from the model's"));
        }
        node.children
            .iter()
            .find_map(|child| mismatch(child, emitted))
    }

    /// **Every registered floater reaches a screen reader as the model reads
    /// it** — and at least one reaches it as a named dialog with a disabled
    /// control in it, so the sweep is not vacuous.
    #[test]
    fn every_floater_is_emitted_as_the_model_reads_it() -> Result<(), String> {
        let mut dialogs = 0_usize;
        let mut disabled = 0_usize;
        for floater in FLOATERS {
            let mut app = floater_app(InteractionTest::new(), floater);
            let roots = snapshot(app.world_mut()).map_err(|error| error.to_string())?;
            let emitted = tree_nodes(&roots, ROOT, "viewer", 1.0, &|_entity: Entity| None);
            let window = emitted.get(&ROOT).ok_or("no window node")?;
            let top: Vec<AkNodeId> = roots
                .iter()
                .filter(|root| root.visibility != NodeVisibility::Hidden)
                .map(|root| AkNodeId(root.id.0))
                .collect();
            assert_eq!(window.children(), top.as_slice(), "`{}`", floater.id);
            if let Some(problem) = roots.iter().find_map(|root| mismatch(root, &emitted)) {
                return Err(format!("`{}`: {problem}", floater.id));
            }
            dialogs = dialogs.saturating_add(
                emitted
                    .values()
                    .filter(|node| node.role() == accesskit::Role::Dialog && node.label().is_some())
                    .count(),
            );
            disabled =
                disabled.saturating_add(emitted.values().filter(|node| node.is_disabled()).count());
        }
        assert!(dialogs > 0, "no floater came through as a named dialog");
        assert!(
            disabled > 0,
            "no floater came through with a disabled control"
        );
        Ok(())
    }

    /// Whether a screen reader could say what a node is called: a name with a
    /// letter or a digit in it, not a glyph (`✕`, `⚙`, `+`) or nothing.
    fn speakable(name: Option<&str>) -> bool {
        name.is_some_and(|name| name.chars().any(char::is_alphanumeric))
    }

    /// Every control of `nodes` and their subtrees a screen reader could not
    /// name, as `role name (test id, in the nearest ancestor's test id)`.
    /// Static text and images are content — an emoji in a picker is what it
    /// shows — so only the roles a user operates are held to it.
    ///
    /// Hidden ones too: a control on a tab nobody opened, in a closed popup or
    /// a collapsed section is spawned and waiting, and is named when it shows
    /// or not at all. What escapes this sweep is what has no content yet — a
    /// pooled list row no item is bound to — and what does not exist yet (a
    /// pane for a conversation nobody started), which is why a glyph button's
    /// name is part of its label's type (`UiLabel::glyph`).
    fn unnamed_controls(nodes: &[UiNode], within: &str, out: &mut Vec<String>) {
        for node in nodes {
            // A virtual list's spare pooled row: hidden, and empty until the
            // list binds an item to it — named by that item when it shows.
            let unbound_row = matches!(node.role, Role::ListItem | Role::TreeItem)
                && node.visibility == NodeVisibility::Hidden
                && !speakable(node.name.as_deref());
            if unbound_row {
                continue;
            }
            let control = matches!(
                node.role,
                Role::Button
                    | Role::Checkbox
                    | Role::Radio
                    | Role::Textbox
                    | Role::Combobox
                    | Role::Slider
                    | Role::SpinButton
                    | Role::ColorWell
                    | Role::Trackball
                    | Role::Tab
                    | Role::MenuItem
                    | Role::TreeItem
                    | Role::ListItem
            );
            if control && !speakable(node.name.as_deref()) {
                out.push(format!(
                    "{:?} {:?} ({:?}, in {within})",
                    node.role, node.name, node.test_id
                ));
            }
            let here = node.test_id.as_deref().unwrap_or(within);
            unnamed_controls(&node.children, here, out);
        }
    }

    /// **Every control a user operates has a name a screen reader can say**:
    /// a close box is "Close", not "✕"; the inventory's options menu is not
    /// "⚙". A glyph is how a control looks, and the skin's glyphs are not in
    /// the model at all, so an icon-only control needs a name of its own.
    #[test]
    fn every_control_has_a_speakable_name() -> Result<(), String> {
        let mut found = Vec::new();
        for floater in FLOATERS {
            let mut app = floater_app(InteractionTest::new(), floater);
            let roots = snapshot(app.world_mut()).map_err(|error| error.to_string())?;
            let mut here = Vec::new();
            unnamed_controls(&roots, "the root", &mut here);
            found.extend(
                here.into_iter()
                    .map(|what| format!("floater `{}`: {what}", floater.id)),
            );
        }
        for element in ELEMENTS {
            let mut app = InteractionTest::new().build();
            install_element_hosting(&mut app);
            spawn_element_into(&mut app, element, ElementCx::new());
            settle(&mut app);
            let roots = snapshot(app.world_mut()).map_err(|error| error.to_string())?;
            let mut here = Vec::new();
            unnamed_controls(&roots, "the root", &mut here);
            found.extend(
                here.into_iter()
                    .map(|what| format!("element `{}`: {what}", element.id)),
            );
        }
        found.sort();
        found.dedup();
        assert!(
            found.is_empty(),
            "controls with no speakable name:\n{}",
            found.join("\n")
        );
        Ok(())
    }

    /// Every **shown** field, spin button and slider of `nodes` and their
    /// subtrees.
    ///
    /// Shown ones only: a panel that swaps its rows by mode (the Texture tab's
    /// diffuse / normal / specular rows) may reuse a caption, and two rows
    /// that never show together are never confused. A tab's fields are
    /// compared by opening the tab (`build_floater_test`).
    pub(crate) fn value_controls<'a>(nodes: &'a [UiNode], out: &mut Vec<&'a UiNode>) {
        for node in nodes {
            if node.visibility == NodeVisibility::Hidden {
                continue;
            }
            if matches!(node.role, Role::Textbox | Role::Slider | Role::SpinButton)
                && node.name.is_some()
            {
                out.push(node);
            }
            value_controls(&node.children, out);
        }
    }

    /// Each name that more than one of `fields` carries, with the test ids.
    pub(crate) fn shared_names(fields: &[&UiNode]) -> Vec<String> {
        let mut seen: HashMap<&str, &UiNode> = HashMap::new();
        let mut shared = Vec::new();
        for field in fields {
            let Some(name) = field.name.as_deref() else {
                continue;
            };
            if let Some(first) = seen.insert(name, field) {
                shared.push(format!(
                    "{name:?} names both {:?} and {:?}",
                    first.test_id, field.test_id
                ));
            }
        }
        shared
    }

    /// **No two fields of one floater share a name**: two fields under one
    /// caption ("Offset (U/V)") are "Offset U" and "Offset V", not "Offset"
    /// twice — a screen reader user could not tell which one they are in.
    #[test]
    fn no_two_fields_of_a_floater_share_a_name() -> Result<(), String> {
        let mut found = Vec::new();
        for floater in FLOATERS {
            let mut app = floater_app(InteractionTest::new(), floater);
            let roots = snapshot(app.world_mut()).map_err(|error| error.to_string())?;
            let mut fields = Vec::new();
            value_controls(&roots, &mut fields);
            found.extend(
                shared_names(&fields)
                    .into_iter()
                    .map(|shared| format!("floater `{}`: {shared}", floater.id)),
            );
        }
        found.sort();
        assert!(
            found.is_empty(),
            "fields sharing a name:\n{}",
            found.join("\n")
        );
        Ok(())
    }
}
