//! Teeth for the semantic model: each state and each visibility reason flips
//! when the widget's does, and stays put otherwise.
//!
//! Every fixture is spawned under the scaffold's UI root in an
//! [`InteractionTest`] app — the picking stack a real click travels through —
//! so "covered" and "hovered" are answered by the same hit test and hover map
//! the viewer runs.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::ui_widgets::{Button, Checkbox, RadioButton, Slider, SliderRange, SliderValue};
use pretty_assertions::assert_eq;
use sl_automation_proto::{NodeState, NodeValue, NodeVisibility, Role, UiNode};
use sl_viewer_testkit::interact::{self, InteractionTest};
use sl_viewer_testkit::{settle, spawn_under_root};
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_widgets::ui_text_input::ReadOnlyField;

use super::{entity_of, node_id, snapshot};

/// A built, settled interaction app with nothing in it yet.
fn app() -> App {
    let mut app = InteractionTest::new().build();
    settle(&mut app);
    app
}

/// The model of the whole UI, after the frames a spawn or a change needs to
/// lay out.
fn snap(app: &mut App) -> Result<Vec<UiNode>, String> {
    settle(app);
    snapshot(app.world_mut()).map_err(|error| error.to_string())
}

/// The node whose test id is `test_id`, anywhere in `nodes`.
fn find<'a>(nodes: &'a [UiNode], test_id: &str) -> Option<&'a UiNode> {
    nodes.iter().find_map(|node| {
        if node.test_id.as_deref() == Some(test_id) {
            Some(node)
        } else {
            find(&node.children, test_id)
        }
    })
}

/// The node named `test_id` in a fresh snapshot, or an error saying it is
/// missing.
fn node(app: &mut App, test_id: &str) -> Result<UiNode, String> {
    let nodes = snap(app)?;
    find(&nodes, test_id)
        .cloned()
        .ok_or_else(|| format!("no node `{test_id}` in {nodes:#?}"))
}

/// An absolutely placed box, `width` × `height` logical px at (`left`, `top`).
fn placed(left: f32, top: f32, width: f32, height: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(left),
        top: Val::Px(top),
        width: Val::Px(width),
        height: Val::Px(height),
        ..default()
    }
}

/// Spawn a named `bevy_ui_widgets` button labelled `label` at a fixed place,
/// returning the button.
fn spawn_button(app: &mut App, name: &str, label: &str) -> Entity {
    let button = spawn_under_root(
        app,
        (
            Name::new(name.to_owned()),
            Button,
            placed(100.0, 100.0, 120.0, 30.0),
        ),
    );
    let text = app.world_mut().spawn(Text::new(label)).id();
    app.world_mut().entity_mut(button).add_child(text);
    button
}

#[test]
fn roles_come_from_widget_components() -> Result<(), String> {
    let mut app = app();
    let panel = spawn_under_root(
        &mut app,
        (
            Name::new("panel"),
            Node {
                flex_direction: FlexDirection::Column,
                ..default()
            },
        ),
    );
    let world = app.world_mut();
    let button = world
        .spawn((Name::new("button"), Button, Node::default()))
        .with_child(Text::new("OK"))
        .id();
    let checkbox = world
        .spawn((Name::new("checkbox"), Checkbox, Node::default()))
        .with_child(Text::new("Always run"))
        .id();
    let radio = world
        .spawn((Name::new("radio"), RadioButton, Node::default()))
        .with_child(Text::new("Metres"))
        .id();
    let slider = world
        .spawn((
            Name::new("slider"),
            Slider::default(),
            SliderValue(0.25),
            SliderRange::new(0.0, 1.0),
            Node {
                width: Val::Px(100.0),
                height: Val::Px(10.0),
                ..default()
            },
        ))
        .id();
    let field = world
        .spawn((Name::new("field"), EditableText::new("hello")))
        .id();
    let text = world.spawn((Name::new("text"), Text::new("Prose"))).id();
    // A layout-only wrapper: no name, no widget. Its text belongs to the
    // panel's tree directly.
    let wrapper = world
        .spawn(Node::default())
        .with_child((Name::new("wrapped"), Text::new("Inside")))
        .id();
    world
        .entity_mut(panel)
        .add_children(&[button, checkbox, radio, slider, field, text, wrapper]);

    let nodes = snap(&mut app)?;
    let panel = find(&nodes, "panel").ok_or("no panel")?;
    assert_eq!(panel.role, Role::Group, "a named container is a group");
    let summary: Vec<(Option<&str>, Role, Option<&str>)> = panel
        .children
        .iter()
        .map(|child| (child.test_id.as_deref(), child.role, child.name.as_deref()))
        .collect();
    assert_eq!(
        summary,
        vec![
            (Some("button"), Role::Button, Some("OK")),
            (Some("checkbox"), Role::Checkbox, Some("Always run")),
            (Some("radio"), Role::Radio, Some("Metres")),
            (Some("slider"), Role::Slider, None),
            (Some("field"), Role::Textbox, None),
            (Some("text"), Role::Text, Some("Prose")),
            (Some("wrapped"), Role::Text, Some("Inside")),
        ],
        "the widgets' labels are their names, not nodes of their own, and the \
         unnamed wrapper is transparent"
    );
    let slider = find(&nodes, "slider").ok_or("no slider")?;
    assert_eq!(slider.value, Some(NodeValue::Number(0.25)), "slider value");
    let field = find(&nodes, "field").ok_or("no field")?;
    assert_eq!(
        field.value,
        Some(NodeValue::Text("hello".to_owned())),
        "field value"
    );
    Ok(())
}

#[test]
fn a_label_overrides_and_a_translated_label_keeps_its_key() -> Result<(), String> {
    let mut app = app();
    let close = spawn_button(&mut app, "close", "×");
    app.world_mut()
        .entity_mut(close)
        .insert(AccessibleLabel::new("Close"));
    let ok = spawn_under_root(&mut app, (Name::new("ok"), Button, Node::default()));
    let glyph = app.world_mut().spawn(Text::new("✓")).id();
    let label = app
        .world_mut()
        .spawn((Text::new("OK"), Translated::new("button-ok")))
        .id();
    app.world_mut().entity_mut(ok).add_children(&[glyph, label]);

    let close = node(&mut app, "close")?;
    assert_eq!(
        (close.name.as_deref(), close.name_key.as_deref()),
        (Some("Close"), None),
        "an AccessibleLabel wins over the glyph drawn inside"
    );
    let ok = node(&mut app, "ok")?;
    assert_eq!(
        (ok.name.as_deref(), ok.name_key.as_deref()),
        (Some("OK"), Some("button-ok")),
        "a translated label wins over other text, and keeps its key"
    );
    Ok(())
}

#[test]
fn disabled_is_inherited_from_an_ancestor() -> Result<(), String> {
    let mut app = app();
    let panel = spawn_under_root(&mut app, (Name::new("panel"), Node::default()));
    let inside = spawn_button(&mut app, "inside", "Inside");
    app.world_mut().entity_mut(panel).add_child(inside);
    spawn_button(&mut app, "outside", "Outside");

    let disabled = |app: &mut App, name: &str| -> Result<bool, String> {
        Ok(node(app, name)?.has_state(NodeState::Disabled))
    };
    assert!(!disabled(&mut app, "inside")?, "enabled before");
    app.world_mut()
        .entity_mut(panel)
        .insert(InteractionDisabled);
    assert!(
        disabled(&mut app, "inside")?,
        "a disabled panel disables its button"
    );
    assert!(!disabled(&mut app, "outside")?, "but not one outside it");
    assert!(
        disabled(&mut app, "panel")?,
        "and the panel itself reports it"
    );
    app.world_mut()
        .entity_mut(panel)
        .remove::<InteractionDisabled>();
    assert!(!disabled(&mut app, "inside")?, "enabled again after");
    Ok(())
}

#[test]
fn read_only_checked_and_focused_follow_their_components() -> Result<(), String> {
    let mut app = app();
    let field = spawn_under_root(
        &mut app,
        (
            Name::new("field"),
            EditableText::new("fixed"),
            ReadOnlyField,
        ),
    );
    spawn_under_root(&mut app, (Name::new("editable"), EditableText::new("free")));
    let checkbox = spawn_under_root(&mut app, (Name::new("checkbox"), Checkbox, Node::default()));

    assert!(
        node(&mut app, "field")?.has_state(NodeState::ReadOnly),
        "read-only"
    );
    assert!(
        !node(&mut app, "editable")?.has_state(NodeState::ReadOnly),
        "an ordinary field is not"
    );

    assert!(
        !node(&mut app, "checkbox")?.has_state(NodeState::Checked),
        "unchecked"
    );
    app.world_mut().entity_mut(checkbox).insert(Checked);
    assert!(
        node(&mut app, "checkbox")?.has_state(NodeState::Checked),
        "checked"
    );
    app.world_mut().entity_mut(checkbox).remove::<Checked>();
    assert!(
        !node(&mut app, "checkbox")?.has_state(NodeState::Checked),
        "cleared"
    );

    assert!(
        !node(&mut app, "field")?.has_state(NodeState::Focused),
        "unfocused"
    );
    *app.world_mut().resource_mut::<InputFocus>() = InputFocus::from_entity(field);
    assert!(
        node(&mut app, "field")?.has_state(NodeState::Focused),
        "focused"
    );
    assert!(
        !node(&mut app, "editable")?.has_state(NodeState::Focused),
        "only the focused field"
    );
    Ok(())
}

#[test]
fn hovered_follows_the_real_pointer() -> Result<(), String> {
    let mut app = app();
    let button = spawn_button(&mut app, "button", "Hover me");
    settle(&mut app);
    let centre = interact::centre_of_entity(&app, button).ok_or("button never laid out")?;

    assert!(
        !node(&mut app, "button")?.has_state(NodeState::Hovered),
        "not yet"
    );
    interact::hover(&mut app, centre);
    assert!(
        node(&mut app, "button")?.has_state(NodeState::Hovered),
        "the pointer over its label counts as over the button"
    );
    interact::hover(&mut app, Vec2::new(900.0, 900.0));
    assert!(
        !node(&mut app, "button")?.has_state(NodeState::Hovered),
        "moved off"
    );
    Ok(())
}

#[test]
fn a_display_none_ancestor_hides() -> Result<(), String> {
    let mut app = app();
    let panel = spawn_under_root(&mut app, (Name::new("panel"), Node::default()));
    let button = spawn_button(&mut app, "button", "Button");
    app.world_mut().entity_mut(panel).add_child(button);

    assert_eq!(
        node(&mut app, "button")?.visibility,
        NodeVisibility::Visible,
        "shown"
    );
    set_display(&mut app, panel, Display::None)?;
    assert_eq!(
        node(&mut app, "button")?.visibility,
        NodeVisibility::Hidden,
        "hidden"
    );
    set_display(&mut app, panel, Display::Flex)?;
    assert_eq!(
        node(&mut app, "button")?.visibility,
        NodeVisibility::Visible,
        "back"
    );
    Ok(())
}

/// Set `entity`'s `Node::display`.
fn set_display(app: &mut App, entity: Entity, display: Display) -> Result<(), String> {
    app.world_mut()
        .get_mut::<Node>(entity)
        .ok_or("no Node")?
        .display = display;
    Ok(())
}

#[test]
fn a_row_scrolled_out_of_its_list_is_clipped() -> Result<(), String> {
    let mut app = app();
    let list = spawn_under_root(
        &mut app,
        (
            Name::new("list"),
            Node {
                flex_direction: FlexDirection::Column,
                height: Val::Px(50.0),
                width: Val::Px(200.0),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition::default(),
        ),
    );
    for row in 0..4 {
        let entity = app
            .world_mut()
            .spawn((
                Name::new(format!("row{row}")),
                Button,
                Node {
                    height: Val::Px(40.0),
                    flex_shrink: 0.0,
                    ..default()
                },
            ))
            .with_child(Text::new(format!("Row {row}")))
            .id();
        app.world_mut().entity_mut(list).add_child(entity);
    }

    assert_eq!(
        node(&mut app, "row0")?.visibility,
        NodeVisibility::Visible,
        "top row"
    );
    assert_eq!(
        node(&mut app, "row3")?.visibility,
        NodeVisibility::Clipped,
        "far row"
    );
    app.world_mut()
        .get_mut::<ScrollPosition>(list)
        .ok_or("no ScrollPosition")?
        .y = 120.0;
    assert_eq!(
        node(&mut app, "row3")?.visibility,
        NodeVisibility::Visible,
        "scrolled to"
    );
    assert_eq!(
        node(&mut app, "row0")?.visibility,
        NodeVisibility::Clipped,
        "scrolled off"
    );
    Ok(())
}

#[test]
fn a_node_outside_the_viewport_is_off_screen() -> Result<(), String> {
    let mut app = app();
    let button = spawn_button(&mut app, "button", "Far away");
    app.world_mut()
        .get_mut::<Node>(button)
        .ok_or("no Node")?
        .left = Val::Px(5000.0);
    assert_eq!(
        node(&mut app, "button")?.visibility,
        NodeVisibility::OffScreen,
        "far"
    );
    app.world_mut()
        .get_mut::<Node>(button)
        .ok_or("no Node")?
        .left = Val::Px(10.0);
    assert_eq!(
        node(&mut app, "button")?.visibility,
        NodeVisibility::Visible,
        "near"
    );
    Ok(())
}

#[test]
fn a_node_under_an_overlapping_panel_is_covered() -> Result<(), String> {
    let mut app = app();
    spawn_button(&mut app, "button", "Beneath");
    let overlay = spawn_under_root(
        &mut app,
        (
            Name::new("overlay"),
            placed(80.0, 80.0, 200.0, 100.0),
            GlobalZIndex(10),
        ),
    );

    assert_eq!(
        node(&mut app, "button")?.visibility,
        NodeVisibility::Covered,
        "covered"
    );
    assert_eq!(
        node(&mut app, "overlay")?.visibility,
        NodeVisibility::Visible,
        "the overlay itself is on top"
    );
    app.world_mut().entity_mut(overlay).insert(Pickable::IGNORE);
    assert_eq!(
        node(&mut app, "button")?.visibility,
        NodeVisibility::Visible,
        "an overlay the pointer passes through covers nothing"
    );
    app.world_mut().entity_mut(overlay).despawn();
    assert_eq!(
        node(&mut app, "button")?.visibility,
        NodeVisibility::Visible,
        "uncovered"
    );
    Ok(())
}

#[test]
fn bounds_are_logical_pixels_and_ids_name_the_entity() -> Result<(), String> {
    let mut app = app();
    let button = spawn_button(&mut app, "button", "Here");
    let button_node = node(&mut app, "button")?;
    assert_eq!(entity_of(button_node.id), Some(button), "id round-trips");
    assert_eq!(button_node.id, node_id(button), "id is the entity's");
    let bounds = button_node.bounds;
    assert_eq!(
        [bounds.x, bounds.y, bounds.width, bounds.height].map(f32::to_bits),
        [100.0_f32, 100.0, 120.0, 30.0].map(f32::to_bits),
        "the box it was placed at"
    );
    Ok(())
}

#[test]
fn a_subtree_snapshot_carries_what_its_ancestors_impose() -> Result<(), String> {
    let mut app = app();
    let panel = spawn_under_root(
        &mut app,
        (Name::new("panel"), Node::default(), InteractionDisabled),
    );
    let wrapper = app.world_mut().spawn(Node::default()).id();
    let button = spawn_button(&mut app, "button", "Deep");
    app.world_mut().entity_mut(panel).add_child(wrapper);
    app.world_mut().entity_mut(wrapper).add_child(button);
    settle(&mut app);

    let mut state = bevy::ecs::system::SystemState::<super::UiModel<'_, '_>>::new(app.world_mut());
    let model = state
        .get(app.world_mut())
        .map_err(|error| error.to_string())?;
    let under = model.snapshot_under(wrapper);
    let summary: Vec<(Option<&str>, bool)> = under
        .iter()
        .map(|node| (node.test_id.as_deref(), node.has_state(NodeState::Disabled)))
        .collect();
    assert_eq!(
        summary,
        vec![(Some("button"), true)],
        "an unnamed root yields its descendants, still disabled by the panel above"
    );
    Ok(())
}
