//! Teeth for the locator engine: each actionability check holds a pursuit up
//! while its condition does and lets it through once it stops, and a node the
//! user would have to scroll to is scrolled to and clicked through the real
//! pointer.
//!
//! Every fixture lives in an [`InteractionTest`] app, so "covered" is the same
//! hit test a click travels through, and "clicked" is a `Pointer<Click>` the
//! picking stack delivered.

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Button;
use bevy_flair::prelude::ClassList;
use pretty_assertions::assert_eq;
use sl_automation_proto::{
    ActionabilityCheck, AutomationError, Deadline, Locator, NodeVisibility, Role,
};
use sl_viewer_testkit::interact::{self, InteractionTest};
use sl_viewer_testkit::{settle, spawn_under_root};
use sl_viewer_ui_core::skin::LIST_ROW_CLASS;
use sl_viewer_ui_core::virtual_list::{VirtualList, VirtualListPlugin, VirtualRow};
use sl_viewer_ui_widgets::ui_text_input::ReadOnlyField;

use super::{Intent, Progress, Pursuit, PursuitError, Target};

/// The names of the nodes a `Pointer<Click>` reached, in order.
#[derive(Resource, Debug, Default)]
struct Clicked(Vec<String>);

/// A built, settled interaction app that records clicks on named nodes.
fn app() -> App {
    let mut app = InteractionTest::new().build();
    app.init_resource::<Clicked>();
    settle(&mut app);
    app
}

/// Record every click on `entity` under `name`.
fn record_clicks(app: &mut App, entity: Entity, name: &str) {
    let name = name.to_owned();
    app.world_mut().entity_mut(entity).observe(
        move |mut click: On<Pointer<Click>>, mut clicked: ResMut<Clicked>| {
            click.propagate(false);
            clicked.0.push(name.clone());
        },
    );
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

/// A named button labelled `label` under `parent` (the UI root when `None`),
/// laid out by `node`, whose clicks are recorded.
fn button_in(app: &mut App, parent: Option<Entity>, name: &str, label: &str, node: Node) -> Entity {
    let bundle = (Name::new(name.to_owned()), Button, node);
    let button = match parent {
        Some(parent) => {
            let button = app.world_mut().spawn(bundle).id();
            app.world_mut().entity_mut(parent).add_child(button);
            button
        }
        None => spawn_under_root(app, bundle),
    };
    let text = app.world_mut().spawn(Text::new(label)).id();
    app.world_mut().entity_mut(button).add_child(text);
    record_clicks(app, button, name);
    button
}

/// Poll `pursuit` a frame at a time until it is ready or gives up.
fn pursue(app: &mut App, pursuit: &mut Pursuit) -> Result<Target, PursuitError> {
    loop {
        match pursuit.poll(app.world_mut())? {
            Progress::Ready(target) => return Ok(target),
            Progress::Waiting(_) => app.update(),
        }
    }
}

/// Poll `pursuit` for `frames` frames and return the check it was still
/// waiting on at the end; an error if it became ready or gave up.
fn still_waiting(
    app: &mut App,
    pursuit: &mut Pursuit,
    frames: u32,
) -> Result<ActionabilityCheck, String> {
    let mut last = None;
    for _frame in 0..frames {
        match pursuit
            .poll(app.world_mut())
            .map_err(|error| error.to_string())?
        {
            Progress::Ready(target) => {
                return Err(format!("ready at {} when it should wait", target.aim));
            }
            Progress::Waiting(check) => last = Some(check),
        }
        app.update();
    }
    last.ok_or_else(|| "polled no frames".to_owned())
}

/// Pursue `locator` for a click and click where it says to aim.
fn click(app: &mut App, locator: Locator) -> Result<Target, PursuitError> {
    let target = pursue(app, &mut Pursuit::new(locator, Intent::Click))?;
    interact::click(app, target.aim, MouseButton::Left);
    Ok(target)
}

/// The clicks recorded so far.
fn clicked(app: &App) -> Vec<String> {
    app.world().resource::<Clicked>().0.clone()
}

/// The automation error a pursuit gave up with.
fn automation_error(result: Result<Target, PursuitError>) -> Result<AutomationError, String> {
    match result {
        Err(PursuitError::Automation(error)) => Ok(*error),
        Err(PursuitError::Model(error)) => Err(format!("the model failed: {error}")),
        Ok(target) => Err(format!("ready at {} when it should fail", target.aim)),
    }
}

/// A short deadline, for the pursuits that are meant to time out.
const SHORT: Deadline = Deadline {
    frames: Some(30),
    millis: None,
};

#[test]
fn a_clear_button_is_clicked_at_its_centre() -> Result<(), String> {
    let mut app = app();
    button_in(
        &mut app,
        None,
        "ok",
        "OK",
        placed(100.0, 100.0, 120.0, 30.0),
    );
    settle(&mut app);
    let target = click(&mut app, Locator::role(Role::Button).named("OK"))
        .map_err(|error| error.to_string())?;
    assert!(
        (target.aim.x - 160.0).abs() < 0.5 && (target.aim.y - 115.0).abs() < 0.5,
        "the aim is the box's centre when all of it shows: {}",
        target.aim
    );
    assert_eq!(clicked(&app), vec!["ok".to_owned()], "and the click lands");
    Ok(())
}

#[test]
fn a_covered_button_waits_until_it_is_uncovered() -> Result<(), String> {
    let mut app = app();
    button_in(
        &mut app,
        None,
        "under",
        "Under",
        placed(100.0, 100.0, 120.0, 30.0),
    );
    let cover = spawn_under_root(
        &mut app,
        (
            Name::new("cover"),
            placed(80.0, 80.0, 200.0, 80.0),
            BackgroundColor(Color::BLACK),
        ),
    );
    settle(&mut app);
    let mut pursuit = Pursuit::new(Locator::test_id("under"), Intent::Click);
    assert_eq!(
        still_waiting(&mut app, &mut pursuit, 5)?,
        ActionabilityCheck::ReceivesEvents,
        "a panel on top of it takes the click"
    );
    app.world_mut().entity_mut(cover).despawn();
    let target = pursue(&mut app, &mut pursuit).map_err(|error| error.to_string())?;
    interact::click(&mut app, target.aim, MouseButton::Left);
    assert_eq!(
        clicked(&app),
        vec!["under".to_owned()],
        "uncovered, it is clicked"
    );
    Ok(())
}

#[test]
fn a_disabled_button_waits_until_it_is_enabled() -> Result<(), String> {
    let mut app = app();
    let panel = spawn_under_root(&mut app, (Name::new("panel"), Node::default()));
    button_in(
        &mut app,
        Some(panel),
        "apply",
        "Apply",
        placed(100.0, 100.0, 120.0, 30.0),
    );
    app.world_mut()
        .entity_mut(panel)
        .insert(InteractionDisabled);
    settle(&mut app);
    let mut pursuit = Pursuit::new(Locator::test_id("apply"), Intent::Click);
    assert_eq!(
        still_waiting(&mut app, &mut pursuit, 5)?,
        ActionabilityCheck::Enabled,
        "a disabled ancestor disables it"
    );
    assert!(
        pursue(
            &mut app,
            &mut Pursuit::new(Locator::test_id("apply"), Intent::Hover)
        )
        .is_ok(),
        "but a disabled node may still be hovered"
    );
    app.world_mut()
        .entity_mut(panel)
        .remove::<InteractionDisabled>();
    let target = pursue(&mut app, &mut pursuit).map_err(|error| error.to_string())?;
    interact::click(&mut app, target.aim, MouseButton::Left);
    assert_eq!(
        clicked(&app),
        vec!["apply".to_owned()],
        "enabled, it is clicked"
    );
    Ok(())
}

/// Moves the [`Drifting`] node one pixel right every frame while it has the
/// marker.
fn drift(mut nodes: Query<&mut Node, With<Drifting>>) {
    for mut node in &mut nodes {
        if let Val::Px(left) = node.left {
            node.left = Val::Px(left + 1.0);
        }
    }
}

/// A node still animating in.
#[derive(Component)]
struct Drifting;

#[test]
fn a_moving_button_waits_until_it_holds_still() -> Result<(), String> {
    let mut app = app();
    app.add_systems(Update, drift);
    let button = button_in(
        &mut app,
        None,
        "sliding",
        "Sliding",
        placed(100.0, 100.0, 120.0, 30.0),
    );
    app.world_mut().entity_mut(button).insert(Drifting);
    settle(&mut app);
    let mut pursuit = Pursuit::new(Locator::test_id("sliding"), Intent::Click);
    assert_eq!(
        still_waiting(&mut app, &mut pursuit, 10)?,
        ActionabilityCheck::Stable,
        "its box changes every frame"
    );
    app.world_mut().entity_mut(button).remove::<Drifting>();
    let target = pursue(&mut app, &mut pursuit).map_err(|error| error.to_string())?;
    interact::click(&mut app, target.aim, MouseButton::Left);
    assert_eq!(
        clicked(&app),
        vec!["sliding".to_owned()],
        "at rest, it is clicked"
    );
    Ok(())
}

/// A column of `count` named, labelled buttons `row-N`, each 30 px tall, in a
/// 100 px tall area laid out by `overflow` at (100, 100).
fn rows_in_area(app: &mut App, overflow: Overflow, count: usize) -> Entity {
    let area = spawn_under_root(
        app,
        (
            Name::new("area"),
            Node {
                flex_direction: FlexDirection::Column,
                overflow,
                ..placed(100.0, 100.0, 200.0, 100.0)
            },
        ),
    );
    for index in 0..count {
        button_in(
            app,
            Some(area),
            &format!("row-{index}"),
            &format!("Row {index}"),
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(30.0),
                flex_shrink: 0.0,
                ..default()
            },
        );
    }
    area
}

#[test]
fn a_row_scrolled_out_of_its_area_is_scrolled_to_and_clicked() -> Result<(), String> {
    let mut app = app();
    rows_in_area(&mut app, Overflow::scroll_y(), 20);
    settle(&mut app);
    let row = Locator::role(Role::Button).named("Row 15");
    let mut pursuit = Pursuit::new(row.clone(), Intent::Click);
    assert_eq!(
        pursuit
            .poll(app.world_mut())
            .map_err(|error| error.to_string())?,
        Progress::Waiting(ActionabilityCheck::Visible),
        "far down the area it cannot be seen"
    );
    app.update();
    let target = pursue(&mut app, &mut pursuit).map_err(|error| error.to_string())?;
    assert!(
        (100.0..=200.0).contains(&target.aim.y),
        "scrolled into the area: aimed at {}",
        target.aim
    );
    interact::click(&mut app, target.aim, MouseButton::Left);
    assert_eq!(clicked(&app), vec!["row-15".to_owned()], "and clicked");
    Ok(())
}

#[test]
fn a_row_clipped_by_an_area_that_cannot_scroll_times_out_invisible() -> Result<(), String> {
    let mut app = app();
    rows_in_area(&mut app, Overflow::clip(), 20);
    settle(&mut app);
    let error = automation_error(pursue(
        &mut app,
        &mut Pursuit::new(Locator::test_id("row-15"), Intent::Click).with_deadline(SHORT),
    ))?;
    let AutomationError::TimedOut {
        failed_check,
        last_observed,
        frames,
        ..
    } = error
    else {
        return Err(format!("a timeout, not {error}"));
    };
    assert_eq!(
        (failed_check, frames),
        (Some(ActionabilityCheck::Visible), 30),
        "nothing can bring it into view"
    );
    assert_eq!(
        last_observed
            .iter()
            .map(|node| (node.test_id.as_deref(), node.visibility))
            .collect::<Vec<_>>(),
        vec![(Some("row-15"), NodeVisibility::Clipped)],
        "the timeout says what it last saw"
    );
    Ok(())
}

#[test]
fn the_aim_is_the_centre_of_the_visible_part() -> Result<(), String> {
    let mut app = app();
    rows_in_area(&mut app, Overflow::clip(), 4);
    settle(&mut app);
    // Row 3 spans 190..220 in an area that ends at 200: ten pixels show.
    let target = pursue(
        &mut app,
        &mut Pursuit::new(Locator::test_id("row-3"), Intent::Click),
    )
    .map_err(|error| error.to_string())?;
    assert!(
        (target.aim.y - 195.0).abs() < 0.5,
        "aimed into the part that shows, not at the box's centre (205): {}",
        target.aim
    );
    interact::click(&mut app, target.aim, MouseButton::Left);
    assert_eq!(
        clicked(&app),
        vec!["row-3".to_owned()],
        "and the click lands"
    );
    Ok(())
}

#[test]
fn a_button_off_screen_times_out_outside_the_viewport() -> Result<(), String> {
    let mut app = app();
    button_in(
        &mut app,
        None,
        "away",
        "Away",
        placed(5000.0, 100.0, 120.0, 30.0),
    );
    settle(&mut app);
    let error = automation_error(pursue(
        &mut app,
        &mut Pursuit::new(Locator::test_id("away"), Intent::Click).with_deadline(SHORT),
    ))?;
    assert!(
        matches!(
            error,
            AutomationError::TimedOut {
                failed_check: Some(ActionabilityCheck::InViewport),
                ..
            }
        ),
        "{error}"
    );
    Ok(())
}

#[test]
fn a_missing_node_waits_to_be_attached_and_says_so() -> Result<(), String> {
    let mut app = app();
    let mut pursuit = Pursuit::new(Locator::role(Role::Button).named("Later"), Intent::Click)
        .with_deadline(SHORT);
    assert_eq!(
        still_waiting(&mut app, &mut pursuit, 3)?,
        ActionabilityCheck::Attached,
        "nothing matches yet"
    );
    button_in(
        &mut app,
        None,
        "later",
        "Later",
        placed(100.0, 100.0, 120.0, 30.0),
    );
    let target = pursue(&mut app, &mut pursuit).map_err(|error| error.to_string())?;
    assert_eq!(
        target.node.test_id.as_deref(),
        Some("later"),
        "found once it appears"
    );

    let error = automation_error(pursue(
        &mut app,
        &mut Pursuit::new(Locator::test_id("never"), Intent::Click).with_deadline(SHORT),
    ))?;
    assert!(
        matches!(
            error,
            AutomationError::TimedOut {
                failed_check: None,
                ref last_observed,
                ..
            } if last_observed.is_empty()
        ),
        "no single node, so no check to name: {error}"
    );
    Ok(())
}

#[test]
fn an_ambiguous_locator_fails_at_once_listing_every_candidate() -> Result<(), String> {
    let mut app = app();
    button_in(
        &mut app,
        None,
        "first",
        "OK",
        placed(100.0, 100.0, 120.0, 30.0),
    );
    button_in(
        &mut app,
        None,
        "second",
        "OK",
        placed(100.0, 200.0, 120.0, 30.0),
    );
    settle(&mut app);
    let mut pursuit = Pursuit::new(Locator::role(Role::Button).named("OK"), Intent::Click);
    let error = automation_error(pursue(&mut app, &mut pursuit))?;
    let message = error.to_string();
    let AutomationError::Ambiguous { candidates, .. } = error else {
        return Err(format!("an ambiguity, not {message}"));
    };
    assert_eq!(
        candidates
            .iter()
            .map(|node| node.test_id.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("first"), Some("second")],
        "both, in reading order"
    );
    assert!(
        message.contains("#first") && message.contains("#second"),
        "the message names them too: {message}"
    );
    assert!(clicked(&app).is_empty(), "and nothing was clicked");
    Ok(())
}

#[test]
fn a_fill_needs_an_editable_field() -> Result<(), String> {
    let mut app = app();
    let field = spawn_under_root(
        &mut app,
        (
            Name::new("field"),
            EditableText::new("fixed"),
            ReadOnlyField,
            placed(100.0, 100.0, 200.0, 30.0),
        ),
    );
    button_in(
        &mut app,
        None,
        "ok",
        "OK",
        placed(100.0, 200.0, 120.0, 30.0),
    );
    settle(&mut app);
    let mut pursuit = Pursuit::new(Locator::test_id("field"), Intent::Fill);
    assert_eq!(
        still_waiting(&mut app, &mut pursuit, 5)?,
        ActionabilityCheck::Editable,
        "a read-only field refuses typing"
    );
    app.world_mut().entity_mut(field).remove::<ReadOnlyField>();
    assert!(
        pursue(&mut app, &mut pursuit).is_ok(),
        "made editable, it can be filled"
    );
    let error = automation_error(pursue(
        &mut app,
        &mut Pursuit::new(Locator::test_id("ok"), Intent::Fill),
    ))?;
    assert!(
        matches!(
            error,
            AutomationError::NotActionable {
                check: ActionabilityCheck::Editable,
                ..
            }
        ),
        "a button can never be typed into, so that fails at once: {error}"
    );
    Ok(())
}

/// Items in the long list.
const LONG_LIST: usize = 500;

/// The label of the item a row shows.
fn label(row: &VirtualRow) -> String {
    row.index
        .map_or_else(String::new, |index| format!("Item {index}"))
}

/// Build each pooled row of a list once — a list-row class, a label and a
/// click recorder — and rebind its label to the item it shows.
fn bind_rows(
    mut commands: Commands,
    added: Query<(Entity, &VirtualRow), Added<VirtualRow>>,
    changed: Query<(&VirtualRow, &Children), Changed<VirtualRow>>,
    mut texts: Query<&mut Text>,
) {
    for (row, bound) in &added {
        commands
            .entity(row)
            .insert(ClassList::new_with_classes([LIST_ROW_CLASS]))
            .with_child(Text::new(label(bound)))
            .observe(
                |mut click: On<Pointer<Click>>,
                 rows: Query<&VirtualRow>,
                 mut clicked: ResMut<Clicked>| {
                    click.propagate(false);
                    if let Ok(VirtualRow {
                        index: Some(index), ..
                    }) = rows.get(click.entity)
                    {
                        clicked.0.push(format!("item-{index}"));
                    }
                },
            );
    }
    for (row, children) in &changed {
        for &child in children {
            if let Ok(mut text) = texts.get_mut(child) {
                text.0 = label(row);
            }
        }
    }
}

/// An app with a 200 px tall virtual list of [`LONG_LIST`] items named
/// `long-list`, scrolled to `offset`.
fn long_list_app(offset: f32) -> (App, Entity) {
    let mut app = app();
    app.add_plugins(VirtualListPlugin);
    app.add_systems(
        Update,
        bind_rows.after(sl_viewer_ui_core::virtual_list::layout_virtual_lists),
    );
    let mut list = VirtualList::new(20.0);
    list.item_count = LONG_LIST;
    list.scroll_by(offset);
    let viewport = spawn_under_root(
        &mut app,
        (
            Name::new("long-list"),
            list,
            Node {
                overflow: Overflow::clip(),
                ..placed(100.0, 100.0, 300.0, 200.0)
            },
        ),
    );
    settle(&mut app);
    settle(&mut app);
    (app, viewport)
}

/// The list's scroll offset.
fn offset_of(app: &App, list: Entity) -> Result<f32, String> {
    app.world()
        .get::<VirtualList>(list)
        .map(VirtualList::scroll_offset)
        .ok_or_else(|| "the list is gone".to_owned())
}

#[test]
fn a_virtual_row_far_down_is_paged_to_and_clicked_by_name() -> Result<(), String> {
    let (mut app, list) = long_list_app(0.0);
    let row = Locator::role(Role::ListItem)
        .named("Item 400")
        .within(Locator::test_id("long-list"));
    let target = click(&mut app, row).map_err(|error| error.to_string())?;
    assert!(
        (100.0..=300.0).contains(&target.aim.y),
        "the row is in the list's viewport: aimed at {}",
        target.aim
    );
    assert_eq!(
        clicked(&app),
        vec!["item-400".to_owned()],
        "and it is the row clicked"
    );
    let offset = offset_of(&app, list)?;
    assert!(
        (400.0 * 20.0 - offset).abs() <= 200.0,
        "the list is left showing it: offset {offset}"
    );
    Ok(())
}

#[test]
fn a_virtual_list_without_the_row_is_searched_to_its_end_and_put_back() -> Result<(), String> {
    let (mut app, list) = long_list_app(300.0);
    let before = offset_of(&app, list)?;
    let mut pursuit = Pursuit::new(
        Locator::role(Role::ListItem).named("Item 9999"),
        Intent::Click,
    )
    .with_deadline(Deadline {
        frames: Some(400),
        millis: None,
    });
    let mut deepest = before;
    let error = loop {
        match pursuit.poll(app.world_mut()) {
            Ok(Progress::Waiting(check)) => {
                assert_eq!(check, ActionabilityCheck::Attached, "nothing ever matches");
            }
            Ok(Progress::Ready(target)) => {
                return Err(format!("found {:?}, which does not exist", target.node));
            }
            Err(error) => break automation_error(Err(error))?,
        }
        app.update();
        deepest = deepest.max(offset_of(&app, list)?);
    };
    assert!(
        matches!(
            error,
            AutomationError::TimedOut {
                failed_check: None,
                ..
            }
        ),
        "there is no such row: {error}"
    );
    let end = 500.0 * 20.0 - 200.0;
    assert!(
        (deepest - end).abs() < 0.5,
        "the search paged to the end of the list ({end}), reaching {deepest}"
    );
    settle(&mut app);
    let after = offset_of(&app, list)?;
    assert!(
        (after - before).abs() < 0.5,
        "and put it back where it was: {before} before, {after} after"
    );
    Ok(())
}
