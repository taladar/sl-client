//! The shared **sRGB decode** shader module (`srgb.wesl`): the reference viewer's
//! `srgb_to_linear`, applied by every shader that samples a world colour texture.
//!
//! A world colour texture is uploaded as plain `Rgba8Unorm`
//! ([`TextureUpload::WORLD_COLOR`](crate::TextureUpload::WORLD_COLOR)), not
//! `Rgba8UnormSrgb`, so the GPU filters the **encoded** bytes and the shader decodes
//! the filtered result — the reference's order. Filtering after the decode (what an
//! sRGB texture format does) blends in linear light, and a black-and-white blend
//! came out at about 188 where the reference draws 128.
//!
//! Like [`crate::water_fog`], there is no plugin: the consumers are materials in
//! several crates, and each calls [`load_srgb_shader`] from its own plugin's
//! `build`; loading the same handle twice is a no-op.

use bevy::app::App;
use bevy::asset::{Handle, load_internal_asset, uuid_handle};
use bevy::shader::Shader;

/// The internal handle the shared sRGB module (`srgb.wesl`) is loaded under.
/// Shaders reach it by its import path (`sl_client_bevy::srgb`), not by this
/// handle; the handle only has to be stable so a second load replaces the module
/// rather than adding a second one.
pub const SRGB_SHADER_HANDLE: Handle<Shader> = uuid_handle!("5d0b8e61-2f47-4c9a-8e13-a7c4f9d2e6b5");

/// Load the shared sRGB shader module into `app`, so a shader that imports
/// `sl_client_bevy::srgb` resolves.
///
/// Idempotent: every plugin whose shaders import the module calls this. Call it
/// from `build`, before the material plugin that needs it.
pub fn load_srgb_shader(app: &mut App) {
    load_internal_asset!(app, SRGB_SHADER_HANDLE, "srgb.wesl", |source, _| {
        Shader::from_wesl(source, "embedded://sl_client_bevy/srgb.wesl")
    });
}
