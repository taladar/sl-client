//! [`InProcessHost`]: the in-process transport on a thread of its own, so an
//! async caller — the driver, a stage — talks to an in-process viewer the way
//! it talks to one behind a socket: requests into one channel, answers and
//! notifications out of another ([`ViewerLink`]).
//!
//! - **The viewers run like processes.** The host thread steps every viewer
//!   it hosts a frame per round, [`FRAME_PAUSE`] apart, whether or not anyone
//!   is waiting — a login, a teleport or a chat line from the grid goes on
//!   while the test does something else, as it does in a viewer process.
//! - **Apps are built on the host thread** (a Bevy `App` is not `Send`): a
//!   viewer is hosted by handing the host a closure that builds it, and a
//!   test reaches into an App (to raise its termination flag, say) by handing
//!   the host a closure that runs there between frames ([`InProcessHost::with_app`]).
//! - **An exited viewer closes its link** once everything it delivered is
//!   sent, so what waits on it learns it will get no more; the App itself
//!   stays until the host stops.
//! - **Stopping the host stops its thread**: it lets each viewer's render
//!   pipelines finish compiling, as a dropped [`InProcessTransport`] does,
//!   and reports a thread that panicked.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Duration;

use bevy::log::error;
use sl_automation_proto::{Request, ViewerMessage};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;

use crate::in_process::{FRAME_PAUSE, HostedApp, InProcessError, InProcessTransport, ViewerHandle};

/// Why a viewer could not be built, as its builder reports it.
pub type BuildError = Box<dyn std::error::Error + Send + Sync>;

/// How long the idle host thread (hosting nothing) waits for a command before
/// it looks again.
const IDLE_WAIT: Duration = Duration::from_millis(100);

/// Why the host could not do what it was asked.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// The viewer's builder failed.
    #[error("viewer {label} could not be built: {source}")]
    Build {
        /// The viewer.
        label: String,
        /// Why.
        #[source]
        source: BuildError,
    },
    /// The transport refused the viewer, or does not know the handle.
    #[error(transparent)]
    Transport(#[from] InProcessError),
    /// The host thread has stopped: it panicked, or the host was stopped.
    #[error("the in-process host thread has stopped")]
    Stopped,
    /// The host thread panicked; the panic's message, when it had one.
    #[error("the in-process host thread panicked: {0}")]
    Panicked(String),
}

/// One end of a hosted viewer's connection: the requests go in, and every
/// response and notification the viewer sends comes out, in the order it
/// sent them. The channel closes when the viewer exits or the host stops.
#[derive(Debug)]
pub struct ViewerLink {
    /// Where the caller's requests go.
    pub requests: UnboundedSender<Request>,
    /// What the viewer sends back.
    pub messages: UnboundedReceiver<ViewerMessage>,
}

/// Something run against a hosted App between frames; handed `None` for a
/// handle the transport does not know.
type AppTask<A> = Box<dyn FnOnce(Option<&mut A>) + Send>;

/// Something to do on the host thread.
enum Command<A> {
    /// Build and host a viewer.
    Host {
        /// Its name in errors.
        label: String,
        /// Builds it.
        build: Box<dyn FnOnce() -> Result<A, BuildError> + Send>,
        /// Where the handle and the link go.
        reply: oneshot::Sender<Result<(ViewerHandle, ViewerLink), HostError>>,
    },
    /// Run something against a viewer's App between frames; it is handed
    /// `None` for a handle the transport does not know.
    With {
        /// The viewer.
        viewer: ViewerHandle,
        /// What to run.
        run: AppTask<A>,
    },
    /// Say when a viewer has exited and its link is closed.
    WhenExited {
        /// The viewer.
        viewer: ViewerHandle,
        /// Told then (dropped for a handle the transport does not know).
        reply: oneshot::Sender<()>,
    },
}

/// One hosted viewer's connection, as the host thread keeps it.
struct Connection {
    /// The viewer.
    viewer: ViewerHandle,
    /// The caller's requests.
    requests: UnboundedReceiver<Request>,
    /// Where its messages go; `None` once it has exited and is closed.
    messages: Option<UnboundedSender<ViewerMessage>>,
    /// Who waits for it to exit.
    exit_waiters: Vec<oneshot::Sender<()>>,
}

/// Viewer Apps hosted and stepped on a thread of their own, each reached
/// through a [`ViewerLink`]. The thread steps every viewer continuously, as a
/// process runs; Apps are built there by the closure [`host`](Self::host)
/// takes, and reached between frames with [`with_app`](Self::with_app).
#[derive(Debug)]
pub struct InProcessHost<A: HostedApp + 'static> {
    /// Where commands go; `None` once stopped.
    commands: Option<Sender<Command<A>>>,
    /// The host thread; `None` once joined.
    thread: Option<JoinHandle<()>>,
}

impl<A: HostedApp + 'static> Drop for InProcessHost<A> {
    fn drop(&mut self) {
        if let Err(error) = self.stop_thread() {
            error!("{error}");
        }
    }
}

impl<A: HostedApp + 'static> InProcessHost<A> {
    /// Start the host thread, hosting nothing yet.
    ///
    /// # Errors
    ///
    /// [`HostError::Stopped`] when the thread cannot be spawned.
    pub fn start() -> Result<Self, HostError> {
        let (commands, inbox) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("sl-viewer-host".to_owned())
            .spawn(move || run(&inbox))
            .map_err(|_error| HostError::Stopped)?;
        Ok(Self {
            commands: Some(commands),
            thread: Some(thread),
        })
    }

    /// Send `command` to the host thread.
    fn send(&self, command: Command<A>) -> Result<(), HostError> {
        self.commands
            .as_ref()
            .ok_or(HostError::Stopped)?
            .send(command)
            .map_err(|_gone| HostError::Stopped)
    }

    /// Build a viewer on the host thread with `build` and host it under
    /// `label`, the name errors call it by; it is stepped from then on. The
    /// App must have the automation executor installed.
    ///
    /// # Errors
    ///
    /// [`HostError::Build`] when `build` fails, [`HostError::Transport`] when
    /// the App has no executor, [`HostError::Stopped`] when the host is gone.
    pub async fn host(
        &self,
        label: impl Into<String>,
        build: impl FnOnce() -> Result<A, BuildError> + Send + 'static,
    ) -> Result<(ViewerHandle, ViewerLink), HostError> {
        let (reply, answer) = oneshot::channel();
        self.send(Command::Host {
            label: label.into(),
            build: Box::new(build),
            reply,
        })?;
        answer.await.map_err(|_gone| HostError::Stopped)?
    }

    /// Run `run` against `viewer`'s App on the host thread, between two of
    /// its frames, and hand back what it returns.
    ///
    /// # Errors
    ///
    /// [`HostError::Transport`] for a handle the host does not know, and
    /// [`HostError::Stopped`] when the host is gone.
    pub async fn with_app<R: Send + 'static>(
        &self,
        viewer: ViewerHandle,
        run: impl FnOnce(&mut A) -> R + Send + 'static,
    ) -> Result<R, HostError> {
        let (reply, answer) = oneshot::channel();
        self.send(Command::With {
            viewer,
            run: Box::new(move |app: Option<&mut A>| {
                let _gone = reply.send(app.map(run));
            }),
        })?;
        answer
            .await
            .map_err(|_gone| HostError::Stopped)?
            .ok_or(HostError::Transport(InProcessError::NoViewer(viewer)))
    }

    /// Wait until `viewer` has exited and everything it delivered has gone
    /// out on its link.
    ///
    /// # Errors
    ///
    /// [`HostError::Stopped`] when the host stops first, or does not know the
    /// handle.
    pub async fn exited(&self, viewer: ViewerHandle) -> Result<(), HostError> {
        let (reply, answer) = oneshot::channel();
        self.send(Command::WhenExited { viewer, reply })?;
        answer.await.map_err(|_gone| HostError::Stopped)
    }

    /// Stop the host thread and wait for it: every link closes, and each
    /// viewer's render pipelines finish compiling first.
    ///
    /// # Errors
    ///
    /// [`HostError::Panicked`] when the thread panicked.
    pub fn stop(mut self) -> Result<(), HostError> {
        self.stop_thread()
    }

    /// Close the command channel and join the thread, once.
    fn stop_thread(&mut self) -> Result<(), HostError> {
        drop(self.commands.take());
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        thread.join().map_err(|panic| {
            HostError::Panicked(
                panic
                    .downcast_ref::<&str>()
                    .map(|message| (*message).to_owned())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "no message".to_owned()),
            )
        })
    }
}

/// The host thread: take commands, feed each viewer its requests, step every
/// viewer a frame, send what each delivered — until the command channel
/// closes.
fn run<A: HostedApp>(inbox: &Receiver<Command<A>>) {
    let mut transport = InProcessTransport::<A>::new();
    let mut connections: Vec<Connection> = Vec::new();
    loop {
        // With nothing to step, wait for a command rather than spin.
        if connections.is_empty() {
            match inbox.recv_timeout(IDLE_WAIT) {
                Ok(command) => obey(command, &mut transport, &mut connections),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            continue;
        }
        if drain(inbox, &mut transport, &mut connections) == Err(TryRecvError::Disconnected) {
            break;
        }
        for connection in &mut connections {
            while let Ok(request) = connection.requests.try_recv() {
                if let Err(error) = transport.send(connection.viewer, request) {
                    error!("{error}");
                }
            }
        }
        transport.step();
        for connection in &mut connections {
            deliver(&mut transport, connection);
        }
        std::thread::sleep(FRAME_PAUSE);
    }
    // Dropping the transport lets each viewer's pipelines finish.
    drop(connections);
    drop(transport);
}

/// Obey every command waiting; `Disconnected` once the host has been
/// stopped.
fn drain<A: HostedApp>(
    inbox: &Receiver<Command<A>>,
    transport: &mut InProcessTransport<A>,
    connections: &mut Vec<Connection>,
) -> Result<(), TryRecvError> {
    loop {
        let command = inbox.try_recv()?;
        obey(command, transport, connections);
    }
}

/// Carry out one command.
fn obey<A: HostedApp>(
    command: Command<A>,
    transport: &mut InProcessTransport<A>,
    connections: &mut Vec<Connection>,
) {
    match command {
        Command::Host {
            label,
            build,
            reply,
        } => {
            let hosted = build()
                .map_err(|source| HostError::Build {
                    label: label.clone(),
                    source,
                })
                .and_then(|app| Ok(transport.host(label, app)?))
                .map(|viewer| {
                    let (requests, requests_in) = unbounded_channel();
                    let (messages_out, messages) = unbounded_channel();
                    connections.push(Connection {
                        viewer,
                        requests: requests_in,
                        messages: Some(messages_out),
                        exit_waiters: Vec::new(),
                    });
                    (viewer, ViewerLink { requests, messages })
                });
            let _gone = reply.send(hosted);
        }
        Command::With { viewer, run } => run(transport.app_mut(viewer)),
        Command::WhenExited { viewer, reply } => {
            if let Some(connection) = connections
                .iter_mut()
                .find(|connection| connection.viewer == viewer)
            {
                if connection.messages.is_some() {
                    connection.exit_waiters.push(reply);
                } else {
                    let _gone = reply.send(());
                }
            }
        }
    }
}

/// Send what the connection's viewer delivered, and close the connection
/// once the viewer has exited.
fn deliver<A: HostedApp>(transport: &mut InProcessTransport<A>, connection: &mut Connection) {
    let Some(messages) = &connection.messages else {
        return;
    };
    match transport.take_messages(connection.viewer) {
        Ok(delivered) => {
            for message in delivered {
                // A caller that dropped its end reads nothing more; the
                // viewer runs on regardless.
                let _gone = messages.send(message);
            }
        }
        Err(error) => error!("{error}"),
    }
    if transport.has_exited(connection.viewer).unwrap_or(true) {
        connection.messages = None;
        for waiter in connection.exit_waiters.drain(..) {
            let _gone = waiter.send(());
        }
    }
}

#[cfg(test)]
mod tests;
