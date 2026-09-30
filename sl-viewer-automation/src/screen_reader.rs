//! The **screen-reader tree, fed from the semantic model**: the viewer's
//! AccessKit tree is built from the same [`UiNode`]s the automation tier
//! reads, so one audit of roles and names serves both a test and a screen
//! reader, and a UI the automation sweep keeps named is a UI Orca can read.
//!
//! - **The model decides the tree.** Each visible semantic node becomes one
//!   AccessKit node — its role, accessible name, disabled / checked /
//!   selected / expanded / read-only states, value, level, accelerator and
//!   bounds — with the model's children as its children. Containers the model
//!   leaves out are left out here too, so a button three plain nodes deep in a
//!   floater is still that floater's child (Bevy's own tree, built from every
//!   `AccessibilityNode` and its `ChildOf`, would hang it off the window). A
//!   hidden node (a closed floater) is not in the tree at all.
//! - **Incremental.** What was sent last is kept; an update carries only the
//!   nodes that differ, and a node that disappears goes by its parent's
//!   children changing, as AccessKit asks. The model is read at most every
//!   [`ACCESSKIT_REFRESH`] (and at once when the keyboard focus moves),
//!   never every frame.
//! - **Only while a screen reader listens.** Nothing is read until AccessKit
//!   has asked for the tree (`AccessibilityRequested`); while the adapter
//!   is inactive what was sent is forgotten, so the next listener gets the
//!   whole tree. A windowless viewer has no adapter, so this does nothing
//!   there — and nothing in the automation path reads from AccessKit.
//! - **Numeric ranges come from Bevy.** `bevy_ui_widgets`' slider keeps its
//!   own `AccessibilityNode` current with the range and step; the model has
//!   the value, the node the bounds of it.
//!
//! Bevy's own per-frame tree push (`ManageAccessibilityUpdates`) is switched
//! off by [`AccessKitBridgePlugin`]: two writers to one adapter would fight.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use accesskit::{Node, NodeId as AkNodeId, Rect, Role as AkRole, Toggled, TreeId, TreeUpdate};
use bevy::a11y::{
    AccessibilityNode, AccessibilityRequested, AccessibilitySystems, ManageAccessibilityUpdates,
};
use bevy::ecs::system::NonSendMarker;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui::UiSystems;
use bevy::window::PrimaryWindow;
use bevy::winit::accessibility::ACCESS_KIT_ADAPTERS;
use sl_automation_proto::{NodeState, NodeValue, NodeVisibility, Role, UiNode};

use crate::ui_model::{UiModel, entity_of, node_id};

/// The longest the tree lags the UI while a screen reader listens: the model
/// is read again at most this often, and at once on a focus change.
pub const ACCESSKIT_REFRESH: Duration = Duration::from_millis(200);

/// Feeds AccessKit from the semantic model; see the module docs.
#[derive(Debug, Default)]
pub struct AccessKitBridgePlugin;

impl Plugin for AccessKitBridgePlugin {
    fn build(&self, app: &mut App) {
        let mut manage = ManageAccessibilityUpdates::default();
        manage.set(false);
        app.insert_resource(manage)
            .init_resource::<AccessKitTree>()
            .add_systems(
                PostUpdate,
                push_tree
                    .after(UiSystems::PostLayout)
                    .in_set(AccessibilitySystems::Update),
            );
    }
}

/// What the screen reader was last sent, to send it only what changed.
#[derive(Resource, Debug, Default)]
pub struct AccessKitTree {
    /// Every node sent, the root included, as it was sent.
    sent: HashMap<AkNodeId, Node>,
    /// The focus sent.
    focus: Option<AkNodeId>,
    /// When the model was last read.
    read_at: Option<Instant>,
}

impl AccessKitTree {
    /// The update that brings the listener from what it was sent to `nodes`
    /// with `focus`: the nodes that are new or differ, or `None` when nothing
    /// does. `nodes` becomes what was sent.
    pub fn update(
        &mut self,
        nodes: HashMap<AkNodeId, Node>,
        focus: AkNodeId,
    ) -> Option<TreeUpdate> {
        let changed: Vec<(AkNodeId, Node)> = nodes
            .iter()
            .filter(|(id, node)| self.sent.get(id) != Some(node))
            .map(|(id, node)| (*id, node.clone()))
            .collect();
        let focus_moved = self.focus != Some(focus);
        self.sent = nodes;
        self.focus = Some(focus);
        (!changed.is_empty() || focus_moved).then_some(TreeUpdate {
            nodes: changed,
            tree: None,
            tree_id: TreeId::ROOT,
            focus,
        })
    }

    /// Forget what was sent: the listener is gone, and the next one starts
    /// from an empty window.
    pub fn forget(&mut self) {
        self.sent.clear();
        self.focus = None;
    }

    /// The nodes last sent, by id.
    #[must_use]
    pub const fn sent(&self) -> &HashMap<AkNodeId, Node> {
        &self.sent
    }
}

/// The AccessKit nodes of a snapshot: `root` (the window, labelled `title`)
/// with every visible top-level node as its child, and each visible node
/// below it. Bounds are in physical pixels, `scale` physical pixels to the
/// logical pixel. `extra` is the entity's own `AccessibilityNode`, where a
/// Bevy widget keeps one, for what the model does not carry (a slider's
/// range and step).
pub fn tree_nodes(
    roots: &[UiNode],
    root: AkNodeId,
    title: &str,
    scale: f64,
    extra: &dyn Fn(Entity) -> Option<Node>,
) -> HashMap<AkNodeId, Node> {
    let mut out = HashMap::new();
    let mut window = Node::new(AkRole::Window);
    if !title.is_empty() {
        window.set_label(title);
    }
    window.set_children(add_all(roots, scale, extra, &mut out));
    out.insert(root, window);
    out
}

/// Add every visible node of `nodes` and its subtree to `out`; their ids.
fn add_all(
    nodes: &[UiNode],
    scale: f64,
    extra: &dyn Fn(Entity) -> Option<Node>,
    out: &mut HashMap<AkNodeId, Node>,
) -> Vec<AkNodeId> {
    nodes
        .iter()
        .filter(|node| node.visibility != NodeVisibility::Hidden)
        .map(|node| {
            let id = AkNodeId(node.id.0);
            let mut ak = accesskit_node(node, scale);
            if let Some(from_bevy) = entity_of(node.id).and_then(extra) {
                copy_range(&from_bevy, &mut ak);
            }
            ak.set_children(add_all(&node.children, scale, extra, out));
            out.insert(id, ak);
            id
        })
        .collect()
}

/// A slider's range and step from the node Bevy keeps for it.
fn copy_range(from: &Node, to: &mut Node) {
    if let Some(min) = from.min_numeric_value() {
        to.set_min_numeric_value(min);
    }
    if let Some(max) = from.max_numeric_value() {
        to.set_max_numeric_value(max);
    }
    if let Some(step) = from.numeric_value_step() {
        to.set_numeric_value_step(step);
    }
}

/// One semantic node as an AccessKit node, without its children.
#[must_use]
pub fn accesskit_node(node: &UiNode, scale: f64) -> Node {
    let named = node.name.as_deref().filter(|name| !name.is_empty());
    let mut ak = Node::new(accesskit_role(node.role, named.is_some()));
    match (node.role, named) {
        // Static text is read as its value, as Bevy's own `Label` is.
        (Role::Text, Some(text)) => ak.set_value(text),
        (_, Some(name)) => ak.set_label(name),
        (_, None) => {}
    }
    if node.role == Role::Trackball {
        ak.set_role_description("trackball");
    }
    match &node.value {
        Some(NodeValue::Text(text)) => ak.set_value(text.as_str()),
        Some(NodeValue::Number(number)) => ak.set_numeric_value(f64::from(*number)),
        None => {}
    }
    if node.has_state(NodeState::Disabled) {
        ak.set_disabled();
    }
    if node.has_state(NodeState::ReadOnly) {
        ak.set_read_only();
    }
    if matches!(node.role, Role::Checkbox | Role::Radio) {
        ak.set_toggled(Toggled::from(node.has_state(NodeState::Checked)));
    }
    if matches!(node.role, Role::Tab | Role::ListItem | Role::TreeItem) {
        ak.set_selected(node.has_state(NodeState::Selected));
    }
    if node.has_state(NodeState::Expanded) {
        ak.set_expanded(true);
    } else if node.role == Role::Combobox {
        ak.set_expanded(false);
    }
    if let Some(level) = node.level.and_then(|level| usize::try_from(level).ok()) {
        ak.set_level(level);
    }
    if let Some(accelerator) = node.accelerator.as_deref() {
        ak.set_keyboard_shortcut(accelerator);
    }
    let bounds = node.bounds;
    ak.set_bounds(Rect::new(
        f64::from(bounds.x) * scale,
        f64::from(bounds.y) * scale,
        f64::from(bounds.x + bounds.width) * scale,
        f64::from(bounds.y + bounds.height) * scale,
    ));
    ak
}

/// The AccessKit role of a semantic role. A group with no name is a
/// generic container, which a screen reader passes over.
#[must_use]
pub const fn accesskit_role(role: Role, named: bool) -> AkRole {
    match role {
        Role::Button => AkRole::Button,
        Role::Checkbox => AkRole::CheckBox,
        Role::Radio => AkRole::RadioButton,
        Role::RadioGroup => AkRole::RadioGroup,
        Role::Textbox => AkRole::TextInput,
        Role::Combobox => AkRole::ComboBox,
        Role::Slider => AkRole::Slider,
        Role::ColorWell => AkRole::ColorWell,
        Role::TabList => AkRole::TabList,
        Role::Tab => AkRole::Tab,
        Role::MenuBar => AkRole::MenuBar,
        Role::Menu => AkRole::Menu,
        Role::MenuItem => AkRole::MenuItem,
        Role::List => AkRole::ListBox,
        Role::ListItem => AkRole::ListBoxOption,
        Role::Tree => AkRole::Tree,
        Role::TreeItem => AkRole::TreeItem,
        // A floater is a non-modal dialog inside the viewer's window: a screen
        // reader names it as focus enters it.
        Role::Window => AkRole::Dialog,
        Role::Text => AkRole::Label,
        Role::Image => AkRole::Image,
        Role::Trackball | Role::Group if named => AkRole::Group,
        Role::Trackball | Role::Group => AkRole::GenericContainer,
    }
}

/// The node the keyboard focus is on, or the nearest ancestor of it in the
/// tree; the window when there is none.
fn focus_node(
    focus: Option<Entity>,
    nodes: &HashMap<AkNodeId, Node>,
    parents: &Query<&ChildOf>,
    root: AkNodeId,
) -> AkNodeId {
    let mut current = focus;
    while let Some(entity) = current {
        let id = AkNodeId(node_id(entity).0);
        if nodes.contains_key(&id) {
            return id;
        }
        current = parents.get(entity).ok().map(ChildOf::parent);
    }
    root
}

/// Read the model and send the listener what changed, while one listens.
#[expect(
    clippy::too_many_arguments,
    reason = "a system: each parameter is one thing the push reads"
)]
fn push_tree(
    model: UiModel,
    requested: Option<Res<AccessibilityRequested>>,
    focus: Option<Res<InputFocus>>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    parents: Query<&ChildOf>,
    bevy_nodes: Query<&AccessibilityNode>,
    mut tree: ResMut<AccessKitTree>,
    _main_thread: NonSendMarker,
) {
    if !requested.is_some_and(|requested| requested.get()) {
        tree.forget();
        return;
    }
    let focus_changed = focus.as_ref().is_some_and(|focus| focus.is_changed());
    let due = tree
        .read_at
        .is_none_or(|read_at| read_at.elapsed() >= ACCESSKIT_REFRESH);
    if !due && !focus_changed {
        return;
    }
    let Ok((window_entity, window)) = windows.single() else {
        return;
    };
    tree.read_at = Some(Instant::now());
    let root = AkNodeId(window_entity.to_bits());
    let extra = |entity: Entity| bevy_nodes.get(entity).ok().map(|node| (**node).clone());
    let nodes = tree_nodes(
        &model.snapshot(),
        root,
        &window.title,
        f64::from(window.scale_factor()),
        &extra,
    );
    let focused = focus_node(
        focus.as_ref().and_then(|focus| focus.get()),
        &nodes,
        &parents,
        root,
    );
    let Some(update) = tree.update(nodes, focused) else {
        return;
    };
    let delivered = ACCESS_KIT_ADAPTERS.with_borrow_mut(|adapters| {
        let Some(adapter) = adapters.get_mut(&window_entity) else {
            return false;
        };
        let mut delivered = false;
        adapter.update_if_active(|| {
            delivered = true;
            update
        });
        delivered
    });
    if !delivered {
        tree.forget();
    }
}

#[cfg(test)]
mod tests;
