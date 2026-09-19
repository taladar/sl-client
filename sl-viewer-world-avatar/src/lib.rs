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
//! Every reach into a lower crate names that crate: a call site says
//! `sl_viewer_kit::coords` or `sl_viewer_world_api::ObjectState`, never a
//! local-looking `crate::` path. So a file that crosses a crate boundary reads
//! as one, and a new reach-across has to be written out rather than inherited
//! from an alias at the top of this file.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `avatars::AvatarBody` and `animations::AnimationManager`. That only \
              became a lint when these items turned `pub` for the crate split; \
              renaming them would churn every call site in the viewer to satisfy \
              a style rule this codebase does not follow"
)]

pub mod animations;
pub mod animesh;
pub mod appearance;
pub mod asset_blacklist;
pub mod avatar_asset_stats;
pub mod avatar_complexity;
pub mod avatar_dump;
pub mod avatar_render_floater;
pub mod avatar_render_settings;
pub mod avatar_replay;
pub mod avatars;
pub mod bake_inputs;
pub mod bake_publish;
pub mod body_physics;
pub mod derender;
pub mod first_person;
pub mod gpu_avatar_spike;
pub mod gpu_avatars;
pub mod ground;
pub mod hand_pose;
#[cfg(test)]
mod headless_gpu;
pub mod ik;
pub mod locomotion;
pub mod locomotion_ik;
pub mod look_at;
pub mod motion_stops;
pub mod name_tag_content;
mod plugin;
pub mod procedural;
pub mod reach;
pub mod replay_bundle;
pub mod rigged_attachments;
pub mod typing;

pub use plugin::WorldAvatarPlugin;
