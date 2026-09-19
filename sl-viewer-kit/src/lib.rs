//! The viewer's leaf toolkit: the pieces the rest of the viewer builds on that
//! do not, themselves, depend on any of it.
//!
//! This is a deliberately mixed bag, and the mix is the point. What these
//! modules have in common is not subject matter but position in the dependency
//! graph — every one of them is a leaf, so none of the viewer's features can
//! drag another in through here. Splitting the rest further by topic would mean
//! more manifests and more per-commit check runs without changing what a given
//! edit rebuilds, since the set of dependents is the same either way.
//!
//! Roughly, they fall into three groups:
//!
//! - **Geometry and math** — [`coords`] (Second Life's Z-up world frame against
//!   Bevy's Y-up one), [`minimap_math`], [`flexi`] (flexible-prim simulation),
//!   [`geometry_cache`], [`raycast_index`] (the static parry3d raycast BVH).
//! - **Render leaves** — [`face_material`] and [`particle_render`], each a
//!   material plus the shader it loads, and the render-layer bookkeeping in
//!   [`probe_layers`].
//! - **Small models** — [`avatar_assets`], [`parcel_names`], [`sky_presets`],
//!   [`slt`].
//!
//! # What is *not* here, and the rule that decides it
//!
//! A module earns its place by having **more than one consumer crate**. Sitting
//! in leaf position is what makes a module *eligible*; it is not what makes it
//! belong. Eight modules — 4,230 lines, 36% of this crate — were named by
//! exactly one crate each, so every edit to one of them rebuilt the twenty-odd
//! crates stacked above this one in order to reach a single caller. They now
//! live in the crate that calls them:
//!
//! | module | now in |
//! | --- | --- |
//! | `radar_model` | `sl_viewer_people` |
//! | `shadow_visibility` | `sl_client_bevy_viewer` |
//! | `edit_math` | `sl_viewer_edit` |
//! | `appearance` | `sl_viewer_world_avatar` |
//! | `world_map_math` | `sl_viewer_map` |
//! | `ik` | `sl_viewer_world_avatar` |
//! | `sit_offset` | `sl_viewer_ui_context_menus` |
//! | `procedural` | `sl_viewer_world_avatar` |
//!
//! The same test the `sl-viewer-ui-widgets` split applied to `pie_menu` and
//! `emoji_complete`. Apply it again when something here acquires — or loses —
//! its second caller.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types and \
              Bevy plugins read as `face_material::FaceMaterial`. That only became \
              a lint when these items turned `pub` for the crate split; renaming \
              them would churn every call site in the viewer to satisfy a style \
              rule this codebase does not follow"
)]

pub mod avatar_assets;
pub mod coords;
pub mod face_material;
pub mod flexi;
pub mod geometry_cache;
pub mod minimap_math;
pub mod parcel_names;
pub mod particle_render;
pub mod probe_layers;
pub mod raycast_index;
pub mod sky_presets;
pub mod slt;
