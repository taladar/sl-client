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
use bevy_flair::prelude::ClassList;
use pretty_assertions::assert_eq;
use sl_automation_proto::{NodeState, NodeValue, NodeVisibility, Role, UiNode};
use sl_viewer_testkit::interact::{self, InteractionTest};
use sl_viewer_testkit::{settle, spawn_under_root};
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::semantic::{Expanded, Semantic};
use sl_viewer_ui_core::skin::{LIST_ROW_CLASS, SELECTED_CLASS};
use sl_viewer_ui_core::virtual_list::VirtualRow;
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

/// Put `class` on `entity`'s class list, or take it off.
fn set_class(app: &mut App, entity: Entity, class: &'static str, on: bool) -> Result<(), String> {
    let mut classes = app
        .world_mut()
        .get_mut::<ClassList>(entity)
        .ok_or("no class list")?;
    if on {
        classes.add(class);
    } else {
        classes.remove(class);
    }
    Ok(())
}

#[test]
fn a_list_row_is_a_list_item_selected_by_its_class() -> Result<(), String> {
    let mut app = app();
    let list = spawn_under_root(
        &mut app,
        (
            Name::new("list"),
            Semantic::new(Role::List),
            Node {
                flex_direction: FlexDirection::Column,
                ..default()
            },
        ),
    );
    let rows: Vec<Entity> = ["Aviary", "Bay City"]
        .into_iter()
        .map(|caption| {
            app.world_mut()
                .spawn((
                    Name::new(caption.to_lowercase()),
                    ClassList::new_with_classes([LIST_ROW_CLASS]),
                    Node::default(),
                    ChildOf(list),
                ))
                .with_child(Text::new(caption))
                .id()
        })
        .collect();
    let [aviary, _bay] = rows.as_slice() else {
        return Err("two rows".to_owned());
    };

    let nodes = snap(&mut app)?;
    let list = find(&nodes, "list").ok_or("no list")?;
    let summary: Vec<(Role, Option<&str>, bool)> = list
        .children
        .iter()
        .map(|row| {
            (
                row.role,
                row.name.as_deref(),
                row.has_state(NodeState::Selected),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            (Role::ListItem, Some("Aviary"), false),
            (Role::ListItem, Some("Bay City"), false),
        ],
        "a row drawn as a list row is a list item, named by its text"
    );

    set_class(&mut app, *aviary, SELECTED_CLASS, true)?;
    assert!(
        node(&mut app, "aviary")?.has_state(NodeState::Selected),
        "the selection class selects the row"
    );
    assert!(
        !node(&mut app, "bay city")?.has_state(NodeState::Selected),
        "and only that row"
    );
    set_class(&mut app, *aviary, SELECTED_CLASS, false)?;
    assert!(
        !node(&mut app, "aviary")?.has_state(NodeState::Selected),
        "dropping the class deselects it"
    );
    Ok(())
}

/// **A parked pooled row is unnamed**, whatever its binder left in it: a
/// virtual list's spare rows keep the text of the item they last showed, and
/// named by it a spare row would make a locator for that item ambiguous.
#[test]
fn a_parked_pool_row_has_no_name() -> Result<(), String> {
    let mut app = app();
    let list = spawn_under_root(
        &mut app,
        (
            Name::new("list"),
            Semantic::new(Role::List),
            Node::default(),
        ),
    );
    for (slot, index, test_id) in [(0, Some(0), "bound"), (1, None, "parked")] {
        app.world_mut()
            .spawn((
                Name::new(test_id),
                ClassList::new_with_classes([LIST_ROW_CLASS]),
                VirtualRow { slot, index },
                Node::default(),
                ChildOf(list),
            ))
            .with_child(Text::new("RestrainedLoveNoSetEnv"));
    }
    assert_eq!(
        node(&mut app, "bound")?.name.as_deref(),
        Some("RestrainedLoveNoSetEnv")
    );
    let parked = node(&mut app, "parked")?;
    assert_eq!(parked.role, Role::ListItem, "still a row of the list");
    assert_eq!(parked.name, None, "but one that names no item");
    Ok(())
}

#[test]
fn a_tree_row_reports_its_level_and_whether_it_is_unfolded() -> Result<(), String> {
    let mut app = app();
    let tree = spawn_under_root(
        &mut app,
        (
            Name::new("tree"),
            Semantic::new(Role::Tree),
            Node::default(),
        ),
    );
    let folder = app
        .world_mut()
        .spawn((
            Name::new("folder"),
            Semantic::new(Role::TreeItem).level(2),
            Node::default(),
            ChildOf(tree),
        ))
        .with_child(Text::new("Clothing"))
        .id();

    let row = node(&mut app, "folder")?;
    assert_eq!(
        (row.role, row.name.as_deref(), row.level),
        (Role::TreeItem, Some("Clothing"), Some(2)),
        "a tree row carries its level"
    );
    assert!(!row.has_state(NodeState::Expanded), "folded to begin with");

    app.world_mut().entity_mut(folder).insert(Expanded);
    assert!(
        node(&mut app, "folder")?.has_state(NodeState::Expanded),
        "the marker unfolds it"
    );
    app.world_mut().entity_mut(folder).remove::<Expanded>();
    assert!(
        !node(&mut app, "folder")?.has_state(NodeState::Expanded),
        "and its removal folds it again"
    );
    Ok(())
}

#[test]
fn a_leaf_keeps_its_label_but_not_the_popup_it_owns() -> Result<(), String> {
    let mut app = app();
    let combo = spawn_button(&mut app, "combo", "Medium");
    app.world_mut()
        .entity_mut(combo)
        .insert(Semantic::new(Role::Combobox));
    let popup = app
        .world_mut()
        .spawn((
            Name::new("popup"),
            Semantic::new(Role::List),
            placed(0.0, 30.0, 120.0, 40.0),
            ChildOf(combo),
        ))
        .id();
    app.world_mut().spawn((
        Name::new("option"),
        ClassList::new_with_classes([LIST_ROW_CLASS]),
        Node::default(),
        Text::new("High"),
        ChildOf(popup),
    ));

    let combo = node(&mut app, "combo")?;
    assert_eq!(
        combo.name.as_deref(),
        Some("Medium"),
        "the popup's rows are not part of the combo's name"
    );
    let owned: Vec<(Option<&str>, Role)> = combo
        .children
        .iter()
        .map(|child| (child.test_id.as_deref(), child.role))
        .collect();
    assert_eq!(
        owned,
        vec![(Some("popup"), Role::List)],
        "the label text is swallowed, the owned popup is not"
    );
    Ok(())
}

#[test]
fn a_semantic_names_by_another_node_or_by_a_key() -> Result<(), String> {
    let mut app = app();
    let window = spawn_under_root(&mut app, (Name::new("window"), Node::default()));
    let title = app
        .world_mut()
        .spawn((
            Text::new("Preferences"),
            Translated::new("floater-preferences"),
            ChildOf(window),
        ))
        .id();
    app.world_mut()
        .spawn((Text::new("A sentence of content."), ChildOf(window)));
    app.world_mut()
        .entity_mut(window)
        .insert(Semantic::new(Role::Window).labelled_by(title));
    let ball = spawn_under_root(
        &mut app,
        (
            Name::new("ball"),
            Semantic::new(Role::Trackball).name_key("trackball-sun"),
            placed(10.0, 10.0, 40.0, 40.0),
        ),
    );
    app.world_mut().spawn((
        Text::new("N"),
        Translated::new("trackball-north"),
        ChildOf(ball),
    ));

    let window = node(&mut app, "window")?;
    assert_eq!(
        (
            window.role,
            window.name.as_deref(),
            window.name_key.as_deref()
        ),
        (
            Role::Window,
            Some("Preferences"),
            Some("floater-preferences")
        ),
        "a window is named by its title, not its content"
    );
    let ball = node(&mut app, "ball")?;
    assert_eq!(
        (ball.name.as_deref(), ball.name_key.as_deref()),
        (Some("trackball-sun"), Some("trackball-sun")),
        "a key names the node — itself, with no locale loaded — over the letters \
         drawn on it"
    );
    Ok(())
}

/// **A skin's icon is decoration**: what a stylesheet writes into a
/// pseudo-element — a glyph host's only content, or a mark beside a caption —
/// is neither a text node of its own nor part of the name of the control it
/// sits in: a close box is not called "✕", a folder row not "📂 Textures".
#[test]
fn a_skin_glyph_names_nothing() -> Result<(), String> {
    let mut app = app();
    let row = spawn_under_root(
        &mut app,
        (Name::new("row"), Button, placed(100.0, 100.0, 200.0, 30.0)),
    );
    let glyph = app
        .world_mut()
        .spawn((
            sl_viewer_ui_core::glyph::glyph_host(
                sl_viewer_ui_core::glyph::CLOSE,
                TextFont::default(),
                [],
            ),
            ChildOf(row),
        ))
        .id();
    // The caption carries pseudo-elements too, as a checkbox's does.
    app.world_mut().spawn((
        Text::new("Textures"),
        bevy_flair::style::components::PseudoElementsSupport,
        ChildOf(row),
    ));
    settle(&mut app);
    // What the stylesheet's `content` would write into every pseudo-element.
    let mut spans = app.world_mut().query::<(&ChildOf, &mut TextSpan)>();
    for (_parent, mut span) in spans.iter_mut(app.world_mut()) {
        span.0 = "📂".to_owned();
    }
    let row_node = node(&mut app, "row")?;
    assert_eq!(row_node.name.as_deref(), Some("Textures"));
    let nodes = snap(&mut app)?;
    assert!(
        !contains_id(&nodes, node_id(glyph)),
        "the glyph is no node of its own: {nodes:#?}"
    );
    Ok(())
}

/// Whether `id` is anywhere in `nodes`.
fn contains_id(nodes: &[UiNode], id: sl_automation_proto::NodeId) -> bool {
    nodes
        .iter()
        .any(|node| node.id == id || contains_id(&node.children, id))
}

/// **Two fields under one caption are told apart by their part**, and a
/// caption that abbreviates is read as its spoken form. With no locale loaded
/// a key names itself, so the keys here are the words.
#[test]
fn paired_fields_are_named_by_caption_and_part() -> Result<(), String> {
    use sl_viewer_ui_core::semantic::{LabelledBy, NamePart, SpokenLabel};

    let mut app = app();
    let row = spawn_under_root(
        &mut app,
        (Name::new("row"), placed(100.0, 100.0, 400.0, 30.0)),
    );
    let caption = app
        .world_mut()
        .spawn((
            Text::new("Offset (U/V)"),
            Translated::new("Offset (U/V)"),
            SpokenLabel("Offset".into()),
            ChildOf(row),
        ))
        .id();
    app.world_mut().entity_mut(row).insert(LabelledBy(caption));
    for (name, part) in [("u", "U"), ("v", "V")] {
        app.world_mut().spawn((
            Name::new(name),
            EditableText::new(""),
            NamePart(part.into()),
            ChildOf(row),
        ));
    }
    assert_eq!(node(&mut app, "u")?.name.as_deref(), Some("Offset U"));
    assert_eq!(node(&mut app, "v")?.name.as_deref(), Some("Offset V"));

    // A field with no part is called what the caption says.
    app.world_mut()
        .spawn((Name::new("whole"), EditableText::new(""), ChildOf(row)));
    assert_eq!(node(&mut app, "whole")?.name.as_deref(), Some("Offset"));
    Ok(())
}
