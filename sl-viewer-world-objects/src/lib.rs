//! The viewer's object layer: what a region's state builds into entities.
//!
//! Prims and their meshes, the materials and textures that dress them, and the
//! avatars — their skeletons, animations, bakes and the tags above their heads.
//! Nothing here knows about the scene around the objects or about the camera
//! looking at them; the scene layer sits above in `sl-viewer-world-scene` and
//! the view layer above that, in `sl-viewer-world-view`.
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
pub(crate) use sl_viewer_kit::flexi;
pub(crate) use sl_viewer_kit::geometry_cache;
pub(crate) use sl_viewer_kit::ik;
pub(crate) use sl_viewer_kit::probe_layers;
pub(crate) use sl_viewer_kit::procedural;
pub(crate) use sl_viewer_platform::asset_retry;
pub(crate) use sl_viewer_platform::paths;
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_ui_core::skin_colors;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_core::ui_sounds;
pub(crate) use sl_viewer_world_api as world_api;

pub mod animations;
pub mod animesh;
pub mod asset_budget;
pub mod avatar_complexity;
pub mod avatar_dump;
pub mod avatar_render_settings;
pub mod avatar_replay;
pub mod avatars;
pub mod bake_inputs;
pub mod bake_publish;
pub mod body_physics;
pub mod bump;
pub mod derender;
pub mod gpu_avatar_spike;
pub mod gpu_avatars;
pub mod ground;
pub mod hand_pose;
pub mod hover_text;
pub mod legacy_materials;
pub mod locomotion;
pub mod locomotion_ik;
pub mod look_at;
pub mod material_cache;
pub mod material_preview;
pub mod materials;
pub mod meshes;
pub mod name_tag_billboard;
pub mod name_tag_content;
pub mod object_cost;
pub mod objects;
pub mod reach;
pub mod render_priority;
pub mod replay_bundle;
pub mod texture_anim;
pub mod textures;
pub mod typing;
