//! Teeth for the in-process transport: several Apps hosted and stepped side
//! by side, each answering under the caller's ids; a wait on one never
//! stalls another; the relay's refusals; a subscription's notifications under
//! the caller's id; and a wait that ends — on an exited viewer, and after the
//! patience — rather than hangs.

use core::time::Duration;

use bevy::prelude::*;
use pretty_assertions::assert_eq;
use sl_automation_proto::{
    AutomationError, Deadline, Locator, LogStream, Notification, Request, RequestBody, RequestId,
    ResponseBody, ViewerMessage, WaitCondition,
};
use sl_viewer_testkit::interact::InteractionTest;
use sl_viewer_testkit::settle;
use sl_viewer_ui_core::ui_element::UiAction;

use super::{InProcessError, InProcessTransport, ViewerHandle};
use crate::executor::{AutomationIdentity, AutomationPlugin};

/// A settled interaction app with the executor, calling itself `viewer`.
fn app(viewer: &str) -> App {
    let mut app = InteractionTest::new().build();
    app.insert_resource(AutomationIdentity {
        viewer: viewer.to_owned(),
        version: "9.9.9".to_owned(),
        grid: None,
        agent_name: None,
    })
    .add_plugins(AutomationPlugin);
    settle(&mut app);
    app
}

/// A transport hosting two such apps, `one` and `two`.
fn pair() -> Result<(InProcessTransport<App>, [ViewerHandle; 2]), InProcessError> {
    let mut transport = InProcessTransport::new();
    let one = transport.host("one", app("one"))?;
    let two = transport.host("two", app("two"))?;
    Ok((transport, [one, two]))
}

/// Request `id` asking `body`.
const fn request(id: u64, body: RequestBody) -> Request {
    Request {
        id: RequestId(id),
        body,
    }
}

/// A wait for a node that never appears, answered only at its deadline.
fn never(frames: u32) -> RequestBody {
    RequestBody::WaitFor {
        locator: Locator::test_id("never-spawned"),
        condition: WaitCondition::Attached,
        deadline: Deadline {
            frames: Some(frames),
            millis: None,
        },
    }
}

/// The viewer a hello answered by `viewer` names.
fn hello_name(
    transport: &mut InProcessTransport<App>,
    viewer: ViewerHandle,
    id: u64,
) -> Result<String, String> {
    let response = transport
        .request(viewer, request(id, RequestBody::Hello))
        .map_err(|error| error.to_string())?;
    assert_eq!(response.id, RequestId(id), "answered under the caller's id");
    match response.result {
        Ok(ResponseBody::Hello { viewer, .. }) => Ok(viewer.viewer),
        other => Err(format!("not a hello: {other:?}")),
    }
}

#[test]
fn each_viewer_answers_for_itself_under_the_same_ids() -> Result<(), String> {
    let (mut transport, [one, two]) = pair().map_err(|error| error.to_string())?;
    assert_eq!(hello_name(&mut transport, one, 1)?, "one");
    assert_eq!(hello_name(&mut transport, two, 1)?, "two");
    assert_eq!(
        hello_name(&mut transport, one, 1)?,
        "one",
        "an id is reusable"
    );
    Ok(())
}

#[test]
fn a_wait_on_one_viewer_steps_the_other() -> Result<(), String> {
    let (mut transport, [one, two]) = pair().map_err(|error| error.to_string())?;
    // A wait on the first that ends at its deadline, a few frames on.
    transport
        .send(one, request(1, never(5)))
        .map_err(|error| error.to_string())?;
    // Answered only if the second is stepped while the caller waits on the
    // first.
    transport
        .send(two, request(1, RequestBody::Hello))
        .map_err(|error| error.to_string())?;
    let timed_out = transport
        .response(one, RequestId(1))
        .map_err(|error| error.to_string())?;
    assert!(
        matches!(timed_out.result, Err(AutomationError::TimedOut { .. })),
        "{timed_out:?}"
    );
    let ViewerMessage::Response(hello) =
        transport.receive(two).map_err(|error| error.to_string())?
    else {
        return Err("the second viewer's first message is not a response".to_owned());
    };
    assert_eq!(hello.id, RequestId(1));
    assert!(matches!(hello.result, Ok(ResponseBody::Hello { .. })));
    Ok(())
}

#[test]
fn an_answer_waited_for_by_id_leaves_the_others_in_order() -> Result<(), String> {
    let (mut transport, [one, _two]) = pair().map_err(|error| error.to_string())?;
    transport
        .send(one, request(1, never(30)))
        .map_err(|error| error.to_string())?;
    transport
        .send(one, request(2, RequestBody::Hello))
        .map_err(|error| error.to_string())?;
    transport
        .send(one, request(3, RequestBody::ReadDiagnostics { cursor: 0 }))
        .map_err(|error| error.to_string())?;
    let first = transport
        .response(one, RequestId(1))
        .map_err(|error| error.to_string())?;
    assert_eq!(first.id, RequestId(1));
    let ids: Vec<RequestId> = core::iter::repeat_with(|| match transport.receive(one) {
        Ok(ViewerMessage::Response(response)) => Ok(response.id),
        other => Err(format!("{other:?}")),
    })
    .take(2)
    .collect::<Result<_, _>>()?;
    assert_eq!(ids, vec![RequestId(2), RequestId(3)]);
    Ok(())
}

#[test]
fn a_duplicate_id_in_flight_is_refused() -> Result<(), String> {
    let (mut transport, [one, _two]) = pair().map_err(|error| error.to_string())?;
    transport
        .send(one, request(4, never(10)))
        .map_err(|error| error.to_string())?;
    transport
        .send(one, request(4, RequestBody::Hello))
        .map_err(|error| error.to_string())?;
    let ViewerMessage::Response(refused) =
        transport.receive(one).map_err(|error| error.to_string())?
    else {
        return Err("not a response".to_owned());
    };
    assert_eq!(refused.id, RequestId(4));
    assert!(
        matches!(refused.result, Err(AutomationError::InvalidRequest { .. })),
        "{refused:?}"
    );
    let original = transport
        .response(one, RequestId(4))
        .map_err(|error| error.to_string())?;
    assert!(
        matches!(original.result, Err(AutomationError::TimedOut { .. })),
        "the first request under the id runs on: {original:?}"
    );
    Ok(())
}

#[test]
fn a_subscription_notifies_under_the_callers_id() -> Result<(), String> {
    let (mut transport, [one, two]) = pair().map_err(|error| error.to_string())?;
    let subscribed = transport
        .request(
            one,
            request(
                9,
                RequestBody::Subscribe {
                    cursor: None,
                    streams: vec![LogStream::UiAction],
                },
            ),
        )
        .map_err(|error| error.to_string())?;
    assert!(
        matches!(subscribed.result, Ok(ResponseBody::Subscribed { .. })),
        "{subscribed:?}"
    );
    for viewer in [one, two] {
        transport
            .app_mut(viewer)
            .ok_or("no app")?
            .world_mut()
            .write_message(UiAction {
                element: "toolbar",
                action: "toggle-inventory",
            });
    }
    let message = transport.receive(one).map_err(|error| error.to_string())?;
    let ViewerMessage::Notification(Notification::Log { subscription, page }) = message else {
        return Err(format!("not a log notification: {message:?}"));
    };
    assert_eq!(subscription, RequestId(9));
    assert_eq!(page.entries.len(), 1, "{page:?}");

    let ended = transport
        .request(
            one,
            request(
                10,
                RequestBody::Unsubscribe {
                    subscription: RequestId(9),
                },
            ),
        )
        .map_err(|error| error.to_string())?;
    assert!(
        matches!(ended.result, Ok(ResponseBody::Unsubscribed)),
        "{ended:?}"
    );
    let unknown = transport
        .request(
            two,
            request(
                1,
                RequestBody::Unsubscribe {
                    subscription: RequestId(9),
                },
            ),
        )
        .map_err(|error| error.to_string())?;
    assert!(
        matches!(unknown.result, Err(AutomationError::InvalidRequest { .. })),
        "the second viewer has no subscription 9: {unknown:?}"
    );
    Ok(())
}

#[test]
fn a_wait_on_an_exited_viewer_fails_and_the_other_runs_on() -> Result<(), String> {
    let (mut transport, [one, two]) = pair().map_err(|error| error.to_string())?;
    transport
        .send(one, request(1, never(100_000)))
        .map_err(|error| error.to_string())?;
    transport
        .app_mut(one)
        .ok_or("no app")?
        .world_mut()
        .write_message(AppExit::Success);
    let exited = transport.response(one, RequestId(1));
    assert!(
        matches!(exited, Err(InProcessError::Exited { ref label, .. }) if label == "one"),
        "{exited:?}"
    );
    assert_eq!(hello_name(&mut transport, two, 1)?, "two");
    Ok(())
}

#[test]
fn a_viewer_that_never_answers_runs_out_the_patience() -> Result<(), String> {
    let (transport, [one, _two]) = pair().map_err(|error| error.to_string())?;
    let mut transport = transport.with_patience(Duration::from_millis(50));
    transport
        .send(one, request(1, never(100_000)))
        .map_err(|error| error.to_string())?;
    let waited = transport.response(one, RequestId(1));
    assert!(
        matches!(waited, Err(InProcessError::NoAnswer { ref label, .. }) if label == "one"),
        "{waited:?}"
    );
    Ok(())
}

#[test]
fn an_app_without_the_executor_is_not_hosted() {
    let mut transport = InProcessTransport::new();
    let hosted = transport.host("bare", InteractionTest::new().build());
    assert_eq!(
        hosted,
        Err(InProcessError::NoExecutor {
            label: "bare".to_owned()
        })
    );
}
