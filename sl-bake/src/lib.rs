//! Pure client-side avatar **bake compositing** for Second Life / OpenSim
//! clients — the legacy / OpenSim baking path, and the fallback whenever a
//! server-published bake is absent.
//!
//! See the crate `README.md` for an overview. Modern Second Life bakes the
//! avatar's wearable layers on the server and publishes the composited textures
//! (the viewer just fetches them — Phase 14). OpenSim, and any grid without
//! server-side baking, instead expects the **client** to composite the bake from
//! the avatar's worn wearable layers; without it our own avatar there is an
//! untextured cloud. This crate is that compositor.
//!
//! Like its `sl-avatar` / `sl-mesh` / `sl-sculpt` / `sl-texture` siblings it is
//! deliberately **Bevy-free and I/O-free**: it never fetches or decodes, taking
//! already-decoded [`sl_texture::DecodedImage`] layers (sourced by the runtime
//! crates from the shared `sl-texture` `TextureStore`) and returning a plain
//! RGBA8 [`BakedImage`].
//!
//! The two pieces are:
//!
//! - [`region`] — [`BakeRegion`], the six base-body bakes (head / upper / lower
//!   / eyes / skirt / hair) and their [`sl_proto`] baked-slot mapping.
//! - [`composite`] — the [`Layer`] stack model ([`LayerKind`] / [`TexGen`] /
//!   tint) and [`composite_region`], which walks a region's ordered layers
//!   bottom-to-top over a transparent canvas following the reference viewer's
//!   `LLTexLayerSet` render, reimplemented idiomatically.
//!
//! This is P15.1 of the viewer road map: region compositing over given layers.
//! Sourcing the layers from the worn wearables (P15.2) and rendering /
//! publishing the composite (P15.3 / P15.4) is the runtime crates' job.

pub mod composite;
pub mod plan;
pub mod region;

pub use composite::{BakedImage, Layer, LayerKind, TexGen, composite_region};
pub use plan::{LayerTint, PlannedLayer, region_layers, region_plan};
pub use region::BakeRegion;

pub use sl_texture::DecodedImage;
