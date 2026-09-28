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

mod ui_model;

pub use crate::ui_model::{UiModel, entity_of, node_id, snapshot};
