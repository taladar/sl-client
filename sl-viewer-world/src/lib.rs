//! The viewer's world layer: what turns region state into a rendered world.
//!
//! Everything here answers to the simulator rather than to the user — the
//! camera and its collisions, the object and avatar graphs, the terrain, sky,
//! water and lighting, and the render passes that draw them. A feature surface
//! sits *above* this crate and reads it; the shared state both tiers need sits
//! *below*, in `sl-viewer-world-api`.
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

// Lower crates re-aliased under their original module names, so the world
// modules keep addressing them as `crate::coords`, `crate::settings` and so
// on rather than gaining a rename in every file.
pub(crate) use sl_viewer_kit::avatar_assets;
pub(crate) use sl_viewer_kit::coords;
pub(crate) use sl_viewer_kit::face_material;
pub(crate) use sl_viewer_kit::flexi;
pub(crate) use sl_viewer_kit::geometry_cache;
pub(crate) use sl_viewer_kit::ik;
pub(crate) use sl_viewer_kit::particle_render;
pub(crate) use sl_viewer_kit::probe_layers;
pub(crate) use sl_viewer_kit::procedural;
pub(crate) use sl_viewer_kit::raycast_index;
pub(crate) use sl_viewer_kit::sky_presets;
pub(crate) use sl_viewer_media::browser_widget;
pub(crate) use sl_viewer_media::media_engine;
pub(crate) use sl_viewer_media::media_keys;
pub(crate) use sl_viewer_platform::asset_retry;
pub(crate) use sl_viewer_platform::environment_assets;
pub(crate) use sl_viewer_platform::paths;
pub(crate) use sl_viewer_platform::sound_cache;
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_spacenav as spacenav;
pub(crate) use sl_viewer_ui_core::skin_colors;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_core::ui_sounds;
pub(crate) use sl_viewer_ui_widgets::floater;
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
pub mod beacons;
pub mod body_physics;
pub mod bump;
pub mod camera;
pub mod derender;
pub mod diagnostics;
pub mod entity_diagnostics;
pub mod environment;
pub mod exposure;
pub mod glow;
pub mod gpu_avatar_spike;
pub mod gpu_avatars;
pub mod gpu_pick;
pub mod ground;
pub mod hand_pose;
pub mod hover_text;
pub mod hud;
pub mod hud_pick;
pub mod input_action;
pub mod input_context;
pub mod legacy_materials;
pub mod lights;
pub mod locomotion;
pub mod locomotion_ik;
pub mod look_at;
pub mod material_cache;
pub mod material_preview;
pub mod materials;
pub mod media_prim;
pub mod meshes;
pub mod movement;
pub mod name_tag_billboard;
pub mod name_tag_content;
pub mod object_cost;
pub mod objects;
pub mod parcel_borders;
pub mod particles;
pub mod physics;
pub mod probes;
pub mod reach;
pub mod render_priority;
pub mod render_scene;
pub mod replay_bundle;
pub mod scene_reset;
pub mod screenshot;
pub mod session;
pub mod sit_camera;
pub mod sky;
pub mod terrain;
pub mod texture_anim;
pub mod textures;
pub mod tonemap;
pub mod transparency;
pub mod typing;
pub mod underwater_fog;
pub mod water;
pub mod water_exclusion;
