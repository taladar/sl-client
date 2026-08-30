//! The viewer's scene layer: what the world's objects are drawn *inside*.
//!
//! Terrain, sky, water and their fog, the lighting and reflection probes, the
//! particle systems, the parcel overlays, and the render passes — glow,
//! exposure, tone map, transparency — that assemble the frame. It reads the
//! object layer below it (`sl-viewer-world-objects`) and knows nothing of the
//! camera or the user's input, which live above it in `sl-viewer-world-view`.
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
pub(crate) use sl_viewer_kit::particle_render;
pub(crate) use sl_viewer_kit::probe_layers;
pub(crate) use sl_viewer_kit::sky_presets;
pub(crate) use sl_viewer_platform::environment_assets;
pub(crate) use sl_viewer_platform::sound_cache;
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_world_api as world_api;
pub(crate) use sl_viewer_world_objects::asset_budget;
pub(crate) use sl_viewer_world_objects::bump;
pub(crate) use sl_viewer_world_objects::legacy_materials;
pub(crate) use sl_viewer_world_objects::material_cache;
pub(crate) use sl_viewer_world_objects::name_tag_billboard;
pub(crate) use sl_viewer_world_objects::objects;
pub(crate) use sl_viewer_world_objects::texture_anim;
pub(crate) use sl_viewer_world_objects::textures;

pub mod beacons;
pub mod diagnostics;
pub mod entity_diagnostics;
pub mod environment;
pub mod exposure;
pub mod glow;
pub mod lights;
pub mod parcel_borders;
pub mod particles;
pub mod probes;
pub mod render_scene;
pub mod sky;
pub mod terrain;
pub mod tonemap;
pub mod transparency;
pub mod underwater_fog;
pub mod water;
pub mod water_clip;
pub mod water_exclusion;
