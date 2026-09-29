//! Teeth for the host thread: viewers built there answer over their links
//! under the caller's ids, keep running while nobody waits, can be reached
//! between frames, and close their links when they exit.

use bevy::prelude::*;
use pretty_assertions::assert_eq;
use sl_automation_proto::{Request, RequestBody, RequestId, ResponseBody, ViewerMessage};
use sl_viewer_testkit::interact::InteractionTest;
use sl_viewer_testkit::settle;

use super::{HostError, InProcessHost, ViewerLink};
use crate::executor::{AutomationIdentity, AutomationPlugin};
use crate::in_process::InProcessError;

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

/// The viewer name the next hello answered on `link` under `id` names.
async fn hello(link: &mut ViewerLink, id: u64) -> Result<String, String> {
    link.requests
        .send(Request {
            id: RequestId(id),
            body: RequestBody::Hello,
        })
        .map_err(|error| error.to_string())?;
    match link.messages.recv().await {
        Some(ViewerMessage::Response(response)) => {
            assert_eq!(response.id, RequestId(id), "answered under the caller's id");
            match response.result {
                Ok(ResponseBody::Hello { viewer, .. }) => Ok(viewer.viewer),
                other => Err(format!("not a hello: {other:?}")),
            }
        }
        other => Err(format!("not a response: {other:?}")),
    }
}

#[tokio::test]
async fn viewers_built_on_the_host_answer_over_their_links() -> Result<(), String> {
    let host = InProcessHost::<App>::start().map_err(|error| error.to_string())?;
    let (_one, mut one) = host
        .host("one", || Ok(app("one")))
        .await
        .map_err(|error| error.to_string())?;
    let (_two, mut two) = host
        .host("two", || Ok(app("two")))
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(hello(&mut one, 1).await?, "one");
    assert_eq!(hello(&mut two, 1).await?, "two");
    host.stop().map_err(|error| error.to_string())?;
    assert!(
        one.messages.recv().await.is_none(),
        "a stopped host closes every link"
    );
    Ok(())
}

#[tokio::test]
async fn a_viewer_runs_while_nobody_waits_and_is_reached_between_frames() -> Result<(), String> {
    let host = InProcessHost::<App>::start().map_err(|error| error.to_string())?;
    let (viewer, _link) = host
        .host("one", || Ok(app("one")))
        .await
        .map_err(|error| error.to_string())?;
    let first = host
        .with_app(viewer, |app| app.world().resource::<Time>().elapsed())
        .await
        .map_err(|error| error.to_string())?;
    tokio::time::sleep(core::time::Duration::from_millis(100)).await;
    let later = host
        .with_app(viewer, |app| app.world().resource::<Time>().elapsed())
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        later > first,
        "stepped with no request in flight: {first:?} → {later:?}"
    );
    Ok(())
}

#[tokio::test]
async fn an_exited_viewer_closes_its_link_after_its_last_answer() -> Result<(), String> {
    let host = InProcessHost::<App>::start().map_err(|error| error.to_string())?;
    let (one, mut link) = host
        .host("one", || Ok(app("one")))
        .await
        .map_err(|error| error.to_string())?;
    let (_two, mut other) = host
        .host("two", || Ok(app("two")))
        .await
        .map_err(|error| error.to_string())?;
    host.with_app(one, |app| {
        app.world_mut().write_message(AppExit::Success);
    })
    .await
    .map_err(|error| error.to_string())?;
    host.exited(one).await.map_err(|error| error.to_string())?;
    assert!(link.messages.recv().await.is_none(), "the link is closed");
    assert_eq!(hello(&mut other, 1).await?, "two", "the other runs on");
    Ok(())
}

#[tokio::test]
async fn a_failed_build_and_an_app_without_the_executor_are_refused() -> Result<(), String> {
    let host = InProcessHost::<App>::start().map_err(|error| error.to_string())?;
    let failed = host.host("broken", || Err("no GPU".into())).await;
    assert!(
        matches!(&failed, Err(HostError::Build { label, .. }) if label == "broken"),
        "{failed:?}"
    );
    let bare = host
        .host("bare", || Ok(InteractionTest::new().build()))
        .await;
    assert!(
        matches!(
            &bare,
            Err(HostError::Transport(InProcessError::NoExecutor { label })) if label == "bare"
        ),
        "{bare:?}"
    );
    Ok(())
}
