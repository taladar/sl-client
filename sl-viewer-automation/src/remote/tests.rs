//! Teeth for the remote transport: the socket file's safety (private, stale
//! ones replaced, live ones refused, removed on drop), and the protocol over
//! it — a hello answered, two clients with the same ids kept apart, a
//! subscription streaming the event log until it is ended, a bad line
//! refused without closing the connection, and a half-closed client still
//! answered.

use std::io::{BufRead as _, BufReader, ErrorKind, Write as _};
use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener as StdUnixListener, UnixStream as StdUnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bevy::prelude::*;
use pretty_assertions::assert_eq;
use sl_automation_proto::{
    AutomationError, LogStream, Notification, PROTOCOL_VERSION, RequestBody, RequestId,
    ResponseBody, ViewerMessage,
};
use sl_viewer_testkit::interact::InteractionTest;
use sl_viewer_testkit::settle;
use sl_viewer_ui_core::ui_element::UiAction;

use super::{RemoteAutomationPlugin, RemoteEndpoint, SocketError};
use crate::executor::AutomationIdentity;

/// The most frames a test waits for a line.
const PATIENCE: u32 = 2000;

/// A fresh, private scratch directory for one test's sockets.
fn scratch(test: &str) -> Result<PathBuf, String> {
    static COUNT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "sl-automation-remote-{test}-{}-{}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    ));
    fs_err::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

/// An interaction app serving the automation protocol at `path`.
fn app(path: &Path) -> Result<App, String> {
    let endpoint = RemoteEndpoint::open(path).map_err(|error| error.to_string())?;
    let mut app = InteractionTest::new().build();
    app.insert_resource(AutomationIdentity {
        viewer: "test-viewer".to_owned(),
        version: "9.9.9".to_owned(),
        grid: Some("fake".to_owned()),
        agent_name: Some("Test Avatar".to_owned()),
    })
    .add_plugins(RemoteAutomationPlugin::new(endpoint));
    settle(&mut app);
    Ok(app)
}

/// A client of the socket, reading without blocking the frame loop.
struct Client {
    /// Where it writes.
    stream: StdUnixStream,
    /// Where it reads.
    reader: BufReader<StdUnixStream>,
    /// A line read in part.
    partial: String,
}

impl Client {
    /// Connect to `path`.
    fn connect(path: &Path) -> Result<Self, String> {
        let stream = StdUnixStream::connect(path).map_err(|error| error.to_string())?;
        stream
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let reader = BufReader::new(stream.try_clone().map_err(|error| error.to_string())?);
        Ok(Self {
            stream,
            reader,
            partial: String::new(),
        })
    }

    /// Write `line` and its newline.
    fn write(&mut self, line: &str) -> Result<(), String> {
        self.stream
            .write_all(format!("{line}\n").as_bytes())
            .map_err(|error| error.to_string())
    }

    /// A whole line, if one has arrived.
    fn poll(&mut self) -> Result<Option<ViewerMessage>, String> {
        match self.reader.read_line(&mut self.partial) {
            Ok(0) => Err("the viewer closed the connection".to_owned()),
            Ok(_read) if self.partial.ends_with('\n') => {
                let line = core::mem::take(&mut self.partial);
                serde_json::from_str(&line)
                    .map(Some)
                    .map_err(|error| format!("{error}: {line}"))
            }
            Ok(_read) => Ok(None),
            Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Run frames until a line arrives.
    fn next(&mut self, app: &mut App) -> Result<ViewerMessage, String> {
        for _frame in 0..PATIENCE {
            if let Some(message) = self.poll()? {
                return Ok(message);
            }
            app.update();
        }
        Err("no line arrived".to_owned())
    }

    /// Run `frames` frames and check that no line arrives.
    fn quiet(&mut self, app: &mut App, frames: u32) -> Result<(), String> {
        for _frame in 0..frames {
            app.update();
            if let Some(message) = self.poll()? {
                return Err(format!("an unexpected line: {message:?}"));
            }
        }
        Ok(())
    }
}

/// The response in `message`: its id and what came of it.
fn response(
    message: ViewerMessage,
) -> Result<(u64, Result<ResponseBody, AutomationError>), String> {
    match message {
        ViewerMessage::Response(response) => Ok((response.id.0, response.result)),
        ViewerMessage::Notification(notification) => {
            Err(format!("a notification, not a response: {notification:?}"))
        }
    }
}

/// Log a toolbar UI action, which the event log records as
/// `toolbar.<action>`.
fn act(app: &mut App, action: &'static str) {
    app.world_mut().write_message(UiAction {
        element: "toolbar",
        action,
    });
}

#[test]
fn the_socket_is_private_and_goes_with_its_endpoint() -> Result<(), String> {
    let dir = scratch("private")?;
    let path = dir.join("viewer.sock");
    let endpoint = RemoteEndpoint::open(&path).map_err(|error| error.to_string())?;
    let metadata = fs_err::symlink_metadata(&path).map_err(|error| error.to_string())?;
    assert!(metadata.file_type().is_socket());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    assert_eq!(endpoint.path(), path);
    let entries: Vec<_> = fs_err::read_dir(&dir)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(entries, vec!["viewer.sock"], "no staging left behind");
    drop(endpoint);
    assert!(
        !path.exists(),
        "the socket file is removed with its endpoint"
    );
    fs_err::remove_dir(&dir).map_err(|error| error.to_string())
}

#[test]
fn a_live_socket_is_refused_a_stale_one_replaced_and_a_file_left_alone() -> Result<(), String> {
    let dir = scratch("stale")?;
    let path = dir.join("viewer.sock");
    let first = RemoteEndpoint::open(&path).map_err(|error| error.to_string())?;
    assert!(
        matches!(RemoteEndpoint::open(&path), Err(SocketError::InUse { .. })),
        "a second viewer may not take a live socket"
    );
    assert!(path.exists(), "the refusal leaves the live socket alone");
    drop(first);

    let stale = dir.join("stale.sock");
    drop(StdUnixListener::bind(&stale).map_err(|error| error.to_string())?);
    assert!(stale.exists(), "a crashed viewer's socket file");
    let replaced = RemoteEndpoint::open(&stale).map_err(|error| error.to_string())?;
    StdUnixStream::connect(&stale).map_err(|error| format!("the new socket answers: {error}"))?;
    drop(replaced);

    let file = dir.join("notes.txt");
    fs_err::write(&file, "not a socket").map_err(|error| error.to_string())?;
    assert!(matches!(
        RemoteEndpoint::open(&file),
        Err(SocketError::NotASocket { .. })
    ));
    assert_eq!(
        fs_err::read_to_string(&file).map_err(|error| error.to_string())?,
        "not a socket"
    );
    fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())
}

#[test]
fn a_hello_names_the_protocol_and_the_viewer() -> Result<(), String> {
    let dir = scratch("hello")?;
    let path = dir.join("viewer.sock");
    let mut app = app(&path)?;
    let mut client = Client::connect(&path)?;
    client.write(r#"{"id":7,"method":"hello"}"#)?;
    let (id, result) = response(client.next(&mut app)?)?;
    assert_eq!(id, 7);
    let Ok(ResponseBody::Hello { protocol, viewer }) = result else {
        return Err(format!("not a hello: {result:?}"));
    };
    assert_eq!(protocol, PROTOCOL_VERSION);
    assert_eq!(viewer.viewer, "test-viewer");
    assert_eq!(viewer.version, "9.9.9");
    assert_eq!(viewer.pid, std::process::id());
    assert_eq!(viewer.grid.as_deref(), Some("fake"));
    assert_eq!(viewer.agent_name.as_deref(), Some("Test Avatar"));
    assert_eq!(viewer.agent_id, None, "not logged in");
    drop(app);
    assert!(!path.exists(), "the socket goes with the app");
    fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())
}

#[test]
fn two_clients_may_use_the_same_ids() -> Result<(), String> {
    let dir = scratch("clients")?;
    let path = dir.join("viewer.sock");
    let mut app = app(&path)?;
    let mut first = Client::connect(&path)?;
    let mut second = Client::connect(&path)?;
    first.write(r#"{"id":1,"method":"hello"}"#)?;
    second.write(r#"{"id":1,"method":"read_diagnostics"}"#)?;
    let (id, result) = response(first.next(&mut app)?)?;
    assert_eq!(id, 1);
    assert!(
        matches!(result, Ok(ResponseBody::Hello { .. })),
        "{result:?}"
    );
    let (id, result) = response(second.next(&mut app)?)?;
    assert_eq!(id, 1);
    assert!(
        matches!(result, Ok(ResponseBody::Diagnostics { .. })),
        "{result:?}"
    );
    first.quiet(&mut app, 5)?;
    fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())
}

#[test]
fn a_subscription_streams_the_log_until_it_is_ended() -> Result<(), String> {
    let dir = scratch("subscribe")?;
    let path = dir.join("viewer.sock");
    let mut app = app(&path)?;
    let mut client = Client::connect(&path)?;
    let subscribe = serde_json::to_string(&sl_automation_proto::Request {
        id: RequestId(3),
        body: RequestBody::Subscribe {
            cursor: None,
            streams: vec![LogStream::UiAction],
        },
    })
    .map_err(|error| error.to_string())?;
    client.write(&subscribe)?;
    let (id, result) = response(client.next(&mut app)?)?;
    assert_eq!(id, 3);
    assert!(
        matches!(result, Ok(ResponseBody::Subscribed { .. })),
        "{result:?}"
    );

    act(&mut app, "first");
    act(&mut app, "second");
    let mut kinds = Vec::new();
    while kinds.len() < 2 {
        match client.next(&mut app)? {
            ViewerMessage::Notification(Notification::Log { subscription, page }) => {
                assert_eq!(subscription, RequestId(3), "under the client's id");
                assert_eq!(page.dropped, 0);
                kinds.extend(page.entries.into_iter().map(|entry| {
                    assert_eq!(entry.stream, LogStream::UiAction);
                    entry.kind
                }));
            }
            other => return Err(format!("not a log notification: {other:?}")),
        }
    }
    assert_eq!(kinds, ["toolbar.first", "toolbar.second"]);

    client.write(r#"{"id":4,"method":"unsubscribe","subscription":3}"#)?;
    let (id, result) = response(client.next(&mut app)?)?;
    assert_eq!(id, 4);
    assert_eq!(result, Ok(ResponseBody::Unsubscribed));
    act(&mut app, "third");
    client.quiet(&mut app, 10)?;

    client.write(r#"{"id":5,"method":"unsubscribe","subscription":3}"#)?;
    let (id, result) = response(client.next(&mut app)?)?;
    assert_eq!(id, 5);
    assert!(
        matches!(result, Err(AutomationError::InvalidRequest { .. })),
        "{result:?}"
    );
    fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())
}

#[test]
fn a_departed_client_takes_its_subscriptions_with_it() -> Result<(), String> {
    let dir = scratch("departed")?;
    let path = dir.join("viewer.sock");
    let mut app = app(&path)?;
    let mut client = Client::connect(&path)?;
    client.write(r#"{"id":1,"method":"subscribe"}"#)?;
    let (_id, result) = response(client.next(&mut app)?)?;
    assert!(
        matches!(result, Ok(ResponseBody::Subscribed { .. })),
        "{result:?}"
    );
    assert_eq!(
        app.world()
            .resource::<RemoteEndpoint>()
            .relay
            .subscription_count(),
        1
    );
    drop(client);
    for _frame in 0..PATIENCE {
        app.update();
        let endpoint = app.world().resource::<RemoteEndpoint>();
        if endpoint.outbound.is_empty() && endpoint.relay.is_idle() {
            // Nothing piles up in the queue for the subscription either.
            act(&mut app, "after");
            app.update();
            app.update();
            let queue = app.world().resource::<crate::AutomationQueue>();
            assert!(queue.is_idle(), "{queue:?}");
            return fs_err::remove_dir_all(&dir).map_err(|error| error.to_string());
        }
    }
    Err("the departed client's connection and subscription were never dropped".to_owned())
}

#[test]
fn a_bad_line_is_refused_and_the_connection_stays() -> Result<(), String> {
    let dir = scratch("refused")?;
    let path = dir.join("viewer.sock");
    let mut app = app(&path)?;
    let mut client = Client::connect(&path)?;
    client.write("this is not json")?;
    let rejected = client.next(&mut app)?;
    assert!(
        matches!(
            rejected,
            ViewerMessage::Notification(Notification::Rejected { .. })
        ),
        "{rejected:?}"
    );
    client.write(r#"{"id":9,"method":"tap"}"#)?;
    let (id, result) = response(client.next(&mut app)?)?;
    assert_eq!(id, 9);
    assert!(
        matches!(result, Err(AutomationError::InvalidRequest { .. })),
        "{result:?}"
    );
    client.write("")?;
    client.write(r#"{"id":10,"method":"hello"}"#)?;
    let (id, result) = response(client.next(&mut app)?)?;
    assert_eq!(id, 10, "the blank line was ignored, the connection kept");
    assert!(
        matches!(result, Ok(ResponseBody::Hello { .. })),
        "{result:?}"
    );
    fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())
}

#[test]
fn a_request_already_in_flight_is_refused() -> Result<(), String> {
    let dir = scratch("in-flight")?;
    let path = dir.join("viewer.sock");
    let mut app = app(&path)?;
    let mut client = Client::connect(&path)?;
    // A wait that cannot hold before its deadline keeps id 2 in flight.
    let wait = r#"{"id":2,"method":"wait_for_state","condition":{"kind":"logged","cursor":0,"kind_is":"never"},"deadline":{"frames":60}}"#;
    client.write(wait)?;
    client.write(r#"{"id":2,"method":"hello"}"#)?;
    let (id, result) = response(client.next(&mut app)?)?;
    assert_eq!(id, 2);
    assert!(
        matches!(
            &result,
            Err(AutomationError::InvalidRequest { reason }) if reason.contains("in flight")
        ),
        "the second request 2 is refused: {result:?}"
    );
    let (id, result) = response(client.next(&mut app)?)?;
    assert_eq!(id, 2);
    assert!(
        matches!(result, Err(AutomationError::StateTimedOut { .. })),
        "the first request 2 runs to its end: {result:?}"
    );
    fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())
}

#[test]
fn a_half_closed_client_still_gets_its_answer() -> Result<(), String> {
    let dir = scratch("half-closed")?;
    let path = dir.join("viewer.sock");
    let mut app = app(&path)?;
    let mut client = Client::connect(&path)?;
    client.write(r#"{"id":1,"method":"hello"}"#)?;
    client
        .stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|error| error.to_string())?;
    let (id, result) = response(client.next(&mut app)?)?;
    assert_eq!(id, 1);
    assert!(
        matches!(result, Ok(ResponseBody::Hello { .. })),
        "{result:?}"
    );
    for _frame in 0..PATIENCE {
        app.update();
        match client.poll() {
            Err(closed) if closed.contains("closed") => {
                return fs_err::remove_dir_all(&dir).map_err(|error| error.to_string());
            }
            Err(error) => return Err(error),
            Ok(Some(message)) => return Err(format!("an unexpected line: {message:?}")),
            Ok(None) => {}
        }
    }
    Err("the viewer never closed the finished connection".to_owned())
}
