//! The viewer's view layer: how the user looks at the world and touches it.
//!
//! The camera and its modes and collisions, avatar movement and physics, the
//! pick buffers behind a click, the HUD attached to the screen, the input
//! contexts and actions that route a key press, and the session state that
//! ties a login to a rendered region. It sits above both the object layer
//! (`sl-viewer-world-objects`) and the scene layer (`sl-viewer-world-scene`).
//!
//! The modules keep the names they had inside the viewer, so a call site reads
//! the same after the move as before it.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `objects::ObjectState` and `terrain::TerrainRegion`. That only \
              became a lint when these items turned `pub` for the crate split; \
              renaming them would churn every call site in the viewer to satisfy \
              a style rule this codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so this crate's
// modules keep addressing them as `crate::coords`, `crate::settings` and so
// on rather than gaining a rename in every file.
pub(crate) use sl_viewer_kit::avatar_assets;
pub(crate) use sl_viewer_kit::coords;
pub(crate) use sl_viewer_kit::face_material;
pub(crate) use sl_viewer_kit::raycast_index;
pub(crate) use sl_viewer_media::browser_widget;
pub(crate) use sl_viewer_media::media_engine;
pub(crate) use sl_viewer_media::media_keys;
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_spacenav as spacenav;
pub(crate) use sl_viewer_world_api as world_api;
pub(crate) use sl_viewer_world_avatar::avatars;
pub(crate) use sl_viewer_world_objects::meshes;
pub(crate) use sl_viewer_world_objects::objects;
pub(crate) use sl_viewer_world_scene::terrain;
pub(crate) use sl_viewer_world_scene::water;

pub mod camera;
pub mod gpu_pick;
pub mod hud;
pub mod hud_pick;
pub mod input_action;
pub mod input_context;
pub mod media_prim;
pub mod movement;
pub mod physics;
pub mod scene_reset;
pub mod screenshot;
pub mod session;
pub mod sit_camera;
