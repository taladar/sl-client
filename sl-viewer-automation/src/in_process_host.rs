//! [`InProcessHost`]: in-process viewers each on a thread of its own, so an
//! async caller — the driver, a stage — talks to an in-process viewer the way
//! it talks to one behind a socket: requests into one channel, answers and
//! notifications out of another ([`ViewerLink`]).
//!
//! - **The viewers run like processes.** Each viewer's thread steps it a
//!   frame, [`FRAME_PAUSE`] apart, whether or not anyone is waiting — a
//!   login, a teleport or a chat line from the grid goes on while the test
//!   does something else, as it does in a viewer process. The viewers do not
//!   take turns: a slow frame of one is no pause for another, and a test that
//!   adds a viewer costs the others no frame rate.
//! - **Apps are built on their own thread** (a Bevy `App` is not `Send`): a
//!   viewer is hosted by handing the host a closure that builds it, and a
//!   test reaches into an App (to raise its termination flag, say) by handing
//!   the host a closure that runs there between two of its frames
//!   ([`InProcessHost::with_app`]). The thread starts in the tracing context
//!   of the code that hosted the viewer.
//! - **An exited viewer closes its link** once everything it delivered is
//!   sent, so what waits on it learns it will get no more; the App itself
//!   stays until the host stops.
//! - **Stopping the host stops every viewer's thread**: each lets its
//!   viewer's render pipelines finish compiling, as a dropped
//!   [`InProcessTransport`] does, and a thread that panicked is reported.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Mutex, PoisonError};
use std::thread::JoinHandle;

use bevy::log::error;
use sl_automation_proto::{Request, ViewerMessage};
use sl_client_bevy::log_context::spawn_named_thread;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;

use crate::in_process::{FRAME_PAUSE, HostedApp, InProcessError, InProcessTransport, ViewerHandle};

/// Why a viewer could not be built, as its builder reports it.
pub type BuildError = Box<dyn std::error::Error + Send + Sync>;

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
    /// The viewer's thread has stopped (it panicked), or could not be
    /// started.
    #[error("the in-process viewer's thread has stopped")]
    Stopped,
    /// A viewer's thread panicked; the panic's message, when it had one.
    #[error("an in-process viewer's thread panicked: {0}")]
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

/// Something run against a hosted App between frames.
type AppTask<A> = Box<dyn FnOnce(&mut A) + Send>;

/// What builds a viewer's App, on its thread.
type Build<A> = Box<dyn FnOnce() -> Result<A, BuildError> + Send>;

/// Something to do on a viewer's thread.
enum Command<A> {
    /// Run something against the viewer's App between frames.
    With(AppTask<A>),
    /// Say when the viewer has exited and its link is closed.
    WhenExited(oneshot::Sender<()>),
}

/// The viewer's connection, as its thread keeps it.
struct Connection {
    /// The viewer, in its thread's transport.
    viewer: ViewerHandle,
    /// The caller's requests.
    requests: UnboundedReceiver<Request>,
    /// Where its messages go; `None` once it has exited and is closed.
    messages: Option<UnboundedSender<ViewerMessage>>,
    /// Who waits for it to exit.
    exit_waiters: Vec<oneshot::Sender<()>>,
}

/// One hosted viewer, as the host keeps it.
#[derive(Debug)]
struct ViewerThread<A> {
    /// Where its commands go.
    commands: Sender<Command<A>>,
    /// Its thread.
    thread: JoinHandle<()>,
}

/// Viewer Apps hosted and stepped each on a thread of its own, each reached
/// through a [`ViewerLink`]. Every thread steps its viewer continuously, as a
/// process runs; an App is built there by the closure [`host`](Self::host)
/// takes, and reached between its frames with [`with_app`](Self::with_app).
#[derive(Debug)]
pub struct InProcessHost<A: HostedApp + 'static> {
    /// The hosted viewers, by handle.
    viewers: Mutex<Vec<ViewerThread<A>>>,
}

impl<A: HostedApp + 'static> Default for InProcessHost<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: HostedApp + 'static> Drop for InProcessHost<A> {
    fn drop(&mut self) {
        if let Err(error) = self.stop_threads() {
            error!("{error}");
        }
    }
}

impl<A: HostedApp + 'static> InProcessHost<A> {
    /// A host hosting nothing yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            viewers: Mutex::new(Vec::new()),
        }
    }

    /// Where `viewer`'s commands go, when the host knows it.
    fn commands(&self, viewer: ViewerHandle) -> Option<Sender<Command<A>>> {
        self.viewers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(viewer.index())
            .map(|hosted| hosted.commands.clone())
    }

    /// Start a thread named for `label`, build a viewer there with `build`
    /// and host it under `label`, the name errors call it by; it is stepped
    /// from then on. The App must have the automation executor installed.
    ///
    /// # Errors
    ///
    /// [`HostError::Build`] when `build` fails, [`HostError::Transport`] when
    /// the App has no executor, [`HostError::Stopped`] when the thread cannot
    /// be started, [`HostError::Panicked`] when the build panics.
    pub async fn host(
        &self,
        label: impl Into<String>,
        build: impl FnOnce() -> Result<A, BuildError> + Send + 'static,
    ) -> Result<(ViewerHandle, ViewerLink), HostError> {
        let label = label.into();
        let (commands, inbox) = mpsc::channel();
        let (reply, answer) = oneshot::channel();
        let thread = {
            let label = label.clone();
            let build: Build<A> = Box::new(build);
            spawn_named_thread(&format!("sl-viewer-{label}"), move || {
                run(label, build, reply, &inbox);
            })
            .map_err(|_error| HostError::Stopped)?
        };
        match answer.await {
            Ok(Ok(link)) => {
                let mut viewers = self.viewers.lock().unwrap_or_else(PoisonError::into_inner);
                viewers.push(ViewerThread { commands, thread });
                Ok((
                    ViewerHandle::from_index(viewers.len().saturating_sub(1)),
                    link,
                ))
            }
            // The thread ends once it has answered.
            Ok(Err(error)) => {
                join(thread)?;
                Err(error)
            }
            Err(_gone) => {
                join(thread)?;
                Err(HostError::Stopped)
            }
        }
    }

    /// Run `run` against `viewer`'s App on its thread, between two of its
    /// frames, and hand back what it returns.
    ///
    /// # Errors
    ///
    /// [`HostError::Transport`] for a handle the host does not know, and
    /// [`HostError::Stopped`] when the viewer's thread is gone.
    pub async fn with_app<R: Send + 'static>(
        &self,
        viewer: ViewerHandle,
        run: impl FnOnce(&mut A) -> R + Send + 'static,
    ) -> Result<R, HostError> {
        let commands = self
            .commands(viewer)
            .ok_or(HostError::Transport(InProcessError::NoViewer(viewer)))?;
        let (reply, answer) = oneshot::channel();
        commands
            .send(Command::With(Box::new(move |app: &mut A| {
                let _gone = reply.send(run(app));
            })))
            .map_err(|_gone| HostError::Stopped)?;
        answer.await.map_err(|_gone| HostError::Stopped)
    }

    /// Wait until `viewer` has exited and everything it delivered has gone
    /// out on its link.
    ///
    /// # Errors
    ///
    /// [`HostError::Stopped`] when its thread stops first, or the host does
    /// not know the handle.
    pub async fn exited(&self, viewer: ViewerHandle) -> Result<(), HostError> {
        let commands = self.commands(viewer).ok_or(HostError::Stopped)?;
        let (reply, answer) = oneshot::channel();
        commands
            .send(Command::WhenExited(reply))
            .map_err(|_gone| HostError::Stopped)?;
        answer.await.map_err(|_gone| HostError::Stopped)
    }

    /// Stop every viewer's thread and wait for them all: every link closes,
    /// and each viewer's render pipelines finish compiling first.
    ///
    /// # Errors
    ///
    /// [`HostError::Panicked`] when a thread panicked — the first of them.
    pub fn stop(mut self) -> Result<(), HostError> {
        self.stop_threads()
    }

    /// Close every command channel, then join every thread, once; the
    /// threads wind down side by side.
    fn stop_threads(&mut self) -> Result<(), HostError> {
        let viewers = std::mem::take(
            self.viewers
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner),
        );
        let threads: Vec<JoinHandle<()>> = viewers
            .into_iter()
            .map(|ViewerThread { commands, thread }| {
                drop(commands);
                thread
            })
            .collect();
        let mut first = Ok(());
        for thread in threads {
            if let Err(panic) = join(thread) {
                if first.is_ok() {
                    first = Err(panic);
                } else {
                    error!("{panic}");
                }
            }
        }
        first
    }
}

/// Join `thread`, reporting its panic.
fn join(thread: JoinHandle<()>) -> Result<(), HostError> {
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

/// A viewer's thread: build the App and answer `reply` with its link, then
/// take commands, feed the viewer its requests, step it a frame and send what
/// it delivered — until the command channel closes.
fn run<A: HostedApp>(
    label: String,
    build: Build<A>,
    reply: oneshot::Sender<Result<ViewerLink, HostError>>,
    inbox: &Receiver<Command<A>>,
) {
    let mut transport = InProcessTransport::<A>::new();
    let hosted = build()
        .map_err(|source| HostError::Build {
            label: label.clone(),
            source,
        })
        .and_then(|app| Ok(transport.host(label, app)?));
    let viewer = match hosted {
        Ok(viewer) => viewer,
        Err(error) => {
            let _gone = reply.send(Err(error));
            return;
        }
    };
    let (requests, requests_in) = unbounded_channel();
    let (messages_out, messages) = unbounded_channel();
    if reply.send(Ok(ViewerLink { requests, messages })).is_err() {
        // Whoever hosted it stopped waiting: nobody can reach it.
        return;
    }
    let mut connection = Connection {
        viewer,
        requests: requests_in,
        messages: Some(messages_out),
        exit_waiters: Vec::new(),
    };
    loop {
        // An exited viewer is stepped no more: wait for a command rather
        // than spin.
        if connection.messages.is_none() {
            match inbox.recv() {
                Ok(command) => obey(command, &mut transport, &mut connection),
                Err(_disconnected) => break,
            }
            continue;
        }
        if drain(inbox, &mut transport, &mut connection) == Err(TryRecvError::Disconnected) {
            break;
        }
        while let Ok(request) = connection.requests.try_recv() {
            if let Err(error) = transport.send(connection.viewer, request) {
                error!("{error}");
            }
        }
        transport.step();
        deliver(&mut transport, &mut connection);
        std::thread::sleep(FRAME_PAUSE);
    }
    // Dropping the transport lets the viewer's pipelines finish.
    drop(connection);
    drop(transport);
}

/// Obey every command waiting; `Disconnected` once the host has stopped the
/// viewer.
fn drain<A: HostedApp>(
    inbox: &Receiver<Command<A>>,
    transport: &mut InProcessTransport<A>,
    connection: &mut Connection,
) -> Result<(), TryRecvError> {
    loop {
        let command = inbox.try_recv()?;
        obey(command, transport, connection);
    }
}

/// Carry out one command.
fn obey<A: HostedApp>(
    command: Command<A>,
    transport: &mut InProcessTransport<A>,
    connection: &mut Connection,
) {
    match command {
        Command::With(run) => match transport.app_mut(connection.viewer) {
            Some(app) => run(app),
            // Its own transport always hosts it; dropping the task fails the
            // wait on it rather than hanging it.
            None => error!("{}", InProcessError::NoViewer(connection.viewer)),
        },
        Command::WhenExited(reply) => {
            if connection.messages.is_some() {
                connection.exit_waiters.push(reply);
            } else {
                let _gone = reply.send(());
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
