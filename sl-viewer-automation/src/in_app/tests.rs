//! Teeth for the in-app helpers: each one goes through the app's executor —
//! the click lands through the synthetic input, the strictness and the
//! actionability checks are the engine's — and a failure is the driver's,
//! printed as the end-to-end tier prints it.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Button;
use pretty_assertions::assert_eq;
use sl_automation_proto::{
    ActionabilityCheck, AutomationError, Deadline, Locator, NodeValue, Role, WaitCondition,
};
use sl_viewer_driver::DriverError;
use sl_viewer_testkit::interact::InteractionTest;
use sl_viewer_testkit::{settle, spawn_under_root};

use super::{
    Options, click, click_while_disabled, expect_disabled, expect_hidden, fill, find, locate,
    press, text,
};

/// The names of the nodes a `Pointer<Click>` reached, in order.
#[derive(Resource, Debug, Default)]
struct Clicked(Vec<String>);

/// A built, settled interaction app; the helpers install the executor.
fn app() -> App {
    let mut app = InteractionTest::new().build();
    app.init_resource::<Clicked>();
    settle(&mut app);
    app
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

/// A named button labelled `label` at `node`, whose clicks are recorded —
/// every pointer click, disabled or not: `InteractionDisabled` is advisory in
/// Bevy, so a click on a disabled button still reaches it.
fn button(app: &mut App, name: &str, label: &str, node: Node) -> Entity {
    let button = spawn_under_root(app, (Name::new(name.to_owned()), Button, node));
    let text = app.world_mut().spawn(Text::new(label)).id();
    app.world_mut().entity_mut(button).add_child(text);
    let name = name.to_owned();
    app.world_mut().entity_mut(button).observe(
        move |mut click: On<Pointer<Click>>, mut clicked: ResMut<Clicked>| {
            click.propagate(false);
            clicked.0.push(name.clone());
        },
    );
    button
}

/// What the app's buttons were clicked, in order.
fn clicked(app: &App) -> Vec<String> {
    app.world().resource::<Clicked>().0.clone()
}

/// The viewer's error inside a driver failure.
fn automation_error(error: &DriverError) -> Result<&AutomationError, String> {
    error
        .automation_error()
        .ok_or_else(|| format!("not a failure the app reported: {error}"))
}

/// A short frame deadline, for the requests that are meant to time out.
fn impatient(app: &mut App) {
    app.insert_resource(Options {
        deadline: Deadline {
            frames: Some(20),
            millis: None,
        },
        ..Options::default()
    });
}

#[test]
fn a_click_through_a_locator_lands_on_its_one_node() -> Result<(), String> {
    let mut app = app();
    button(&mut app, "ok", "OK", placed(100.0, 100.0, 120.0, 30.0));
    button(
        &mut app,
        "cancel",
        "Cancel",
        placed(300.0, 100.0, 120.0, 30.0),
    );
    settle(&mut app);
    let node = click(&mut app, &Locator::role(Role::Button).named("OK"))
        .map_err(|error| error.to_string())?;
    assert_eq!(node.test_id.as_deref(), Some("ok"));
    assert_eq!(clicked(&app), vec!["ok".to_owned()]);

    let any = Locator::role(Role::Button);
    let Err(error) = click(&mut app, &any) else {
        return Err("two buttons are ambiguous".to_owned());
    };
    assert!(
        matches!(
            automation_error(&error)?,
            AutomationError::Ambiguous { candidates, .. } if candidates.len() == 2
        ),
        "{error}"
    );
    assert_eq!(clicked(&app), vec!["ok".to_owned()], "nothing more clicked");
    Ok(())
}

/// **A disabled button fails a click the way the end-to-end tier's does** —
/// the same error kind (a timeout on the `enabled` check, the button as last
/// observed) under the same words: `viewer <label>: click <locator> failed:
/// <error>`.
#[test]
fn a_disabled_button_fails_a_click_as_the_driver_prints_it() -> Result<(), String> {
    let mut app = app();
    impatient(&mut app);
    let off = button(&mut app, "off", "Off", placed(100.0, 100.0, 120.0, 30.0));
    app.world_mut().entity_mut(off).insert(InteractionDisabled);
    settle(&mut app);
    let locator = Locator::test_id("off");
    let Err(error) = click(&mut app, &locator) else {
        return Err("a disabled button was clicked".to_owned());
    };
    let inner = automation_error(&error)?;
    let AutomationError::TimedOut {
        failed_check,
        last_observed,
        ..
    } = inner
    else {
        return Err(format!("not a timeout: {inner}"));
    };
    assert_eq!(*failed_check, Some(ActionabilityCheck::Enabled));
    assert_eq!(last_observed.len(), 1, "the button, as last seen");
    let printed = error.to_string();
    assert!(
        printed.starts_with(&format!("viewer app: click {locator} failed: {inner}")),
        "{printed}"
    );
    assert!(
        error
            .failure()
            .is_some_and(|failure| failure.report.is_some()),
        "the report travels with the error"
    );
    assert!(clicked(&app).is_empty(), "no click was played");
    Ok(())
}

/// **A click on a disabled button really lands** — so a test that it does
/// nothing can fail — and is refused on an enabled one, where a plain click
/// is what was meant.
#[test]
fn a_click_while_disabled_reaches_the_node_and_only_a_disabled_one() -> Result<(), String> {
    let mut app = app();
    impatient(&mut app);
    let off = button(&mut app, "off", "Off", placed(100.0, 100.0, 120.0, 30.0));
    app.world_mut().entity_mut(off).insert(InteractionDisabled);
    button(&mut app, "on", "On", placed(300.0, 100.0, 120.0, 30.0));
    settle(&mut app);

    let node = click_while_disabled(&mut app, &Locator::test_id("off"))
        .map_err(|error| error.to_string())?;
    assert_eq!(node.test_id.as_deref(), Some("off"));
    assert_eq!(
        clicked(&app),
        vec!["off".to_owned()],
        "the pointer pressed and released on the disabled button"
    );

    let Err(error) = click_while_disabled(&mut app, &Locator::test_id("on")) else {
        return Err("an enabled button was clicked as a disabled one".to_owned());
    };
    assert!(
        matches!(
            automation_error(&error)?,
            AutomationError::TimedOut {
                condition: Some(WaitCondition::Disabled),
                ..
            }
        ),
        "{error}"
    );
    assert_eq!(
        clicked(&app),
        vec!["off".to_owned()],
        "nothing more clicked"
    );
    Ok(())
}

#[test]
fn a_fill_a_press_and_a_read() -> Result<(), String> {
    let mut app = app();
    spawn_under_root(
        &mut app,
        (
            Name::new("field"),
            EditableText::new("old words"),
            TabIndex(0),
            placed(100.0, 100.0, 240.0, 30.0),
        ),
    );
    settle(&mut app);
    let field = Locator::test_id("field");
    let node = fill(&mut app, &field, "new").map_err(|error| error.to_string())?;
    assert_eq!(node.value, Some(NodeValue::Text("new".to_owned())));
    press(&mut app, "Shift+!").map_err(|error| error.to_string())?;
    assert_eq!(
        text(&mut app, &field).map_err(|error| error.to_string())?,
        Some("new!".to_owned()),
        "the key went to the field the fill focused"
    );
    Ok(())
}

#[test]
fn reads_and_expectations() -> Result<(), String> {
    let mut app = app();
    impatient(&mut app);
    let off = button(&mut app, "off", "Off", placed(100.0, 100.0, 120.0, 30.0));
    app.world_mut().entity_mut(off).insert(InteractionDisabled);
    settle(&mut app);

    let node = locate(&mut app, &Locator::test_id("off")).map_err(|error| error.to_string())?;
    assert_eq!(node.name.as_deref(), Some("Off"), "named by its label");
    assert_eq!(
        find(&mut app, &Locator::role(Role::Button))
            .map_err(|error| error.to_string())?
            .len(),
        1
    );
    expect_disabled(&mut app, &Locator::test_id("off")).map_err(|error| error.to_string())?;
    expect_hidden(&mut app, &Locator::test_id("nowhere")).map_err(|error| error.to_string())?;

    let Err(error) = locate(&mut app, &Locator::test_id("nowhere")) else {
        return Err("a read of nothing found something".to_owned());
    };
    assert!(
        error
            .to_string()
            .starts_with("viewer app: read: wait for [test_id=nowhere] to be attached failed: "),
        "{error}"
    );
    Ok(())
}
