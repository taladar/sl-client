//! Requests to a viewer and the responses it sends back.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::action::{DragAmount, DragModifiers, SnapSide, WorldAction, WorldWaitCondition};
use crate::failure::AutomationError;
use crate::locator::{Locator, NameMatcher};
use crate::probe::{DiagnosticsReadout, LogEntry, LogPage, LogStream};
use crate::report::FailureReport;
use crate::snapshot::UiNode;
use crate::state::{Probe, ProbeReadout, StateCondition, StateObservation};
use crate::world::{GroundPoint, WorldLocator, WorldNode};

/// Pairs a [`Response`] with the [`Request`] it answers, so several requests
/// may be in flight on one channel. Chosen by the requester; the viewer only
/// echoes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(pub u64);

/// A request to a viewer.
///
/// In JSON the body's fields sit beside the id, tagged by `method`:
/// `{"id":1,"method":"click","locator":{"role":"button","name":{"exact":"OK"}}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Echoed in the response.
    pub id: RequestId,
    /// What to do.
    #[serde(flatten)]
    pub body: RequestBody,
}

/// What a [`Request`] asks the viewer to do.
///
/// A request that waits — for its node to become actionable, or for a
/// condition — takes a [`Deadline`]; left out, it is the viewer's default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum RequestBody {
    /// The semantic UI tree, whole or under one node. Answered with
    /// [`ResponseBody::Snapshot`].
    Snapshot {
        /// Only the subtree of the one node this resolves to; the whole UI
        /// when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        within: Option<Locator>,
    },
    /// Every node the locator matches, without waiting and without
    /// strictness. Answered with [`ResponseBody::Found`], possibly empty.
    Find {
        /// The nodes to find.
        locator: Locator,
    },
    /// Wait until exactly one node matches and is actionable, then click it
    /// through the viewer's real input path. Answered with
    /// [`ResponseBody::Done`].
    Click {
        /// The node to click.
        locator: Locator,
        /// The button to click with.
        #[serde(default, skip_serializing_if = "PointerButton::is_primary")]
        button: PointerButton,
        /// Two clicks within the viewer's multi-click interval — a double
        /// click — rather than one.
        #[serde(default, skip_serializing_if = "is_false")]
        double: bool,
        /// When to give up waiting for it.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Wait until exactly one node matches the source and is actionable, then
    /// until exactly one matches the target and is actionable, and drag the
    /// source onto the target with the left button through the viewer's real
    /// input path: press on the source, move across in steps, rest over the
    /// target, release. Answered with [`ResponseBody::Done`] naming the
    /// target.
    DragTo {
        /// What to press on.
        source: Locator,
        /// What to release over.
        target: Locator,
        /// When to give up waiting for each.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Wait until exactly one node matches and is actionable, then drag it
    /// by `offset` with the left button through the viewer's real input
    /// path: press on it, move across in steps, rest, release — how a window
    /// is moved by its title bar or resized by its grip, where what is
    /// dragged *is* the target and no node marks where it ends. The offset is
    /// relative, in logical pixels, `x` rightwards and `y` downwards. Answered
    /// with [`ResponseBody::Done`] naming the node as it was when pressed.
    DragBy {
        /// What to press on.
        source: Locator,
        /// How far to move it, `[x, y]`.
        offset: [f32; 2],
        /// When to give up waiting for it.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Wait until exactly one node matches and is actionable (a disabled one
    /// may be hovered), then rest the pointer on it. Answered with
    /// [`ResponseBody::Done`].
    Hover {
        /// The node to hover.
        locator: Locator,
        /// When to give up waiting for it.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Wait until exactly one text field matches and is actionable and
    /// editable, then replace its text by typing: click into it, select all,
    /// delete, type. Answered with [`ResponseBody::Done`] once the field holds
    /// the text.
    Fill {
        /// The field to fill.
        locator: Locator,
        /// The text it should hold afterwards.
        text: String,
        /// When to give up waiting for it.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Press a key or a chord on the keyboard — to whatever has the focus.
    /// Answered with [`ResponseBody::Pressed`].
    Press {
        /// The key, after its modifiers, joined by `+`: `Enter`, `Escape`,
        /// `a`, `Ctrl+Shift+S`, `Alt+F4`. The named keys are `Enter`,
        /// `Escape`, `Tab`, `Space`, `Backspace`, `Delete`, `Insert`, `Home`,
        /// `End`, `PageUp`, `PageDown`, `ArrowUp`, `ArrowDown`, `ArrowLeft`,
        /// `ArrowRight` and `F1` … `F12`; the modifiers `Ctrl`, `Shift`,
        /// `Alt` and `Super`; any other key is one character.
        keys: String,
        /// How many frames the key stays down before it is let go — what
        /// moves an avatar or a flycam, which act for as long as a key is
        /// held. `0` (and `1`) is a tap: down one frame, up the next.
        #[serde(default, skip_serializing_if = "is_zero")]
        hold_frames: u32,
    },
    /// Walk a menu path from the menu bar by the entries' Fluent keys: click
    /// the bar menu open, hover each submenu open, click the last entry.
    /// Answered with [`ResponseBody::Done`] naming the last entry.
    MenuPath {
        /// The entries' Fluent keys, the bar menu's first.
        path: Vec<String>,
        /// When to give up waiting for each entry.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Pick an option from a combo box: open it unless it is open, then
    /// click the option in its list. Answered with [`ResponseBody::Done`]
    /// naming the option.
    SelectOption {
        /// The combo box.
        combo: Locator,
        /// The option, looked for inside the combo.
        option: Locator,
        /// When to give up waiting for each.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Click a slice of the open pie menu. Answered with
    /// [`ResponseBody::Done`].
    PieSlice {
        /// The slice, looked for inside the pie menu.
        slice: Locator,
        /// When to give up waiting for it.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Open the window whose stable floater id this is, as the viewer's
    /// `SL_VIEWER_OPEN_FLOATER` does — a shortcut for a test whose subject
    /// is not how the window is opened. Answered with
    /// [`ResponseBody::Opened`].
    OpenFloater {
        /// The floater's id (`inventory`, `preferences`, …).
        floater: String,
    },
    /// Wait until the locator's matches satisfy a condition. Answered with
    /// [`ResponseBody::Satisfied`].
    WaitFor {
        /// The nodes to watch.
        locator: Locator,
        /// What they must come to satisfy.
        condition: WaitCondition,
        /// When to give up.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Every in-world thing a world locator matches, waiting only for the
    /// names and owners the viewer must ask the simulator for. Answered with
    /// [`ResponseBody::FoundWorld`], possibly empty.
    FindWorld {
        /// The things to find.
        locator: WorldLocator,
        /// When to give up waiting for names and owners.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Wait until a world locator's matches satisfy a condition. Answered
    /// with [`ResponseBody::WorldSatisfied`].
    WaitForWorld {
        /// The things to watch.
        locator: WorldLocator,
        /// What they must come to satisfy.
        condition: WorldWaitCondition,
        /// When to give up.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Wait until exactly one thing matches, find a point where a click lands
    /// on it — asking the viewer's own pick resolver, and framing it with the
    /// camera when no point does — and do the action there through the real
    /// input path. Answered with [`ResponseBody::WorldDone`].
    WorldAction {
        /// The thing to act on.
        locator: WorldLocator,
        /// What to do to it.
        action: WorldAction,
        /// Whether the camera may frame the thing when no point of it takes a
        /// click; the camera stays where that leaves it.
        #[serde(default = "yes", skip_serializing_if = "is_true")]
        reveal: bool,
        /// When to give up.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Do a world action on a point of the ground: find where on screen a
    /// click lands on that ground — asking the viewer's own pick resolver,
    /// which must say the ground there, and framing the point with the camera
    /// when no click reaches it — and do the action there through the real
    /// input path. A select or shift-select has nothing to select there and
    /// is refused. Answered with [`ResponseBody::GroundDone`].
    GroundAction {
        /// The ground to act on.
        ground: GroundPoint,
        /// What to do there.
        action: WorldAction,
        /// Whether the camera may frame the point when no click reaches it;
        /// the camera stays where that leaves it.
        #[serde(default = "yes", skip_serializing_if = "is_true")]
        reveal: bool,
        /// When to give up.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Drag one of the build tool's transform handles by an amount, on a side
    /// of the snap guide, holding modifier keys — planned by the build tool
    /// itself, played through the real input path. Needs build mode with a
    /// selection. Answered with [`ResponseBody::Dragged`].
    DragHandle {
        /// The handle by its test address: `translate-x`,
        /// `translate-plane-z`, `rotate-y`, `scale-face-x-pos`,
        /// `scale-corner-pnp`.
        handle: String,
        /// How much.
        amount: DragAmount,
        /// Which side of the snap guide.
        #[serde(default, skip_serializing_if = "is_free")]
        snap: SnapSide,
        /// The modifier keys held through the drag.
        #[serde(default, skip_serializing_if = "is_unmodified")]
        modifiers: DragModifiers,
        /// When to give up waiting for the drag to start.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Draw the build tool's rubber band so that it selects exactly the
    /// things a world locator names, and wait for the selection to be them.
    /// Answered with [`ResponseBody::Swept`].
    Sweep {
        /// The things to select.
        locator: WorldLocator,
        /// When to give up.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Read one of the viewer's state readouts. Answered with
    /// [`ResponseBody::Readout`].
    Read {
        /// The readout.
        probe: Probe,
    },
    /// Read the event log from a cursor. Answered with
    /// [`ResponseBody::Log`].
    ReadLog {
        /// The first sequence number wanted; `0` for everything kept.
        #[serde(default)]
        cursor: u64,
        /// Only entries of these streams; every stream when empty.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        streams: Vec<LogStream>,
        /// At most this many entries; the page's cursor continues the read.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u32>,
    },
    /// Read the warnings and errors the viewer logged, from a cursor.
    /// Answered with [`ResponseBody::Diagnostics`].
    ReadDiagnostics {
        /// The first sequence number wanted; `0` for everything kept.
        #[serde(default)]
        cursor: u64,
    },
    /// Wait until a condition over the viewer's state holds. Answered with
    /// [`ResponseBody::StateHeld`].
    WaitForState {
        /// What must come to hold.
        condition: StateCondition,
        /// When to give up.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Capture the viewer's window — the UI over the world — as a PNG file,
    /// with the boxes of a locator's matches outlined. Answered with
    /// [`ResponseBody::Screenshot`].
    ///
    /// The frame travels as a file the viewer writes, not as bytes in the
    /// response: the requester shares the viewer's machine (the socket is
    /// local, or the viewer runs in its own process), keeps it in its
    /// artifact directory anyway, and a full-size frame would be megabytes
    /// on the channel every other response shares.
    Screenshot {
        /// Where to write the PNG: an absolute path, overwritten.
        path: String,
        /// Outline the boxes of this locator's matches.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outline: Option<Locator>,
    },
    /// Answer the file dialog the viewer is waiting on — as the user would
    /// pick in the desktop's chooser — with `path`, or Cancel it when there is
    /// none. Waits for a dialog to be asked for. Answered with
    /// [`ResponseBody::FileDialogAnswered`].
    ///
    /// Only a viewer with no window of its own waits for an answer; an
    /// interactive one shows the desktop's chooser.
    AnswerFileDialog {
        /// The file or folder picked: an absolute path. Absent for Cancel.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        /// When to give up waiting for a dialog.
        #[serde(default, skip_serializing_if = "Deadline::is_default")]
        deadline: Deadline,
    },
    /// Who is answering: the protocol version and the viewer's identity.
    /// Answered with [`ResponseBody::Hello`]. The first request a client on
    /// a socket sends, to learn whether it speaks the same protocol.
    Hello,
    /// Stream the event log: every entry recorded from the cursor on arrives
    /// as a [`Notification::Log`] under this request's id, until
    /// [`RequestBody::Unsubscribe`] or the connection closes. Answered with
    /// [`ResponseBody::Subscribed`] before the first notification.
    Subscribe {
        /// The first sequence number wanted; absent for only what is
        /// recorded from now on.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<u64>,
        /// Only entries of these streams; every stream when empty.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        streams: Vec<LogStream>,
    },
    /// End a subscription. Answered with [`ResponseBody::Unsubscribed`]; no
    /// notification for it follows the answer.
    Unsubscribe {
        /// The id of the [`RequestBody::Subscribe`] request that started it.
        subscription: RequestId,
    },
}

/// The version of this protocol a viewer speaks, reported by
/// [`RequestBody::Hello`]. Raised when a change would make an older client
/// misread a newer viewer.
pub const PROTOCOL_VERSION: u32 = 1;

/// Which viewer answered a [`RequestBody::Hello`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerIdentity {
    /// The viewer program (`sl-client-bevy-viewer`).
    pub viewer: String,
    /// Its version.
    pub version: String,
    /// The id of the viewer's process.
    pub pid: u32,
    /// The grid it logs in to, when it logs in to one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<String>,
    /// The name of the avatar it logs in as, when it logs in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    /// The agent id, once logged in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<uuid::Uuid>,
}

/// A [`RequestBody::WorldAction`]'s default: reveal.
const fn yes() -> bool {
    true
}

/// Whether a flag is set, to leave the default out of the JSON.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
const fn is_true(flag: &bool) -> bool {
    *flag
}

/// Whether a flag is unset, to leave the default out of the JSON.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
const fn is_false(flag: &bool) -> bool {
    !*flag
}

/// Whether a frame count is zero, to leave the default out of the JSON.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
const fn is_zero(count: &u32) -> bool {
    *count == 0
}

/// The mouse button a [`RequestBody::Click`] clicks with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerButton {
    /// The primary button: activate, focus, select.
    #[default]
    Left,
    /// The secondary button: a context menu.
    Right,
}

impl PointerButton {
    /// Whether this is the primary button, to leave the default out of the
    /// JSON.
    #[must_use]
    pub const fn is_primary(&self) -> bool {
        matches!(self, Self::Left)
    }
}

/// Whether a snap side is the default, to leave it out of the JSON.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
const fn is_free(side: &SnapSide) -> bool {
    matches!(side, SnapSide::Free)
}

/// Whether no modifier is held, to leave the default out of the JSON.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
const fn is_unmodified(modifiers: &DragModifiers) -> bool {
    matches!(modifiers, DragModifiers::None)
}

/// What a [`RequestBody::WaitFor`] waits for, evaluated in the viewer each
/// frame over the nodes the locator matches.
///
/// In JSON: `"visible"`, or `{"text":{"contains":"Done"}}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitCondition {
    /// At least one node matches.
    Attached,
    /// No node matches.
    Detached,
    /// At least one matching node is visible.
    Visible,
    /// No matching node is visible (including when none matches).
    Hidden,
    /// At least one node matches and none of them is disabled.
    Enabled,
    /// At least one node matches and all of them are disabled.
    Disabled,
    /// At least one matching node's text satisfies the matcher: a text
    /// field's or a label's value, else its accessible name.
    Text(NameMatcher),
}

impl fmt::Display for WaitCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Attached => f.write_str("attached"),
            Self::Detached => f.write_str("detached"),
            Self::Visible => f.write_str("visible"),
            Self::Hidden => f.write_str("hidden"),
            Self::Enabled => f.write_str("enabled"),
            Self::Disabled => f.write_str("disabled"),
            Self::Text(NameMatcher::Exact(text)) => write!(f, "text={text:?}"),
            Self::Text(NameMatcher::Contains(part)) => write!(f, "text~={part:?}"),
        }
    }
}

/// When a wait gives up: after a number of frames or of wall-clock
/// milliseconds, whichever comes first. An unset limit is the viewer's
/// default, so [`Deadline::default`] asks for the default wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deadline {
    /// The most frames to wait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<u32>,
    /// The most wall-clock milliseconds to wait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub millis: Option<u64>,
}

impl Deadline {
    /// Whether this is the default deadline, to leave it out of the JSON.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        self.frames.is_none() && self.millis.is_none()
    }
}

/// A viewer's answer to one [`Request`].
///
/// In JSON the outcome is one of two keys, `ok` or `error`:
/// `{"id":1,"ok":{"kind":"found","nodes":[]}}` or
/// `{"id":1,"error":{"kind":"not_found","locator":{}},"report":{…}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// The id of the request this answers.
    pub id: RequestId,
    /// What came of it.
    #[serde(flatten, with = "outcome")]
    pub result: Result<ResponseBody, AutomationError>,
    /// For an error, what was around it: the tree near the scope, the event
    /// tail and the warnings logged while the request ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<FailureReport>,
}

/// What a request that succeeded produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponseBody {
    /// The answer to [`RequestBody::Snapshot`]: the top-level nodes of the
    /// requested tree, each with its children.
    Snapshot {
        /// The tree's roots in reading order.
        roots: Vec<UiNode>,
    },
    /// The answer to [`RequestBody::Find`].
    Found {
        /// Every match in reading order, each without its children.
        nodes: Vec<UiNode>,
    },
    /// The answer to a UI action ([`RequestBody::Click`],
    /// [`RequestBody::DragTo`], [`RequestBody::DragBy`], [`RequestBody::Hover`],
    /// [`RequestBody::Fill`], and the routes).
    Done {
        /// The node acted on, without its children: as it was when the
        /// action was applied, or for a fill as it is once it holds the text.
        node: UiNode,
    },
    /// The answer to [`RequestBody::Press`]: the keys went down and up.
    Pressed,
    /// The answer to [`RequestBody::OpenFloater`].
    Opened {
        /// The locator of the window, the scope to find its content within.
        window: Locator,
    },
    /// The answer to [`RequestBody::WaitFor`].
    Satisfied {
        /// The matches in the frame the condition held, without children;
        /// empty for a condition that holds with no match.
        nodes: Vec<UiNode>,
    },
    /// The answer to [`RequestBody::FindWorld`].
    FoundWorld {
        /// Every match, in the resolver's order.
        nodes: Vec<WorldNode>,
    },
    /// The answer to [`RequestBody::WaitForWorld`].
    WorldSatisfied {
        /// The matches in the frame the condition held.
        nodes: Vec<WorldNode>,
    },
    /// The answer to [`RequestBody::WorldAction`].
    WorldDone {
        /// The thing acted on, as resolved.
        node: Box<WorldNode>,
        /// Where on it the pick resolver said the click landed, region-local
        /// metres; absent for a select, whose resolver names the object only.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hit_point: Option<[f32; 3]>,
    },
    /// The answer to [`RequestBody::GroundAction`].
    GroundDone {
        /// Where the pick resolver said the click lands, in the named region's
        /// own metres: the ground's height there as the viewer has it.
        hit_point: [f32; 3],
    },
    /// The answer to [`RequestBody::DragHandle`].
    Dragged {
        /// What the build tool predicted the drag does: the amount asked, or
        /// on the grid side the grid mark or detent it lands on.
        predicted: DragAmount,
    },
    /// The answer to [`RequestBody::Sweep`].
    Swept {
        /// The things the band selected.
        nodes: Vec<WorldNode>,
    },
    /// The answer to [`RequestBody::Read`].
    Readout {
        /// What the probe read.
        readout: ProbeReadout,
    },
    /// The answer to [`RequestBody::ReadLog`].
    Log {
        /// The entries from the cursor on.
        page: LogPage<LogEntry>,
    },
    /// The answer to [`RequestBody::ReadDiagnostics`].
    Diagnostics {
        /// The counts and the lines from the cursor on.
        readout: DiagnosticsReadout,
    },
    /// The answer to [`RequestBody::WaitForState`].
    StateHeld {
        /// What made the condition hold.
        observed: StateObservation,
    },
    /// The answer to [`RequestBody::Screenshot`].
    Screenshot {
        /// Where the PNG was written.
        path: String,
        /// Its width in physical pixels.
        width: u32,
        /// Its height in physical pixels.
        height: u32,
        /// The nodes whose boxes are outlined, without children.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        outlined: Vec<UiNode>,
    },
    /// The answer to [`RequestBody::AnswerFileDialog`]: the dialog that was
    /// answered.
    FileDialogAnswered {
        /// What the viewer asked the file for: the purpose its reply is tagged
        /// with (`settings-editor-import-sky`, `bulk-import-skies`, …).
        purpose: String,
        /// The dialog's title.
        title: String,
        /// Whether it asked for a folder rather than a file.
        folder: bool,
    },
    /// The answer to [`RequestBody::Hello`].
    Hello {
        /// The [`PROTOCOL_VERSION`] the viewer speaks.
        protocol: u32,
        /// Which viewer it is.
        viewer: ViewerIdentity,
    },
    /// The answer to [`RequestBody::Subscribe`].
    Subscribed {
        /// The sequence number the stream starts at.
        cursor: u64,
    },
    /// The answer to [`RequestBody::Unsubscribe`].
    Unsubscribed,
}

/// Something a viewer sends that answers no request: the entries of a
/// subscription, or the refusal of a line that was not a request.
///
/// In JSON, tagged by `notification`:
/// `{"notification":"log","subscription":3,"page":{"entries":[…],"next":9}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "notification", rename_all = "snake_case")]
pub enum Notification {
    /// Entries a subscription's stream recorded, oldest first.
    Log {
        /// The id of the [`RequestBody::Subscribe`] request.
        subscription: RequestId,
        /// The entries; its `dropped` counts the ones the log lost before
        /// the stream could send them.
        page: LogPage<LogEntry>,
    },
    /// A line that was not a request and carried no id to answer under.
    Rejected {
        /// Why it was refused.
        reason: String,
    },
}

/// One line a viewer sends on a connection: a [`Response`] or a
/// [`Notification`], told apart by the `notification` key.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ViewerMessage {
    /// The answer to a request, boxed: most lines on a busy connection are
    /// small notifications.
    Response(Box<Response>),
    /// A subscription's entries, or a refused line.
    Notification(Notification),
}

impl<'de> Deserialize<'de> for ViewerMessage {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.get("notification").is_some() {
            Notification::deserialize(value)
                .map(Self::Notification)
                .map_err(D::Error::custom)
        } else {
            Response::deserialize(value)
                .map(|response| Self::Response(Box::new(response)))
                .map_err(D::Error::custom)
        }
    }
}

/// The JSON shape of [`Response::result`]: `{"ok": …}` or `{"error": …}`
/// rather than serde's default `{"Ok": …}` / `{"Err": …}`.
mod outcome {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::failure::AutomationError;
    use crate::message::ResponseBody;

    /// The borrowed form written out.
    #[derive(Serialize)]
    #[serde(rename_all = "snake_case")]
    enum Borrowed<'a> {
        /// A success.
        Ok(&'a ResponseBody),
        /// A failure.
        Error(&'a AutomationError),
    }

    /// The owned form read back.
    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Owned {
        /// A success.
        Ok(ResponseBody),
        /// A failure.
        Error(AutomationError),
    }

    /// Writes a result as `{"ok": …}` or `{"error": …}`.
    pub(super) fn serialize<S: Serializer>(
        result: &Result<ResponseBody, AutomationError>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match result {
            Ok(body) => Borrowed::Ok(body),
            Err(error) => Borrowed::Error(error),
        }
        .serialize(serializer)
    }

    /// Reads a result written by [`serialize`].
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Result<ResponseBody, AutomationError>, D::Error> {
        Ok(match Owned::deserialize(deserializer)? {
            Owned::Ok(body) => Ok(body),
            Owned::Error(error) => Err(error),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use pretty_assertions::assert_eq;
    use serde::Serialize;
    use serde::de::DeserializeOwned;

    use super::{
        Deadline, Notification, PROTOCOL_VERSION, PointerButton, Request, RequestBody, RequestId,
        Response, ResponseBody, ViewerIdentity, ViewerMessage, WaitCondition,
    };
    use crate::action::{DragAmount, DragModifiers, SnapSide, WorldAction, WorldWaitCondition};
    use crate::failure::{ActionabilityCheck, AutomationError};
    use crate::locator::{Locator, NameMatcher};
    use crate::probe::{
        DiagnosticLine, DiagnosticsReadout, InventoryRoot, LogEntry, LogLevel, LogPage, LogStream,
        QuiescenceReadout,
    };
    use crate::report::FailureReport;
    use crate::snapshot::{Bounds, NodeId, NodeState, NodeValue, NodeVisibility, Role, UiNode};
    use crate::state::{Probe, ProbeReadout, StateCondition, StateObservation, ValueTest};
    use crate::world::{GroundPoint, WorldKind, WorldLocator, WorldNode};

    /// Serializes `value`, reads it back and checks nothing was lost.
    fn round_trip<T>(value: &T) -> Result<String, serde_json::Error>
    where
        T: Serialize + DeserializeOwned + PartialEq + core::fmt::Debug,
    {
        let json = serde_json::to_string(value)?;
        let back: T = serde_json::from_str(&json)?;
        assert_eq!(&back, value, "{json}");
        Ok(json)
    }

    /// A text field with every optional part filled, and one child.
    fn full_node() -> UiNode {
        UiNode {
            id: NodeId(42),
            role: Role::Textbox,
            name: Some("Display name".to_owned()),
            name_key: Some("profile-display-name".to_owned()),
            test_id: Some("profile.display_name".to_owned()),
            states: [NodeState::Focused, NodeState::ReadOnly]
                .into_iter()
                .collect::<BTreeSet<_>>(),
            value: Some(NodeValue::Text("Avatar".to_owned())),
            color: None,
            level: Some(2),
            accelerator: Some("Ctrl+P".to_owned()),
            bounds: Bounds {
                x: 10.5,
                y: 20.25,
                width: 200.0,
                height: 18.0,
            },
            visibility: NodeVisibility::Covered,
            children: vec![bare_node()],
        }
    }

    /// A slider with nothing optional set.
    fn bare_node() -> UiNode {
        UiNode {
            id: NodeId(43),
            role: Role::Slider,
            name: None,
            name_key: None,
            test_id: None,
            states: BTreeSet::new(),
            value: Some(NodeValue::Number(0.3)),
            color: None,
            level: None,
            accelerator: None,
            bounds: Bounds::default(),
            visibility: NodeVisibility::Visible,
            children: Vec::new(),
        }
    }

    /// An unnamed prim with nothing optional set.
    fn world_node() -> WorldNode {
        WorldNode {
            kind: WorldKind::Object,
            own: false,
            full_id: uuid::Uuid::from_u128(3),
            local_id: None,
            pcode: 9,
            name: None,
            description: None,
            owner: None,
            position: None,
            rotation: None,
            scale: None,
            parent: None,
            children: Vec::new(),
            attachment_point: None,
            worn_by: None,
            sitting_on: None,
            selected: false,
            hover_text: None,
            name_tag: None,
        }
    }

    /// An event log entry.
    fn log_entry() -> LogEntry {
        LogEntry {
            seq: 7,
            stream: LogStream::Event,
            kind: "ChatReceived".to_owned(),
            detail: "ChatReceived(..)".to_owned(),
        }
    }

    /// A locator using every field.
    fn full_locator() -> Locator {
        Locator::role(Role::Checkbox)
            .named("Always run")
            .name_key("pref-always-run")
            .within(Locator::test_id("floater.preferences").name_containing("Pref"))
            .nth(0)
            .enabled(true)
            .checked(false)
            .selected(false)
            .expanded(false)
            .focused(true)
    }

    #[test]
    fn every_request_round_trips() -> Result<(), serde_json::Error> {
        let bodies = [
            RequestBody::Snapshot { within: None },
            RequestBody::Snapshot {
                within: Some(full_locator()),
            },
            RequestBody::Find {
                locator: full_locator(),
            },
            RequestBody::Click {
                locator: Locator::role(Role::Button).named("OK"),
                button: PointerButton::Left,
                double: false,
                deadline: Deadline::default(),
            },
            RequestBody::Click {
                locator: Locator::test_id("row"),
                button: PointerButton::Right,
                double: true,
                deadline: Deadline::default(),
            },
            RequestBody::DragTo {
                source: Locator::test_id("row"),
                target: Locator::role(Role::TreeItem).named("Objects"),
                deadline: Deadline::default(),
            },
            RequestBody::DragBy {
                source: Locator::test_id("floater-title-bar"),
                offset: [120.0, -40.5],
                deadline: Deadline::default(),
            },
            RequestBody::Hover {
                locator: Locator::role(Role::Button).named("OK"),
                deadline: Deadline {
                    frames: Some(5),
                    millis: None,
                },
            },
            RequestBody::Fill {
                locator: Locator::role(Role::Textbox),
                text: "hello \"world\"".to_owned(),
                deadline: Deadline::default(),
            },
            RequestBody::Press {
                keys: "Ctrl+Shift+S".to_owned(),
                hold_frames: 0,
            },
            RequestBody::Press {
                keys: "w".to_owned(),
                hold_frames: 60,
            },
            RequestBody::MenuPath {
                path: vec!["menu-world".to_owned(), "menu-world-midday".to_owned()],
                deadline: Deadline::default(),
            },
            RequestBody::SelectOption {
                combo: Locator::role(Role::Combobox),
                option: Locator::role(Role::ListItem).named("Low"),
                deadline: Deadline::default(),
            },
            RequestBody::PieSlice {
                slice: Locator::default().name_key("pie-touch"),
                deadline: Deadline::default(),
            },
            RequestBody::OpenFloater {
                floater: "inventory".to_owned(),
            },
            RequestBody::FindWorld {
                locator: WorldLocator::kind(WorldKind::Object).named("Door"),
                deadline: Deadline::default(),
            },
            RequestBody::WaitForWorld {
                locator: WorldLocator::own_avatar(),
                condition: WorldWaitCondition::Detached,
                deadline: Deadline::default(),
            },
            RequestBody::WorldAction {
                locator: WorldLocator::kind(WorldKind::Object).named("Door"),
                action: WorldAction::DropFrom(Locator::test_id("row")),
                reveal: false,
                deadline: Deadline::default(),
            },
            RequestBody::GroundAction {
                ground: GroundPoint::new("Next Door", 20.0, 128.5),
                action: WorldAction::DoubleClick,
                reveal: false,
                deadline: Deadline::default(),
            },
            RequestBody::DragHandle {
                handle: "rotate-z".to_owned(),
                amount: DragAmount::Angle(0.5),
                snap: SnapSide::Grid,
                modifiers: DragModifiers::Ctrl,
                deadline: Deadline::default(),
            },
            RequestBody::Sweep {
                locator: WorldLocator::kind(WorldKind::Object),
                deadline: Deadline::default(),
            },
            RequestBody::Read {
                probe: Probe::Inventory {
                    root: InventoryRoot::Agent,
                    path: vec!["Objects".to_owned()],
                },
            },
            RequestBody::ReadLog {
                cursor: 4,
                streams: vec![LogStream::Command],
                limit: Some(10),
            },
            RequestBody::ReadDiagnostics { cursor: 0 },
            RequestBody::WaitForState {
                condition: StateCondition::Probe {
                    probe: Probe::Agent,
                    pointer: "/region/name".to_owned(),
                    test: ValueTest::Equals(serde_json::json!("Home")),
                },
                deadline: Deadline::default(),
            },
            RequestBody::Screenshot {
                path: "/tmp/frame.png".to_owned(),
                outline: Some(Locator::role(Role::Button)),
            },
            RequestBody::WaitFor {
                locator: Locator::test_id("floater.inventory"),
                condition: WaitCondition::Detached,
                deadline: Deadline::default(),
            },
            RequestBody::WaitFor {
                locator: Locator::test_id("floater.inventory"),
                condition: WaitCondition::Text(NameMatcher::Contains("Done".to_owned())),
                deadline: Deadline {
                    frames: Some(600),
                    millis: Some(10_000),
                },
            },
            RequestBody::AnswerFileDialog {
                path: Some("/presets/skies/Dawn.xml".to_owned()),
                deadline: Deadline::default(),
            },
            RequestBody::AnswerFileDialog {
                path: None,
                deadline: Deadline {
                    frames: None,
                    millis: Some(5_000),
                },
            },
            RequestBody::Hello,
            RequestBody::Subscribe {
                cursor: None,
                streams: Vec::new(),
            },
            RequestBody::Subscribe {
                cursor: Some(12),
                streams: vec![LogStream::Event, LogStream::UiAction],
            },
            RequestBody::Unsubscribe {
                subscription: RequestId(3),
            },
        ];
        for (index, body) in bodies.into_iter().enumerate() {
            round_trip(&Request {
                id: RequestId(u64::try_from(index).unwrap_or(u64::MAX)),
                body,
            })?;
        }
        Ok(())
    }

    #[test]
    fn every_response_round_trips() -> Result<(), serde_json::Error> {
        let results = [
            Ok(ResponseBody::Snapshot {
                roots: vec![full_node()],
            }),
            Ok(ResponseBody::Found {
                nodes: vec![bare_node(), full_node()],
            }),
            Ok(ResponseBody::Done { node: full_node() }),
            Ok(ResponseBody::Satisfied { nodes: Vec::new() }),
            Ok(ResponseBody::Pressed),
            Ok(ResponseBody::Opened {
                window: Locator::test_id("floater:inventory"),
            }),
            Ok(ResponseBody::FoundWorld {
                nodes: vec![world_node()],
            }),
            Ok(ResponseBody::WorldSatisfied { nodes: Vec::new() }),
            Ok(ResponseBody::WorldDone {
                node: Box::new(world_node()),
                hit_point: Some([1.0, 2.0, 3.0]),
            }),
            Ok(ResponseBody::GroundDone {
                hit_point: [20.0, 128.5, 25.0],
            }),
            Ok(ResponseBody::Dragged {
                predicted: DragAmount::Offset([0.5, 0.0]),
            }),
            Ok(ResponseBody::Swept {
                nodes: vec![world_node()],
            }),
            Ok(ResponseBody::Readout {
                readout: ProbeReadout::Inventory(None),
            }),
            Ok(ResponseBody::Log {
                page: LogPage {
                    entries: vec![log_entry()],
                    next: 8,
                    dropped: 1,
                },
            }),
            Ok(ResponseBody::Diagnostics {
                readout: DiagnosticsReadout::default(),
            }),
            Ok(ResponseBody::StateHeld {
                observed: StateObservation::Logged {
                    entry: log_entry(),
                    next: 8,
                },
            }),
            Ok(ResponseBody::Screenshot {
                path: "/tmp/frame.png".to_owned(),
                width: 640,
                height: 360,
                outlined: vec![bare_node()],
            }),
            Ok(ResponseBody::Hello {
                protocol: PROTOCOL_VERSION,
                viewer: ViewerIdentity {
                    viewer: "sl-client-bevy-viewer".to_owned(),
                    version: "0.1.0".to_owned(),
                    pid: 4242,
                    grid: Some("localhost".to_owned()),
                    agent_name: Some("Test Avatar".to_owned()),
                    agent_id: Some(uuid::Uuid::from_u128(5)),
                },
            }),
            Ok(ResponseBody::Hello {
                protocol: PROTOCOL_VERSION,
                viewer: ViewerIdentity {
                    viewer: "sl-client-bevy-viewer".to_owned(),
                    version: "0.1.0".to_owned(),
                    pid: 1,
                    grid: None,
                    agent_name: None,
                    agent_id: None,
                },
            }),
            Ok(ResponseBody::FileDialogAnswered {
                purpose: "bulk-import-skies".to_owned(),
                title: "Import skies".to_owned(),
                folder: true,
            }),
            Ok(ResponseBody::Subscribed { cursor: 17 }),
            Ok(ResponseBody::Unsubscribed),
            Err(AutomationError::NotFound {
                locator: full_locator(),
            }),
            Err(AutomationError::Ambiguous {
                locator: Locator::role(Role::Button),
                candidates: vec![bare_node(), full_node()],
            }),
            Err(AutomationError::NotActionable {
                locator: full_locator(),
                check: ActionabilityCheck::Editable,
                node: full_node(),
            }),
            Err(AutomationError::TimedOut {
                locator: full_locator(),
                condition: Some(WaitCondition::Hidden),
                failed_check: None,
                last_observed: vec![full_node()],
                frames: 600,
                millis: 10_000,
            }),
            Err(AutomationError::WorldAmbiguous {
                locator: WorldLocator::kind(WorldKind::Object).named("Door"),
                candidates: vec![world_node(), world_node()],
            }),
            Err(AutomationError::ManipulatorRefused {
                handle: "rotate-z".to_owned(),
                reason: "the rig has no such handle".to_owned(),
            }),
            Err(AutomationError::ManipulatorTimedOut {
                handle: "translate-x".to_owned(),
                failed_check: ActionabilityCheck::Stable,
                frames: 600,
                millis: 10_000,
            }),
            Err(AutomationError::SweepInexact {
                locator: WorldLocator::kind(WorldKind::Object).named("Door"),
                missing: vec![uuid::Uuid::from_u128(4)],
                extra: Vec::new(),
            }),
            Err(AutomationError::WorldNotActionable {
                locator: WorldLocator::kind(WorldKind::Object).named("Door"),
                check: ActionabilityCheck::ReceivesEvents,
                node: Box::new(world_node()),
                covered_by: Some(uuid::Uuid::from_u128(9)),
            }),
            Err(AutomationError::GroundNotActionable {
                ground: GroundPoint::new("Home", 8.0, 8.0),
                check: ActionabilityCheck::ReceivesEvents,
                covered_by: Some(uuid::Uuid::from_u128(9)),
            }),
            Err(AutomationError::GroundTimedOut {
                ground: GroundPoint::new("Home", 8.0, 8.0),
                failed_check: ActionabilityCheck::Attached,
                frames: 600,
                millis: 10_000,
            }),
            Err(AutomationError::WorldTimedOut {
                locator: WorldLocator::own_avatar(),
                failed_check: Some(ActionabilityCheck::Stable),
                unresolved: vec![world_node()],
                last_observed: Vec::new(),
                frames: 600,
                millis: 10_000,
            }),
            Err(AutomationError::TimedOut {
                locator: full_locator(),
                condition: None,
                failed_check: Some(ActionabilityCheck::Stable),
                last_observed: Vec::new(),
                frames: 1,
                millis: 16,
            }),
            Err(AutomationError::FillMismatch {
                locator: Locator::role(Role::Textbox),
                text: "42".to_owned(),
                node: full_node(),
            }),
            Err(AutomationError::StateTimedOut {
                condition: StateCondition::Quiet,
                last_observed: Some(StateObservation::Quiet {
                    readout: QuiescenceReadout::default(),
                }),
                frames: 600,
                millis: 10_000,
            }),
            Err(AutomationError::InventoryFolderNotFound {
                root: InventoryRoot::Library,
                path: vec!["Nope".to_owned()],
                index: 0,
            }),
            Err(AutomationError::Unavailable {
                what: "conversation model".to_owned(),
            }),
            Err(AutomationError::InvalidRequest {
                reason: "a relative path".to_owned(),
            }),
            Err(AutomationError::ScreenshotFailed {
                reason: "no window".to_owned(),
            }),
            Err(AutomationError::NoFileDialog {
                frames: 600,
                millis: 10_000,
            }),
        ];
        for (index, result) in results.into_iter().enumerate() {
            let report = result.is_err().then(|| FailureReport {
                tree: vec![bare_node()],
                events: vec![log_entry()],
                diagnostics: vec![DiagnosticLine {
                    seq: 2,
                    level: LogLevel::Warn,
                    target: "viewer".to_owned(),
                    message: "slow".to_owned(),
                }],
            });
            round_trip(&Response {
                id: RequestId(u64::try_from(index).unwrap_or(u64::MAX)),
                result,
                report,
            })?;
        }
        Ok(())
    }

    #[test]
    fn every_enum_spelling_round_trips() -> Result<(), serde_json::Error> {
        for role in [
            Role::Button,
            Role::Checkbox,
            Role::Radio,
            Role::RadioGroup,
            Role::Textbox,
            Role::Combobox,
            Role::Slider,
            Role::ColorWell,
            Role::Trackball,
            Role::TabList,
            Role::Tab,
            Role::MenuBar,
            Role::Menu,
            Role::MenuItem,
            Role::List,
            Role::ListItem,
            Role::Tree,
            Role::TreeItem,
            Role::Window,
            Role::Text,
            Role::Image,
            Role::Document,
            Role::Group,
        ] {
            let json = round_trip(&role)?;
            assert_eq!(json, format!("\"{role}\""), "display matches serde");
        }
        for check in [
            ActionabilityCheck::Attached,
            ActionabilityCheck::Visible,
            ActionabilityCheck::InViewport,
            ActionabilityCheck::Stable,
            ActionabilityCheck::Enabled,
            ActionabilityCheck::Editable,
            ActionabilityCheck::ReceivesEvents,
            ActionabilityCheck::BuildMode,
            ActionabilityCheck::CreateTool,
        ] {
            let json = round_trip(&check)?;
            assert_eq!(json, format!("\"{check}\""), "display matches serde");
        }
        for condition in [
            WaitCondition::Attached,
            WaitCondition::Detached,
            WaitCondition::Visible,
            WaitCondition::Hidden,
            WaitCondition::Enabled,
            WaitCondition::Disabled,
        ] {
            let json = round_trip(&condition)?;
            assert_eq!(json, format!("\"{condition}\""), "display matches serde");
        }
        let text = WaitCondition::Text(NameMatcher::Exact("OK".to_owned()));
        assert_eq!(round_trip(&text)?, r#"{"text":{"exact":"OK"}}"#);
        assert_eq!(text.to_string(), r#"text="OK""#);
        for state in [
            NodeState::Disabled,
            NodeState::ReadOnly,
            NodeState::Checked,
            NodeState::Selected,
            NodeState::Expanded,
            NodeState::Focused,
            NodeState::Hovered,
        ] {
            round_trip(&state)?;
        }
        for visibility in [
            NodeVisibility::Visible,
            NodeVisibility::Hidden,
            NodeVisibility::Clipped,
            NodeVisibility::OffScreen,
            NodeVisibility::Covered,
        ] {
            round_trip(&visibility)?;
        }
        Ok(())
    }

    #[test]
    fn wire_shape_is_flat_and_terse() -> Result<(), serde_json::Error> {
        let request = Request {
            id: RequestId(1),
            body: RequestBody::Click {
                locator: Locator::role(Role::Button).named("OK"),
                button: PointerButton::Left,
                double: false,
                deadline: Deadline::default(),
            },
        };
        assert_eq!(
            serde_json::to_string(&request)?,
            r#"{"id":1,"method":"click","locator":{"role":"button","name":{"exact":"OK"}}}"#
        );
        let right = Request {
            id: RequestId(1),
            body: RequestBody::Click {
                locator: Locator::test_id("row"),
                button: PointerButton::Right,
                double: true,
                deadline: Deadline::default(),
            },
        };
        assert_eq!(
            serde_json::to_string(&right)?,
            r#"{"id":1,"method":"click","locator":{"test_id":"row"},"button":"right","double":true}"#
        );
        let found = Response {
            id: RequestId(2),
            result: Ok(ResponseBody::Found { nodes: Vec::new() }),
            report: None,
        };
        assert_eq!(
            serde_json::to_string(&found)?,
            r#"{"id":2,"ok":{"kind":"found","nodes":[]}}"#
        );
        let missing = Response {
            id: RequestId(3),
            result: Err(AutomationError::NotFound {
                locator: Locator::default(),
            }),
            report: Some(FailureReport::default()),
        };
        assert_eq!(
            serde_json::to_string(&missing)?,
            r#"{"id":3,"error":{"kind":"not_found","locator":{}},"report":{}}"#
        );
        let world = Request {
            id: RequestId(4),
            body: RequestBody::WorldAction {
                locator: WorldLocator::own_avatar(),
                action: WorldAction::RightClick,
                reveal: true,
                deadline: Deadline::default(),
            },
        };
        assert_eq!(
            serde_json::to_string(&world)?,
            r#"{"id":4,"method":"world_action","locator":{"kind":"avatar","own":true},"action":"right_click"}"#,
            "a revealing action with the default deadline says neither"
        );
        let read = Request {
            id: RequestId(5),
            body: RequestBody::Read {
                probe: Probe::Agent,
            },
        };
        assert_eq!(
            serde_json::to_string(&read)?,
            r#"{"id":5,"method":"read","probe":{"probe":"agent"}}"#
        );
        Ok(())
    }

    #[test]
    fn a_wait_without_deadline_asks_for_the_default() -> Result<(), serde_json::Error> {
        let request: Request = serde_json::from_str(
            r#"{"id":9,"method":"wait_for","locator":{"test_id":"x"},"condition":"visible"}"#,
        )?;
        assert_eq!(
            request.body,
            RequestBody::WaitFor {
                locator: Locator::test_id("x"),
                condition: WaitCondition::Visible,
                deadline: Deadline::default(),
            }
        );
        Ok(())
    }

    #[test]
    fn a_viewer_message_is_a_response_or_a_notification() -> Result<(), serde_json::Error> {
        let messages = [
            ViewerMessage::Response(Box::new(Response {
                id: RequestId(1),
                result: Ok(ResponseBody::Unsubscribed),
                report: None,
            })),
            ViewerMessage::Response(Box::new(Response {
                id: RequestId(2),
                result: Err(AutomationError::InvalidRequest {
                    reason: "no such method".to_owned(),
                }),
                report: Some(FailureReport::default()),
            })),
            ViewerMessage::Notification(Notification::Log {
                subscription: RequestId(3),
                page: LogPage {
                    entries: vec![log_entry()],
                    next: 8,
                    dropped: 2,
                },
            }),
            ViewerMessage::Notification(Notification::Rejected {
                reason: "not JSON".to_owned(),
            }),
        ];
        for message in &messages {
            round_trip(message)?;
        }
        assert_eq!(
            serde_json::to_string(&Notification::Log {
                subscription: RequestId(3),
                page: LogPage {
                    entries: Vec::new(),
                    next: 9,
                    dropped: 0,
                },
            })?,
            r#"{"notification":"log","subscription":3,"page":{"entries":[],"next":9}}"#
        );
        assert_eq!(
            serde_json::to_string(&Request {
                id: RequestId(1),
                body: RequestBody::Hello,
            })?,
            r#"{"id":1,"method":"hello"}"#
        );
        Ok(())
    }

    #[test]
    fn misspelt_fields_are_refused() {
        for json in [
            r#"{"id":1,"method":"find","locator":{"role":"button","nme":"OK"}}"#,
            r#"{"id":1,"method":"wait_for","locator":{},"condition":"visible","deadline":{"frame":3}}"#,
            r#"{"id":1,"method":"tap","locator":{}}"#,
        ] {
            assert!(
                serde_json::from_str::<Request>(json).is_err(),
                "accepted {json}"
            );
        }
    }
}
