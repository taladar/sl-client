//! The viewer's leaf toolkit: the pieces the rest of the viewer builds on that
//! do not, themselves, depend on any of it.
//!
//! This is a deliberately mixed bag, and the mix is the point. What these
//! modules have in common is not subject matter but position in the dependency
//! graph — every one of them is a leaf, so none of the viewer's features can
//! drag another in through here. Splitting them further by topic would mean
//! more manifests and more per-commit check runs without changing what a given
//! edit rebuilds, since the set of dependents is the same either way.
//!
//! Roughly, they fall into three groups:
//!
//! - **Geometry and math** — [`coords`] (Second Life's Z-up world frame against
//!   Bevy's Y-up one), [`edit_math`], [`minimap_math`], [`world_map_math`],
//!   [`ik`], [`sit_offset`], [`procedural`], [`flexi`] (flexible-prim
//!   simulation), [`geometry_cache`], [`raycast_index`] (the static parry3d
//!   raycast BVH).
//! - **Render leaves** — [`face_material`] and [`particle_render`], each a
//!   material plus the shader it loads, and the render-layer bookkeeping in
//!   [`shadow_visibility`] and [`probe_layers`].
//! - **Small models** — [`radar_model`], [`appearance`], [`avatar_assets`],
//!   [`parcel_names`], [`sky_presets`].

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types and \
              Bevy plugins read as `face_material::FaceMaterial`. That only became \
              a lint when these items turned `pub` for the crate split; renaming \
              them would churn every call site in the viewer to satisfy a style \
              rule this codebase does not follow"
)]

pub mod appearance;
pub mod avatar_assets;
pub mod coords;
pub mod edit_math;
pub mod face_material;
pub mod flexi;
pub mod geometry_cache;
pub mod ik;
pub mod minimap_math;
pub mod parcel_names;
pub mod particle_render;
pub mod probe_layers;
pub mod procedural;
pub mod radar_model;
pub mod raycast_index;
pub mod shadow_visibility;
pub mod sit_offset;
pub mod sky_presets;
pub mod world_map_math;
