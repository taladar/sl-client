//! The shared **water fog** shader module (`water_fog.wgsl`): the port of the
//! reference viewer's `getWaterFogViewNoClip` / `applyWaterFogViewLinear`, in one
//! place because more than one shader has to compute it.
//!
//! The reference applies its water fog twice over: once across the opaque scene, in
//! the deferred haze pass, and once **per fragment** inside every alpha-blended
//! surface's own shader — because a translucent draw writes no depth and so is
//! nowhere in the buffer the haze pass reads. This viewer does the same, so the
//! haze pass (`sl_viewer_world_scene::underwater_fog`), the water surface's own
//! underside ([`crate::water`]), and the face material
//! (`sl_viewer_kit::face_material`) all import this module rather than each
//! carrying a copy of the arithmetic.
//!
//! There is no plugin: a shader module is loaded, not registered, and the consumers
//! are in three different crates with no ordering between them. Each calls
//! [`load_water_fog_shader`] from its own plugin's `build`; loading the same handle
//! twice inserts the same shader twice, which is a no-op.

use bevy::app::App;
use bevy::asset::{Handle, load_internal_asset, uuid_handle};
use bevy::shader::Shader;

/// The internal handle the shared water-fog module (`water_fog.wgsl`) is loaded
/// under. Shaders reach it by its `#define_import_path`
/// (`sl_client_bevy::water_fog`), not by this handle; the handle only has to be
/// stable so a second load replaces the module rather than adding a second one.
pub const WATER_FOG_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("0c7d1e4a-5b28-4f36-9a71-6d3e8f2b4c90");

/// Load the shared water-fog shader module into `app`, so a shader that
/// `#import`s `sl_client_bevy::water_fog` resolves.
///
/// Idempotent: every plugin whose shaders import the module calls this, and the
/// second and later calls simply re-insert the same asset. Call it from `build`,
/// before the material plugin that needs it — a shader is resolved when its
/// pipeline is first specialized, which is long after any plugin's `build`.
pub fn load_water_fog_shader(app: &mut App) {
    load_internal_asset!(
        app,
        WATER_FOG_SHADER_HANDLE,
        "water_fog.wgsl",
        Shader::from_wgsl
    );
}
