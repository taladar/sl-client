//! [`Connection`]: one client's side of a viewer's automation channel —
//! whichever transport carries it. A transport is a pair of channels:
//! requests go out on one, and every response and notification the viewer
//! sends comes back on the other. The socket is bridged to such a pair
//! ([`Connection::connect`]); an in-process viewer's host hands one out
//! directly ([`Connection::over`]).
//!
//! The connection numbers its requests, routes each response to the call
//! waiting for it and each subscription's entries to its stream, and — once
//! the viewer's side closes — fails whatever is still waiting.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use sl_automation_proto::{
    LogEntry, LogPage, Notification, Request, RequestBody, RequestId, Response, ViewerMessage,
};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::warn;

use crate::error::DriverError;

/// What the connection keeps between the calls and the dispatcher.
#[derive(Debug, Default)]
struct Routes {
    /// The calls waiting for an answer, by request id.
    pending: HashMap<RequestId, oneshot::Sender<Response>>,
    /// The running subscriptions' streams, by the id that started each.
    subscriptions: HashMap<RequestId, UnboundedSender<LogPage<LogEntry>>>,
    /// Whether the viewer's side has closed.
    closed: bool,
}

/// The routes, locked; a poisoned lock is taken as is — the routes stay
/// consistent whatever panicked while holding them.
fn lock(routes: &Mutex<Routes>) -> MutexGuard<'_, Routes> {
    routes.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One client's side of a viewer's automation channel.
#[derive(Debug)]
pub(crate) struct Connection {
    /// The viewer, by its label, for errors.
    label: String,
    /// Where requests go.
    requests: UnboundedSender<Request>,
    /// The routes, shared with the dispatcher.
    routes: Arc<Mutex<Routes>>,
    /// The id the next request gets.
    next_id: AtomicU64,
    /// The dispatcher, stopped on drop.
    dispatcher: JoinHandle<()>,
    /// The socket bridge, stopped on drop; `None` over a link.
    bridge: Option<JoinHandle<()>>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.dispatcher.abort();
        if let Some(bridge) = &self.bridge {
            bridge.abort();
        }
    }
}

impl Connection {
    /// A connection over a channel pair: `requests` to the viewer, and
    /// `messages` from it. Must be called inside a Tokio runtime.
    pub(crate) fn over(
        label: String,
        requests: UnboundedSender<Request>,
        messages: UnboundedReceiver<ViewerMessage>,
    ) -> Self {
        let routes = Arc::new(Mutex::new(Routes::default()));
        let dispatcher = tokio::spawn(dispatch(messages, Arc::clone(&routes)));
        Self {
            label,
            requests,
            routes,
            next_id: AtomicU64::new(1),
            dispatcher,
            bridge: None,
        }
    }

    /// A connection to the automation socket at `path`: one line of JSON per
    /// request out, one per message in.
    ///
    /// # Errors
    ///
    /// [`DriverError::Connect`] when the socket cannot be reached.
    pub(crate) async fn connect(label: String, path: &Path) -> Result<Self, DriverError> {
        let stream = UnixStream::connect(path)
            .await
            .map_err(|source| DriverError::Connect {
                path: path.to_owned(),
                source,
            })?;
        let (requests, requests_out) = unbounded_channel();
        let (messages_in, messages) = unbounded_channel();
        let bridge = tokio::spawn(bridge(stream, requests_out, messages_in, label.clone()));
        let mut connection = Self::over(label, requests, messages);
        connection.bridge = Some(bridge);
        Ok(connection)
    }

    /// The viewer's label.
    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    /// A fresh request id.
    fn allocate(&self) -> RequestId {
        RequestId(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Ask `body` and wait for the answer, at most `patience`.
    ///
    /// # Errors
    ///
    /// [`DriverError::Closed`] when the connection closes first, and
    /// [`DriverError::NoAnswer`] after the patience.
    pub(crate) async fn request(
        &self,
        body: RequestBody,
        patience: Duration,
    ) -> Result<Response, DriverError> {
        let what = method(&body);
        let id = self.allocate();
        let (reply, answer) = oneshot::channel();
        {
            let mut routes = lock(&self.routes);
            if routes.closed {
                return Err(self.closed(what));
            }
            let _previous = routes.pending.insert(id, reply);
        }
        if self.requests.send(Request { id, body }).is_err() {
            let _gone = lock(&self.routes).pending.remove(&id);
            return Err(self.closed(what));
        }
        match tokio::time::timeout(patience, answer).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_dropped)) => Err(self.closed(what)),
            Err(_elapsed) => {
                let _gone = lock(&self.routes).pending.remove(&id);
                Err(DriverError::NoAnswer {
                    viewer: self.label.clone(),
                    what,
                    waited: patience,
                })
            }
        }
    }

    /// Start a subscription with `body` (a [`RequestBody::Subscribe`]): its
    /// entries arrive on the returned receiver, from before the answer comes.
    ///
    /// # Errors
    ///
    /// As [`request`](Self::request).
    pub(crate) async fn subscribe(
        &self,
        body: RequestBody,
        patience: Duration,
    ) -> Result<(RequestId, Response, UnboundedReceiver<LogPage<LogEntry>>), DriverError> {
        let what = method(&body);
        let id = self.allocate();
        let (reply, answer) = oneshot::channel();
        let (pages, stream) = unbounded_channel();
        {
            let mut routes = lock(&self.routes);
            if routes.closed {
                return Err(self.closed(what));
            }
            let _previous = routes.pending.insert(id, reply);
            let _previous = routes.subscriptions.insert(id, pages);
        }
        let forget = || {
            let mut routes = lock(&self.routes);
            let _gone = routes.pending.remove(&id);
            let _gone = routes.subscriptions.remove(&id);
        };
        if self.requests.send(Request { id, body }).is_err() {
            forget();
            return Err(self.closed(what));
        }
        match tokio::time::timeout(patience, answer).await {
            Ok(Ok(response)) => {
                if response.result.is_err() {
                    forget();
                }
                Ok((id, response, stream))
            }
            Ok(Err(_dropped)) => Err(self.closed(what)),
            Err(_elapsed) => {
                forget();
                Err(DriverError::NoAnswer {
                    viewer: self.label.clone(),
                    what,
                    waited: patience,
                })
            }
        }
    }

    /// End the subscription `subscription` without waiting for the answer:
    /// its stream ends now.
    pub(crate) fn unsubscribe(&self, subscription: RequestId) {
        let _gone = lock(&self.routes).subscriptions.remove(&subscription);
        let id = self.allocate();
        // An answer nobody waits for is dropped by the dispatcher; a closed
        // connection has no subscription left to end.
        let _closed = self.requests.send(Request {
            id,
            body: RequestBody::Unsubscribe { subscription },
        });
    }

    /// The error for a request the connection could not answer.
    fn closed(&self, what: String) -> DriverError {
        DriverError::Closed {
            viewer: self.label.clone(),
            what,
        }
    }
}

/// A request's method, for errors: the name its JSON is tagged with.
fn method(body: &RequestBody) -> String {
    serde_json::to_value(body)
        .ok()
        .and_then(|value| {
            value
                .get("method")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "a request".to_owned())
}

/// Route what the viewer sends until its side closes, then fail everything
/// still waiting.
async fn dispatch(mut messages: UnboundedReceiver<ViewerMessage>, routes: Arc<Mutex<Routes>>) {
    while let Some(message) = messages.recv().await {
        route(&routes, message);
    }
    let mut routes = lock(&routes);
    routes.closed = true;
    routes.pending.clear();
    routes.subscriptions.clear();
}

/// Hand one message to whoever waits for it.
fn route(routes: &Mutex<Routes>, message: ViewerMessage) {
    match message {
        ViewerMessage::Response(response) => {
            let reply = lock(routes).pending.remove(&response.id);
            if let Some(reply) = reply {
                let _gone = reply.send(*response);
            }
        }
        ViewerMessage::Notification(Notification::Log { subscription, page }) => {
            let mut routes = lock(routes);
            if let Some(pages) = routes.subscriptions.get(&subscription)
                && pages.send(page).is_err()
            {
                let _gone = routes.subscriptions.remove(&subscription);
            }
        }
        ViewerMessage::Notification(Notification::Rejected { reason }) => {
            warn!("the viewer rejected a line: {reason}");
        }
    }
}

/// Carry a channel pair over `stream`: each request out as a line of JSON,
/// each line in as a message, until either side ends.
async fn bridge(
    stream: UnixStream,
    mut requests: UnboundedReceiver<Request>,
    messages: UnboundedSender<ViewerMessage>,
    label: String,
) {
    let (reader, mut writer) = stream.into_split();
    let reading = async {
        let mut lines = BufReader::new(reader).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => match serde_json::from_str::<ViewerMessage>(&line) {
                    Ok(message) => {
                        if messages.send(message).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        warn!("viewer {label} sent a line that is not a message: {error}");
                    }
                },
                Ok(None) => return,
                Err(error) => {
                    warn!("reading from viewer {label} failed: {error}");
                    return;
                }
            }
        }
    };
    let writing = async {
        while let Some(request) = requests.recv().await {
            let mut line = match serde_json::to_string(&request) {
                Ok(line) => line,
                Err(error) => {
                    warn!("could not encode a request to viewer {label}: {error}");
                    continue;
                }
            };
            line.push('\n');
            if let Err(error) = writer.write_all(line.as_bytes()).await {
                warn!("writing to viewer {label} failed: {error}");
                return;
            }
        }
    };
    // Either side ending ends the connection: the dispatcher sees the
    // message channel close and fails what is still waiting.
    tokio::select! {
        () = reading => {}
        () = writing => {}
    }
}
