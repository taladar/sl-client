//! The viewer's avatar layer: what an agent's state builds into entities.
//!
//! Skeletons and the animations that pose them, the bakes and wearables that
//! dress them, the procedural adjusters (look-at, reach, foot IK, hand pose),
//! the tags above their heads, and the worn attachments rigged to them.
//!
//! In Second Life an avatar *is* an object and a worn attachment *is* an object
//! parented to one, so this layer sits **above** the object layer
//! (`sl-viewer-world-objects`) rather than beside it: it calls down into the
//! texture, mesh and material pipelines for what it needs to dress an avatar,
//! and nothing in the object layer names anything here. The one piece that
//! genuinely serves both — the world-anchored text billboard, which draws name
//! tags and an object's `llSetText` through the same renderer — stays below, in
//! the object layer, so the graph runs one way.
//!
//! The modules keep the names they had inside the object layer, so a call site
//! reads the same after the move as before it.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `avatars::AvatarBody` and `animations::AnimationManager`. That only \
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
pub(crate) use sl_viewer_kit::geometry_cache;
pub(crate) use sl_viewer_kit::ik;
pub(crate) use sl_viewer_kit::probe_layers;
pub(crate) use sl_viewer_kit::procedural;
pub(crate) use sl_viewer_platform::paths;
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_ui_core::skin_colors;
pub(crate) use sl_viewer_ui_core::ui_sounds;
pub(crate) use sl_viewer_world_api as world_api;
// The object layer below, likewise re-aliased: an avatar's own body, its worn
// attachments and its bakes are all built out of the same texture / mesh /
// material pipelines the prims use.
pub(crate) use sl_viewer_world_objects::asset_budget;
pub(crate) use sl_viewer_world_objects::meshes;
pub(crate) use sl_viewer_world_objects::name_tag_billboard;
pub(crate) use sl_viewer_world_objects::objects;
pub(crate) use sl_viewer_world_objects::textures;

pub mod animations;
pub mod animesh;
pub mod avatar_asset_stats;
pub mod avatar_complexity;
pub mod avatar_dump;
pub mod avatar_render_settings;
pub mod avatar_replay;
pub mod avatars;
pub mod bake_inputs;
pub mod bake_publish;
pub mod body_physics;
pub mod derender;
pub mod gpu_avatar_spike;
pub mod gpu_avatars;
pub mod ground;
pub mod hand_pose;
pub mod locomotion;
pub mod locomotion_ik;
pub mod look_at;
pub mod name_tag_content;
pub mod reach;
pub mod replay_bundle;
pub mod rigged_attachments;
pub mod typing;
