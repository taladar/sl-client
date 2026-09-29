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

mod locate;
mod manipulator_drag;
mod pursuit;
mod reveal;
mod route;
mod ui_model;
mod world_aim;
mod world_model;
mod world_query;
mod world_sweep;

pub use crate::locate::{find_all, find_one, shallow};
pub use crate::manipulator_drag::{DragProgress, DragStage, HeldKeys, ManipulatorDrag};
pub use crate::pursuit::{
    DEFAULT_DEADLINE, DEFAULT_DEADLINE_FRAMES, Intent, Progress, Pursuit, PursuitError, Target,
};
pub use crate::reveal::{open_floater, scroll_into_view};
pub use crate::route::{Gesture, Route, RouteProgress};
pub use crate::ui_model::{UiModel, entity_of, node_id, snapshot};
pub use crate::world_aim::{
    AimProgress, AimStage, ScreenProjection, WorldAim, WorldIntent, WorldTarget, screen_projection,
};
pub use crate::world_model::{ObjectFacts, WorldModel, WorldModelPlugin, world_snapshot};
pub use crate::world_query::{WorldProgress, WorldQuery, WorldWant, find_world};
pub use crate::world_sweep::{SweepProgress, SweepTarget, WorldSweep};
