//! The viewer's view layer: how the user looks at the world and touches it.
//!
//! The camera and its modes and collisions, avatar movement and physics, the
//! pick buffers behind a click, the HUD attached to the screen, the input
//! contexts and actions that route a key press, and the session state that
//! ties a login to a rendered region. It sits above both the object layer
//! (`sl-viewer-world-objects`) and the scene layer (`sl-viewer-world-scene`).
//!
//! Two surfaces here are screen-space UI rather than view machinery, and are
//! here because of what they read, not what they draw: [`hover_tooltip`] is the
//! dwell tip over whatever [`gpu_pick`] resolved under the cursor, and
//! [`media_controls`] is the bar that drives a [`media_prim`] surface and the
//! camera focus that frames it. Neither could sit lower without dragging the
//! pick buffers and the camera down with it.
//!
//! Every reach into a lower crate names that crate: a call site says
//! `sl_viewer_kit::coords` or `sl_viewer_world_api::ObjectState`, never a
//! local-looking `crate::` path. So a file that crosses a crate boundary reads
//! as one, and a new reach-across has to be written out rather than inherited
//! from an alias at the top of this file.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `objects::ObjectState` and `terrain::TerrainRegion`. That only \
              became a lint when these items turned `pub` for the crate split; \
              renaming them would churn every call site in the viewer to satisfy \
              a style rule this codebase does not follow"
)]

pub mod arrival;
pub mod camera;
pub mod gpu_pick;
pub mod harness_status;
pub mod hover_tooltip;
pub mod hud;
pub mod hud_pick;
pub mod input_action;
pub mod input_context;
pub mod media_controls;
pub mod media_prim;
pub mod movement;
pub mod panorama;
pub mod physics;
pub mod quiescence;
pub mod scene_dump;
pub mod screenshot;
pub mod session;
pub mod sit_camera;
