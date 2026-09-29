//! [`Viewer`]: a handle on one viewer, whichever transport reaches it — the
//! requests every other handle is built from, the viewer-wide verbs and the
//! state probes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use sl_automation_proto::{
    AgentReadout, AutomationError, ConversationReadout, Deadline, DiagnosticsReadout,
    InventoryFolderReadout, InventoryRoot, Locator, LogEntry, LogPage, LogStream,
    NotificationReadout, PROTOCOL_VERSION, Probe, ProbeReadout, QuiescenceReadout, Request,
    RequestBody, ResponseBody, SelectedObject, StateCondition, StateObservation, StatusReadout,
    UiNode, ViewerIdentity, ViewerMessage,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::artifacts::{Subject, save};
use crate::connection::Connection;
use crate::error::{DriverError, Failure};
use crate::events::{EventCursor, EventStream};
use crate::expect::{ChatExpect, NotificationExpect, StateExpect, UiExpect, WorldExpect};
use crate::ui::{Ui, UiLocator};
use crate::world::{World, WorldHandle};

/// How long an action or an expectation waits by default: for its node to
/// become actionable, or its condition to hold.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long past a request's own deadline the driver waits for the viewer to
/// answer before it gives up on the viewer — which then has stopped
/// answering, since the viewer answers every request by its deadline.
pub const DEFAULT_GRACE: Duration = Duration::from_secs(30);

/// How a [`Viewer`] handle behaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerOptions {
    /// The viewer's name in errors and artifact paths.
    pub label: String,
    /// Where a failure's screenshot, tree and event tail are saved, in a
    /// directory per failure; nothing is saved without one.
    pub artifact_dir: Option<PathBuf>,
    /// How long an action or an expectation waits unless told otherwise.
    pub timeout: Duration,
    /// How long past a request's own deadline to wait for its answer.
    pub grace: Duration,
}

impl ViewerOptions {
    /// Options for a viewer called `label`, saving no artifacts, with the
    /// default timeout and grace.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            artifact_dir: None,
            timeout: DEFAULT_TIMEOUT,
            grace: DEFAULT_GRACE,
        }
    }

    /// Save failure artifacts under `dir`.
    #[must_use]
    pub fn with_artifacts(mut self, dir: impl Into<PathBuf>) -> Self {
        self.artifact_dir = Some(dir.into());
        self
    }

    /// Wait `timeout` by default.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// What the handles share.
#[derive(Debug)]
struct Inner {
    /// The channel to the viewer.
    connection: Connection,
    /// How the handle behaves.
    options: ViewerOptions,
    /// Who answered the hello.
    identity: ViewerIdentity,
    /// How many failures have saved artifacts, to number the next.
    failures: AtomicU32,
}

/// A handle on one viewer, over either transport: cheap to clone, and every
/// clone drives the same viewer.
///
/// Built with [`Viewer::connect`] for a viewer with an automation socket, or
/// [`Viewer::over_link`] for one hosted in this process. Either way the
/// viewer answers a hello first, naming itself and its protocol version.
#[derive(Debug, Clone)]
pub struct Viewer {
    /// What the handles share.
    inner: Arc<Inner>,
}

impl Viewer {
    /// Connect to the viewer whose automation socket is at `path`.
    ///
    /// # Errors
    ///
    /// [`DriverError::Connect`] when the socket cannot be reached, and as
    /// [`Viewer::over_link`] for the hello.
    pub async fn connect(path: &Path, options: ViewerOptions) -> Result<Self, DriverError> {
        let connection = Connection::connect(options.label.clone(), path).await?;
        Self::greet(connection, options).await
    }

    /// Drive the viewer at the other end of a channel pair — an in-process
    /// viewer's link: `requests` to it, `messages` from it. Must be called
    /// inside a Tokio runtime.
    ///
    /// # Errors
    ///
    /// [`DriverError::Protocol`] when the viewer speaks another protocol
    /// version, and [`DriverError::Closed`] / [`DriverError::NoAnswer`] when
    /// it does not answer the hello.
    pub async fn over_link(
        requests: UnboundedSender<Request>,
        messages: UnboundedReceiver<ViewerMessage>,
        options: ViewerOptions,
    ) -> Result<Self, DriverError> {
        let connection = Connection::over(options.label.clone(), requests, messages);
        Self::greet(connection, options).await
    }

    /// Ask the viewer who it is, and keep the answer.
    async fn greet(connection: Connection, options: ViewerOptions) -> Result<Self, DriverError> {
        let response = connection
            .request(RequestBody::Hello, options.grace)
            .await?;
        let label = connection.label().to_owned();
        let (protocol, identity) = match response.result {
            Ok(ResponseBody::Hello { protocol, viewer }) => (protocol, viewer),
            other => {
                return Err(DriverError::Unexpected {
                    viewer: label,
                    what: "hello".to_owned(),
                    got: format!("{other:?}"),
                });
            }
        };
        if protocol != PROTOCOL_VERSION {
            return Err(DriverError::Protocol {
                viewer: label,
                expected: PROTOCOL_VERSION,
                found: protocol,
            });
        }
        Ok(Self {
            inner: Arc::new(Inner {
                connection,
                options,
                identity,
                failures: AtomicU32::new(0),
            }),
        })
    }

    /// The viewer's label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.inner.options.label
    }

    /// Who the viewer said it is when the handle connected. The agent it
    /// names is the one logged in then; ask [`hello`](Self::hello) for now.
    #[must_use]
    pub fn identity(&self) -> &ViewerIdentity {
        &self.inner.identity
    }

    /// The handle's options.
    #[must_use]
    pub fn options(&self) -> &ViewerOptions {
        &self.inner.options
    }

    /// The deadline a request waiting at most `timeout` carries: wall-clock
    /// only, since a viewer stepped as fast as it renders counts frames
    /// faster than a person's.
    pub(crate) fn deadline(timeout: Duration) -> Deadline {
        Deadline {
            frames: Some(u32::MAX),
            millis: Some(u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)),
        }
    }

    /// Send `body`, waiting up to `timeout` in the viewer, and return what
    /// it answered; a failure in the viewer saves the artifacts of
    /// `subject` and comes back as [`DriverError::Failed`] naming `action`.
    pub(crate) async fn ask(
        &self,
        body: RequestBody,
        timeout: Duration,
        action: &str,
        subject: Subject<'_>,
    ) -> Result<ResponseBody, DriverError> {
        let patience = timeout.saturating_add(self.inner.options.grace);
        let response = self.inner.connection.request(body, patience).await?;
        match response.result {
            Ok(body) => Ok(body),
            Err(error) => Err(self.fail(action, error, response.report, subject).await),
        }
    }

    /// Send `body`, answered at once by the viewer, and return its answer as
    /// it came — a failure saves nothing. What saving artifacts asks with.
    pub(crate) async fn raw(
        &self,
        body: RequestBody,
    ) -> Result<Result<ResponseBody, AutomationError>, DriverError> {
        let response = self
            .inner
            .connection
            .request(body, self.inner.options.grace)
            .await?;
        Ok(response.result)
    }

    /// Send `body`, answered at once by the viewer.
    pub(crate) async fn ask_now(
        &self,
        body: RequestBody,
        action: &str,
    ) -> Result<ResponseBody, DriverError> {
        self.ask(body, Duration::ZERO, action, Subject::Viewer)
            .await
    }

    /// The failure of `action` with `error`, its artifacts saved.
    pub(crate) async fn fail(
        &self,
        action: &str,
        error: AutomationError,
        report: Option<sl_automation_proto::FailureReport>,
        subject: Subject<'_>,
    ) -> DriverError {
        let artifacts = match &self.inner.options.artifact_dir {
            Some(dir) => {
                let number = self
                    .inner
                    .failures
                    .fetch_add(1, Ordering::Relaxed)
                    .saturating_add(1);
                save(self, dir, number, action, &error, report.as_ref(), subject).await
            }
            None => crate::error::Artifacts::default(),
        };
        DriverError::Failed(Box::new(Failure {
            viewer: self.label().to_owned(),
            action: action.to_owned(),
            error,
            report,
            artifacts,
        }))
    }

    /// The error for an answer of the wrong kind to `what`.
    pub(crate) fn unexpected(&self, what: &str, got: &ResponseBody) -> DriverError {
        DriverError::Unexpected {
            viewer: self.label().to_owned(),
            what: what.to_owned(),
            got: format!("{got:?}"),
        }
    }

    /// The UI, to find nodes in.
    #[must_use]
    pub fn ui(&self) -> Ui {
        Ui::new(self.clone())
    }

    /// The node `locator` names.
    #[must_use]
    pub fn locator(&self, locator: Locator) -> UiLocator {
        UiLocator::new(self.clone(), locator)
    }

    /// The world, to find things in.
    #[must_use]
    pub fn world(&self) -> World {
        World::new(self.clone())
    }

    /// Expect something of the nodes `locator` names.
    #[must_use]
    pub fn expect(&self, locator: &UiLocator) -> UiExpect {
        UiExpect::new(locator.clone())
    }

    /// Expect something of the things `handle` names.
    #[must_use]
    pub fn expect_world(&self, handle: &WorldHandle) -> WorldExpect {
        WorldExpect::new(handle.clone())
    }

    /// Expect something of the chat and instant-message transcripts.
    #[must_use]
    pub fn expect_chat(&self) -> ChatExpect {
        ChatExpect::new(self.clone())
    }

    /// Expect something of the notifications.
    #[must_use]
    pub fn expect_notification(&self) -> NotificationExpect {
        NotificationExpect::new(self.clone())
    }

    /// Expect a probe's readout to come to hold a value.
    #[must_use]
    pub fn expect_state(&self, probe: Probe) -> StateExpect {
        StateExpect::new(self.clone(), probe)
    }

    /// Who is answering now: the protocol version and the viewer's identity,
    /// with the agent logged in now.
    ///
    /// # Errors
    ///
    /// As any request.
    pub async fn hello(&self) -> Result<ViewerIdentity, DriverError> {
        match self.ask_now(RequestBody::Hello, "hello").await? {
            ResponseBody::Hello { viewer, .. } => Ok(viewer),
            other => Err(self.unexpected("hello", &other)),
        }
    }

    /// The whole semantic UI tree.
    ///
    /// # Errors
    ///
    /// As any request.
    pub async fn snapshot(&self) -> Result<Vec<UiNode>, DriverError> {
        match self
            .ask_now(RequestBody::Snapshot { within: None }, "snapshot")
            .await?
        {
            ResponseBody::Snapshot { roots } => Ok(roots),
            other => Err(self.unexpected("snapshot", &other)),
        }
    }

    /// Press a key or a chord — `Enter`, `Ctrl+Shift+S` — on whatever has the
    /// focus.
    ///
    /// # Errors
    ///
    /// As any request; [`AutomationError::InvalidRequest`] for keys the
    /// viewer cannot parse.
    pub async fn press(&self, keys: &str) -> Result<(), DriverError> {
        let action = format!("press {keys}");
        self.ask_now(
            RequestBody::Press {
                keys: keys.to_owned(),
            },
            &action,
        )
        .await
        .map(drop)
    }

    /// Walk a menu path from the menu bar by the entries' Fluent keys, the
    /// bar menu's first, and click the last entry.
    ///
    /// # Errors
    ///
    /// As any action.
    pub async fn menu_path(&self, path: &[&str]) -> Result<UiNode, DriverError> {
        let timeout = self.inner.options.timeout;
        let action = format!("menu path {}", path.join(" > "));
        match self
            .ask(
                RequestBody::MenuPath {
                    path: path.iter().map(|&key| key.to_owned()).collect(),
                    deadline: Self::deadline(timeout),
                },
                timeout,
                &action,
                Subject::Viewer,
            )
            .await?
        {
            ResponseBody::Done { node } => Ok(node),
            other => Err(self.unexpected(&action, &other)),
        }
    }

    /// Click a slice of the open pie menu.
    ///
    /// # Errors
    ///
    /// As any action.
    pub async fn pie_slice(&self, slice: Locator) -> Result<UiNode, DriverError> {
        let timeout = self.inner.options.timeout;
        let action = format!("pie slice {slice}");
        match self
            .ask(
                RequestBody::PieSlice {
                    slice: slice.clone(),
                    deadline: Self::deadline(timeout),
                },
                timeout,
                &action,
                Subject::Ui(&slice),
            )
            .await?
        {
            ResponseBody::Done { node } => Ok(node),
            other => Err(self.unexpected(&action, &other)),
        }
    }

    /// Open the window whose floater id this is (`inventory`,
    /// `preferences`), the way a debug run's `SL_VIEWER_OPEN_FLOATER` does,
    /// and name it as a scope.
    ///
    /// # Errors
    ///
    /// As any request; [`AutomationError::NotFound`] for an unknown id.
    pub async fn open_floater(&self, floater: &str) -> Result<UiLocator, DriverError> {
        let action = format!("open floater {floater}");
        match self
            .ask_now(
                RequestBody::OpenFloater {
                    floater: floater.to_owned(),
                },
                &action,
            )
            .await?
        {
            ResponseBody::Opened { window } => Ok(self.locator(window)),
            other => Err(self.unexpected(&action, &other)),
        }
    }

    /// Read one of the state probes.
    ///
    /// # Errors
    ///
    /// As any request; [`AutomationError::Unavailable`] when the viewer has
    /// no such model.
    pub async fn read(&self, probe: Probe) -> Result<ProbeReadout, DriverError> {
        let action = format!("read {probe}");
        match self.ask_now(RequestBody::Read { probe }, &action).await? {
            ResponseBody::Readout { readout } => Ok(readout),
            other => Err(self.unexpected(&action, &other)),
        }
    }

    /// The own agent: region, position, seat, teleport, camera.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read).
    pub async fn agent(&self) -> Result<AgentReadout, DriverError> {
        match self.read(Probe::Agent).await? {
            ProbeReadout::Agent(agent) => Ok(agent),
            other => Err(self.unexpected("read agent", &ResponseBody::Readout { readout: other })),
        }
    }

    /// What the status bar shows.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read).
    pub async fn status(&self) -> Result<StatusReadout, DriverError> {
        match self.read(Probe::Status).await? {
            ProbeReadout::Status(status) => Ok(status),
            other => Err(self.unexpected("read status", &ResponseBody::Readout { readout: other })),
        }
    }

    /// Every open conversation and its transcript, Nearby first.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read).
    pub async fn conversations(&self) -> Result<Vec<ConversationReadout>, DriverError> {
        match self.read(Probe::Conversations).await? {
            ProbeReadout::Conversations(conversations) => Ok(conversations),
            other => Err(self.unexpected(
                "read conversations",
                &ResponseBody::Readout { readout: other },
            )),
        }
    }

    /// Every notification in the viewer's history, oldest first.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read).
    pub async fn notifications(&self) -> Result<Vec<NotificationReadout>, DriverError> {
        match self.read(Probe::Notifications).await? {
            ProbeReadout::Notifications(notifications) => Ok(notifications),
            other => Err(self.unexpected(
                "read notifications",
                &ResponseBody::Readout { readout: other },
            )),
        }
    }

    /// The edit selection, the primary last.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read).
    pub async fn selection(&self) -> Result<Vec<SelectedObject>, DriverError> {
        match self.read(Probe::Selection).await? {
            ProbeReadout::Selection(selection) => Ok(selection),
            other => {
                Err(self.unexpected("read selection", &ResponseBody::Readout { readout: other }))
            }
        }
    }

    /// One inventory folder by path — each segment a sub-folder's exact name
    /// — or nothing while no folder is there.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read).
    pub async fn inventory(
        &self,
        root: InventoryRoot,
        path: &[&str],
    ) -> Result<Option<InventoryFolderReadout>, DriverError> {
        let probe = Probe::Inventory {
            root,
            path: path.iter().map(|&segment| segment.to_owned()).collect(),
        };
        match self.read(probe).await? {
            ProbeReadout::Inventory(folder) => Ok(folder),
            other => {
                Err(self.unexpected("read inventory", &ResponseBody::Readout { readout: other }))
            }
        }
    }

    /// Whether the scene has settled.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read).
    pub async fn quiescence(&self) -> Result<QuiescenceReadout, DriverError> {
        match self.read(Probe::Quiescence).await? {
            ProbeReadout::Quiescence(quiescence) => Ok(quiescence),
            other => {
                Err(self.unexpected("read quiescence", &ResponseBody::Readout { readout: other }))
            }
        }
    }

    /// Wait up to `timeout` for `condition` over the viewer's state, and
    /// return what made it hold.
    ///
    /// # Errors
    ///
    /// As any request; [`AutomationError::StateTimedOut`] when it does not
    /// come to hold.
    pub async fn wait_for_state(
        &self,
        condition: StateCondition,
        timeout: Duration,
    ) -> Result<StateObservation, DriverError> {
        let action = format!("wait for {condition}");
        match self
            .ask(
                RequestBody::WaitForState {
                    condition,
                    deadline: Self::deadline(timeout),
                },
                timeout,
                &action,
                Subject::Viewer,
            )
            .await?
        {
            ResponseBody::StateHeld { observed } => Ok(observed),
            other => Err(self.unexpected(&action, &other)),
        }
    }

    /// Wait up to `timeout` for the scene to settle: a region up, nothing
    /// the scene asked for outstanding, no render pipeline compiling.
    ///
    /// # Errors
    ///
    /// As [`wait_for_state`](Self::wait_for_state).
    pub async fn wait_until_quiet(&self, timeout: Duration) -> Result<(), DriverError> {
        self.wait_for_state(StateCondition::Quiet, timeout)
            .await
            .map(drop)
    }

    /// Read the event log from `cursor` (`0` for everything kept), only the
    /// entries of `streams` (every stream when empty).
    ///
    /// # Errors
    ///
    /// As any request.
    pub async fn read_log(
        &self,
        cursor: u64,
        streams: &[LogStream],
    ) -> Result<LogPage<LogEntry>, DriverError> {
        match self
            .ask_now(
                RequestBody::ReadLog {
                    cursor,
                    streams: streams.to_vec(),
                    limit: None,
                },
                "read the event log",
            )
            .await?
        {
            ResponseBody::Log { page } => Ok(page),
            other => Err(self.unexpected("read the event log", &other)),
        }
    }

    /// A cursor into the event log, past everything recorded so far.
    ///
    /// # Errors
    ///
    /// As any request.
    pub async fn events(&self) -> Result<EventCursor, DriverError> {
        let page = self.read_log(0, &[]).await?;
        Ok(EventCursor::new(self.clone(), page.next))
    }

    /// Stream the event log's entries of `streams` (every stream when empty)
    /// from now on.
    ///
    /// # Errors
    ///
    /// As any request.
    pub async fn subscribe(&self, streams: &[LogStream]) -> Result<EventStream, DriverError> {
        let (id, response, pages) = self
            .inner
            .connection
            .subscribe(
                RequestBody::Subscribe {
                    cursor: None,
                    streams: streams.to_vec(),
                },
                self.inner.options.grace,
            )
            .await?;
        match response.result {
            Ok(ResponseBody::Subscribed { .. }) => Ok(EventStream::new(self.clone(), id, pages)),
            Ok(other) => Err(self.unexpected("subscribe", &other)),
            Err(error) => Err(self
                .fail("subscribe", error, response.report, Subject::Viewer)
                .await),
        }
    }

    /// End the subscription `id`.
    pub(crate) fn unsubscribe(&self, id: sl_automation_proto::RequestId) {
        self.inner.connection.unsubscribe(id);
    }

    /// The warnings and errors the viewer logged, from `cursor` (`0` for
    /// everything kept).
    ///
    /// # Errors
    ///
    /// As any request.
    pub async fn diagnostics(&self, cursor: u64) -> Result<DiagnosticsReadout, DriverError> {
        match self
            .ask_now(RequestBody::ReadDiagnostics { cursor }, "read diagnostics")
            .await?
        {
            ResponseBody::Diagnostics { readout } => Ok(readout),
            other => Err(self.unexpected("read diagnostics", &other)),
        }
    }

    /// Capture the viewer's window to a PNG at `path` (made absolute), with
    /// the boxes of `outline`'s matches outlined.
    ///
    /// # Errors
    ///
    /// As any request; [`AutomationError::ScreenshotFailed`] when the viewer
    /// has no window to capture.
    pub async fn screenshot(
        &self,
        path: &Path,
        outline: Option<Locator>,
    ) -> Result<Screenshot, DriverError> {
        let absolute = std::path::absolute(path).unwrap_or_else(|_error| path.to_owned());
        let action = format!("screenshot {}", absolute.display());
        match self
            .ask_now(
                RequestBody::Screenshot {
                    path: absolute.display().to_string(),
                    outline,
                },
                &action,
            )
            .await?
        {
            ResponseBody::Screenshot {
                path,
                width,
                height,
                outlined,
            } => Ok(Screenshot {
                path: PathBuf::from(path),
                width,
                height,
                outlined,
            }),
            other => Err(self.unexpected(&action, &other)),
        }
    }
}

/// A screenshot the viewer wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct Screenshot {
    /// Where the PNG is.
    pub path: PathBuf,
    /// Its width in physical pixels.
    pub width: u32,
    /// Its height in physical pixels.
    pub height: u32,
    /// The nodes whose boxes are outlined.
    pub outlined: Vec<UiNode>,
}
