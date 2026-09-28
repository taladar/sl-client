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

mod locate;
mod pursuit;
mod reveal;
mod route;
mod ui_model;

pub use crate::locate::{find_all, find_one, shallow};
pub use crate::pursuit::{
    DEFAULT_DEADLINE, DEFAULT_DEADLINE_FRAMES, Intent, Progress, Pursuit, PursuitError, Target,
};
pub use crate::reveal::{open_floater, scroll_into_view};
pub use crate::route::{Gesture, Route, RouteProgress};
pub use crate::ui_model::{UiModel, entity_of, node_id, snapshot};
