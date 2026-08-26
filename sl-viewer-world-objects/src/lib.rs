//! The viewer's object layer: what a region's state builds into entities.
//!
//! Prims and their meshes, the materials and textures that dress them, and the
//! world-anchored text billboards drawn over them. Nothing here knows about the
//! avatars wearing those objects, about the scene around them or about the
//! camera looking at them: the avatar layer sits above in
//! `sl-viewer-world-avatar`, the scene layer in `sl-viewer-world-scene` and the
//! view layer above that, in `sl-viewer-world-view`.
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
pub(crate) use sl_viewer_kit::coords;
pub(crate) use sl_viewer_kit::face_material;
pub(crate) use sl_viewer_kit::flexi;
pub(crate) use sl_viewer_kit::geometry_cache;
pub(crate) use sl_viewer_kit::probe_layers;
pub(crate) use sl_viewer_platform::asset_retry;
pub(crate) use sl_viewer_platform::paths;
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_world_api as world_api;

pub mod asset_budget;
pub mod asset_stats;
pub mod bump;
pub mod hover_text;
pub mod legacy_materials;
pub mod material_cache;
pub mod material_preview;
pub mod materials;
pub mod meshes;
pub mod name_tag_billboard;
pub mod object_cost;
pub mod objects;
pub mod render_priority;
pub mod texture_anim;
pub mod textures;
