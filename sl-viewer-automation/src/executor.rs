//! [`AutomationPlugin`]: the in-viewer request executor. It takes each
//! [`Request`] submitted to the [`AutomationQueue`], carries it out across as
//! many frames as it takes — resolve, wait for actionability, play the input,
//! wait for its frames, confirm — and answers with a [`Response`], however the
//! request arrived.
//!
//! - **Several requests are in flight at once.** Reads and waits run side by
//!   side; the requests that play input (a click, a fill, a key press, a menu
//!   path, a world action, a handle drag, a rubber band) take turns in the
//!   order they were submitted, because one pointer cannot be in two places.
//!   Each is answered on its own.
//! - **Waits live here.** A wait is a predicate evaluated each frame with a
//!   frame and a wall-clock deadline: over a UI locator's matches, a world
//!   locator's, or the viewer's state (quiet, a probe's value, an entry in the
//!   event log after a cursor). A timeout carries the last thing observed.
//! - **A failure explains itself.** Every error response carries a
//!   [`FailureReport`]: the semantic tree around a UI locator's scope, the
//!   event tail, and the warnings and errors logged while the request ran.
//! - **Subscriptions stream the event log.** A subscription answers at once
//!   and then, each frame something new was recorded, queues a
//!   [`Notification`] with the entries under its id, until it is ended.
//! - **Off by default.** The plugin is added only when automation is asked
//!   for — a runtime switch in the viewer's assembly, never a Cargo feature —
//!   and costs one resource check a frame while nothing is submitted.

mod keys;
mod state;
mod ui;
mod world;

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use sl_automation_proto::{
    AutomationError, Deadline, FailureReport, Locator, LogStream, Notification, Request,
    RequestBody, RequestId, Response, ResponseBody, UiNode,
};
use sl_viewer_ui_core::synthetic_input::{
    ActionStatus, InputAction, InputActionId, SyntheticInput,
};

use crate::diagnostics::{diagnostics_cursor, read_diagnostics};
use crate::event_log::EventLog;
use crate::locate::find_one;
use crate::pursuit::{DEFAULT_DEADLINE, DEFAULT_DEADLINE_FRAMES, PursuitError};
use crate::route::Route;
use crate::ui_model::snapshot;
use crate::world_model::WorldModelPlugin;

/// How many of the event log's last entries a failure report carries.
pub const REPORT_EVENTS: usize = 32;

/// How many of the warnings and errors logged while a request ran a failure
/// report carries — the last ones.
pub const REPORT_DIAGNOSTICS: usize = 32;

/// How many levels below the scope a failure report's tree excerpt goes.
const EXCERPT_DEPTH: usize = 4;

/// How many levels of the whole tree an excerpt shows when the locator has
/// no scope that resolves.
const EXCERPT_TOP_DEPTH: usize = 3;

/// The most nodes a failure report's tree excerpt holds.
const EXCERPT_NODES: usize = 300;

/// The most entries one subscription notification carries; the rest follow
/// in the next frame's.
pub const NOTIFICATION_ENTRIES: usize = 256;

/// The ordering of automation's systems in `Last`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AutomationSystems {
    /// The event log records the frame's messages.
    Record,
    /// The executor advances every request in flight — after the recording,
    /// so a wait on the log sees this frame's entries.
    Execute,
}

/// The in-viewer request executor, with everything it reads installed: the
/// state probes' recorders ([`crate::StateProbesPlugin`]) and the world
/// model's property collector ([`WorldModelPlugin`]), unless the app has them
/// already.
///
/// Requests go in and responses come out through the [`AutomationQueue`]
/// resource; a transport is whatever moves them between it and a test.
#[derive(Debug, Default)]
pub struct AutomationPlugin;

impl Plugin for AutomationPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<crate::StateProbesPlugin>() {
            app.add_plugins(crate::StateProbesPlugin);
        }
        if !app.is_plugin_added::<WorldModelPlugin>() {
            app.add_plugins(WorldModelPlugin);
        }
        app.init_resource::<AutomationQueue>()
            .init_resource::<Executor>()
            .configure_sets(
                Last,
                AutomationSystems::Execute.after(AutomationSystems::Record),
            )
            .add_systems(Last, run_requests.in_set(AutomationSystems::Execute));
    }
}

/// Where requests are submitted and their responses collected.
///
/// A request submitted during a frame starts in that frame's `Last`, so a
/// transport may submit at any point of the frame. Responses stay until
/// taken.
#[derive(Resource, Debug, Default)]
pub struct AutomationQueue {
    /// Requests not yet started, oldest first.
    submitted: VecDeque<Request>,
    /// Responses not yet taken, in the order they were answered.
    answered: VecDeque<Response>,
    /// Subscription notifications not yet taken, in the order they were
    /// sent.
    notified: VecDeque<Notification>,
}

impl AutomationQueue {
    /// Submit `request`; its response arrives under its id.
    pub fn submit(&mut self, request: Request) {
        self.submitted.push_back(request);
    }

    /// Take the response to the request `id`, once there is one.
    pub fn take_response(&mut self, id: RequestId) -> Option<Response> {
        let index = self
            .answered
            .iter()
            .position(|response| response.id == id)?;
        self.answered.remove(index)
    }

    /// Take the responses to the requests `ours` claims, in the order they
    /// were answered, leaving the rest.
    pub(crate) fn take_responses(
        &mut self,
        mut ours: impl FnMut(RequestId) -> bool,
    ) -> Vec<Response> {
        let (taken, kept): (Vec<Response>, Vec<Response>) = self
            .answered
            .drain(..)
            .partition(|response| ours(response.id));
        self.answered = kept.into();
        taken
    }

    /// Take every response not yet taken, in the order they were answered.
    pub fn drain_responses(&mut self) -> Vec<Response> {
        self.answered.drain(..).collect()
    }

    /// Whether nothing is waiting in it: no request to start, no response or
    /// notification to take.
    #[cfg(test)]
    pub(crate) fn is_idle(&self) -> bool {
        self.submitted.is_empty() && self.answered.is_empty() && self.notified.is_empty()
    }

    /// Take the notifications of the subscription started by the request
    /// `subscription`, in the order they were sent.
    pub fn take_notifications(&mut self, subscription: RequestId) -> Vec<Notification> {
        let mut taken = Vec::new();
        self.notified.retain(|notification| {
            let ours = matches!(
                notification,
                Notification::Log { subscription: id, .. } if *id == subscription
            );
            if ours {
                taken.push(notification.clone());
            }
            !ours
        });
        taken
    }
}

/// What a request's poll came to this frame.
enum Step {
    /// Not done: poll again next frame.
    Pending,
    /// Done: the answer.
    Answer(Box<Answer>),
}

/// A request's answer, its error boxed: the executor passes it about by value
/// and most answers succeed.
type Answer = Result<ResponseBody, Box<AutomationError>>;

impl Step {
    /// A failure.
    fn fail(error: impl Into<Box<AutomationError>>) -> Self {
        Self::Answer(Box::new(Err(error.into())))
    }

    /// A success.
    fn done(body: ResponseBody) -> Self {
        Self::Answer(Box::new(Ok(body)))
    }
}

/// A request under way.
enum Task {
    /// On the UI.
    Ui(Box<ui::UiTask>),
    /// On the world.
    World(Box<world::WorldTask>),
    /// On the viewer's state.
    State(Box<state::StateTask>),
}

impl Task {
    /// A UI task.
    fn ui(task: ui::UiTask) -> Self {
        Self::Ui(Box::new(task))
    }

    /// A world task.
    fn world(task: world::WorldTask) -> Self {
        Self::World(Box::new(task))
    }

    /// A state task.
    fn state(task: state::StateTask) -> Self {
        Self::State(Box::new(task))
    }

    /// Whether it plays input, and so must take its turn.
    const fn acts(&self) -> bool {
        match self {
            Self::Ui(task) => task.acts(),
            Self::World(task) => task.acts(),
            Self::State(_task) => false,
        }
    }

    /// Advance it by a frame.
    fn poll(&mut self, world: &mut World) -> Step {
        match self {
            Self::Ui(task) => task.poll(world),
            Self::World(task) => task.poll(world),
            Self::State(task) => task.poll(world),
        }
    }
}

/// One request in flight.
struct InFlight {
    /// Its id.
    id: RequestId,
    /// Its work.
    task: Task,
    /// The diagnostics cursor when it started: a failure reports what was
    /// logged since.
    diagnostics_from: u64,
}

/// Who this viewer is, as a hello reports it beside the protocol version, the
/// process and the agent. The viewer's assembly inserts it; without it a hello
/// names an unknown viewer.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct AutomationIdentity {
    /// The viewer program.
    pub viewer: String,
    /// Its version.
    pub version: String,
    /// The grid it logs in to, when it logs in to one.
    pub grid: Option<String>,
    /// The name of the avatar it logs in as, when it logs in.
    pub agent_name: Option<String>,
}

impl Default for AutomationIdentity {
    fn default() -> Self {
        Self {
            viewer: "unknown".to_owned(),
            version: "unknown".to_owned(),
            grid: None,
            agent_name: None,
        }
    }
}

/// One event log subscription.
struct Subscription {
    /// The id of the request that started it.
    id: RequestId,
    /// The next sequence number to send.
    cursor: u64,
    /// Only entries of these streams; every stream when empty.
    streams: Vec<LogStream>,
}

/// The requests in flight, which of them has the input, and the
/// subscriptions.
#[derive(Resource, Default)]
struct Executor {
    /// In submission order.
    in_flight: Vec<InFlight>,
    /// The request playing input now; the others that act wait their turn.
    acting: Option<RequestId>,
    /// In the order they were started.
    subscriptions: Vec<Subscription>,
}

/// Start what was submitted and advance everything in flight by a frame.
fn run_requests(world: &mut World) {
    let submitted: Vec<Request> = world
        .resource_mut::<AutomationQueue>()
        .submitted
        .drain(..)
        .collect();
    let executor = world.resource::<Executor>();
    if submitted.is_empty() && executor.in_flight.is_empty() && executor.subscriptions.is_empty() {
        return;
    }
    world.resource_scope(|world, mut executor: Mut<'_, Executor>| {
        let mut answers: Vec<Response> = Vec::new();
        for Request { id, body } in submitted {
            let diagnostics_from = diagnostics_cursor(world);
            if executor.in_flight.iter().any(|flight| flight.id == id) {
                let error = AutomationError::InvalidRequest {
                    reason: format!("request {} is already in flight", id.0),
                };
                answers.push(respond(world, id, Err(Box::new(error)), diagnostics_from));
                continue;
            }
            match start(world, &mut executor.subscriptions, id, body) {
                Started::Answered(answer) => {
                    answers.push(respond(world, id, *answer, diagnostics_from));
                }
                Started::Running(task) => executor.in_flight.push(InFlight {
                    id,
                    task,
                    diagnostics_from,
                }),
            }
        }
        let executor = &mut *executor;
        let mut index = 0;
        while let Some((id, acts)) = executor
            .in_flight
            .get(index)
            .map(|flight| (flight.id, flight.task.acts()))
        {
            if acts {
                match executor.acting {
                    Some(owner) if owner != id => {
                        index = index.saturating_add(1);
                        continue;
                    }
                    _ => executor.acting = Some(id),
                }
            }
            let Some(flight) = executor.in_flight.get_mut(index) else {
                break;
            };
            match flight.task.poll(world) {
                Step::Pending => index = index.saturating_add(1),
                Step::Answer(answer) => {
                    let done = executor.in_flight.remove(index);
                    if executor.acting == Some(done.id) {
                        executor.acting = None;
                    }
                    answers.push(respond(world, done.id, *answer, done.diagnostics_from));
                }
            }
        }
        let notifications = notify(world, &mut executor.subscriptions);
        let mut queue = world.resource_mut::<AutomationQueue>();
        queue.answered.extend(answers);
        queue.notified.extend(notifications);
    });
}

/// Each subscription's notification of what the event log recorded since it
/// last sent one, and its cursor moved past it.
fn notify(world: &World, subscriptions: &mut [Subscription]) -> Vec<Notification> {
    let Some(log) = world.get_resource::<EventLog>() else {
        return Vec::new();
    };
    let mut notifications = Vec::new();
    for subscription in subscriptions {
        if log.cursor() <= subscription.cursor {
            continue;
        }
        let page = log.read(
            subscription.cursor,
            &subscription.streams,
            NOTIFICATION_ENTRIES,
        );
        subscription.cursor = page.next;
        if !page.entries.is_empty() || page.dropped > 0 {
            notifications.push(Notification::Log {
                subscription: subscription.id,
                page,
            });
        }
    }
    notifications
}

/// What starting a request came to.
enum Started {
    /// Answered at once.
    Answered(Box<Answer>),
    /// Under way.
    Running(Task),
}

impl Started {
    /// Answered at once with `answer`.
    fn answered(answer: Answer) -> Self {
        Self::Answered(Box::new(answer))
    }
}

/// Start `body`, the request `id`: answer it at once when it needs no frames,
/// else set up its task. A subscription is started or ended in
/// `subscriptions`.
fn start(
    world: &mut World,
    subscriptions: &mut Vec<Subscription>,
    id: RequestId,
    body: RequestBody,
) -> Started {
    match body {
        RequestBody::Snapshot { within } => Started::answered(ui::read_snapshot(world, within)),
        RequestBody::Find { locator } => Started::answered(ui::find(world, &locator)),
        RequestBody::OpenFloater { floater } => Started::answered(ui::open(world, &floater)),
        RequestBody::Click {
            locator,
            button,
            double,
            deadline,
        } => ui::act(locator, ui::UiActKind::Click { button, double }, deadline),
        RequestBody::DragTo {
            source,
            target,
            deadline,
        } => ui::drag_to(source, target, deadline),
        RequestBody::DragBy {
            source,
            offset,
            deadline,
        } => ui::drag_by(source, offset, deadline),
        RequestBody::Hover { locator, deadline } => {
            ui::act(locator, ui::UiActKind::Hover, deadline)
        }
        RequestBody::Fill {
            locator,
            text,
            deadline,
        } => ui::act(locator, ui::UiActKind::Fill(text), deadline),
        RequestBody::Press { keys } => ui::press(&keys),
        RequestBody::MenuPath { path, deadline } => ui::menu_path(&path, deadline),
        RequestBody::SelectOption {
            combo,
            option,
            deadline,
        } => ui::route(Route::select_option(combo, option).with_deadline(deadline)),
        RequestBody::PieSlice { slice, deadline } => {
            ui::route(Route::pie_slice(slice).with_deadline(deadline))
        }
        RequestBody::WaitFor {
            locator,
            condition,
            deadline,
        } => ui::wait(locator, condition, deadline),
        RequestBody::FindWorld { locator, deadline } => world::find(locator, deadline),
        RequestBody::WaitForWorld {
            locator,
            condition,
            deadline,
        } => world::wait(locator, condition, deadline),
        RequestBody::WorldAction {
            locator,
            action,
            reveal,
            deadline,
        } => world::act(locator, action, reveal, deadline),
        RequestBody::DragHandle {
            handle,
            amount,
            snap,
            modifiers,
            deadline,
        } => world::drag(&handle, amount, snap, modifiers, deadline),
        RequestBody::Sweep { locator, deadline } => world::sweep(locator, deadline),
        RequestBody::Read { probe } => Started::answered(
            state::read(world, &probe).map(|readout| ResponseBody::Readout { readout }),
        ),
        RequestBody::ReadLog {
            cursor,
            streams,
            limit,
        } => Started::answered(state::read_log(world, cursor, &streams, limit)),
        RequestBody::ReadDiagnostics { cursor } => {
            Started::answered(Ok(ResponseBody::Diagnostics {
                readout: read_diagnostics(world, cursor),
            }))
        }
        RequestBody::WaitForState {
            condition,
            deadline,
        } => state::wait(condition, deadline),
        RequestBody::Screenshot { path, outline } => state::screenshot(world, path, outline),
        RequestBody::AnswerFileDialog { path, deadline } => {
            state::answer_file_dialog(path, deadline)
        }
        RequestBody::Hello => Started::answered(Ok(state::hello(world))),
        RequestBody::Subscribe { cursor, streams } => {
            Started::answered(subscribe(world, subscriptions, id, cursor, streams))
        }
        RequestBody::Unsubscribe { subscription } => {
            let before = subscriptions.len();
            subscriptions.retain(|kept| kept.id != subscription);
            Started::answered(if subscriptions.len() < before {
                Ok(ResponseBody::Unsubscribed)
            } else {
                Err(Box::new(AutomationError::InvalidRequest {
                    reason: format!("there is no subscription {}", subscription.0),
                }))
            })
        }
    }
}

/// Start the subscription `id` to the event log from `cursor` (from now on
/// when absent).
///
/// # Errors
///
/// [`AutomationError::Unavailable`] when the app keeps no event log, and
/// [`AutomationError::InvalidRequest`] when `id` is a subscription already.
fn subscribe(
    world: &World,
    subscriptions: &mut Vec<Subscription>,
    id: RequestId,
    cursor: Option<u64>,
    streams: Vec<LogStream>,
) -> Answer {
    let log = world.get_resource::<EventLog>().ok_or_else(|| {
        Box::new(AutomationError::Unavailable {
            what: "event log".to_owned(),
        })
    })?;
    if subscriptions
        .iter()
        .any(|subscription| subscription.id == id)
    {
        return Err(Box::new(AutomationError::InvalidRequest {
            reason: format!("subscription {} is already running", id.0),
        }));
    }
    let cursor = cursor.unwrap_or_else(|| log.cursor());
    subscriptions.push(Subscription {
        id,
        cursor,
        streams,
    });
    Ok(ResponseBody::Subscribed { cursor })
}

/// The response to `id`, with a failure report on an error.
fn respond(world: &mut World, id: RequestId, answer: Answer, diagnostics_from: u64) -> Response {
    let report = match &answer {
        Ok(_body) => None,
        Err(error) => Some(failure_report(world, error, diagnostics_from)),
    };
    Response {
        id,
        result: answer.map_err(|error| *error),
        report,
    }
}

/// What a failed request's response carries beside the error: the tree
/// around a UI locator's scope, the event tail, the warnings logged since
/// `diagnostics_from`.
fn failure_report(
    world: &mut World,
    error: &AutomationError,
    diagnostics_from: u64,
) -> FailureReport {
    let tree = ui_locator(error)
        .map(|locator| tree_excerpt(world, locator))
        .unwrap_or_default();
    let events = world
        .get_resource::<EventLog>()
        .map_or_else(Vec::new, |log| {
            let from = log
                .cursor()
                .saturating_sub(u64::try_from(REPORT_EVENTS).unwrap_or(u64::MAX));
            log.read(from, &[], REPORT_EVENTS).entries
        });
    let mut diagnostics = read_diagnostics(world, diagnostics_from).lines.entries;
    let excess = diagnostics.len().saturating_sub(REPORT_DIAGNOSTICS);
    diagnostics.drain(..excess);
    FailureReport {
        tree,
        events,
        diagnostics,
    }
}

/// The UI locator an error is about, when it is about one.
const fn ui_locator(error: &AutomationError) -> Option<&Locator> {
    match error {
        AutomationError::NotFound { locator }
        | AutomationError::Ambiguous { locator, .. }
        | AutomationError::NotActionable { locator, .. }
        | AutomationError::TimedOut { locator, .. }
        | AutomationError::FillMismatch { locator, .. } => Some(locator),
        AutomationError::WorldAmbiguous { .. }
        | AutomationError::WorldNotActionable { .. }
        | AutomationError::WorldTimedOut { .. }
        | AutomationError::ManipulatorRefused { .. }
        | AutomationError::ManipulatorTimedOut { .. }
        | AutomationError::SweepInexact { .. }
        | AutomationError::StateTimedOut { .. }
        | AutomationError::InventoryFolderNotFound { .. }
        | AutomationError::Unavailable { .. }
        | AutomationError::InvalidRequest { .. }
        | AutomationError::ScreenshotFailed { .. }
        | AutomationError::NoFileDialog { .. } => None,
    }
}

/// The semantic tree around `locator`'s scope: the scope's subtree when it
/// has a scope that resolves, else the top of the whole tree — cut off
/// [`EXCERPT_DEPTH`] (or [`EXCERPT_TOP_DEPTH`]) levels down and after
/// [`EXCERPT_NODES`] nodes. Empty when the model cannot be read.
fn tree_excerpt(world: &mut World, locator: &Locator) -> Vec<UiNode> {
    let Ok(roots) = snapshot(world) else {
        return Vec::new();
    };
    let mut budget = EXCERPT_NODES;
    if let Some(scope) = &locator.within
        && let Ok(node) = find_one(&roots, scope)
    {
        return prune(core::slice::from_ref(node), EXCERPT_DEPTH, &mut budget);
    }
    prune(&roots, EXCERPT_TOP_DEPTH, &mut budget)
}

/// `nodes` and their descendants down to `depth` levels, at most `budget`
/// nodes in all, in reading order.
fn prune(nodes: &[UiNode], depth: usize, budget: &mut usize) -> Vec<UiNode> {
    let mut kept = Vec::new();
    for node in nodes {
        if *budget == 0 || depth == 0 {
            break;
        }
        *budget = budget.saturating_sub(1);
        let children = prune(&node.children, depth.saturating_sub(1), budget);
        kept.push(UiNode {
            children,
            ..node.clone()
        });
    }
    kept
}

/// A request's own frame and wall-clock deadline, for the waits that are not
/// a pursuit's.
#[derive(Debug)]
struct Clock {
    /// The most frames.
    max_frames: u32,
    /// The most wall-clock time.
    max_time: Duration,
    /// When the first tick ran.
    started: Option<Instant>,
    /// The ticks so far.
    frames: u32,
}

impl Clock {
    /// A clock that runs out after `deadline`; an unset limit is the default.
    fn new(deadline: Deadline) -> Self {
        Self {
            max_frames: deadline.frames.unwrap_or(DEFAULT_DEADLINE_FRAMES),
            max_time: deadline
                .millis
                .map_or(DEFAULT_DEADLINE, Duration::from_millis),
            started: None,
            frames: 0,
        }
    }

    /// Count a frame.
    fn tick(&mut self) {
        let _started = self.started.get_or_insert_with(Instant::now);
        self.frames = self.frames.saturating_add(1);
    }

    /// The frames and milliseconds waited, once the deadline has passed.
    fn expired(&self) -> Option<(u32, u64)> {
        let waited = self
            .started
            .map_or(Duration::ZERO, |started| started.elapsed());
        (self.frames >= self.max_frames || waited >= self.max_time).then(|| {
            (
                self.frames,
                u64::try_from(waited.as_millis()).unwrap_or(u64::MAX),
            )
        })
    }
}

/// A pursuit's failure as the protocol's error.
fn automation_error(error: PursuitError) -> AutomationError {
    match error {
        PursuitError::Automation(error) => *error,
        PursuitError::Model(error) => AutomationError::Unavailable {
            what: format!("readable semantic or world model ({error})"),
        },
    }
}

/// Queue `action` on the app's synthetic input.
///
/// # Errors
///
/// [`AutomationError::Unavailable`] when the app has none.
fn enqueue(world: &mut World, action: InputAction) -> Result<InputActionId, Box<AutomationError>> {
    let mut input = world.get_resource_mut::<SyntheticInput>().ok_or_else(|| {
        Box::new(AutomationError::Unavailable {
            what: "synthetic input injector".to_owned(),
        })
    })?;
    Ok(input.enqueue(action))
}

/// Whether the synthetic input has played `id` to its end.
fn finished(world: &World, id: InputActionId) -> bool {
    world.get_resource::<SyntheticInput>().is_none_or(|input| {
        matches!(
            input.status(id),
            ActionStatus::Done { .. } | ActionStatus::Expired | ActionStatus::Unknown
        )
    })
}

#[cfg(test)]
mod tests;
