//! The command line against a scripted viewer at the far end of a channel
//! pair: what each verb asks the viewer, what it prints as text and as JSON,
//! how a failure reads, and how `attach` runs a script of lines.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser as _;
use pretty_assertions::assert_eq;
use serde_json::json;
use sl_automation_proto::{
    AutomationError, Bounds, EnvironmentReadout, Locator, NodeId, NodeState, NodeVisibility,
    PROTOCOL_VERSION, PointerButton, Probe, ProbeReadout, Request, RequestBody, Response,
    ResponseBody, Role, SkyReadout, UiNode, ViewerIdentity, ViewerMessage, WaitCondition,
};
use sl_viewer_driver::{Viewer, ViewerOptions};
use tokio::sync::mpsc::unbounded_channel;

use crate::attach;
use crate::cli::{Cli, Command, Verb};
use crate::output::Printer;
use crate::verbs;

/// The boxed error every test here reports through.
type TestError = Box<dyn core::error::Error>;

/// A viewer's answer to a request body.
type Script = dyn Fn(&RequestBody) -> Result<ResponseBody, Box<AutomationError>> + Send + Sync;

/// The Build window's disabled Apply button, inside the window.
fn apply_button() -> UiNode {
    UiNode {
        id: NodeId(2),
        role: Role::Button,
        name: Some("Apply".to_owned()),
        name_key: Some("build-apply".to_owned()),
        test_id: Some("build.apply".to_owned()),
        states: BTreeSet::from([NodeState::Disabled]),
        value: None,
        level: None,
        accelerator: None,
        bounds: Bounds {
            x: 10.0,
            y: 20.0,
            width: 60.0,
            height: 24.0,
        },
        visibility: NodeVisibility::Visible,
        children: Vec::new(),
    }
}

/// The Build window, holding the Apply button.
fn build_window() -> UiNode {
    UiNode {
        id: NodeId(1),
        role: Role::Window,
        name: Some("Build".to_owned()),
        name_key: None,
        test_id: Some("floater:build".to_owned()),
        states: BTreeSet::new(),
        value: None,
        level: None,
        accelerator: None,
        bounds: Bounds {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 400.0,
        },
        visibility: NodeVisibility::Visible,
        children: vec![apply_button()],
    }
}

/// A scripted viewer answering each request by `script` (a hello by
/// itself), recording every request body in the list it answers with.
async fn scripted(
    script: Arc<Script>,
) -> Result<(Viewer, Arc<Mutex<Vec<RequestBody>>>), TestError> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (requests, mut incoming) = unbounded_channel::<Request>();
    let (outgoing, messages) = unbounded_channel();
    let record = Arc::clone(&seen);
    drop(tokio::spawn(async move {
        while let Some(Request { id, body }) = incoming.recv().await {
            if let Ok(mut seen) = record.lock() {
                seen.push(body.clone());
            }
            let result = match &body {
                RequestBody::Hello => Ok(ResponseBody::Hello {
                    protocol: PROTOCOL_VERSION,
                    viewer: ViewerIdentity {
                        viewer: "scripted".to_owned(),
                        version: "0.0.0".to_owned(),
                        pid: 7,
                        grid: Some("fake".to_owned()),
                        agent_name: Some("Stage a".to_owned()),
                        agent_id: None,
                    },
                }),
                other => script(other).map_err(|error| *error),
            };
            let _sent = outgoing.send(ViewerMessage::Response(Box::new(Response {
                id,
                result,
                report: None,
            })));
        }
    }));
    let options = ViewerOptions::new("a").with_timeout(Duration::from_secs(1));
    let viewer = Viewer::over_link(requests, messages, options).await?;
    Ok((viewer, seen))
}

/// What `verb` prints on the scripted viewer, as text or JSON, and what it
/// asked.
async fn run_verb(
    script: Arc<Script>,
    arguments: &[&str],
    json: bool,
) -> Result<(String, Vec<RequestBody>), TestError> {
    let cli = Cli::try_parse_from(["sl-viewer-ctl"].iter().chain(arguments))?;
    let Command::Verb(verb) = cli.command else {
        return Err(format!("{arguments:?} is not a verb").into());
    };
    let (viewer, seen) = scripted(script).await?;
    let mut printer = Printer::new(Vec::new(), json);
    verbs::run(&viewer, &verb, &mut printer, core::future::pending()).await?;
    let printed = String::from_utf8(printer.into_inner())?;
    let asked = seen
        .lock()
        .map_err(|error| error.to_string())?
        .iter()
        .filter(|body| !matches!(body, RequestBody::Hello))
        .cloned()
        .collect();
    Ok((printed, asked))
}

/// The Apply button's locator, as the acceptance writes it.
const APPLY: &str = "window[test_id=floater:build] >> button[name_key=build-apply]";

/// The locator [`APPLY`] parses to.
fn apply_locator() -> Locator {
    Locator::role(Role::Button)
        .name_key("build-apply")
        .within(Locator {
            role: Some(Role::Window),
            ..Locator::test_id("floater:build")
        })
}

#[test]
fn a_selector_argument_parses_in_the_grammar_or_as_json() -> Result<(), TestError> {
    let grammar = Cli::try_parse_from(["sl-viewer-ctl", "click", APPLY])?;
    let json = serde_json::to_string(&apply_locator())?;
    let as_json = Cli::try_parse_from(["sl-viewer-ctl", "click", json.as_str()])?;
    for cli in [grammar, as_json] {
        match cli.command {
            Command::Verb(Verb::Click { selector, .. }) => assert_eq!(selector, apply_locator()),
            other => return Err(format!("parsed as {other:?}").into()),
        }
    }
    let refused = Cli::try_parse_from(["sl-viewer-ctl", "click", "knob[name=OK]"])
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(
        refused.contains("column 1: unknown role `knob`"),
        "the grammar's error reaches the user: {refused}"
    );
    Ok(())
}

#[tokio::test]
async fn click_asks_for_a_click_and_prints_the_node() -> Result<(), TestError> {
    let (printed, asked) = run_verb(
        Arc::new(|_body| {
            Ok(ResponseBody::Done {
                node: apply_button(),
            })
        }),
        &["click", "--right", APPLY],
        false,
    )
    .await?;
    assert_eq!(
        printed,
        "right-clicked button \"Apply\" key=build-apply #build.apply at 10,20 60x24 [disabled]\n"
    );
    match asked.as_slice() {
        [
            RequestBody::Click {
                locator,
                button: PointerButton::Right,
                double: false,
                ..
            },
        ] => assert_eq!(*locator, apply_locator()),
        other => return Err(format!("asked {other:?}").into()),
    }
    Ok(())
}

/// The Build window's title bar, by its test id.
const TITLE_BAR: &str = "window[test_id=floater:build] >> [test_id=floater-title-bar]";

#[tokio::test]
async fn drag_onto_asks_for_a_drop_and_prints_the_target() -> Result<(), TestError> {
    let (printed, asked) = run_verb(
        Arc::new(|_body| {
            Ok(ResponseBody::Done {
                node: build_window(),
            })
        }),
        &["drag", APPLY, "--onto", "window[test_id=floater:build]"],
        false,
    )
    .await?;
    assert_eq!(
        printed,
        "dropped onto window \"Build\" #floater:build at 0,0 300x400\n"
    );
    match asked.as_slice() {
        [RequestBody::DragTo { source, target, .. }] => {
            assert_eq!(*source, apply_locator());
            assert_eq!(
                *target,
                Locator {
                    role: Some(Role::Window),
                    ..Locator::test_id("floater:build")
                }
            );
        }
        other => return Err(format!("asked {other:?}").into()),
    }
    Ok(())
}

#[tokio::test]
async fn drag_by_takes_a_negative_offset_and_prints_the_pressed_node() -> Result<(), TestError> {
    let script: Arc<Script> = Arc::new(|_body| {
        Ok(ResponseBody::Done {
            node: apply_button(),
        })
    });
    let (printed, asked) = run_verb(
        Arc::clone(&script),
        &["drag", TITLE_BAR, "--by", "-40.5,12"],
        false,
    )
    .await?;
    assert_eq!(
        printed,
        "dragged button \"Apply\" key=build-apply #build.apply at 10,20 60x24 [disabled]\n"
    );
    match asked.as_slice() {
        [RequestBody::DragBy { source, offset, .. }] => {
            assert_eq!(source.to_string(), TITLE_BAR);
            assert_eq!(format!("{offset:?}"), "[-40.5, 12.0]");
        }
        other => return Err(format!("asked {other:?}").into()),
    }
    let (json, _asked) =
        run_verb(script, &["--json", "drag", TITLE_BAR, "--by=0,-3"], true).await?;
    let parsed: serde_json::Value = serde_json::from_str(&json)?;
    assert_eq!(parsed.pointer("/done"), Some(&json!("dragged")), "{json}");
    assert_eq!(
        parsed.pointer("/node/test_id"),
        Some(&json!("build.apply")),
        "{json}"
    );
    Ok(())
}

#[test]
fn drag_refuses_both_destinations_neither_and_a_malformed_offset() {
    let refusal = |arguments: &[&str]| {
        Cli::try_parse_from(["sl-viewer-ctl", "drag", TITLE_BAR].iter().chain(arguments))
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default()
    };
    let both = refusal(&["--onto", APPLY, "--by", "1,2"]);
    assert!(both.contains("cannot be used with"), "{both}");
    let neither = refusal(&[]);
    assert!(
        neither.contains("the following required arguments were not provided"),
        "{neither}"
    );
    for malformed in ["12", "1,two", "1,inf", ","] {
        let refused = refusal(&["--by", malformed]);
        assert!(
            refused.contains("is not") && refused.contains(malformed),
            "{malformed}: {refused}"
        );
    }
}

#[tokio::test]
async fn a_tree_prints_indented_and_cut_to_its_depth() -> Result<(), TestError> {
    let script: Arc<Script> = Arc::new(|_body| {
        Ok(ResponseBody::Snapshot {
            roots: vec![build_window()],
        })
    });
    let (whole, _asked) = run_verb(Arc::clone(&script), &["tree"], false).await?;
    assert_eq!(
        whole,
        "window \"Build\" #floater:build at 0,0 300x400\n  button \"Apply\" key=build-apply \
         #build.apply at 10,20 60x24 [disabled]\n"
    );
    let (cut, _asked) = run_verb(Arc::clone(&script), &["tree", "--depth", "0"], false).await?;
    assert_eq!(
        cut,
        "window \"Build\" #floater:build at 0,0 300x400\n  … 1 more\n"
    );
    let (json, _asked) = run_verb(script, &["--json", "tree", "--depth", "0"], true).await?;
    let parsed: serde_json::Value = serde_json::from_str(&json)?;
    assert_eq!(
        parsed.pointer("/0/children"),
        None,
        "the JSON is cut the same way: {json}"
    );
    Ok(())
}

#[tokio::test]
async fn environment_reads_the_probe_and_prints_the_sky() -> Result<(), TestError> {
    let script: Arc<Script> = Arc::new(|_body| {
        Ok(ResponseBody::Readout {
            readout: ProbeReadout::Environment(EnvironmentReadout {
                sky: Some(SkyReadout {
                    name: "Midday".to_owned(),
                    ambient: [1.0, 0.0, 0.0],
                }),
                local_sky: true,
            }),
        })
    });
    let (printed, asked) = run_verb(Arc::clone(&script), &["environment"], false).await?;
    assert_eq!(
        printed,
        "sky      Midday\nambient  1.000000, 0.000000, 0.000000\nlayer    local\n"
    );
    match asked.as_slice() {
        [
            RequestBody::Read {
                probe: Probe::Environment,
            },
        ] => {}
        other => return Err(format!("asked {other:?}").into()),
    }
    let (json, _asked) = run_verb(script, &["--json", "environment"], true).await?;
    let parsed: serde_json::Value = serde_json::from_str(&json)?;
    assert_eq!(parsed.pointer("/local_sky"), Some(&json!(true)), "{json}");
    Ok(())
}

#[tokio::test]
async fn wait_asks_the_viewer_to_wait_for_the_state() -> Result<(), TestError> {
    let (printed, asked) = run_verb(
        Arc::new(|_body| {
            Ok(ResponseBody::Satisfied {
                nodes: vec![apply_button()],
            })
        }),
        &["wait", APPLY, "--for", "disabled"],
        true,
    )
    .await?;
    let parsed: serde_json::Value = serde_json::from_str(&printed)?;
    assert_eq!(
        parsed.pointer("/0/states"),
        Some(&json!(["disabled"])),
        "{printed}"
    );
    match asked.as_slice() {
        [
            RequestBody::WaitFor {
                locator,
                condition: WaitCondition::Disabled,
                ..
            },
        ] => assert_eq!(*locator, apply_locator()),
        other => return Err(format!("asked {other:?}").into()),
    }
    Ok(())
}

#[tokio::test]
async fn a_failure_names_the_action_in_the_grammar() -> Result<(), TestError> {
    let cli = Cli::try_parse_from(["sl-viewer-ctl", "click", APPLY])?;
    let Command::Verb(verb) = cli.command else {
        return Err("not a verb".into());
    };
    let (viewer, _seen) = scripted(Arc::new(|body| match body {
        RequestBody::Click { locator, .. } => Err(Box::new(AutomationError::NotFound {
            locator: locator.clone(),
        })),
        other => Err(Box::new(AutomationError::InvalidRequest {
            reason: format!("{other:?}"),
        })),
    }))
    .await?;
    let mut printer = Printer::new(Vec::new(), false);
    let error = verbs::run(&viewer, &verb, &mut printer, core::future::pending())
        .await
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(
        error.starts_with(&format!("viewer a: click {APPLY} failed: ")),
        "{error}"
    );
    Ok(())
}

#[tokio::test]
async fn attach_runs_each_line_and_counts_the_failures() -> Result<(), TestError> {
    let (viewer, seen) = scripted(Arc::new(|body| match body {
        RequestBody::Find { .. } => Ok(ResponseBody::Found {
            nodes: vec![apply_button()],
        }),
        RequestBody::Press { .. } => Ok(ResponseBody::Pressed),
        other => Err(Box::new(AutomationError::InvalidRequest {
            reason: format!("{other:?}"),
        })),
    }))
    .await?;
    let script = format!(
        "# a comment, then a blank line\n\nfind '{APPLY}'\nbogus\npress \"Ctrl+A\"\nquit\nfind \
         never\n"
    );
    let mut printer = Printer::new(Vec::new(), false);
    let failures = attach::session(&viewer, script.as_bytes(), &mut printer).await?;
    let printed = String::from_utf8(printer.into_inner())?;
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(failures, 1, "only `bogus` fails: {printed}");
    assert_eq!(
        lines.first().copied(),
        Some("scripted 0.0.0 (pid 7) as Stage a on fake")
    );
    assert_eq!(
        lines.get(1).copied(),
        Some("button \"Apply\" key=build-apply #build.apply at 10,20 60x24 [disabled]")
    );
    assert!(
        lines
            .get(2)
            .is_some_and(|line| line.starts_with("error: error: unrecognized subcommand 'bogus'")),
        "{printed}"
    );
    assert_eq!(lines.last().copied(), Some("pressed Ctrl+A"), "{printed}");
    let asked = seen.lock().map_err(|error| error.to_string())?.len();
    assert_eq!(asked, 3, "hello, find and press — nothing after quit");
    Ok(())
}
