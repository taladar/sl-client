//! Teeth for the bridge's two halves: the mapping from a snapshot to
//! AccessKit nodes (roles, names, states, structure, what is left out) and
//! the incremental update (only what changed, removals by the parent).

use std::collections::{BTreeSet, HashMap};

use accesskit::{Node, NodeId as AkNodeId, Role as AkRole, Toggled};
use bevy::prelude::Entity;
use pretty_assertions::assert_eq;
use sl_automation_proto::{Bounds, NodeId, NodeState, NodeValue, NodeVisibility, Role, UiNode};

use super::{AccessKitTree, tree_nodes};

/// The window's id in these trees.
const ROOT: AkNodeId = AkNodeId(u64::MAX);

/// A visible node.
fn node(id: u64, role: Role, name: Option<&str>, children: Vec<UiNode>) -> UiNode {
    UiNode {
        id: NodeId(id),
        role,
        name: name.map(str::to_owned),
        name_key: None,
        test_id: None,
        states: BTreeSet::new(),
        value: None,
        color: None,
        level: None,
        accelerator: None,
        bounds: Bounds {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        },
        visibility: NodeVisibility::Visible,
        children,
    }
}

/// `node` with `state` on.
fn with(mut node: UiNode, state: NodeState) -> UiNode {
    let _new = node.states.insert(state);
    node
}

/// No Bevy-kept node for any entity.
fn no_extra(_entity: Entity) -> Option<Node> {
    None
}

/// A floater with a disabled OK button, a checked box, a field and a label,
/// and a closed floater beside it.
fn floaters() -> Vec<UiNode> {
    let mut field = node(5, Role::Textbox, Some("Name"), Vec::new());
    field.value = Some(NodeValue::Text("Box".to_owned()));
    vec![
        node(
            1,
            Role::Window,
            Some("Build"),
            vec![
                with(
                    node(2, Role::Button, Some("OK"), Vec::new()),
                    NodeState::Disabled,
                ),
                with(
                    node(3, Role::Checkbox, Some("Snap"), Vec::new()),
                    NodeState::Checked,
                ),
                node(4, Role::Checkbox, Some("Local"), Vec::new()),
                node(6, Role::Group, None, vec![field]),
                node(7, Role::Text, Some("Nothing selected"), Vec::new()),
            ],
        ),
        UiNode {
            visibility: NodeVisibility::Hidden,
            ..node(
                8,
                Role::Window,
                Some("Closed"),
                vec![node(9, Role::Button, Some("Hidden"), Vec::new())],
            )
        },
    ]
}

/// The node `id` of `nodes`.
fn get(nodes: &HashMap<AkNodeId, Node>, id: u64) -> Result<&Node, String> {
    nodes
        .get(&AkNodeId(id))
        .ok_or_else(|| format!("no node {id} in the tree"))
}

#[test]
fn a_snapshot_maps_to_roles_names_states_and_structure() -> Result<(), String> {
    let nodes = tree_nodes(&floaters(), ROOT, "Viewer", 1.5, &no_extra);
    let window = get(&nodes, ROOT.0)?;
    assert_eq!(window.role(), AkRole::Window);
    assert_eq!(window.label(), Some("Viewer"));
    assert_eq!(
        window.children(),
        &[AkNodeId(1)],
        "the closed floater is not in the tree"
    );
    assert!(!nodes.contains_key(&AkNodeId(8)) && !nodes.contains_key(&AkNodeId(9)));

    let floater = get(&nodes, 1)?;
    assert_eq!(floater.role(), AkRole::Dialog);
    assert_eq!(floater.label(), Some("Build"));
    assert_eq!(
        floater.children(),
        &[
            AkNodeId(2),
            AkNodeId(3),
            AkNodeId(4),
            AkNodeId(6),
            AkNodeId(7)
        ]
    );

    let ok = get(&nodes, 2)?;
    assert_eq!(
        (ok.role(), ok.label(), ok.is_disabled()),
        (AkRole::Button, Some("OK"), true)
    );
    assert_eq!(get(&nodes, 3)?.toggled(), Some(Toggled::True));
    assert_eq!(get(&nodes, 4)?.toggled(), Some(Toggled::False));
    assert!(!get(&nodes, 4)?.is_disabled());

    let group = get(&nodes, 6)?;
    assert_eq!(group.role(), AkRole::GenericContainer, "an unnamed group");
    let field = get(&nodes, 5)?;
    assert_eq!(
        (field.role(), field.label(), field.value()),
        (AkRole::TextInput, Some("Name"), Some("Box"))
    );
    let text = get(&nodes, 7)?;
    assert_eq!(
        (text.role(), text.value()),
        (AkRole::Label, Some("Nothing selected")),
        "static text reads as its value"
    );
    let bounds = ok.bounds().ok_or("no bounds")?;
    assert_eq!(
        (bounds.x0, bounds.y0, bounds.x1, bounds.y1),
        (15.0, 30.0, 60.0, 90.0),
        "physical pixels"
    );
    Ok(())
}

#[test]
fn a_colour_well_tells_its_colour_and_text_its_ink() -> Result<(), String> {
    let mut well = node(1, Role::ColorWell, Some("Chat"), Vec::new());
    well.value = Some(NodeValue::Color("#ff8000".to_owned()));
    let mut text = node(2, Role::Text, Some("Hello"), Vec::new());
    text.color = Some("#00ff0080".to_owned());
    let mut garbled = node(3, Role::Text, Some("Odd"), Vec::new());
    garbled.color = Some("green".to_owned());
    let nodes = tree_nodes(&[well, text, garbled], ROOT, "", 1.0, &|_entity| None);
    let colour = |red, green, blue, alpha| accesskit::Color {
        red,
        green,
        blue,
        alpha,
    };
    assert_eq!(
        get(&nodes, 1)?.color_value(),
        Some(colour(0xff, 0x80, 0x00, 0xff))
    );
    assert_eq!(
        get(&nodes, 2)?.foreground_color(),
        Some(colour(0x00, 0xff, 0x00, 0x80))
    );
    assert_eq!(get(&nodes, 3)?.foreground_color(), None, "not a colour");
    Ok(())
}

#[test]
fn a_slider_takes_its_range_from_the_node_bevy_keeps() -> Result<(), String> {
    let mut slider = node(1, Role::Slider, Some("Glow"), Vec::new());
    slider.value = Some(NodeValue::Number(0.25));
    let extra = |_entity: Entity| {
        let mut kept = Node::new(AkRole::Slider);
        kept.set_min_numeric_value(0.0);
        kept.set_max_numeric_value(1.0);
        kept.set_numeric_value_step(0.05);
        Some(kept)
    };
    let nodes = tree_nodes(&[slider], ROOT, "", 1.0, &extra);
    let slider = get(&nodes, 1)?;
    assert_eq!(
        (
            slider.numeric_value(),
            slider.min_numeric_value(),
            slider.max_numeric_value(),
            slider.numeric_value_step()
        ),
        (Some(0.25), Some(0.0), Some(1.0), Some(0.05))
    );
    Ok(())
}

/// **Only what changed is sent**: the first update is everything, a repeat
/// is nothing, a changed state is that node alone, and a node that goes is
/// its parent's new children — never the node itself.
#[test]
fn updates_are_incremental() -> Result<(), String> {
    let mut tree = AccessKitTree::default();
    let first = tree
        .update(
            tree_nodes(&floaters(), ROOT, "Viewer", 1.0, &no_extra),
            ROOT,
        )
        .ok_or("the first update is empty")?;
    assert_eq!(first.nodes.len(), 8, "the window and seven visible nodes");

    assert!(
        tree.update(
            tree_nodes(&floaters(), ROOT, "Viewer", 1.0, &no_extra),
            ROOT
        )
        .is_none(),
        "nothing changed, nothing sent"
    );

    let mut enabled = floaters();
    if let Some(ok) = enabled
        .first_mut()
        .and_then(|floater| floater.children.first_mut())
    {
        let _was = ok.states.remove(&NodeState::Disabled);
    }
    let update = tree
        .update(tree_nodes(&enabled, ROOT, "Viewer", 1.0, &no_extra), ROOT)
        .ok_or("an enabled button sent nothing")?;
    assert_eq!(
        update.nodes.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![AkNodeId(2)]
    );

    let mut fewer = enabled;
    if let Some(floater) = fewer.first_mut() {
        floater.children.truncate(1);
    }
    let update = tree
        .update(
            tree_nodes(&fewer, ROOT, "Viewer", 1.0, &no_extra),
            AkNodeId(2),
        )
        .ok_or("a removal sent nothing")?;
    assert_eq!(
        update.nodes.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![AkNodeId(1)],
        "the parent, with its child gone"
    );
    assert_eq!(update.focus, AkNodeId(2));

    tree.forget();
    let again = tree
        .update(
            tree_nodes(&fewer, ROOT, "Viewer", 1.0, &no_extra),
            AkNodeId(2),
        )
        .ok_or("a new listener was sent nothing")?;
    assert_eq!(again.nodes.len(), 3, "a new listener gets the whole tree");
    Ok(())
}
