//! [`UiModel`]: the semantic snapshot of the UI, read from the ECS on request.

use std::collections::{BTreeSet, HashSet};

use bevy::ecs::query::QueryData;
use bevy::ecs::system::{SystemParam, SystemParamValidationError, SystemState};
use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::{
    Checked, ComputedStackIndex, InteractionDisabled, Selected, UiStack, clip_check_recursive,
};
use bevy::ui_widgets::{Button, Checkbox, RadioButton, RadioGroup, Slider, SliderValue};
use bevy_flair::prelude::ClassList;
use sl_automation_proto::{Bounds, NodeId, NodeState, NodeValue, NodeVisibility, Role, UiNode};
use sl_viewer_ui_core::i18n::{Translated, Translator};
use sl_viewer_ui_core::semantic::{
    Expanded, LabelledBy, Semantic, SemanticName, role_from_classes, selected_by_class,
};
use sl_viewer_ui_widgets::ui_text_input::ReadOnlyField;

/// The handle a snapshot reports for `entity`: its bits, stable for the
/// entity's lifetime and meaningless to any other viewer.
#[must_use]
pub const fn node_id(entity: Entity) -> NodeId {
    NodeId(entity.to_bits())
}

/// The entity a snapshot's [`NodeId`] names, or `None` for bits no entity
/// could have.
#[must_use]
pub const fn entity_of(id: NodeId) -> Option<Entity> {
    Entity::try_from_bits(id.0)
}

/// Take a snapshot of the whole UI of `world`: every root's semantic tree, in
/// stacking order (back to front).
///
/// For a caller holding the world exclusively — a test, or the executor's
/// exclusive system. A system that already runs with parameters takes a
/// [`UiModel`] instead.
///
/// # Errors
///
/// When the model's parameters fail validation. Every resource it reads is
/// optional, so that would be Bevy refusing to run it at all, and it is passed
/// on rather than read as an empty UI.
pub fn snapshot(world: &mut World) -> Result<Vec<UiNode>, SystemParamValidationError> {
    let mut state = SystemState::<UiModel<'_, '_>>::new(world);
    let model = state.get(world)?;
    Ok(model.snapshot())
}

/// Everything about one UI node the model reads.
#[derive(QueryData)]
struct NodeFacts {
    /// The node itself.
    entity: Entity,
    /// Its laid-out box, in physical pixels.
    computed: &'static ComputedNode,
    /// Where the box sits, in physical pixels.
    transform: &'static UiGlobalTransform,
    /// Its style, for `Display::None`.
    node: &'static Node,
    /// Whether it and every ancestor are visible.
    inherited_visibility: Option<&'static InheritedVisibility>,
    /// Its test id.
    name: Option<&'static Name>,
    /// An explicit accessible name.
    label: Option<&'static AccessibleLabel>,
    /// The Fluent key its text is resolved from.
    translated: Option<&'static Translated>,
    /// Its text, when it is a text node.
    text: Option<&'static Text>,
    /// Its editable text, when it is a field.
    editable: Option<&'static EditableText>,
    /// Its value, when it is a slider.
    slider_value: Option<&'static SliderValue>,
    /// The clip rectangle its scroll ancestors impose, in physical pixels.
    clip: Option<&'static CalculatedClip>,
    /// The render target it lays out into.
    target: Option<&'static ComputedUiRenderTargetInfo>,
    /// Its place in the stacking order.
    stack_index: Option<&'static ComputedStackIndex>,
    /// What a custom widget says it is.
    semantic: Option<&'static Semantic>,
    /// Its skin classes, which mark list rows and the selected one.
    classes: Option<&'static ClassList>,
    /// The marker components the role and states are read from.
    markers: NodeMarkers,
}

/// The marker components a node's role and states are read from.
#[derive(QueryData)]
struct NodeMarkers {
    /// `bevy_ui_widgets`' headless button.
    button: Has<Button>,
    /// `bevy_ui`'s own button, pressed through `Interaction`.
    interaction_button: Has<bevy::ui::widget::Button>,
    /// A set of radio options.
    radio_group: Has<RadioGroup>,
    /// A check box.
    checkbox: Has<Checkbox>,
    /// A radio option.
    radio: Has<RadioButton>,
    /// A slider.
    slider: Has<Slider>,
    /// An image.
    image: Has<ImageNode>,
    /// Disabled on the node itself.
    disabled: Has<InteractionDisabled>,
    /// Checked.
    checked: Has<Checked>,
    /// Selected.
    selected: Has<Selected>,
    /// Open, or unfolded.
    expanded: Has<Expanded>,
    /// A read-only text field.
    read_only: Has<ReadOnlyField>,
}

/// What a node inherits from its ancestors.
#[derive(Debug, Clone, Copy, Default)]
struct Inherited {
    /// An ancestor is disabled.
    disabled: bool,
    /// An ancestor is laid out away (`Display::None`).
    display_none: bool,
}

impl Inherited {
    /// What a child of the node `facts` describes inherits.
    fn through(self, facts: &NodeFactsItem<'_, '_>) -> Self {
        Self {
            disabled: self.disabled || facts.markers.disabled,
            display_none: self.display_none || facts.node.display == Display::None,
        }
    }
}

/// The per-snapshot facts shared by every node: who is hovered, who has
/// focus.
struct Frame {
    /// Every hovered entity and every ancestor of one — a node counts as
    /// hovered when the pointer is over it or over a descendant.
    hovered: HashSet<Entity>,
    /// The entity holding the keyboard focus.
    focused: Option<Entity>,
}

/// The semantic model of the UI: a system parameter whose snapshot methods
/// read the whole tree, or one subtree, as [`UiNode`]s.
///
/// Nothing is computed until a method is called, so holding one in a system
/// that rarely asks costs nothing.
#[derive(SystemParam)]
#[expect(
    missing_debug_implementations,
    reason = "a bundle of queries; its Debug would print query state, not the UI"
)]
pub struct UiModel<'w, 's> {
    /// Every UI node.
    nodes: Query<'w, 's, NodeFacts>,
    /// Every entity's children, for the walk down.
    children: Query<'w, 's, &'static Children>,
    /// Every entity's parent, for the walk up.
    parents: Query<'w, 's, &'static ChildOf>,
    /// Text spans under a text node, for its full content.
    spans: Query<'w, 's, &'static TextSpan>,
    /// The clip walk's view of an ancestor, exactly as `bevy_ui`'s picking
    /// backend reads it.
    clipping: Query<
        'w,
        's,
        (
            &'static ComputedNode,
            &'static UiGlobalTransform,
            &'static Node,
        ),
    >,
    /// The clip walk's parent links, stopping at an [`OverrideClip`].
    clip_parents: Query<'w, 's, &'static ChildOf, Without<OverrideClip>>,
    /// Pickability, for whether a hit blocks the nodes below it.
    pickables: Query<'w, 's, &'static Pickable>,
    /// Form rows whose label names the controls in them.
    labellings: Query<'w, 's, &'static LabelledBy>,
    /// The UI stack, back to front, for the hit test.
    stack: Option<Res<'w, UiStack>>,
    /// The keyboard focus.
    focus: Option<Res<'w, InputFocus>>,
    /// What each pointer is over.
    hover: Option<Res<'w, HoverMap>>,
    /// The string lookup, for a name given as a Fluent key. Absent where no
    /// locale is loaded; the key then names the node as it is.
    translator: Option<Translator<'w>>,
}

impl UiModel<'_, '_> {
    /// The whole UI: every root's semantic tree, roots in stacking order
    /// (back to front).
    #[must_use]
    pub fn snapshot(&self) -> Vec<UiNode> {
        let frame = self.frame();
        let mut roots: Vec<(u32, Entity)> = self
            .nodes
            .iter()
            .filter(|facts| {
                !self
                    .parents
                    .get(facts.entity)
                    .is_ok_and(|parent| self.nodes.contains(parent.parent()))
            })
            .map(|facts| (facts.stack_index.map_or(0, |index| index.0), facts.entity))
            .collect();
        roots.sort_unstable();
        let mut out = Vec::new();
        for (_index, root) in roots {
            self.collect(root, Inherited::default(), &frame, &mut out);
        }
        out
    }

    /// The semantic nodes of the subtree under `root`: `root`'s own node when
    /// it has a role, else the nodes of its descendants. Empty when `root` is
    /// not a UI node.
    #[must_use]
    pub fn snapshot_under(&self, root: Entity) -> Vec<UiNode> {
        let frame = self.frame();
        let mut out = Vec::new();
        self.collect(root, self.inherited_at(root), &frame, &mut out);
        out
    }

    /// The facts every node of one snapshot shares.
    fn frame(&self) -> Frame {
        let mut hovered = HashSet::new();
        if let Some(hover) = &self.hover {
            for hits in hover.values() {
                for &entity in hits.keys() {
                    hovered.insert(entity);
                    hovered.extend(self.parents.iter_ancestors(entity));
                }
            }
        }
        Frame {
            hovered,
            focused: self.focus.as_ref().and_then(|focus| focus.get()),
        }
    }

    /// What `entity` inherits from its ancestors, found by walking up — for a
    /// snapshot that starts part-way down the tree.
    fn inherited_at(&self, entity: Entity) -> Inherited {
        let mut inherited = Inherited::default();
        for ancestor in self.parents.iter_ancestors(entity) {
            if let Ok(facts) = self.nodes.get(ancestor) {
                inherited.disabled |= facts.markers.disabled;
                inherited.display_none |= facts.node.display == Display::None;
            }
        }
        inherited
    }

    /// Append `entity`'s semantic node to `out` — or, when it has no role,
    /// the semantic nodes of its children, so a layout-only container never
    /// appears in the tree.
    fn collect(&self, entity: Entity, inherited: Inherited, frame: &Frame, out: &mut Vec<UiNode>) {
        let Ok(facts) = self.nodes.get(entity) else {
            return;
        };
        let here = inherited.through(&facts);
        let Some(role) = self.role_of(&facts) else {
            self.collect_children(entity, here, frame, out);
            return;
        };
        let mut node = self.describe(&facts, role, here, frame);
        if is_leaf(role) {
            self.collect_owned(entity, here, frame, &mut node.children);
        } else {
            self.collect_children(entity, here, frame, &mut node.children);
        }
        out.push(node);
    }

    /// The popups a leaf widget owns: a leaf's subtree is its label, but a
    /// combo's list and a menu button's menu are spawned under it too, and
    /// they are nodes of their own (ARIA's `aria-owns`). Any descendant whose
    /// [`Semantic`] is a container role is collected as the leaf's child; the
    /// rest of the subtree stays the label.
    fn collect_owned(
        &self,
        entity: Entity,
        inherited: Inherited,
        frame: &Frame,
        out: &mut Vec<UiNode>,
    ) {
        let Ok(children) = self.children.get(entity) else {
            return;
        };
        for &child in children {
            let Ok(facts) = self.nodes.get(child) else {
                continue;
            };
            if facts
                .semantic
                .is_some_and(|semantic| !is_leaf(semantic.role()))
            {
                self.collect(child, inherited, frame, out);
            } else {
                self.collect_owned(child, inherited.through(&facts), frame, out);
            }
        }
    }

    /// [`Self::collect`] over each child of `entity`, in order.
    fn collect_children(
        &self,
        entity: Entity,
        inherited: Inherited,
        frame: &Frame,
        out: &mut Vec<UiNode>,
    ) {
        if let Ok(children) = self.children.get(entity) {
            for &child in children {
                self.collect(child, inherited, frame, out);
            }
        }
    }

    /// The role a node plays, or `None` for a layout-only container.
    ///
    /// A [`Semantic`] wins over everything: it is the widget saying what it
    /// is. After it, widget components win over what the node happens to draw. A text node
    /// with nothing in it, and an image nobody named, are decoration rather
    /// than content; a container is a group only when it has a test id or an
    /// accessible label to scope a locator by.
    fn role_of(&self, facts: &NodeFactsItem<'_, '_>) -> Option<Role> {
        let markers = &facts.markers;
        if let Some(semantic) = facts.semantic {
            Some(semantic.role())
        } else if facts.editable.is_some() {
            Some(Role::Textbox)
        } else if markers.slider {
            Some(Role::Slider)
        } else if markers.checkbox {
            Some(Role::Checkbox)
        } else if markers.radio {
            Some(Role::Radio)
        } else if markers.button || markers.interaction_button {
            Some(Role::Button)
        } else if markers.radio_group {
            Some(Role::RadioGroup)
        } else if let Some(role) = facts.classes.and_then(role_from_classes) {
            Some(role)
        } else if facts.text.is_some() {
            (!self.text_content(facts.entity).trim().is_empty()).then_some(Role::Text)
        } else if markers.image && (facts.name.is_some() || facts.label.is_some()) {
            Some(Role::Image)
        } else if facts.name.is_some() || facts.label.is_some() {
            Some(Role::Group)
        } else {
            None
        }
    }

    /// Build the node for `facts`, without its children.
    fn describe(
        &self,
        facts: &NodeFactsItem<'_, '_>,
        role: Role,
        inherited: Inherited,
        frame: &Frame,
    ) -> UiNode {
        let (name, name_key) = self.name_of(facts, role);
        // The front tab of a strip is the strip's checked radio option: to its
        // user it is selected, not ticked.
        let checked_is_selected = role == Role::Tab;
        let selected = facts.markers.selected
            || facts.classes.is_some_and(selected_by_class)
            || (checked_is_selected && facts.markers.checked);
        let mut states = BTreeSet::new();
        for (state, holds) in [
            (NodeState::Disabled, inherited.disabled),
            (NodeState::ReadOnly, facts.markers.read_only),
            (
                NodeState::Checked,
                facts.markers.checked && !checked_is_selected,
            ),
            (NodeState::Selected, selected),
            (NodeState::Expanded, facts.markers.expanded),
            (NodeState::Focused, frame.focused == Some(facts.entity)),
            (NodeState::Hovered, frame.hovered.contains(&facts.entity)),
        ] {
            if holds {
                states.insert(state);
            }
        }
        let semantic = facts.semantic;
        let value = if let Some(editable) = facts.editable {
            Some(NodeValue::Text(editable.editor.text().to_string()))
        } else if let Some(shown) = semantic.and_then(Semantic::value_node) {
            Some(NodeValue::Text(self.label_text(shown).0))
        } else {
            facts.slider_value.map(|value| NodeValue::Number(value.0))
        };
        UiNode {
            id: node_id(facts.entity),
            role,
            name,
            name_key,
            test_id: facts.name.map(|name| name.as_str().to_owned()),
            states,
            value,
            level: semantic.and_then(Semantic::tree_level),
            accelerator: semantic.and_then(|semantic| semantic.shortcut().map(str::to_owned)),
            bounds: logical_bounds(facts.computed, facts.transform),
            visibility: self.visibility_of(facts, inherited),
            children: Vec::new(),
        }
    }

    /// The accessible name and the Fluent key it came from.
    ///
    /// An [`AccessibleLabel`] overrides everything (and has no key). A text
    /// node names itself. A widget is named by the first translated label in
    /// its subtree, else by all the text in it; a group or an image only by a
    /// label, since the text inside a panel is its content, not its name.
    fn name_of(
        &self,
        facts: &NodeFactsItem<'_, '_>,
        role: Role,
    ) -> (Option<String>, Option<String>) {
        if let Some(label) = facts.label {
            return (non_empty(label.0.clone()), None);
        }
        match facts.semantic.map(Semantic::name) {
            Some(SemanticName::LabelledBy(label)) => {
                let (text, key) = self.label_text(*label);
                return (non_empty(text), key);
            }
            Some(SemanticName::Key(key)) => {
                let text = self
                    .translator
                    .as_ref()
                    .map_or_else(|| key.to_string(), |translator| translator.get(key));
                return (non_empty(text), Some(key.to_string()));
            }
            Some(SemanticName::Content) | None => {}
        }
        match role {
            Role::Text => (
                non_empty(self.text_content(facts.entity)),
                facts.translated.map(|key| key.key().to_owned()),
            ),
            Role::Group
            | Role::Image
            | Role::Window
            | Role::TabList
            | Role::MenuBar
            | Role::Menu
            | Role::List
            | Role::Tree => (None, None),
            Role::Button
            | Role::Checkbox
            | Role::Radio
            | Role::Textbox
            | Role::Combobox
            | Role::Slider
            | Role::ColorWell
            | Role::Trackball
            | Role::Tab
            | Role::MenuItem
            | Role::ListItem
            | Role::TreeItem => {
                let (text, key) = self.label_text(facts.entity);
                match non_empty(text) {
                    Some(text) => (Some(text), key),
                    None => self.labelled_by_of(facts.entity),
                }
            }
            // A radio group is named by the row it sits in, when it has one.
            Role::RadioGroup => self.labelled_by_of(facts.entity),
        }
    }

    /// The name a [`LabelledBy`] gives `entity` — its own, else its nearest
    /// ancestor's: that label's text and key. A field, a slider or a swatch
    /// that draws no text of its own is called what its row says.
    fn labelled_by_of(&self, entity: Entity) -> (Option<String>, Option<String>) {
        let Some(row) = core::iter::once(entity)
            .chain(self.parents.iter_ancestors(entity))
            .find_map(|node| self.labellings.get(node).ok())
        else {
            return (None, None);
        };
        let (text, key) = self.label_text(row.0);
        match non_empty(text) {
            Some(text) => (Some(text), key),
            None => (None, None),
        }
    }

    /// The text `entity` shows as a label: its first translated text, with
    /// the key, else all its text joined.
    fn label_text(&self, entity: Entity) -> (String, Option<String>) {
        if let Some((text, key)) = self.translated_label(entity) {
            (text, Some(key))
        } else {
            let mut texts = Vec::new();
            self.descendant_texts(entity, &mut texts);
            (texts.join(" "), None)
        }
    }

    /// The first `Translated` text node in `entity`'s subtree, itself first,
    /// depth first: its resolved text and its key.
    fn translated_label(&self, entity: Entity) -> Option<(String, String)> {
        if let Ok(facts) = self.nodes.get(entity)
            && let (Some(key), Some(_text)) = (facts.translated, facts.text)
        {
            return Some((self.text_content(entity), key.key().to_owned()));
        }
        self.children
            .get(entity)
            .ok()?
            .iter()
            .filter(|&child| !self.is_owned_popup(child))
            .find_map(|child| self.translated_label(child))
    }

    /// Whether `entity` is a popup its leaf ancestor owns — a node of its own
    /// (see [`Self::collect_owned`]), so none of its text names that leaf.
    fn is_owned_popup(&self, entity: Entity) -> bool {
        self.nodes.get(entity).is_ok_and(|facts| {
            facts
                .semantic
                .is_some_and(|semantic| !is_leaf(semantic.role()))
        })
    }

    /// Every non-empty text in `entity`'s subtree, itself first, depth first.
    fn descendant_texts(&self, entity: Entity, out: &mut Vec<String>) {
        if self
            .nodes
            .get(entity)
            .is_ok_and(|facts| facts.text.is_some())
        {
            let text = self.text_content(entity);
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                out.push(trimmed.to_owned());
            }
        }
        if let Ok(children) = self.children.get(entity) {
            for &child in children {
                if !self.is_owned_popup(child) {
                    self.descendant_texts(child, out);
                }
            }
        }
    }

    /// A text node's whole content: its own text and its spans'.
    fn text_content(&self, entity: Entity) -> String {
        let mut content = self
            .nodes
            .get(entity)
            .ok()
            .and_then(|facts| facts.text.map(|text| text.0.clone()))
            .unwrap_or_default();
        if let Ok(children) = self.children.get(entity) {
            for span in self.spans.iter_many(children) {
                content.push_str(&span.0);
            }
        }
        content
    }

    /// The first reason the node cannot be seen, or
    /// [`NodeVisibility::Visible`].
    fn visibility_of(&self, facts: &NodeFactsItem<'_, '_>, inherited: Inherited) -> NodeVisibility {
        match visible_part(facts, inherited) {
            Err(reason) => reason,
            Ok(visible) if self.hit_reaches(visible.center(), facts.entity) => {
                NodeVisibility::Visible
            }
            Ok(_covered) => NodeVisibility::Covered,
        }
    }

    /// Where a pointer aimed at `entity` goes, in logical pixels: the centre of
    /// the part of it a user can see — not of its box, which a scroll area or
    /// the viewport edge may cut. The same point the model's covered test is
    /// made at, so a node the model calls visible is hit there.
    ///
    /// `None` when `entity` is not a UI node, or no part of it can be seen.
    #[must_use]
    pub fn aim_point(&self, entity: Entity) -> Option<Vec2> {
        let facts = self.nodes.get(entity).ok()?;
        let inherited = self.inherited_at(entity).through(&facts);
        let centre = visible_part(&facts, inherited).ok()?.center();
        let scale = facts.computed.inverse_scale_factor;
        Some(Vec2::new(centre.x * scale, centre.y * scale))
    }

    /// Whether a pointer at `point` (physical pixels) would reach `target` —
    /// `bevy_ui`'s picking rules, front to back: a node that contains the
    /// point, is drawn and is not clipped away there is hit; the walk stops at
    /// the first hit that blocks the nodes below it (any node without a
    /// [`Pickable`] saying otherwise). Reaching `target` or a descendant of it
    /// counts, since a widget's label sits on top of the widget.
    ///
    /// Without a UI stack there is no hit test to make, and nothing is called
    /// covered.
    fn hit_reaches(&self, point: Vec2, target: Entity) -> bool {
        let Some(stack) = &self.stack else {
            return true;
        };
        for &entity in stack.uinodes.iter().rev() {
            let Ok(facts) = self.nodes.get(entity) else {
                continue;
            };
            let drawn = facts
                .inherited_visibility
                .is_some_and(|visibility| visibility.get());
            if !drawn
                || facts.computed.size == Vec2::ZERO
                || !facts.computed.contains_point(*facts.transform, point)
                || !clip_check_recursive(point, entity, &self.clipping, &self.clip_parents)
            {
                continue;
            }
            if entity == target || self.parents.iter_ancestors(entity).any(|up| up == target) {
                return true;
            }
            let blocks = self
                .pickables
                .get(entity)
                .map_or(true, |pickable| pickable.should_block_lower);
            if blocks {
                return false;
            }
        }
        false
    }

    /// Whether a click at `point` (physical pixels) would go to the UI rather
    /// than the world: the world's own rule (`pointer_over_blocking_ui`) — a
    /// UI node with an area under the pointer that blocks what is below it —
    /// made with the same front-to-back walk as the covered test.
    ///
    /// Without a UI stack there is no UI to take it.
    #[must_use]
    pub fn takes_click_at(&self, point: Vec2) -> bool {
        let Some(stack) = &self.stack else {
            return false;
        };
        for &entity in stack.uinodes.iter().rev() {
            let Ok(facts) = self.nodes.get(entity) else {
                continue;
            };
            let drawn = facts
                .inherited_visibility
                .is_some_and(|visibility| visibility.get());
            let size = facts.computed.size;
            if !drawn
                || size.x <= 0.0
                || size.y <= 0.0
                || !facts.computed.contains_point(*facts.transform, point)
                || !clip_check_recursive(point, entity, &self.clipping, &self.clip_parents)
            {
                continue;
            }
            let pickable = self.pickables.get(entity).ok();
            if pickable.is_none_or(|pickable| pickable.should_block_lower) {
                // Hovered, it suppresses the world pick; not hoverable, it still
                // hides everything under it from the hover map.
                return pickable.is_none_or(|pickable| pickable.is_hoverable);
            }
        }
        false
    }
}

/// The part of the node a user can see, in physical pixels: its box cut
/// down by its scroll ancestors' clip and by the viewport — or the first
/// reason there is none (hidden, clipped or off screen). Whether something
/// covers it is not asked here.
fn visible_part(
    facts: &NodeFactsItem<'_, '_>,
    inherited: Inherited,
) -> Result<Rect, NodeVisibility> {
    let shown = facts
        .inherited_visibility
        .is_some_and(|visibility| visibility.get());
    let size = facts.computed.size;
    if !shown || inherited.display_none || size.x <= 0.0 || size.y <= 0.0 {
        return Err(NodeVisibility::Hidden);
    }
    let mut visible = physical_rect(facts.computed, facts.transform);
    if let Some(clip) = facts.clip {
        visible = visible.intersect(clip.clip);
        if visible.is_empty() {
            return Err(NodeVisibility::Clipped);
        }
    }
    if let Some(target) = facts.target
        && target.physical_size() != UVec2::ZERO
    {
        visible = visible.intersect(Rect::from_corners(
            Vec2::ZERO,
            target.physical_size().as_vec2(),
        ));
        if visible.is_empty() {
            return Err(NodeVisibility::OffScreen);
        }
    }
    Ok(visible)
}

/// Whether a role's subtree belongs to it — its label and decoration — rather
/// than holding nodes of its own.
///
/// A row is not a leaf: a list row may hold a button, a tree row a check box,
/// and each is its own node. A row is still *named* by all its text.
const fn is_leaf(role: Role) -> bool {
    !matches!(
        role,
        Role::Group
            | Role::RadioGroup
            | Role::Window
            | Role::TabList
            | Role::MenuBar
            | Role::Menu
            | Role::List
            | Role::ListItem
            | Role::Tree
            | Role::TreeItem
    )
}

/// `text`, unless it is blank.
fn non_empty(text: String) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// A node's box in physical pixels.
fn physical_rect(computed: &ComputedNode, transform: &UiGlobalTransform) -> Rect {
    let centre = transform.translation;
    let (half_x, half_y) = (computed.size.x / 2.0, computed.size.y / 2.0);
    Rect {
        min: Vec2::new(centre.x - half_x, centre.y - half_y),
        max: Vec2::new(centre.x + half_x, centre.y + half_y),
    }
}

/// A node's box in logical pixels.
fn logical_bounds(computed: &ComputedNode, transform: &UiGlobalTransform) -> Bounds {
    let rect = physical_rect(computed, transform);
    let scale = computed.inverse_scale_factor;
    Bounds {
        x: rect.min.x * scale,
        y: rect.min.y * scale,
        width: computed.size.x * scale,
        height: computed.size.y * scale,
    }
}

#[cfg(test)]
mod tests;
