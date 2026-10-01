//! Teeth for the driver against a scripted viewer at the far end of a
//! channel pair (and of a socket): answers reach their callers whatever
//! order they come in, a closed viewer fails what waits, a protocol mismatch
//! is refused, a subscription streams under its id and ends when dropped,
//! and a failed expectation saves its three artifacts and names them.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pretty_assertions::assert_eq;
use sl_automation_proto::{
    AutomationError, Bounds, FailureReport, LogEntry, LogPage, LogStream, NodeId, NodeVisibility,
    Notification, PROTOCOL_VERSION, Request, RequestBody, RequestId, Response, ResponseBody, Role,
    StateCondition, StateObservation, UiNode, ViewerIdentity, ViewerMessage, WaitCondition,
};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::{DriverError, Viewer, ViewerOptions};

/// A viewer's answer to a request body, or `None` to answer nothing.
type Script = dyn Fn(&RequestBody) -> Option<Result<ResponseBody, AutomationError>> + Send + Sync;

/// A hello answer speaking `protocol`.
fn hello(protocol: u32) -> ResponseBody {
    ResponseBody::Hello {
        protocol,
        viewer: ViewerIdentity {
            viewer: "scripted".to_owned(),
            version: "0.0.0".to_owned(),
            pid: 1,
            grid: None,
            agent_name: None,
            agent_id: None,
        },
    }
}

/// A plain OK button.
fn button() -> UiNode {
    UiNode {
        id: NodeId(1),
        role: Role::Button,
        name: Some("OK".to_owned()),
        name_key: Some("button-ok".to_owned()),
        test_id: Some("ok".to_owned()),
        states: BTreeSet::new(),
        value: None,
        level: None,
        accelerator: None,
        bounds: Bounds::default(),
        visibility: NodeVisibility::Visible,
        children: Vec::new(),
    }
}

/// An event log entry of `kind`.
fn entry(seq: u64, kind: &str) -> LogEntry {
    LogEntry {
        seq,
        stream: LogStream::UiAction,
        kind: kind.to_owned(),
        detail: format!("{kind}()"),
    }
}

/// Run a scripted viewer on the far end of a fresh channel pair: each
/// request is answered by `script` (a hello always by one speaking
/// `protocol`); `None` from the script answers nothing. A click's success
/// is held back and sent after the next answer, so answers come out of order.
/// Every request is recorded in `seen`.
fn scripted(
    protocol: u32,
    script: Arc<Script>,
    seen: Arc<Mutex<Vec<RequestBody>>>,
) -> (UnboundedSender<Request>, UnboundedReceiver<ViewerMessage>) {
    let (requests, mut incoming) = unbounded_channel::<Request>();
    let (outgoing, messages) = unbounded_channel();
    drop(tokio::spawn(async move {
        let mut held: Option<Response> = None;
        while let Some(Request { id, body }) = incoming.recv().await {
            if let Ok(mut seen) = seen.lock() {
                seen.push(body.clone());
            }
            let result = match &body {
                RequestBody::Hello => Some(Ok(hello(protocol))),
                other => script(other),
            };
            let Some(result) = result else { continue };
            let report = result.is_err().then(|| FailureReport {
                tree: vec![UiNode {
                    children: vec![button()],
                    ..button()
                }],
                events: vec![entry(4, "toolbar.inventory")],
                diagnostics: Vec::new(),
            });
            if let RequestBody::Subscribe { .. } = &body {
                let _sent = outgoing.send(ViewerMessage::Response(Box::new(Response {
                    id,
                    result,
                    report,
                })));
                let _sent = outgoing.send(ViewerMessage::Notification(Notification::Log {
                    subscription: id,
                    page: LogPage {
                        entries: vec![entry(5, "toolbar.build")],
                        next: 6,
                        dropped: 0,
                    },
                }));
                continue;
            }
            let response = Response { id, result, report };
            if matches!(body, RequestBody::Click { .. }) && response.result.is_ok() {
                held = Some(response);
                continue;
            }
            for response in core::iter::once(response).chain(held.take()) {
                if outgoing
                    .send(ViewerMessage::Response(Box::new(response)))
                    .is_err()
                {
                    return;
                }
            }
        }
    }));
    (requests, messages)
}

/// A viewer handle on a scripted viewer.
async fn viewer(
    options: ViewerOptions,
    script: Arc<Script>,
) -> Result<(Viewer, Arc<Mutex<Vec<RequestBody>>>), DriverError> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (requests, messages) = scripted(PROTOCOL_VERSION, script, Arc::clone(&seen));
    Ok((Viewer::over_link(requests, messages, options).await?, seen))
}

/// A scratch directory of this test's own.
fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("sl-viewer-driver-{name}-{}", std::process::id()))
}

#[tokio::test]
async fn a_hello_names_the_viewer_and_a_mismatched_protocol_is_refused() -> Result<(), String> {
    let (viewer, _seen) = viewer(ViewerOptions::new("one"), Arc::new(|_body| None))
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(viewer.identity().viewer, "scripted");
    assert_eq!(viewer.label(), "one");

    let seen = Arc::new(Mutex::new(Vec::new()));
    let (requests, messages) = scripted(PROTOCOL_VERSION + 1, Arc::new(|_body| None), seen);
    let refused = Viewer::over_link(requests, messages, ViewerOptions::new("two")).await;
    assert!(
        matches!(refused, Err(DriverError::Protocol { found, .. }) if found == PROTOCOL_VERSION + 1),
        "{refused:?}"
    );
    Ok(())
}

#[tokio::test]
async fn answers_reach_their_callers_in_whatever_order_they_come() -> Result<(), String> {
    // The click's answer is held until the find is answered, so the answers
    // arrive in the opposite order to the requests: each caller still gets
    // its own.
    let (viewer, _seen) = viewer(
        ViewerOptions::new("one"),
        Arc::new(|body| match body {
            RequestBody::Click { .. } => Some(Ok(ResponseBody::Done { node: button() })),
            RequestBody::Find { .. } => Some(Ok(ResponseBody::Found {
                nodes: vec![button(), button()],
            })),
            _ => None,
        }),
    )
    .await
    .map_err(|error| error.to_string())?;
    let ok = viewer.ui().button("OK");
    let (clicked, count) = tokio::join!(ok.click(), ok.count());
    assert_eq!(
        clicked
            .map_err(|error| error.to_string())?
            .test_id
            .as_deref(),
        Some("ok")
    );
    assert_eq!(count.map_err(|error| error.to_string())?, 2);
    Ok(())
}

#[tokio::test]
async fn a_closed_viewer_fails_what_waits_on_it() -> Result<(), String> {
    let seen = Arc::new(Mutex::new(Vec::<RequestBody>::new()));
    let (requests, mut incoming) = unbounded_channel::<Request>();
    let (outgoing, messages) = unbounded_channel();
    let answering = tokio::spawn(async move {
        // Answer the hello, then close on the next request.
        if let Some(Request { id, .. }) = incoming.recv().await {
            let _sent = outgoing.send(ViewerMessage::Response(Box::new(Response {
                id,
                result: Ok(hello(PROTOCOL_VERSION)),
                report: None,
            })));
        }
        let _next = incoming.recv().await;
        drop(outgoing);
        drop(seen);
    });
    let viewer = Viewer::over_link(requests, messages, ViewerOptions::new("one"))
        .await
        .map_err(|error| error.to_string())?;
    let read = viewer.agent().await;
    assert!(
        matches!(&read, Err(DriverError::Closed { viewer, .. }) if viewer == "one"),
        "{read:?}"
    );
    answering.await.map_err(|error| error.to_string())?;
    let again = viewer.agent().await;
    assert!(
        matches!(again, Err(DriverError::Closed { .. })),
        "a closed connection refuses at once: {again:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_viewer_that_stops_answering_runs_out_the_grace() -> Result<(), String> {
    let mut options = ViewerOptions::new("one");
    options.grace = Duration::from_millis(50);
    let (viewer, _seen) = viewer(options, Arc::new(|_body| None))
        .await
        .map_err(|error| error.to_string())?;
    let read = viewer.agent().await;
    assert!(
        matches!(read, Err(DriverError::NoAnswer { ref what, .. }) if what == "read"),
        "{read:?}"
    );
    Ok(())
}

/// A cursor from the start waits from sequence number 0 — so it sees what the
/// viewer logged before the wait began — for an entry of one kind whose
/// detail contains the part asked for, and moves past what it found.
#[tokio::test]
async fn a_wait_from_the_start_matches_kind_and_detail() -> Result<(), String> {
    let (viewer, seen) = viewer(
        ViewerOptions::new("one"),
        Arc::new(|body| match body {
            RequestBody::WaitForState { .. } => Some(Ok(ResponseBody::StateHeld {
                observed: StateObservation::Logged {
                    entry: entry(3, "GenericMessage"),
                    next: 4,
                },
            })),
            _ => None,
        }),
    )
    .await
    .map_err(|error| error.to_string())?;
    let mut cursor = viewer.events_from_start();
    assert_eq!(cursor.position(), 0);
    let found = cursor
        .wait_for_containing("GenericMessage", "params: [\"go\"]", Duration::from_secs(5))
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(found.seq, 3);
    assert_eq!(cursor.position(), 4, "the cursor moves past the entry");
    let seen = seen.lock().map_err(|error| error.to_string())?.clone();
    assert!(
        seen.iter().any(|body| matches!(
            body,
            RequestBody::WaitForState {
                condition: StateCondition::Logged {
                    cursor: 0,
                    kind_is: Some(kind),
                    detail_contains: Some(part),
                    ..
                },
                ..
            } if kind == "GenericMessage" && part == "params: [\"go\"]"
        )),
        "{seen:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_subscription_streams_under_its_id_and_ends_when_dropped() -> Result<(), String> {
    let (viewer, seen) = viewer(
        ViewerOptions::new("one"),
        Arc::new(|body| match body {
            RequestBody::Subscribe { .. } => Some(Ok(ResponseBody::Subscribed { cursor: 5 })),
            RequestBody::Unsubscribe { .. } => Some(Ok(ResponseBody::Unsubscribed)),
            _ => None,
        }),
    )
    .await
    .map_err(|error| error.to_string())?;
    let mut stream = viewer
        .subscribe(&[LogStream::UiAction])
        .await
        .map_err(|error| error.to_string())?;
    let page = stream.next().await.ok_or("the stream ended")?;
    assert_eq!(page.entries, vec![entry(5, "toolbar.build")]);
    drop(stream);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let seen = seen.lock().map_err(|error| error.to_string())?.clone();
    assert!(
        matches!(
            seen.last(),
            Some(RequestBody::Unsubscribe { subscription }) if *subscription == RequestId(2)
        ),
        "the dropped stream ended its subscription: {seen:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_failed_expectation_saves_its_artifacts_and_names_them() -> Result<(), String> {
    let dir = scratch("artifacts");
    let (viewer, _seen) = viewer(
        ViewerOptions::new("one").with_artifacts(&dir),
        Arc::new(|body| match body {
            RequestBody::WaitFor {
                locator, condition, ..
            } => Some(Err(AutomationError::TimedOut {
                locator: locator.clone(),
                condition: Some(condition.clone()),
                failed_check: None,
                last_observed: vec![button()],
                frames: 3,
                millis: 50,
            })),
            RequestBody::Screenshot { path, .. } => Some(
                fs_err::write(path, b"\x89PNG")
                    .map(|()| ResponseBody::Screenshot {
                        path: path.clone(),
                        width: 1,
                        height: 1,
                        outlined: Vec::new(),
                    })
                    .map_err(|error| AutomationError::ScreenshotFailed {
                        reason: error.to_string(),
                    }),
            ),
            _ => None,
        }),
    )
    .await
    .map_err(|error| error.to_string())?;
    let ok = viewer.ui().window("preferences").button_key("button-ok");
    let failed = viewer.expect(&ok).to_be_disabled().await;
    let Err(error) = failed else {
        return Err("the expectation held".to_owned());
    };
    assert!(
        matches!(
            error.automation_error(),
            Some(AutomationError::TimedOut {
                condition: Some(WaitCondition::Disabled),
                ..
            })
        ),
        "{error}"
    );
    let failure = error.failure().ok_or("not a failure")?;
    let message = error.to_string();
    for (artifact, name) in [
        (&failure.artifacts.screenshot, "screenshot.png"),
        (&failure.artifacts.tree, "tree.txt"),
        (&failure.artifacts.events, "events.txt"),
    ] {
        let path = artifact
            .clone()
            .ok_or_else(|| format!("no {name}"))?
            .map_err(|reason| format!("{name} not saved: {reason}"))?;
        assert!(path.ends_with(name), "{}", path.display());
        assert!(path.is_file(), "{} exists", path.display());
        assert!(
            message.contains(&path.display().to_string()),
            "the message names {name}: {message}"
        );
    }
    let tree_path = failure
        .artifacts
        .tree
        .clone()
        .ok_or("no tree")?
        .map_err(|reason| reason.clone())?;
    let tree = fs_err::read_to_string(tree_path).map_err(|error| error.to_string())?;
    assert!(
        tree.contains("  button \"OK\" key=button-ok #ok"),
        "the report's excerpt, indented: {tree}"
    );
    let events_path = failure
        .artifacts
        .events
        .clone()
        .ok_or("no events")?
        .map_err(|reason| reason.clone())?;
    let events = fs_err::read_to_string(events_path).map_err(|error| error.to_string())?;
    assert!(events.contains("toolbar.inventory"), "{events}");
    assert!(
        message.starts_with(
            "viewer one: expect window[test_id=floater:preferences] >> \
             button[name_key=button-ok] to be disabled failed: timed out"
        ),
        "{message}"
    );
    fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn a_failure_without_an_artifact_directory_saves_nothing() -> Result<(), String> {
    let (viewer, seen) = viewer(
        ViewerOptions::new("one"),
        Arc::new(|body| match body {
            RequestBody::Click { locator, .. } => Some(Err(AutomationError::NotFound {
                locator: locator.clone(),
            })),
            _ => None,
        }),
    )
    .await
    .map_err(|error| error.to_string())?;
    let error = viewer
        .ui()
        .test_id("nowhere")
        .click()
        .await
        .err()
        .ok_or("the click succeeded")?;
    assert!(
        error.to_string().contains("no artifact directory"),
        "{error}"
    );
    let seen = seen.lock().map_err(|error| error.to_string())?.clone();
    assert!(
        !seen
            .iter()
            .any(|body| matches!(body, RequestBody::Screenshot { .. })),
        "no screenshot was asked for: {seen:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_viewer_is_driven_over_a_socket_the_same_way() -> Result<(), String> {
    let dir = scratch("socket");
    fs_err::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("viewer.sock");
    let listener = tokio::net::UnixListener::bind(&path).map_err(|error| error.to_string())?;
    let serving = tokio::spawn(async move {
        let Ok((stream, _address)) = listener.accept().await else {
            return;
        };
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(Request { id, body }) = serde_json::from_str::<Request>(&line) else {
                return;
            };
            let result = match body {
                RequestBody::Hello => Ok(hello(PROTOCOL_VERSION)),
                RequestBody::Click { .. } => Ok(ResponseBody::Done { node: button() }),
                _ => Err(AutomationError::InvalidRequest {
                    reason: "not scripted".to_owned(),
                }),
            };
            let Ok(mut answer) =
                serde_json::to_string(&ViewerMessage::Response(Box::new(Response {
                    id,
                    result,
                    report: None,
                })))
            else {
                return;
            };
            answer.push('\n');
            if writer.write_all(answer.as_bytes()).await.is_err() {
                return;
            }
        }
    });
    let viewer = Viewer::connect(&path, ViewerOptions::new("remote"))
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(viewer.identity().viewer, "scripted");
    let node = viewer
        .ui()
        .button("OK")
        .click()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(node.test_id.as_deref(), Some("ok"));
    drop(viewer);
    serving.await.map_err(|error| error.to_string())?;
    fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())?;

    let missing = Viewer::connect(&dir.join("gone.sock"), ViewerOptions::new("gone")).await;
    assert!(
        matches!(missing, Err(DriverError::Connect { .. })),
        "{missing:?}"
    );
    Ok(())
}

/// A file dialog is answered with an absolute path — a relative one is made
/// absolute against the test's directory, as a screenshot path is — or
/// cancelled with none, and the answer names the dialog.
#[tokio::test]
async fn a_file_dialog_is_answered_with_an_absolute_path_or_cancelled() -> Result<(), String> {
    let (viewer, seen) = viewer(
        ViewerOptions::new("one"),
        Arc::new(|body| match body {
            RequestBody::AnswerFileDialog { .. } => Some(Ok(ResponseBody::FileDialogAnswered {
                purpose: "settings-editor-import-sky".to_owned(),
                title: "Import".to_owned(),
                folder: false,
            })),
            _ => None,
        }),
    )
    .await
    .map_err(|error| error.to_string())?;
    let answered = viewer
        .answer_file_dialog(Some(std::path::Path::new("skies/Dawn.xml")))
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(answered.purpose, "settings-editor-import-sky");
    let _cancelled = viewer
        .answer_file_dialog(None)
        .await
        .map_err(|error| error.to_string())?;
    let paths: Vec<Option<String>> = seen
        .lock()
        .map_err(|error| error.to_string())?
        .iter()
        .filter_map(|body| match body {
            RequestBody::AnswerFileDialog { path, .. } => Some(path.clone()),
            _ => None,
        })
        .collect();
    let expected = std::path::absolute("skies/Dawn.xml")
        .map_err(|error| error.to_string())?
        .display()
        .to_string();
    assert_eq!(paths, vec![Some(expected), None]);
    Ok(())
}
