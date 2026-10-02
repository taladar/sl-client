//! The **in-process transport**: viewers as Apps inside the test process,
//! driven by the same [`Request`]s the remote transport carries and answered
//! with the same [`ViewerMessage`]s — faster to start than a viewer process,
//! debuggable in one process, and free to hold a viewer's clock.
//!
//! - **It owns the Apps and steps them.** An [`InProcessTransport`] hosts any
//!   number of viewers, each its own App ([`HostedApp`]) with its own
//!   executor, and while a caller waits it steps every live one a frame per
//!   round, pausing [`FRAME_PAUSE`] between rounds: a login, an asset fetch
//!   and a CAPS long-poll progress on other threads, and a loop that never
//!   sleeps starves them. A wait on one viewer is therefore never a pause for
//!   the others — what they were asked goes on, and their answers wait in
//!   their inboxes.
//! - **Requests go straight to the executor.** A request is put into the
//!   viewer's [`AutomationQueue`] at once, renumbered from
//!   [`IN_PROCESS_ID_BASE`] as the remote transport renumbers its clients',
//!   and its answer comes back under the caller's id; a duplicate id in
//!   flight, a subscription already running or the end of one that is not are
//!   refused the same way.
//! - **A wait never hangs.** Beside a request's own deadline, which the
//!   executor keeps, a wait here gives up after the transport's patience
//!   ([`DEFAULT_PATIENCE`] unless set) and names the viewer; a viewer that
//!   exits fails every wait on it instead of leaving it to the patience.
//! - **Apps must stay on the thread that built them** (a Bevy `App` is not
//!   `Send`), so the transport steps them on the caller's thread; a caller
//!   that wants them elsewhere builds them, and the transport, there.
//! - **A dropped transport finishes its viewers' GPU work**: it steps each
//!   until no render pipeline is queued or compiling, since a pipeline still
//!   inside the driver when the process exits crashes it after the test has
//!   passed.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use sl_automation_proto::{Request, RequestId, Response, ViewerMessage};

use crate::executor::AutomationQueue;
use crate::relay::Relay;
use crate::render_settle::PipelineStatus;

/// The first id the in-process transport gives a request in a viewer's
/// [`AutomationQueue`]: below the remote transport's
/// ([`crate::REMOTE_ID_BASE`]), and above anything else that submits to the
/// same queue.
pub const IN_PROCESS_ID_BASE: u64 = 1 << 47;

/// How long the transport sleeps between rounds of frames while a caller
/// waits.
pub const FRAME_PAUSE: Duration = Duration::from_millis(2);

/// How long a wait lasts before the transport gives up on the viewer: long
/// past any request's own deadline, so it only ever ends a wait on a viewer
/// that has stopped answering.
pub const DEFAULT_PATIENCE: Duration = Duration::from_secs(300);

/// The most frames a dropped transport steps a viewer while its pipelines
/// finish compiling — a cold shader cache takes a few hundred.
const DRAIN_FRAMES: u32 = 2000;

/// A viewer App the transport can host: the App, and how to step it a frame.
///
/// A plain [`App`] steps with [`App::update`]; the viewer's own builder
/// wraps its App to step it inside the viewer's log span.
pub trait HostedApp {
    /// The App, to read.
    fn app(&self) -> &App;

    /// The App, to submit to and read from between frames.
    fn app_mut(&mut self) -> &mut App;

    /// Step one frame.
    fn step(&mut self) {
        self.app_mut().update();
    }
}

impl HostedApp for App {
    fn app(&self) -> &App {
        self
    }

    fn app_mut(&mut self) -> &mut App {
        self
    }
}

/// One viewer the transport hosts, by the order it was hosted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViewerHandle(usize);

impl ViewerHandle {
    /// The handle of the viewer hosted `index`-th — for a host that keeps
    /// its viewers, each in a transport of its own, under handles of its own.
    pub(crate) const fn from_index(index: usize) -> Self {
        Self(index)
    }

    /// The order it was hosted in.
    pub(crate) const fn index(self) -> usize {
        self.0
    }
}

/// Why a request through the in-process transport got no answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InProcessError {
    /// The handle names no viewer of this transport.
    #[error("no viewer {} is hosted by this transport", .0.0)]
    NoViewer(ViewerHandle),
    /// The App has no executor to take requests.
    #[error(
        "viewer {label} has no automation queue: build it with the automation executor installed"
    )]
    NoExecutor {
        /// The viewer.
        label: String,
    },
    /// The viewer exited with nothing left to deliver.
    #[error("viewer {label} exited before {what}")]
    Exited {
        /// The viewer.
        label: String,
        /// What was being waited for.
        what: String,
    },
    /// The viewer did not deliver within the transport's patience.
    #[error("viewer {label} gave no {what} within {}s", .waited.as_secs_f32())]
    NoAnswer {
        /// The viewer.
        label: String,
        /// What was being waited for.
        what: String,
        /// How long the transport waited.
        waited: Duration,
    },
}

/// One hosted viewer.
#[derive(Debug)]
struct Hosted<A> {
    /// Its name in errors.
    label: String,
    /// The App.
    app: A,
    /// The transport's requests in its queue; the transport is its one
    /// client.
    relay: Relay<()>,
    /// What it answered or notified that the caller has not taken yet, in
    /// the order it arrived.
    inbox: VecDeque<ViewerMessage>,
    /// Whether it has exited: it is no longer stepped.
    exited: bool,
}

impl<A: HostedApp> Hosted<A> {
    /// Step a frame, unless it has exited, and move what the executor
    /// answered into the inbox.
    fn step(&mut self) {
        if self.exited {
            return;
        }
        self.app.step();
        self.exited = self.app.app().should_exit().is_some();
        if self.relay.is_idle() {
            return;
        }
        let world = self.app.app_mut().world_mut();
        let Some(mut queue) = world.get_resource_mut::<AutomationQueue>() else {
            return;
        };
        let delivered = self.relay.deliver(&mut queue);
        self.inbox
            .extend(delivered.into_iter().map(|((), message)| message));
    }

    /// Step until no render pipeline is queued or compiling, at most
    /// [`DRAIN_FRAMES`] frames — exited or not.
    fn drain_pipelines(&mut self) {
        let Some(status) = self
            .app
            .app()
            .world()
            .get_resource::<PipelineStatus>()
            .cloned()
        else {
            return;
        };
        for _frame in 0..DRAIN_FRAMES {
            if status.waiting() == 0 {
                return;
            }
            self.app.step();
        }
        error!(
            "viewer {} still had {} pipeline(s) compiling after {DRAIN_FRAMES} frames; the \
             process may crash on exit",
            self.label,
            status.waiting()
        );
    }
}

/// Viewers as Apps in this process, driven by protocol requests: it hosts
/// them, hands each request to its viewer's executor, and steps every live
/// viewer a frame per round while a caller waits for an answer.
#[derive(Debug)]
pub struct InProcessTransport<A: HostedApp> {
    /// The hosted viewers, by handle.
    viewers: Vec<Hosted<A>>,
    /// How long a wait lasts.
    patience: Duration,
}

impl<A: HostedApp> Default for InProcessTransport<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: HostedApp> Drop for InProcessTransport<A> {
    fn drop(&mut self) {
        for viewer in &mut self.viewers {
            viewer.drain_pipelines();
        }
    }
}

impl<A: HostedApp> InProcessTransport<A> {
    /// A transport hosting nothing yet, waiting [`DEFAULT_PATIENCE`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            viewers: Vec::new(),
            patience: DEFAULT_PATIENCE,
        }
    }

    /// The same, giving up on a viewer after `patience` instead.
    #[must_use]
    pub const fn with_patience(mut self, patience: Duration) -> Self {
        self.patience = patience;
        self
    }

    /// Host `app` under `label`, the name errors call it by. The App must be
    /// built — and, for a viewer that renders, finished — with the automation
    /// executor installed.
    ///
    /// # Errors
    ///
    /// [`InProcessError::NoExecutor`] when it has no [`AutomationQueue`].
    pub fn host(
        &mut self,
        label: impl Into<String>,
        app: A,
    ) -> Result<ViewerHandle, InProcessError> {
        let label = label.into();
        if app
            .app()
            .world()
            .get_resource::<AutomationQueue>()
            .is_none()
        {
            return Err(InProcessError::NoExecutor { label });
        }
        let mut relay = Relay::new(IN_PROCESS_ID_BASE);
        relay.connect(());
        self.viewers.push(Hosted {
            label,
            app,
            relay,
            inbox: VecDeque::new(),
            exited: false,
        });
        Ok(ViewerHandle(self.viewers.len().saturating_sub(1)))
    }

    /// The viewer `viewer`.
    fn hosted(&mut self, viewer: ViewerHandle) -> Result<&mut Hosted<A>, InProcessError> {
        self.viewers
            .get_mut(viewer.0)
            .ok_or(InProcessError::NoViewer(viewer))
    }

    /// The App of `viewer`, to read.
    #[must_use]
    pub fn app(&self, viewer: ViewerHandle) -> Option<&A> {
        self.viewers.get(viewer.0).map(|hosted| &hosted.app)
    }

    /// The App of `viewer`, for what a test does to it besides requests.
    pub fn app_mut(&mut self, viewer: ViewerHandle) -> Option<&mut A> {
        self.viewers.get_mut(viewer.0).map(|hosted| &mut hosted.app)
    }

    /// Send `request` to `viewer`: into its queue now, to start in its next
    /// frame. The answer arrives under the request's id; a refusal (a
    /// duplicate id in flight) arrives the same way, at once.
    ///
    /// # Errors
    ///
    /// [`InProcessError::NoViewer`] for a handle of another transport.
    pub fn send(&mut self, viewer: ViewerHandle, request: Request) -> Result<(), InProcessError> {
        let hosted = self.hosted(viewer)?;
        let world = hosted.app.app_mut().world_mut();
        let Some(mut queue) = world.get_resource_mut::<AutomationQueue>() else {
            return Err(InProcessError::NoExecutor {
                label: hosted.label.clone(),
            });
        };
        if let Some(refusal) = hosted.relay.submit((), request, &mut queue) {
            hosted.inbox.push_back(refusal);
        }
        Ok(())
    }

    /// Step every viewer that has not exited one frame, in the order they
    /// were hosted, and collect what each answered.
    pub fn step(&mut self) {
        for viewer in &mut self.viewers {
            viewer.step();
        }
    }

    /// Everything `viewer` delivered that nobody has taken yet, in the order
    /// it arrived, without stepping anything.
    ///
    /// # Errors
    ///
    /// [`InProcessError::NoViewer`] for a handle of another transport.
    pub fn take_messages(
        &mut self,
        viewer: ViewerHandle,
    ) -> Result<Vec<ViewerMessage>, InProcessError> {
        Ok(self.hosted(viewer)?.inbox.drain(..).collect())
    }

    /// Whether `viewer` has exited: it is stepped no more, and what it
    /// delivered before is all it ever will.
    ///
    /// # Errors
    ///
    /// [`InProcessError::NoViewer`] for a handle of another transport.
    pub fn has_exited(&self, viewer: ViewerHandle) -> Result<bool, InProcessError> {
        self.viewers
            .get(viewer.0)
            .map(|hosted| hosted.exited)
            .ok_or(InProcessError::NoViewer(viewer))
    }

    /// The name errors call `viewer` by.
    #[must_use]
    pub fn label(&self, viewer: ViewerHandle) -> Option<&str> {
        self.viewers
            .get(viewer.0)
            .map(|hosted| hosted.label.as_str())
    }

    /// Step rounds of frames until `found` takes something out of `viewer`'s
    /// inbox.
    ///
    /// # Errors
    ///
    /// [`InProcessError::NoViewer`] for a handle of another transport,
    /// [`InProcessError::Exited`] once the viewer has exited and nothing it
    /// delivered is taken, [`InProcessError::NoAnswer`] after the patience.
    fn wait_for<T>(
        &mut self,
        viewer: ViewerHandle,
        what: &str,
        mut found: impl FnMut(&mut VecDeque<ViewerMessage>) -> Option<T>,
    ) -> Result<T, InProcessError> {
        let started = Instant::now();
        loop {
            let patience = self.patience;
            let hosted = self.hosted(viewer)?;
            if let Some(value) = found(&mut hosted.inbox) {
                return Ok(value);
            }
            if hosted.exited {
                return Err(InProcessError::Exited {
                    label: hosted.label.clone(),
                    what: what.to_owned(),
                });
            }
            let waited = started.elapsed();
            if waited >= patience {
                return Err(InProcessError::NoAnswer {
                    label: hosted.label.clone(),
                    what: what.to_owned(),
                    waited,
                });
            }
            self.step();
            std::thread::sleep(FRAME_PAUSE);
        }
    }

    /// The next message `viewer` sends — a response or a notification — in
    /// the order it arrived, stepping every viewer until there is one.
    ///
    /// # Errors
    ///
    /// As [`response`](Self::response).
    pub fn receive(&mut self, viewer: ViewerHandle) -> Result<ViewerMessage, InProcessError> {
        self.wait_for(viewer, "message", VecDeque::pop_front)
    }

    /// The answer to the request `id` sent to `viewer`, stepping every viewer
    /// until it comes; whatever else arrives meanwhile stays for
    /// [`receive`](Self::receive).
    ///
    /// # Errors
    ///
    /// [`InProcessError::NoViewer`] for a handle of another transport,
    /// [`InProcessError::Exited`] when the viewer exits unanswered,
    /// [`InProcessError::NoAnswer`] after the patience.
    pub fn response(
        &mut self,
        viewer: ViewerHandle,
        id: RequestId,
    ) -> Result<Response, InProcessError> {
        self.wait_for(viewer, &format!("answer to request {}", id.0), |inbox| {
            let index = inbox.iter().position(
                |message| matches!(message, ViewerMessage::Response(response) if response.id == id),
            )?;
            match inbox.remove(index) {
                Some(ViewerMessage::Response(response)) => Some(*response),
                _ => None,
            }
        })
    }

    /// [`send`](Self::send) `request` and wait for its
    /// [`response`](Self::response).
    ///
    /// # Errors
    ///
    /// As [`response`](Self::response).
    pub fn request(
        &mut self,
        viewer: ViewerHandle,
        request: Request,
    ) -> Result<Response, InProcessError> {
        let id = request.id;
        self.send(viewer, request)?;
        self.response(viewer, id)
    }
}

#[cfg(test)]
mod tests;
