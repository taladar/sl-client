//! The vocabulary for driving a Second Life viewer from a test or an agent.
//!
//! Everything that drives a viewer — the in-process automation transport, the
//! remote one, the driver library, the command line tool and one day the
//! patched Firestorm — speaks these types. They are pure data: serde, no
//! runtime, no Bevy.
//!
//! - A [`Locator`] names a UI node **semantically**: a [`Role`] plus an
//!   accessible name (a [`NameMatcher`] against the resolved text, or better
//!   the locale-independent Fluent key), or a test id, scoped with
//!   [`Locator::within`], picked with [`Locator::nth`] and narrowed by state
//!   filters. Coordinates never appear in a locator.
//! - A [`UiNode`] is one node of a semantic snapshot: role, name, name key,
//!   test id, [`NodeState`]s, [`NodeValue`], [`Bounds`] in logical pixels and
//!   [`NodeVisibility`].
//! - A [`WorldLocator`] names an in-world thing the same way: a [`WorldKind`]
//!   (object, avatar, attachment), the own avatar, a name, an id, an owner, an
//!   object class or floating text, ordered by distance with [`Near`] and
//!   picked with [`WorldLocator::nth`]. A [`WorldNode`] is what the viewer
//!   reports for one: ids, name, owner, region-local placement, link set,
//!   attachment point, sit state, selection and the text shown over it.
//! - A state probe reads what a test asserts on that is not one widget: a
//!   [`ConversationReadout`] (local chat and instant-message transcripts), a
//!   [`NotificationReadout`] (text and offered buttons), the [`StatusReadout`]
//!   of the status bar, the own [`AgentReadout`] (region, position, seat,
//!   teleport, camera, heading), the [`EnvironmentReadout`] of the sky being
//!   drawn, the [`WorldMapReadout`] of what the world map knows, the
//!   [`SelectedObject`]s, an
//!   [`InventoryFolderReadout`] by path, the sequence-numbered [`LogEntry`]s
//!   of events, commands and UI actions read by cursor as a [`LogPage`], the
//!   [`DiagnosticsReadout`] of warnings and errors, and the
//!   [`QuiescenceReadout`] that says whether the scene has settled.
//! - A [`Request`] and its [`Response`] share a [`RequestId`], so several
//!   requests may be in flight on one channel. The requests read the UI and
//!   act on it (a click with either button, single or double, a drag onto
//!   another node, hover, fill, a key press, a menu path, a combo's option, a
//!   pie slice), read and act on the world ([`WorldAction`], a
//!   drag of a transform handle, a rubber band), read a [`Probe`], the event
//!   log or the diagnostics, wait on UI nodes ([`WaitCondition`]), world
//!   things ([`WorldWaitCondition`]) or the viewer's state
//!   ([`StateCondition`]: quiet, a probe's value, a log entry), and capture a
//!   screenshot. [`RequestBody::Hello`] names the viewer and the
//!   [`PROTOCOL_VERSION`] it speaks, and [`RequestBody::Subscribe`] streams
//!   the event log as [`Notification`]s; on a connection each line the viewer
//!   sends is a [`ViewerMessage`], a response or a notification.
//! - An [`AutomationError`] is what a test matches on: not found, ambiguous
//!   (with every candidate), not actionable (with the failing
//!   [`ActionabilityCheck`]) and timed out (with the last observed state);
//!   every error response also carries a [`FailureReport`] — the tree around
//!   the scope, the event tail and the warnings logged meanwhile.
//!
//! Both locators also have a **string form** for a command line and an error
//! message — `window[test_id=floater:build] >> button[name_key=build-apply]`,
//! `object[name=Door][near=own_avatar]` — which they print as (`Display`)
//! and parse from (`FromStr`, refusing a mistake with a [`SelectorError`]
//! naming the column); printing and parsing agree on every locator.
//!
//! The *types* are shared between viewers, but a Fluent key or a test id means
//! something only to the viewer that owns it: selectors are a per-viewer
//! namespace.
//!
//! Every field that is optional in a locator is omitted from its JSON when
//! unset, and unknown fields are refused, so a hand-written selector is terse
//! and a misspelt field is an error rather than a silently broader match:
//!
//! ```
//! use sl_automation_proto::{Locator, Role};
//!
//! let ok = Locator::role(Role::Button)
//!     .name_key("button-ok")
//!     .within(Locator::test_id("floater.preferences"));
//! let json = serde_json::to_string(&ok)?;
//! assert_eq!(
//!     json,
//!     r#"{"role":"button","name_key":"button-ok","within":{"test_id":"floater.preferences"}}"#
//! );
//! assert!(serde_json::from_str::<Locator>(r#"{"rol":"button"}"#).is_err());
//! # Ok::<(), serde_json::Error>(())
//! ```

mod action;
mod failure;
mod locator;
mod message;
mod probe;
mod report;
mod selector;
mod snapshot;
mod state;
mod world;

pub use crate::action::{DragAmount, DragModifiers, SnapSide, WorldAction, WorldWaitCondition};
pub use crate::failure::{ActionabilityCheck, AutomationError};
pub use crate::locator::{Locator, NameMatcher};
pub use crate::message::{
    Deadline, Notification, PROTOCOL_VERSION, PointerButton, Request, RequestBody, RequestId,
    Response, ResponseBody, ViewerIdentity, ViewerMessage, WaitCondition,
};
pub use crate::probe::{
    AgentReadout, CameraView, ChatKind, ClockTime, ConversationReadout, ConversationRef,
    DiagnosticLine, DiagnosticsReadout, EnvironmentReadout, InventoryEntry, InventoryFolderReadout,
    InventoryRoot, LogEntry, LogLevel, LogPage, LogStream, MapItemLayerReadout, MapItemReadout,
    MapRegionReadout, NotificationReadout, OfferedButton, QuiescenceReadout, RegionReadout,
    SelectedObject, SkyReadout, SpeakerKind, StatusReadout, TeleportReadout, TeleportState,
    TranscriptLine, WaterReadout, WorldMapReadout,
};
pub use crate::report::FailureReport;
pub use crate::selector::SelectorError;
pub use crate::snapshot::{Bounds, NodeId, NodeState, NodeValue, NodeVisibility, Role, UiNode};
pub use crate::state::{
    Probe, ProbeReadout, StateCondition, StateObservation, ValueTest, includes,
};
pub use crate::world::{Anchor, GroundPoint, Near, WorldKind, WorldLocator, WorldNode};
