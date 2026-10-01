//! Teeth for the executor: every request is carried out through the queue
//! alone — submitted, advanced by the app's own frames, answered — and each
//! failure kind comes back with the report it documents.
//!
//! Every fixture lives in an [`InteractionTest`] app, so a click is the
//! synthetic input's, travelling the same hit test a user's does.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::Button;
use pretty_assertions::assert_eq;
use serde_json::json;
use sl_automation_proto::{
    ActionabilityCheck, AutomationError, Deadline, Locator, LogStream, NameMatcher, NodeValue,
    Probe, ProbeReadout, Request, RequestBody, RequestId, Response, ResponseBody, Role,
    StateCondition, StateObservation, ValueTest, WaitCondition,
};
use sl_viewer_testkit::interact::{self, InteractionTest};
use sl_viewer_testkit::{settle, spawn_under_root};
use sl_viewer_ui_core::ui_element::UiAction;
use sl_viewer_world_api::SelectionSet;

use super::{AutomationPlugin, AutomationQueue};
use crate::diagnostics::{DiagnosticsSource, LogTally};

/// The most frames a test waits for a response.
const PATIENCE: u32 = 2000;

/// A deadline short enough for the requests that are meant to time out.
const SHORT: Deadline = Deadline {
    frames: Some(20),
    millis: None,
};

/// The names of the nodes a `Pointer<Click>` reached, in order.
#[derive(Resource, Debug, Default)]
struct Clicked(Vec<String>);

/// A built, settled interaction app with the executor installed.
fn app() -> App {
    let mut app = InteractionTest::new().build();
    app.add_plugins(AutomationPlugin).init_resource::<Clicked>();
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

/// A named button labelled `label` at `node`, whose clicks are recorded.
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

/// Submit `body` as request `id`.
fn submit(app: &mut App, id: u64, body: RequestBody) {
    app.world_mut()
        .resource_mut::<AutomationQueue>()
        .submit(Request {
            id: RequestId(id),
            body,
        });
}

/// The response to `id`, if it has come.
fn take(app: &mut App, id: u64) -> Option<Response> {
    app.world_mut()
        .resource_mut::<AutomationQueue>()
        .take_response(RequestId(id))
}

/// Run frames until `id` is answered.
fn answer(app: &mut App, id: u64) -> Result<Response, String> {
    for _frame in 0..PATIENCE {
        app.update();
        if let Some(response) = take(app, id) {
            return Ok(response);
        }
    }
    Err(format!("request {id} was never answered"))
}

/// Submit `body` and run frames until it is answered.
fn request(app: &mut App, body: RequestBody) -> Result<Response, String> {
    submit(app, 1, body);
    answer(app, 1)
}

/// The body of a successful response.
fn ok(response: Response) -> Result<ResponseBody, String> {
    response
        .result
        .map_err(|error| format!("the request failed: {error}"))
}

/// The error of a failed response, with its report.
fn failed(
    response: Response,
) -> Result<(AutomationError, sl_automation_proto::FailureReport), String> {
    match response.result {
        Ok(body) => Err(format!("the request succeeded: {body:?}")),
        Err(error) => Ok((error, response.report.ok_or("an error with no report")?)),
    }
}

/// A click request on `locator` under the default deadline.
fn click(locator: Locator) -> RequestBody {
    RequestBody::Click {
        locator,
        button: sl_automation_proto::PointerButton::Left,
        double: false,
        deadline: Deadline::default(),
    }
}

#[test]
fn a_click_is_carried_out_through_the_queue_alone() -> Result<(), String> {
    let mut app = app();
    button(&mut app, "ok", "OK", placed(100.0, 100.0, 120.0, 30.0));
    settle(&mut app);
    let body = ok(request(
        &mut app,
        click(Locator::role(Role::Button).named("OK")),
    )?)?;
    let ResponseBody::Done { node } = body else {
        return Err(format!("not a done: {body:?}"));
    };
    assert_eq!(node.test_id.as_deref(), Some("ok"));
    assert_eq!(
        app.world().resource::<Clicked>().0,
        vec!["ok".to_owned()],
        "the synthetic click landed, and the answer came after it"
    );
    Ok(())
}

/// The clicks that reached a node: the button and the click count of each.
#[derive(Resource, Debug, Default)]
struct Presses(Vec<(PointerButton, u8)>);

#[test]
fn a_click_takes_either_button_and_may_be_a_double_click() -> Result<(), String> {
    let mut app = app();
    app.init_resource::<Presses>();
    let target = button(&mut app, "row", "Row", placed(100.0, 100.0, 120.0, 30.0));
    app.world_mut().entity_mut(target).observe(
        |click: On<Pointer<Click>>, mut presses: ResMut<Presses>| {
            presses.0.push((click.button, click.count));
        },
    );
    settle(&mut app);
    for (button, double) in [
        (sl_automation_proto::PointerButton::Right, false),
        (sl_automation_proto::PointerButton::Left, true),
    ] {
        let body = ok(request(
            &mut app,
            RequestBody::Click {
                locator: Locator::test_id("row"),
                button,
                double,
                deadline: Deadline::default(),
            },
        )?)?;
        assert!(matches!(body, ResponseBody::Done { .. }), "{body:?}");
    }
    let presses = &app.world().resource::<Presses>().0;
    assert_eq!(
        presses.first(),
        Some(&(PointerButton::Secondary, 1)),
        "a right click: {presses:?}"
    );
    assert!(
        presses.contains(&(PointerButton::Primary, 2)),
        "a double click reaches the node as one: {presses:?}"
    );
    Ok(())
}

/// What was dropped on a node: the dragged node's name.
#[derive(Resource, Debug, Default)]
struct Dropped(Vec<String>);

#[test]
fn a_drag_carries_one_node_onto_another() -> Result<(), String> {
    let mut app = app();
    app.init_resource::<Dropped>();
    button(&mut app, "source", "Item", placed(50.0, 50.0, 120.0, 30.0));
    let target = button(
        &mut app,
        "target",
        "Folder",
        placed(300.0, 200.0, 120.0, 30.0),
    );
    app.world_mut().entity_mut(target).observe(
        |drop: On<Pointer<DragDrop>>, names: Query<'_, '_, &Name>, mut dropped: ResMut<Dropped>| {
            if let Ok(name) = names.get(drop.dropped) {
                dropped.0.push(name.as_str().to_owned());
            }
        },
    );
    settle(&mut app);
    let body = ok(request(
        &mut app,
        RequestBody::DragTo {
            source: Locator::test_id("source"),
            target: Locator::role(Role::Button).named("Folder"),
            deadline: Deadline::default(),
        },
    )?)?;
    let ResponseBody::Done { node } = body else {
        return Err(format!("not a done: {body:?}"));
    };
    assert_eq!(
        node.test_id.as_deref(),
        Some("target"),
        "the answer names the target"
    );
    assert_eq!(
        app.world().resource::<Dropped>().0,
        vec!["source".to_owned()],
        "the source was dropped on the target through the picking drag"
    );
    let (error, _report) = failed(request(
        &mut app,
        RequestBody::DragTo {
            source: Locator::test_id("source"),
            target: Locator::test_id("nowhere"),
            deadline: SHORT,
        },
    )?)?;
    assert!(
        matches!(error, AutomationError::TimedOut { ref locator, .. } if locator.test_id.as_deref() == Some("nowhere")),
        "a missing target times out naming it: {error}"
    );
    Ok(())
}

/// How far a node was dragged: the sum of its drag events' deltas.
#[derive(Resource, Debug, Default)]
struct Dragged(Vec2);

#[test]
fn a_drag_by_an_offset_moves_the_pointer_that_far_with_the_node_held() -> Result<(), String> {
    let mut app = app();
    app.init_resource::<Dragged>();
    let grip = button(&mut app, "grip", "Grip", placed(100.0, 100.0, 120.0, 30.0));
    app.world_mut().entity_mut(grip).observe(
        |drag: On<Pointer<Drag>>, mut dragged: ResMut<Dragged>| {
            dragged.0 += drag.delta;
        },
    );
    settle(&mut app);
    let body = ok(request(
        &mut app,
        RequestBody::DragBy {
            source: Locator::test_id("grip"),
            offset: [150.0, -60.0],
            deadline: Deadline::default(),
        },
    )?)?;
    let ResponseBody::Done { node } = body else {
        return Err(format!("not a done: {body:?}"));
    };
    assert_eq!(
        node.test_id.as_deref(),
        Some("grip"),
        "the answer names the node dragged"
    );
    let travelled = app.world().resource::<Dragged>().0;
    assert!(
        travelled.distance(Vec2::new(150.0, -60.0)) < 0.5,
        "the drag carried the node by the offset: {travelled}"
    );
    Ok(())
}

#[test]
fn requests_in_flight_are_answered_independently_and_actions_take_turns() -> Result<(), String> {
    let mut app = app();
    button(
        &mut app,
        "first",
        "First",
        placed(100.0, 100.0, 120.0, 30.0),
    );
    button(
        &mut app,
        "second",
        "Second",
        placed(100.0, 200.0, 120.0, 30.0),
    );
    settle(&mut app);
    submit(
        &mut app,
        1,
        RequestBody::WaitFor {
            locator: Locator::test_id("late"),
            condition: WaitCondition::Visible,
            deadline: Deadline {
                frames: Some(PATIENCE),
                millis: Some(60_000),
            },
        },
    );
    submit(&mut app, 2, click(Locator::test_id("first")));
    submit(&mut app, 3, click(Locator::test_id("second")));
    let mut order = Vec::new();
    for _frame in 0..PATIENCE {
        app.update();
        for response in app
            .world_mut()
            .resource_mut::<AutomationQueue>()
            .drain_responses()
        {
            ok(response.clone())?;
            order.push(response.id.0);
        }
        if order.len() == 2 {
            break;
        }
    }
    assert_eq!(
        order,
        vec![2, 3],
        "the clicks, in turn; the wait still waits"
    );
    assert_eq!(
        app.world().resource::<Clicked>().0,
        vec!["first".to_owned(), "second".to_owned()]
    );
    button(&mut app, "late", "Late", placed(300.0, 100.0, 120.0, 30.0));
    let body = ok(answer(&mut app, 1)?)?;
    assert!(
        matches!(&body, ResponseBody::Satisfied { nodes } if nodes.len() == 1),
        "{body:?}"
    );
    Ok(())
}

#[test]
fn a_fill_replaces_the_text_by_typing_and_confirms_it() -> Result<(), String> {
    let mut app = app();
    spawn_under_root(
        &mut app,
        (
            Name::new("field"),
            EditableText::new("old words"),
            // Focusable, as every viewer field is: a click on a field with no
            // tab index bubbles its focus request to the window, which clears
            // it.
            TabIndex(0),
            placed(100.0, 100.0, 240.0, 30.0),
        ),
    );
    settle(&mut app);
    let body = ok(request(
        &mut app,
        RequestBody::Fill {
            locator: Locator::test_id("field"),
            text: "new text".to_owned(),
            deadline: Deadline::default(),
        },
    )?)?;
    let ResponseBody::Done { node } = body else {
        return Err(format!("not a done: {body:?}"));
    };
    assert_eq!(node.value, Some(NodeValue::Text("new text".to_owned())));
    assert_eq!(
        interact::text_of(&mut app, "field").as_deref(),
        Some("new text")
    );
    Ok(())
}

#[test]
fn a_key_press_reaches_the_focused_field() -> Result<(), String> {
    let mut app = app();
    let field = spawn_under_root(
        &mut app,
        (
            Name::new("field"),
            EditableText::new(""),
            placed(100.0, 100.0, 240.0, 30.0),
        ),
    );
    settle(&mut app);
    interact::focus(&mut app, field);
    for keys in ["h", "i", "Shift+!"] {
        let body = ok(request(
            &mut app,
            RequestBody::Press {
                keys: keys.to_owned(),
                hold_frames: 0,
            },
        )?)?;
        assert_eq!(body, ResponseBody::Pressed);
    }
    let wait = ok(request(
        &mut app,
        RequestBody::WaitFor {
            locator: Locator::test_id("field"),
            condition: WaitCondition::Text(NameMatcher::Exact("hi!".to_owned())),
            deadline: Deadline::default(),
        },
    )?)?;
    assert!(matches!(wait, ResponseBody::Satisfied { .. }), "{wait:?}");
    let (error, _report) = failed(request(
        &mut app,
        RequestBody::Press {
            keys: "Hyper+a".to_owned(),
            hold_frames: 0,
        },
    )?)?;
    assert!(
        matches!(error, AutomationError::InvalidRequest { .. }),
        "{error}"
    );
    Ok(())
}

#[test]
fn an_ambiguity_reports_the_candidates_the_tree_and_the_event_tail() -> Result<(), String> {
    let mut app = app();
    let window = spawn_under_root(
        &mut app,
        (Name::new("window"), placed(0.0, 0.0, 400.0, 400.0)),
    );
    for (name, top) in [("a", 10.0), ("b", 60.0)] {
        let child = button(&mut app, name, "OK", placed(10.0, top, 120.0, 30.0));
        app.world_mut().entity_mut(window).add_child(child);
    }
    app.world_mut().write_message(UiAction {
        element: "toolbar",
        action: "before",
    });
    settle(&mut app);
    let (error, report) = failed(request(
        &mut app,
        click(
            Locator::role(Role::Button)
                .named("OK")
                .within(Locator::test_id("window")),
        ),
    )?)?;
    let AutomationError::Ambiguous { candidates, .. } = &error else {
        return Err(format!("not an ambiguity: {error}"));
    };
    assert_eq!(candidates.len(), 2);
    assert_eq!(
        report
            .tree
            .iter()
            .map(|node| node.test_id.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("window")],
        "the excerpt is the scope's subtree"
    );
    let first = report.tree.first().ok_or("an empty excerpt")?;
    assert_eq!(first.children.len(), 2, "both buttons are in it");
    assert!(
        report
            .events
            .iter()
            .any(|entry| entry.stream == LogStream::UiAction && entry.kind == "toolbar.before"),
        "the event tail: {:?}",
        report.events
    );
    assert!(
        app.world().resource::<Clicked>().0.is_empty(),
        "nothing was clicked"
    );
    Ok(())
}

#[test]
fn not_actionable_and_timed_out_name_their_check() -> Result<(), String> {
    let mut app = app();
    button(&mut app, "ok", "OK", placed(100.0, 100.0, 120.0, 30.0));
    settle(&mut app);
    let (error, report) = failed(request(
        &mut app,
        RequestBody::Fill {
            locator: Locator::test_id("ok"),
            text: "x".to_owned(),
            deadline: Deadline::default(),
        },
    )?)?;
    assert!(
        matches!(
            error,
            AutomationError::NotActionable {
                check: ActionabilityCheck::Editable,
                ..
            }
        ),
        "{error}"
    );
    assert!(
        !report.tree.is_empty(),
        "the top of the tree, with no scope"
    );
    let (error, _report) = failed(request(
        &mut app,
        RequestBody::Click {
            locator: Locator::test_id("nowhere"),
            button: sl_automation_proto::PointerButton::Left,
            double: false,
            deadline: SHORT,
        },
    )?)?;
    assert!(
        matches!(
            error,
            AutomationError::TimedOut {
                failed_check: None,
                condition: None,
                ..
            }
        ),
        "{error}"
    );
    let (error, _report) = failed(request(
        &mut app,
        RequestBody::WaitFor {
            locator: Locator::test_id("ok"),
            condition: WaitCondition::Disabled,
            deadline: SHORT,
        },
    )?)?;
    let AutomationError::TimedOut {
        condition,
        last_observed,
        ..
    } = &error
    else {
        return Err(format!("not a timeout: {error}"));
    };
    assert_eq!(condition, &Some(WaitCondition::Disabled));
    assert_eq!(last_observed.len(), 1, "the button, as last seen");
    let (error, _report) = failed(request(
        &mut app,
        RequestBody::Snapshot {
            within: Some(Locator::test_id("nowhere")),
        },
    )?)?;
    assert!(matches!(error, AutomationError::NotFound { .. }), "{error}");
    Ok(())
}

#[test]
fn a_failure_reports_what_was_logged_while_it_ran() -> Result<(), String> {
    let mut app = app();
    let tally = LogTally::default();
    app.insert_resource(DiagnosticsSource(tally.clone()));
    let subscriber = tracing_subscriber::layer::SubscriberExt::with(
        tracing_subscriber::registry(),
        tally.layer(),
    );
    let _guard = tracing::subscriber::set_default(subscriber);
    tracing::warn!("before the request");
    submit(
        &mut app,
        1,
        RequestBody::Click {
            locator: Locator::test_id("nowhere"),
            button: sl_automation_proto::PointerButton::Left,
            double: false,
            deadline: SHORT,
        },
    );
    app.update();
    tracing::warn!("while it ran");
    let (_error, report) = failed(answer(&mut app, 1)?)?;
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|line| line.message.as_str())
            .collect::<Vec<_>>(),
        vec!["while it ran"],
        "only what was logged after it started"
    );
    Ok(())
}

#[test]
fn state_waits_hold_on_a_probe_value_or_a_log_entry_and_time_out_with_the_last_seen()
-> Result<(), String> {
    let mut app = app();
    app.init_resource::<SelectionSet>();
    let body = ok(request(
        &mut app,
        RequestBody::WaitForState {
            condition: StateCondition::Probe {
                probe: Probe::Selection,
                pointer: String::new(),
                test: ValueTest::Equals(json!([])),
            },
            deadline: Deadline::default(),
        },
    )?)?;
    assert_eq!(
        body,
        ResponseBody::StateHeld {
            observed: StateObservation::Probe {
                readout: ProbeReadout::Selection(Vec::new()),
            },
        }
    );
    let (error, _report) = failed(request(
        &mut app,
        RequestBody::WaitForState {
            condition: StateCondition::Probe {
                probe: Probe::Selection,
                pointer: "/0/primary".to_owned(),
                test: ValueTest::Present,
            },
            deadline: SHORT,
        },
    )?)?;
    assert!(
        matches!(
            &error,
            AutomationError::StateTimedOut {
                last_observed: Some(StateObservation::Probe {
                    readout: ProbeReadout::Selection(selection)
                }),
                ..
            } if selection.is_empty()
        ),
        "{error}"
    );

    let cursor = app.world().resource::<crate::EventLog>().cursor();
    submit(
        &mut app,
        2,
        RequestBody::WaitForState {
            condition: StateCondition::Logged {
                cursor,
                streams: vec![LogStream::UiAction],
                kind_is: Some("toolbar.go".to_owned()),
                detail_contains: None,
            },
            deadline: Deadline::default(),
        },
    );
    for action in ["stay", "go"] {
        app.world_mut().write_message(UiAction {
            element: "toolbar",
            action,
        });
        app.update();
    }
    let body = ok(answer(&mut app, 2)?)?;
    let ResponseBody::StateHeld {
        observed: StateObservation::Logged { entry, next },
    } = body
    else {
        return Err(format!("not a log entry: {body:?}"));
    };
    assert_eq!(entry.kind, "toolbar.go", "not the entry before it");
    assert_eq!(next, entry.seq + 1);
    let log = ok(request(
        &mut app,
        RequestBody::ReadLog {
            cursor,
            streams: vec![LogStream::UiAction],
            limit: None,
        },
    )?)?;
    let ResponseBody::Log { page } = log else {
        return Err(format!("not a log: {log:?}"));
    };
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.kind.as_str())
            .collect::<Vec<_>>(),
        vec!["toolbar.stay", "toolbar.go"]
    );
    Ok(())
}

#[test]
fn what_cannot_be_asked_is_refused_at_once() -> Result<(), String> {
    let mut app = app();
    let (error, _report) = failed(request(
        &mut app,
        RequestBody::Read {
            probe: Probe::Conversations,
        },
    )?)?;
    assert!(
        matches!(error, AutomationError::Unavailable { .. }),
        "no conversation model in this app: {error}"
    );
    for body in [
        RequestBody::Screenshot {
            path: "relative.png".to_owned(),
            outline: None,
        },
        RequestBody::MenuPath {
            path: Vec::new(),
            deadline: Deadline::default(),
        },
        RequestBody::WaitForState {
            condition: StateCondition::Probe {
                probe: Probe::Agent,
                pointer: "region".to_owned(),
                test: ValueTest::Present,
            },
            deadline: Deadline::default(),
        },
        RequestBody::DragHandle {
            handle: "translate-w".to_owned(),
            amount: sl_automation_proto::DragAmount::Distance(1.0),
            snap: sl_automation_proto::SnapSide::Free,
            modifiers: sl_automation_proto::DragModifiers::None,
            deadline: Deadline::default(),
        },
    ] {
        let (error, _report) = failed(request(&mut app, body)?)?;
        assert!(
            matches!(error, AutomationError::InvalidRequest { .. }),
            "{error}"
        );
    }
    submit(
        &mut app,
        7,
        RequestBody::WaitFor {
            locator: Locator::test_id("never"),
            condition: WaitCondition::Attached,
            deadline: SHORT,
        },
    );
    submit(
        &mut app,
        7,
        RequestBody::Find {
            locator: Locator::default(),
        },
    );
    app.update();
    let (error, _report) = failed(take(&mut app, 7).ok_or("the duplicate was not refused")?)?;
    assert!(
        matches!(error, AutomationError::InvalidRequest { .. }),
        "a second request under an id in flight: {error}"
    );
    Ok(())
}

/// A stand-in for the viewer's file dialog service: the dialog waiting, if
/// any, whether the desktop shows it instead, and what it was answered with.
#[derive(Resource, Debug, Default)]
struct DialogStandIn {
    /// The purpose of the dialog waiting for an answer.
    waiting: Option<String>,
    /// Whether the desktop shows its own chooser instead.
    desktop: bool,
    /// The answers given, in order.
    answers: Vec<Option<std::path::PathBuf>>,
}

/// The stand-in's answerer, as the viewer's assembly registers the real one.
fn answer_stand_in(
    world: &mut World,
    picked: Option<std::path::PathBuf>,
) -> crate::FileDialogAnswer {
    let mut stand_in = world.resource_mut::<DialogStandIn>();
    if stand_in.desktop {
        return crate::FileDialogAnswer::ShownOnDesktop;
    }
    let Some(purpose) = stand_in.waiting.take() else {
        return crate::FileDialogAnswer::NothingWaiting;
    };
    stand_in.answers.push(picked);
    crate::FileDialogAnswer::Answered {
        purpose,
        title: "Import a sky".to_owned(),
        folder: false,
    }
}

/// An app whose file dialog is the stand-in.
fn dialog_app() -> App {
    let mut app = app();
    app.init_resource::<DialogStandIn>()
        .insert_resource(crate::ProbeSources {
            file_dialog: Some(answer_stand_in),
            ..crate::ProbeSources::default()
        });
    app
}

/// An answer waits for its dialog — it is usually sent before the click that
/// asks for one lands — and then answers exactly that dialog, once.
#[test]
fn a_file_dialog_answer_waits_for_the_dialog_and_answers_it_once() -> Result<(), String> {
    let mut app = dialog_app();
    submit(
        &mut app,
        1,
        RequestBody::AnswerFileDialog {
            path: Some("/presets/skies/Dawn.xml".to_owned()),
            deadline: Deadline::default(),
        },
    );
    for _frame in 0..5 {
        app.update();
    }
    assert!(
        take(&mut app, 1).is_none(),
        "answered before a dialog was asked for"
    );
    app.world_mut().resource_mut::<DialogStandIn>().waiting =
        Some("settings-editor-import-sky".to_owned());
    let body = ok(answer(&mut app, 1)?)?;
    assert_eq!(
        body,
        ResponseBody::FileDialogAnswered {
            purpose: "settings-editor-import-sky".to_owned(),
            title: "Import a sky".to_owned(),
            folder: false,
        }
    );
    assert_eq!(
        app.world().resource::<DialogStandIn>().answers,
        vec![Some(std::path::PathBuf::from("/presets/skies/Dawn.xml"))],
        "the dialog got the path, once"
    );
    Ok(())
}

/// No dialog in time is its own error; a relative path, a viewer with no
/// dialog service, and one that shows the desktop's chooser are refused.
#[test]
fn a_file_dialog_answer_fails_without_a_dialog_it_can_answer() -> Result<(), String> {
    let mut bare = app();
    let mut app = dialog_app();
    let (error, _report) = failed(request(
        &mut app,
        RequestBody::AnswerFileDialog {
            path: None,
            deadline: SHORT,
        },
    )?)?;
    assert!(
        matches!(error, AutomationError::NoFileDialog { frames: 20, .. }),
        "{error}"
    );
    let (error, _report) = failed(request(
        &mut app,
        RequestBody::AnswerFileDialog {
            path: Some("relative.xml".to_owned()),
            deadline: SHORT,
        },
    )?)?;
    assert!(
        matches!(error, AutomationError::InvalidRequest { .. }),
        "{error}"
    );
    app.world_mut().resource_mut::<DialogStandIn>().desktop = true;
    let (error, _report) = failed(request(
        &mut app,
        RequestBody::AnswerFileDialog {
            path: None,
            deadline: SHORT,
        },
    )?)?;
    assert!(
        matches!(error, AutomationError::Unavailable { .. }),
        "{error}"
    );
    let (error, _report) = failed(request(
        &mut bare,
        RequestBody::AnswerFileDialog {
            path: None,
            deadline: SHORT,
        },
    )?)?;
    assert!(
        matches!(error, AutomationError::Unavailable { .. }),
        "{error}"
    );
    assert!(
        app.world().resource::<DialogStandIn>().answers.is_empty(),
        "nothing was answered"
    );
    Ok(())
}
