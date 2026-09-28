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
//! - A [`Request`] and its [`Response`] share a [`RequestId`], so several
//!   requests may be in flight on one channel.
//! - An [`AutomationError`] is what a test matches on: not found, ambiguous
//!   (with every candidate), not actionable (with the failing
//!   [`ActionabilityCheck`]) and timed out (with the last observed state).
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

mod failure;
mod locator;
mod message;
mod snapshot;

pub use crate::failure::{ActionabilityCheck, AutomationError};
pub use crate::locator::{Locator, NameMatcher};
pub use crate::message::{
    Deadline, Request, RequestBody, RequestId, Response, ResponseBody, WaitCondition,
};
pub use crate::snapshot::{Bounds, NodeId, NodeState, NodeValue, NodeVisibility, Role, UiNode};
