//! The viewer's scene layer: what the world's objects are drawn *inside*.
//!
//! Terrain, sky, water and their fog, the lighting and reflection probes, the
//! particle systems, the parcel overlays, and the render passes — glow,
//! exposure, tone map, transparency — that assemble the frame. It reads the
//! object layer below it (`sl-viewer-world-objects`) and knows nothing of the
//! camera or the user's input, which live above it in `sl-viewer-world-view`.
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

pub mod beacons;
/// The scene app the day-cycle assertions run in — test support, so it is built
/// only for `cargo test`. See the module's own docs for what it exists to catch.
#[cfg(test)]
mod day_cycle_fixture;
pub mod debug_beacons;
pub mod diagnostics;
pub mod entity_diagnostics;
pub mod environment;
pub mod exposure;
pub mod glow;
pub mod lights;
pub mod parcel_borders;
pub mod parcel_owners;
pub mod particles;
mod plugin;
pub mod probes;
pub mod render_overrides;
pub mod resolution_divisor;
pub mod sky;
pub mod terrain;
pub mod tonemap;
pub mod transparency;
pub mod underwater_fog;
pub mod viewer_camera;
pub mod water;
pub mod water_clip;
pub mod water_exclusion;
pub mod water_fog;
pub mod water_scene_depth;

pub use plugin::WorldScenePlugin;
