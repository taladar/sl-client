//! The **remote transport**: the automation protocol over a private Unix
//! socket, for whatever drives the viewer from outside its process — the
//! end-to-end stage, the command line tool, an agent.
//!
//! - **Opened only when asked.** [`RemoteEndpoint::open`] binds the socket the
//!   viewer's `--automation-socket` names; without the switch there is no
//!   socket. The file is created mode `0600` (bound in a private directory
//!   and linked into place, so it is never reachable with looser
//!   permissions), an existing socket nobody answers on is replaced as stale,
//!   and a live one, or a file that is not a socket, is an error. The file is
//!   removed when the endpoint is dropped.
//! - **Line-delimited JSON.** Each line a client writes is a
//!   [`sl_automation_proto::Request`]; each line the viewer writes back is a
//!   [`ViewerMessage`] — a response under the request's id, or a subscription
//!   notification. A line that is not a request is answered under its id when
//!   it has one ([`AutomationError::InvalidRequest`]) and with a
//!   [`Notification::Rejected`] when it has none; either way the connection
//!   stays open.
//! - **Several requests in flight.** The listener runs on the shared async
//!   runtime and hands requests to the frame loop over a channel; they go into
//!   the [`AutomationQueue`], and each response is written as the executor
//!   answers it. A client picks its own ids, so the transport renumbers each
//!   request for the queue and gives the answer back under the client's id —
//!   two clients may both use id 1.
//! - **Subscriptions end with their connection.** A client that closes its
//!   write half still gets the answers to what it sent; one that goes away
//!   entirely has its subscriptions ended and its pending answers dropped.

use std::collections::{HashMap, HashSet};
use std::io;
use std::os::unix::fs::{
    DirBuilderExt as _, FileTypeExt as _, MetadataExt as _, PermissionsExt as _,
};
use std::os::unix::net::{UnixListener as StdUnixListener, UnixStream as StdUnixStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bevy::prelude::*;
use sl_automation_proto::{
    AutomationError, Notification, Request, RequestBody, RequestId, Response, ViewerMessage,
};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::{JoinHandle, JoinSet};

use crate::executor::{AutomationPlugin, AutomationQueue, AutomationSystems};

/// The first id the transport gives a request in the [`AutomationQueue`].
/// Anything else that submits to the same queue must stay below it.
pub const REMOTE_ID_BASE: u64 = 1 << 48;

/// The staging directories this process has made, to number the next: one
/// process may open several endpoints.
static STAGED: AtomicU32 = AtomicU32::new(0);

/// How long the listener waits after a failed accept before the next, so a
/// persistent failure (out of file descriptors) logs rather than spins.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

/// Why the automation socket could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum SocketError {
    /// No path was given and there is no runtime directory to default to.
    #[error("no automation socket path was given and XDG_RUNTIME_DIR is not set")]
    NoRuntimeDir,
    /// A running viewer answers on the path already.
    #[error("the automation socket {} is in use by a running process", .path.display())]
    InUse {
        /// The socket.
        path: PathBuf,
    },
    /// The path names something that is not a socket.
    #[error("{} exists and is not a socket", .path.display())]
    NotASocket {
        /// The path.
        path: PathBuf,
    },
    /// A file system or socket call failed.
    #[error("could not {action} {}: {source}", .path.display())]
    Io {
        /// What was being done.
        action: &'static str,
        /// To what.
        path: PathBuf,
        /// The failure.
        #[source]
        source: io::Error,
    },
    /// The shared async runtime, which the listener runs on, is unavailable.
    #[error("the shared async runtime the automation socket listens on could not be started")]
    NoRuntime,
}

impl SocketError {
    /// An [`Self::Io`] error of `action` on `path`.
    fn io(action: &'static str, path: &Path) -> impl FnOnce(io::Error) -> Self {
        let path = path.to_owned();
        move |source| Self::Io {
            action,
            path,
            source,
        }
    }
}

/// Where a viewer's socket goes when no path is given:
/// `$XDG_RUNTIME_DIR/<program>/automation-<pid>.sock`, its directory created
/// private (`0700`) when missing.
///
/// # Errors
///
/// [`SocketError::NoRuntimeDir`] without `XDG_RUNTIME_DIR`, and
/// [`SocketError::Io`] when the directory cannot be created.
pub fn default_socket_path(program: &str) -> Result<PathBuf, SocketError> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .ok_or(SocketError::NoRuntimeDir)?;
    let dir = PathBuf::from(runtime).join(program);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .map_err(SocketError::io("create the directory", &dir))?;
    Ok(dir.join(format!("automation-{}.sock", std::process::id())))
}

/// The socket file this endpoint bound, removed on drop — unless something
/// else has replaced it since.
#[derive(Debug)]
struct SocketFile {
    /// Its path.
    path: PathBuf,
    /// Its device and inode, to recognise it at removal.
    inode: (u64, u64),
}

impl Drop for SocketFile {
    fn drop(&mut self) {
        let ours = fs_err::symlink_metadata(&self.path)
            .is_ok_and(|metadata| (metadata.dev(), metadata.ino()) == self.inode);
        if ours && let Err(error) = fs_err::remove_file(&self.path) {
            error!(
                "could not remove the automation socket {}: {error}",
                self.path.display()
            );
        }
    }
}

/// Clear the way for a socket at `path`: nothing there, or a socket nobody
/// answers on, which is removed.
///
/// # Errors
///
/// [`SocketError::InUse`] when something answers, [`SocketError::NotASocket`]
/// for another kind of file, [`SocketError::Io`] when it cannot be examined
/// or removed.
fn clear_stale(path: &Path) -> Result<(), SocketError> {
    let metadata = match fs_err::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(SocketError::io("examine", path)(error)),
    };
    if !metadata.file_type().is_socket() {
        return Err(SocketError::NotASocket {
            path: path.to_owned(),
        });
    }
    match StdUnixStream::connect(path) {
        Ok(_stream) => Err(SocketError::InUse {
            path: path.to_owned(),
        }),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
            warn!("removing the stale automation socket {}", path.display());
            fs_err::remove_file(path).map_err(SocketError::io("remove the stale socket", path))
        }
        Err(error) => Err(SocketError::io("probe the existing socket", path)(error)),
    }
}

/// Bind a listening socket at `path`, mode `0600` from the moment it is
/// reachable there: bound in a fresh private directory beside it, restricted,
/// then hard-linked into place (which, unlike a rename, never replaces
/// something that appeared meanwhile).
///
/// # Errors
///
/// [`SocketError::Io`] naming the step that failed.
fn bind_private(path: &Path) -> Result<(StdUnixListener, SocketFile), SocketError> {
    if path.file_name().is_none() {
        return Err(SocketError::Io {
            action: "bind",
            path: path.to_owned(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "the path names no file"),
        });
    }
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    // Short names: a socket path is limited to about a hundred bytes, and the
    // staged one must fit as well as the final one.
    let staging = parent.join(format!(
        ".{}.{}",
        std::process::id(),
        STAGED.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&staging)
        .map_err(SocketError::io("create the private directory", &staging))?;
    let staged = staging.join("s");
    let bound = (|| {
        let listener =
            StdUnixListener::bind(&staged).map_err(SocketError::io("bind a socket at", &staged))?;
        fs_err::set_permissions(&staged, std::fs::Permissions::from_mode(0o600))
            .map_err(SocketError::io("restrict", &staged))?;
        fs_err::hard_link(&staged, path).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                SocketError::InUse {
                    path: path.to_owned(),
                }
            } else {
                SocketError::io("link the socket to", path)(error)
            }
        })?;
        Ok(listener)
    })();
    // The staging name and directory go whatever happened; a failure to
    // remove them is worth a line, never the endpoint.
    for (removal, what) in [
        (fs_err::remove_file(&staged), &staged),
        (fs_err::remove_dir(&staging), &staging),
    ] {
        if let Err(error) = removal
            && error.kind() != io::ErrorKind::NotFound
        {
            warn!("could not remove {}: {error}", what.display());
        }
    }
    let listener = bound?;
    let metadata = fs_err::symlink_metadata(path).map_err(SocketError::io("examine", path))?;
    Ok((
        listener,
        SocketFile {
            path: path.to_owned(),
            inode: (metadata.dev(), metadata.ino()),
        },
    ))
}

/// One connection, by the listener's count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ConnectionId(u64);

/// What the listener hands the frame loop.
#[derive(Debug)]
enum Inbound {
    /// A client connected; its lines go to `outbound`.
    Connected {
        /// Which.
        connection: ConnectionId,
        /// Where its lines are written.
        outbound: UnboundedSender<String>,
    },
    /// A client sent a request.
    Request {
        /// Which client.
        connection: ConnectionId,
        /// What it asked, boxed: most messages are small.
        request: Box<Request>,
    },
    /// A client closed its write half: it sends nothing more, but waits for
    /// its answers.
    Finished {
        /// Which.
        connection: ConnectionId,
    },
    /// A client went away.
    Closed {
        /// Which.
        connection: ConnectionId,
    },
}

/// A connected client, as the frame loop keeps it.
#[derive(Debug)]
struct Connection {
    /// Where its lines are written; dropped with the connection, which ends
    /// its writer.
    outbound: UnboundedSender<String>,
    /// The ids of its requests not answered yet.
    in_flight: HashSet<RequestId>,
    /// Its subscriptions: its id for each, and the queue's.
    subscriptions: HashMap<RequestId, RequestId>,
    /// Whether it has closed its write half.
    finished: bool,
}

impl Connection {
    /// Write `message` to the client. A client that has gone is noticed by
    /// the listener, which reports it.
    fn send(&self, message: &ViewerMessage) {
        match serde_json::to_string(message) {
            Ok(line) => {
                let _gone = self.outbound.send(line);
            }
            Err(error) => error!("could not encode an automation message: {error}"),
        }
    }

    /// Answer the request `id` with an error, without the queue.
    fn refuse(&self, id: RequestId, reason: String) {
        self.send(&ViewerMessage::Response(Box::new(Response {
            id,
            result: Err(AutomationError::InvalidRequest { reason }),
            report: None,
        })));
    }
}

/// A request in the queue on a client's behalf.
#[derive(Debug)]
struct Pending {
    /// Whose, and under which of its ids; `None` for the transport's own
    /// (ending a departed client's subscriptions), whose answer is dropped.
    client: Option<(ConnectionId, RequestId)>,
    /// Whether it starts a subscription.
    subscribe: bool,
}

/// The open automation socket and its clients: a resource of the App whose
/// executor it feeds, installed by [`RemoteAutomationPlugin`].
#[derive(Resource, Debug)]
pub struct RemoteEndpoint {
    /// The socket file, removed on drop.
    socket: SocketFile,
    /// What the listener hands the frame loop.
    inbound: UnboundedReceiver<Inbound>,
    /// The listener, stopped on drop (and every connection with it).
    listener: JoinHandle<()>,
    /// The connected clients.
    connections: HashMap<ConnectionId, Connection>,
    /// The requests in the queue, by the queue's id.
    pending: HashMap<RequestId, Pending>,
    /// The running subscriptions, by the queue's id: whose, under which id.
    subscriptions: HashMap<RequestId, (ConnectionId, RequestId)>,
    /// The queue id the next request gets.
    next_id: u64,
}

impl Drop for RemoteEndpoint {
    fn drop(&mut self) {
        self.listener.abort();
    }
}

impl RemoteEndpoint {
    /// Open the socket at `path` and start listening on it, on the shared
    /// async runtime.
    ///
    /// # Errors
    ///
    /// [`SocketError::InUse`] when a live process answers there,
    /// [`SocketError::NotASocket`] for another kind of file,
    /// [`SocketError::NoRuntime`] without the shared runtime and
    /// [`SocketError::Io`] when a file system or socket call fails.
    pub fn open(path: &Path) -> Result<Self, SocketError> {
        let runtime = sl_client_bevy::shared_runtime().ok_or(SocketError::NoRuntime)?;
        clear_stale(path)?;
        let (listener, socket) = bind_private(path)?;
        listener
            .set_nonblocking(true)
            .map_err(SocketError::io("configure", path))?;
        let listener = {
            let _entered = runtime.enter();
            UnixListener::from_std(listener).map_err(SocketError::io("listen on", path))?
        };
        let (sender, inbound) = unbounded_channel();
        let listener = runtime.spawn(accept(listener, sender, path.to_owned()));
        info!("automation socket listening at {}", path.display());
        Ok(Self {
            socket,
            inbound,
            listener,
            connections: HashMap::new(),
            pending: HashMap::new(),
            subscriptions: HashMap::new(),
            next_id: REMOTE_ID_BASE,
        })
    }

    /// Where the socket is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.socket.path
    }

    /// A fresh queue id.
    const fn allocate(&mut self) -> RequestId {
        let id = RequestId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Put the client's `request` into the queue under a fresh id — or,
    /// when it cannot be, answer it at once.
    fn submit(&mut self, connection: ConnectionId, request: Request, queue: &mut AutomationQueue) {
        let Request { id, body } = request;
        let Some(client) = self.connections.get(&connection) else {
            return;
        };
        if client.in_flight.contains(&id) {
            client.refuse(id, format!("request {} is already in flight", id.0));
            return;
        }
        let subscribe = matches!(body, RequestBody::Subscribe { .. });
        if subscribe && client.subscriptions.contains_key(&id) {
            client.refuse(id, format!("subscription {} is already running", id.0));
            return;
        }
        let body = match body {
            RequestBody::Unsubscribe { subscription } => {
                let Some(&queued) = client.subscriptions.get(&subscription) else {
                    client.refuse(id, format!("there is no subscription {}", subscription.0));
                    return;
                };
                RequestBody::Unsubscribe {
                    subscription: queued,
                }
            }
            body => body,
        };
        let queued = self.allocate();
        let Some(client) = self.connections.get_mut(&connection) else {
            return;
        };
        let _new = client.in_flight.insert(id);
        if subscribe {
            let _previous = client.subscriptions.insert(id, queued);
            let _previous = self.subscriptions.insert(queued, (connection, id));
        }
        let _previous = self.pending.insert(
            queued,
            Pending {
                client: Some((connection, id)),
                subscribe,
            },
        );
        queue.submit(Request { id: queued, body });
    }

    /// The client closed its write half: end its subscriptions; it keeps its
    /// connection until its last answer is written, and loses it now when
    /// nothing is left to answer.
    fn finish(&mut self, connection: ConnectionId, queue: &mut AutomationQueue) {
        let Some(client) = self.connections.get_mut(&connection) else {
            return;
        };
        client.finished = true;
        let answered = client.in_flight.is_empty();
        let subscriptions: Vec<RequestId> = client
            .subscriptions
            .drain()
            .map(|(_id, queued)| queued)
            .collect();
        if answered {
            let _gone = self.connections.remove(&connection);
        }
        for subscription in subscriptions {
            self.end_subscription(subscription, queue);
        }
    }

    /// The client went away: end its subscriptions and forget its requests.
    fn close(&mut self, connection: ConnectionId, queue: &mut AutomationQueue) {
        self.finish(connection, queue);
        let _gone = self.connections.remove(&connection);
        for pending in self.pending.values_mut() {
            if pending
                .client
                .is_some_and(|(owner, _id)| owner == connection)
            {
                pending.client = None;
            }
        }
    }

    /// End the subscription the queue knows as `subscription`, on the
    /// transport's own behalf.
    fn end_subscription(&mut self, subscription: RequestId, queue: &mut AutomationQueue) {
        let _gone = self.subscriptions.remove(&subscription);
        let queued = self.allocate();
        let _previous = self.pending.insert(
            queued,
            Pending {
                client: None,
                subscribe: false,
            },
        );
        queue.submit(Request {
            id: queued,
            body: RequestBody::Unsubscribe { subscription },
        });
    }

    /// Write every answer and notification the queue holds for a client, and
    /// let a finished client with nothing left to wait for go.
    fn deliver(&mut self, queue: &mut AutomationQueue) {
        let answered: Vec<(RequestId, Response)> = self
            .pending
            .keys()
            .filter_map(|&queued| {
                queue
                    .take_response(queued)
                    .map(|response| (queued, response))
            })
            .collect();
        for (queued, mut response) in answered {
            let Some(pending) = self.pending.remove(&queued) else {
                continue;
            };
            let Some((connection, id)) = pending.client else {
                continue;
            };
            let Some(client) = self.connections.get_mut(&connection) else {
                continue;
            };
            let _answered = client.in_flight.remove(&id);
            if pending.subscribe && response.result.is_err() {
                let _gone = client.subscriptions.remove(&id);
                let _gone = self.subscriptions.remove(&queued);
            }
            response.id = id;
            client.send(&ViewerMessage::Response(Box::new(response)));
        }
        for (&queued, &(connection, id)) in &self.subscriptions {
            let notifications = queue.take_notifications(queued);
            let Some(client) = self.connections.get(&connection) else {
                continue;
            };
            for notification in notifications {
                let notification = match notification {
                    Notification::Log { page, .. } => Notification::Log {
                        subscription: id,
                        page,
                    },
                    other @ Notification::Rejected { .. } => other,
                };
                client.send(&ViewerMessage::Notification(notification));
            }
        }
        self.connections
            .retain(|_id, client| !(client.finished && client.in_flight.is_empty()));
    }
}

/// Serves the automation protocol on a [`RemoteEndpoint`], whose socket the
/// caller opened (so that a failure to open it fails the caller's start-up),
/// installing the [`AutomationPlugin`] it feeds unless the App has it.
#[derive(Debug)]
pub struct RemoteAutomationPlugin {
    /// The endpoint, until the plugin is built into the App.
    endpoint: Mutex<Option<RemoteEndpoint>>,
}

impl RemoteAutomationPlugin {
    /// Serve on `endpoint`.
    #[must_use]
    pub const fn new(endpoint: RemoteEndpoint) -> Self {
        Self {
            endpoint: Mutex::new(Some(endpoint)),
        }
    }
}

impl Plugin for RemoteAutomationPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<AutomationPlugin>() {
            app.add_plugins(AutomationPlugin);
        }
        let endpoint = self
            .endpoint
            .lock()
            .ok()
            .and_then(|mut endpoint| endpoint.take());
        let Some(endpoint) = endpoint else {
            error!("the automation socket plugin was built twice; the second has no socket");
            return;
        };
        app.insert_resource(endpoint).add_systems(
            Last,
            (
                receive.before(AutomationSystems::Execute),
                send.after(AutomationSystems::Execute),
            ),
        );
    }
}

/// Take what the listener received into the queue.
fn receive(mut endpoint: ResMut<'_, RemoteEndpoint>, mut queue: ResMut<'_, AutomationQueue>) {
    while let Ok(inbound) = endpoint.inbound.try_recv() {
        match inbound {
            Inbound::Connected {
                connection,
                outbound,
            } => {
                let _previous = endpoint.connections.insert(
                    connection,
                    Connection {
                        outbound,
                        in_flight: HashSet::new(),
                        subscriptions: HashMap::new(),
                        finished: false,
                    },
                );
            }
            Inbound::Request {
                connection,
                request,
            } => endpoint.submit(connection, *request, &mut queue),
            Inbound::Finished { connection } => endpoint.finish(connection, &mut queue),
            Inbound::Closed { connection } => endpoint.close(connection, &mut queue),
        }
    }
}

/// Write the queue's answers and notifications to their clients.
fn send(mut endpoint: ResMut<'_, RemoteEndpoint>, mut queue: ResMut<'_, AutomationQueue>) {
    if endpoint.pending.is_empty() && endpoint.subscriptions.is_empty() {
        return;
    }
    endpoint.deliver(&mut queue);
}

/// Accept clients on `listener` until stopped, serving each on its own task.
async fn accept(listener: UnixListener, inbound: UnboundedSender<Inbound>, path: PathBuf) {
    let mut clients = JoinSet::new();
    let mut count: u64 = 0;
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _address)) => {
                    count = count.saturating_add(1);
                    let _handle = clients.spawn(serve(ConnectionId(count), stream, inbound.clone()));
                }
                Err(error) => {
                    error!(
                        "the automation socket {} could not accept a connection: {error}",
                        path.display()
                    );
                    tokio::time::sleep(ACCEPT_BACKOFF).await;
                }
            },
            Some(_served) = clients.join_next(), if !clients.is_empty() => {}
        }
    }
}

/// Serve one client: read its lines into requests for the frame loop, write
/// what the frame loop sends it, until it goes away.
async fn serve(connection: ConnectionId, stream: UnixStream, inbound: UnboundedSender<Inbound>) {
    let (reader, mut writer) = stream.into_split();
    let (outbound, mut lines_out) = unbounded_channel::<String>();
    if inbound
        .send(Inbound::Connected {
            connection,
            outbound: outbound.clone(),
        })
        .is_err()
    {
        return;
    }
    let reading = {
        let inbound = inbound.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => match parse(&line) {
                        Parsed::Request(request) => {
                            if inbound
                                .send(Inbound::Request {
                                    connection,
                                    request,
                                })
                                .is_err()
                            {
                                return;
                            }
                        }
                        Parsed::Answer(answer) => {
                            let _gone = outbound.send(answer);
                        }
                        Parsed::Blank => {}
                    },
                    Ok(None) => break,
                    Err(error) => {
                        warn!("an automation client's connection failed: {error}");
                        break;
                    }
                }
            }
            // Dropping `outbound` here leaves the frame loop's copy as the
            // writer's last: the connection lives until it has answered.
            let _gone = inbound.send(Inbound::Finished { connection });
        })
    };
    while let Some(mut line) = lines_out.recv().await {
        line.push('\n');
        if let Err(error) = writer.write_all(line.as_bytes()).await {
            if error.kind() != io::ErrorKind::BrokenPipe {
                warn!("could not write to an automation client: {error}");
            }
            break;
        }
    }
    reading.abort();
    let _gone = inbound.send(Inbound::Closed { connection });
}

/// What a line a client wrote comes to.
#[derive(Debug)]
enum Parsed {
    /// A request.
    Request(Box<Request>),
    /// Not a request: the line to answer it with.
    Answer(String),
    /// Nothing at all.
    Blank,
}

/// Read `line` as a request; a line that is not one is answered under its id
/// when it has one, and rejected when it has none.
fn parse(line: &str) -> Parsed {
    if line.trim().is_empty() {
        return Parsed::Blank;
    }
    let error = match serde_json::from_str::<Request>(line) {
        Ok(request) => return Parsed::Request(Box::new(request)),
        Err(error) => error,
    };
    let id = serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|value| value.get("id").and_then(serde_json::Value::as_u64));
    let message = match id {
        Some(id) => ViewerMessage::Response(Box::new(Response {
            id: RequestId(id),
            result: Err(AutomationError::InvalidRequest {
                reason: error.to_string(),
            }),
            report: None,
        })),
        None => ViewerMessage::Notification(Notification::Rejected {
            reason: format!("not a request: {error}"),
        }),
    };
    match serde_json::to_string(&message) {
        Ok(answer) => Parsed::Answer(answer),
        Err(error) => {
            error!("could not encode an automation refusal: {error}");
            Parsed::Blank
        }
    }
}

#[cfg(test)]
mod tests;
