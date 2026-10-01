//! The async test API over the viewer automation protocol: what a test, a
//! stage or the command line tool drives a Second Life viewer with — through
//! the same calls whether the viewer runs in its own process behind an
//! automation socket or in the test's process.
//!
//! - **One handle over both transports.** A [`Viewer`] is built with
//!   [`Viewer::connect`] (the socket a viewer's `--automation-socket` opened)
//!   or [`Viewer::over_link`] (the request and message channels an
//!   in-process host hands out). Underneath, a transport is exactly that
//!   pair of channels; the connection numbers requests, routes each answer
//!   to its caller and each subscription's entries to its stream, and fails
//!   whatever waits when the viewer's side closes. Handles are cheap clones;
//!   several viewers are several handles.
//! - **Locators read like the intent.** `viewer.ui().window("build")
//!   .button_key("build-apply").click()` — a [`UiLocator`] is a semantic
//!   [`sl_automation_proto::Locator`] bound to its viewer: `click`,
//!   `double_click`, `right_click`, `hover`, `fill`, `press`, `check` /
//!   `uncheck`, `select_option`, `drag_to`, `drag_by`, and the reads `text`,
//!   `value`, `is_disabled`, `is_checked`, `is_visible`, `count`, `all`. A
//!   [`WorldHandle`] names an in-world thing — `viewer.world()
//!   .object_named("Door").touch()` — with `touch`, `open_pie`, `hover`,
//!   `select`, `sit`, `drop_from` and its readouts.
//! - **Waits live in the viewer.** Every action waits there for its one node
//!   to be actionable, and every expectation —
//!   `viewer.expect(&button).to_be_disabled()`,
//!   `viewer.expect_chat().to_contain("hello")` — is a condition the viewer
//!   evaluates each frame until it holds or its timeout runs out (the
//!   viewer's default, [`DEFAULT_TIMEOUT`], unless set per call). No call
//!   sleeps. Past that, the driver gives a viewer that stopped answering
//!   [`DEFAULT_GRACE`] before it gives up on it.
//! - **Probes** read the rest: the agent, the status bar, the transcripts,
//!   the notifications, the selection, an inventory folder, whether the
//!   scene has settled, the event log by [`EventCursor`] or as an
//!   [`EventStream`], the diagnostics, a screenshot.
//! - **A failure explains itself.** A failed action or expectation is a
//!   [`DriverError::Failed`] carrying the viewer's error and report, and —
//!   when the viewer has an artifact directory — a screenshot with the
//!   locator's matches outlined, the semantic tree around the scope and the
//!   event tail, saved there and named in the error's message.

mod artifacts;
mod connection;
mod error;
mod events;
mod expect;
mod ui;
mod viewer;
mod world;

pub use crate::error::{Artifacts, DriverError, Failure};
pub use crate::events::{EventCursor, EventStream};
pub use crate::expect::{ChatExpect, NotificationExpect, StateExpect, UiExpect, WorldExpect};
pub use crate::ui::{Ui, UiLocator};
pub use crate::viewer::{
    AnsweredFileDialog, DEFAULT_GRACE, DEFAULT_TIMEOUT, Screenshot, Viewer, ViewerOptions,
};
pub use crate::world::{World, WorldHandle};

#[cfg(test)]
mod tests;
