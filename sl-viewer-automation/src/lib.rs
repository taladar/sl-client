//! The viewer side of automation: a **semantic model** of the UI.
//!
//! Tests used to address widgets by `Name` string and read text through a
//! helper; a browser test addresses them by **role and accessible name** and
//! reads their **state**, which is what lets it survive a layout change and
//! read like the user's intent. [`UiModel`] reads that model out of the ECS as
//! [`sl_automation_proto::UiNode`]s:
//!
//! - **role**, inferred from the widget components: `bevy_ui_widgets`'
//!   `Button` (not the prelude one), `Checkbox`, `RadioButton` and `Slider`,
//!   an `EditableText`, a non-empty `Text`, a labelled `ImageNode`, and a named
//!   container as a group;
//! - **name**: an `AccessibleLabel` override, else the Fluent key of a
//!   `Translated` label (the key and the resolved text are both kept), else
//!   the descendant text; the entity's `Name` is the test id;
//! - **states**: disabled (`InteractionDisabled` on the node **or an
//!   ancestor**), read-only, checked, selected, focused, hovered; the **value**
//!   of text fields and sliders;
//! - **geometry and visibility**: bounds in logical pixels, and the first
//!   reason a node cannot be seen — hidden, clipped out of a scroll area, off
//!   screen, or covered (a UI hit test at its centre lands on something else).
//!
//! The model is computed on request, never per frame, so a viewer with
//! automation installed costs nothing while idle. It depends on no AccessKit
//! (which exists only under a winit window), so a windowless viewer has it too.
//!
//! On top of the model sits the **locator engine**:
//!
//! - [`find_all`] and [`find_one`] resolve a locator against a snapshot — the
//!   scope first and strictly, then the matches in reading order, then `nth`.
//!   An action's resolution is strict: several matches are an error listing
//!   every candidate, never "the first".
//! - A [`Pursuit`] is one action's wait for its node, polled once a frame. It
//!   makes the node actionable where a user would — scrolling a scroll area or
//!   a virtual list to it, paging a virtual list until the row is bound — and
//!   checks, in order, that it is attached, visible, in the viewport, stable,
//!   enabled, editable (for typing) and not covered, reporting the first check
//!   that fails by name. Once it passes, the [`Target`] carries the point to
//!   aim at: the centre of the node's visible part.
//! - A [`Route`] is the gestures that reach a node only an opened menu shows:
//!   a menu path, a combo's option, a pie slice. It says what to do where; the
//!   caller does it through the real input path.
//! - [`open_floater`] opens a window by id, as a debug run's
//!   `SL_VIEWER_OPEN_FLOATER` does, and names it as a scope.
//!
//! Beside the UI sits the **world model**: [`WorldModel`] reads the objects,
//! avatars and attachments the viewer tracks as
//! [`sl_automation_proto::WorldNode`]s — ids, name, owner, region-local
//! placement, link set, attachment point, sit state, selection, floating text
//! and name tag — from the world layers' own bookkeeping. The one thing those
//! do not keep is an object's name and owner, which arrive in property replies;
//! [`WorldModelPlugin`] collects them into [`ObjectFacts`]. [`find_world`]
//! resolves a [`sl_automation_proto::WorldLocator`], and a [`WorldQuery`] waits
//! for it, asking the simulator for the properties of the objects it cannot
//! judge yet.
//!
//! A world action aims through [`WorldAim`]: it resolves the one thing, waits
//! for the camera to hold still, and asks the viewer's own pick resolver
//! (`sl_viewer_world_api::PickProbes`) about candidate points on the thing's
//! box, so a click is aimed only where it lands on *that* thing — and when no
//! point does, the camera frames the thing once and the aim starts over.
//! [`WorldTarget::input`] is the gesture, for the synthetic input to play.
//!
//! Build mode has its own gestures, judged by the build tool's own resolvers:
//! a [`ManipulatorDrag`] drags a transform handle by a stated amount, on a
//! stated side of the snap guide, with the modifier keys held that pick the
//! rig; a [`WorldSweep`] draws the rubber band that selects exactly the things
//! a locator names.
//!
//! Beside both sit the **state probes**: what a test asserts on that is not
//! one widget, read from the models the viewer already keeps — the
//! conversations ([`read_conversations`]), the notifications and the buttons
//! they offer ([`read_notifications`]), the status bar ([`read_status`]), the
//! own agent ([`read_agent`]), the environment drawn ([`read_environment`]),
//! the selection ([`read_selection`]), an inventory folder by path
//! ([`read_inventory`]) and whether the scene has settled
//! ([`read_quiescence`], with [`PipelineStatus`] for the render half). The models that live in the viewer's heavy crates are read through
//! [`ProbeSources`], which the viewer's assembly fills. What happened is read
//! by cursor: the [`EventLog`] of session events, outbound commands and UI
//! actions, and the warnings and errors a [`LogTally`] counted
//! ([`read_diagnostics`]). [`request_screenshot`] captures the primary window
//! with the boxes of a locator's matches outlined. [`StateProbesPlugin`]
//! installs what records.
//!
//! Over all of it sits the **executor**: [`AutomationPlugin`] takes the
//! protocol's requests from the [`AutomationQueue`], carries each out across
//! as many frames as it takes — resolve, wait for actionability, play the
//! input through the synthetic input, wait for its frames, confirm — and
//! answers each with a response, several in flight at once; the requests that
//! play input take turns. A failure's response carries a report: the tree
//! around the scope, the event tail, the warnings logged meanwhile.
//!
//! A **transport** moves requests into that queue and responses out of it.
//! The remote one is a [`RemoteEndpoint`]: line-delimited JSON on a private
//! Unix socket, served by [`RemoteAutomationPlugin`], so a test, the command
//! line tool or an agent drives a viewer running in its own process. The
//! in-process one is an [`InProcessTransport`]: it hosts viewer Apps in the
//! test's own process, steps them while a caller waits, and hands the same
//! requests to their executors directly. Both keep their clients' ids apart
//! the same way. An [`InProcessHost`] runs an in-process transport on a
//! thread of its own and steps its viewers continuously, as a process runs,
//! so an async caller reaches each through a [`ViewerLink`] — a request
//! channel and a message channel, the shape a socket connection has.
//!
//! The cheap test tiers need no transport: [`in_app`] submits the same
//! requests to an `&mut App`'s own executor and steps the app until they are
//! answered, so a fixture test clicks, fills and expects through locators and
//! fails with the driver's own error, printed as the end-to-end tier prints
//! it.
//!
//! The same model feeds **screen readers**: [`AccessKitBridgePlugin`] builds
//! the viewer's AccessKit tree from it while an assistive technology listens,
//! sending only what changed, so one audit of roles and names serves both.

mod diagnostics;
mod event_log;
mod executor;
pub mod in_app;
mod in_process;
mod in_process_host;
mod locate;
mod manipulator_drag;
mod probe_sources;
mod probes;
mod pursuit;
mod relay;
mod remote;
mod render_settle;
mod reveal;
mod route;
mod screen_reader;
mod screenshot;
mod ui_model;
mod world_aim;
mod world_model;
mod world_query;
mod world_sweep;

pub use crate::diagnostics::{
    DiagnosticsSource, LogTally, LogTallyLayer, RECENT_LINES, diagnostics_cursor, read_diagnostics,
};
pub use crate::event_log::{DETAIL_LIMIT, EventLog, EventLogPlugin};
pub use crate::executor::{
    AutomationIdentity, AutomationPlugin, AutomationQueue, AutomationSystems, NOTIFICATION_ENTRIES,
    REPORT_DIAGNOSTICS, REPORT_EVENTS,
};
pub use crate::in_process::{
    DEFAULT_PATIENCE, FRAME_PAUSE, HostedApp, IN_PROCESS_ID_BASE, InProcessError,
    InProcessTransport, ViewerHandle,
};
pub use crate::in_process_host::{BuildError, HostError, InProcessHost, ViewerLink};
pub use crate::locate::{find_all, find_one, shallow};
pub use crate::manipulator_drag::{DragProgress, DragStage, HeldKeys, ManipulatorDrag};
pub use crate::probe_sources::{LiveNotifications, ProbeSources, SceneWorkReader};
pub use crate::probes::{
    ProbeError, read_agent, read_conversations, read_environment, read_inventory,
    read_notifications, read_quiescence, read_selection, read_status,
};
pub use crate::pursuit::{
    DEFAULT_DEADLINE, DEFAULT_DEADLINE_FRAMES, Intent, Progress, Pursuit, PursuitError, Target,
};
pub use crate::remote::{
    REMOTE_ID_BASE, RemoteAutomationPlugin, RemoteEndpoint, SocketError, default_socket_path,
};
pub use crate::render_settle::{PipelineStatus, PipelineStatusPlugin};
pub use crate::reveal::{open_floater, scroll_into_view};
pub use crate::route::{Gesture, Route, RouteProgress};
pub use crate::screen_reader::{
    ACCESSKIT_REFRESH, AccessKitBridgePlugin, AccessKitTree, accesskit_node, accesskit_role,
    tree_nodes,
};
pub use crate::screenshot::{
    CapturedFrame, OVERLAY_COLOUR, OVERLAY_THICKNESS, ScreenshotError, ScreenshotProbePlugin,
    ScreenshotTicket, Screenshots, request_screenshot, take_screenshot,
};
pub use crate::ui_model::{UiModel, entity_of, node_id, snapshot};
pub use crate::world_aim::{
    AimProgress, AimStage, ScreenProjection, WorldAim, WorldIntent, WorldTarget, screen_projection,
};
pub use crate::world_model::{ObjectFacts, WorldModel, WorldModelPlugin, world_snapshot};
pub use crate::world_query::{WorldProgress, WorldQuery, WorldWant, find_world};
pub use crate::world_sweep::{SweepProgress, SweepTarget, WorldSweep};

/// Installs what the state probes record as the app runs: the [`EventLog`],
/// the render-settle [`PipelineStatus`] and the [`Screenshots`] store. The
/// readers themselves need nothing installed — they read the models the
/// viewer keeps anyway, when asked.
///
/// Added with automation, never by default: the event log clones every
/// message it records.
#[derive(Debug, Default)]
pub struct StateProbesPlugin;

impl bevy::app::Plugin for StateProbesPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_plugins((EventLogPlugin, PipelineStatusPlugin, ScreenshotProbePlugin));
    }
}
