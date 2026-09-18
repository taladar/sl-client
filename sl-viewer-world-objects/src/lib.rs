//! The viewer's object layer: what a region's state builds into entities.
//!
//! Prims and their meshes, the materials and textures that dress them, and the
//! world-anchored text billboards drawn over them. Nothing here knows about the
//! avatars wearing those objects, about the scene around them or about the
//! camera looking at them: the avatar layer sits above in
//! `sl-viewer-world-avatar`, the scene layer in `sl-viewer-world-scene` and the
//! view layer above that, in `sl-viewer-world-view`.
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
mod plugin;
pub mod render_priority;
pub mod texture_anim;
pub mod textures;

pub use plugin::WorldObjectsPlugin;
