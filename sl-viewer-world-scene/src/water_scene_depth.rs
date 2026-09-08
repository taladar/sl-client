//! The opaque scene depth the water refraction rejects against
//! (`viewer-water-refraction-smears-avatar-silhouette`).
//!
//! **The bug.** `water.wgsl` refracts by sampling Bevy's
//! `view_transmission_texture` — a copy of the screen holding the whole opaque
//! scene — at a UV the wave normal displaces. The avatar is opaque, so it is *in*
//! that copy, and a water fragment just outside its silhouette samples, at the
//! displaced UV, a texel from *inside* the silhouette: the sea gets painted with a
//! ragged skin-coloured fringe hugging the outline. The displacement is a
//! screen-space offset, so the distance to the water never enters into it.
//!
//! **The reference's answer.** `class3/environment/waterF.glsl` reconstructs the
//! view-space position of the texel it is about to sample from the scene depth
//! buffer and, if that position is nearer than the water surface, throws the
//! displacement away and samples straight ahead instead:
//!
//! ```text
//! depth  = texture(depthMap, distort2).r;
//! refPos = getPositionWithNDC(vec3(distort2 * 2.0 - vec2(1.0), depth * 2.0 - 1.0));
//! if (pos.z < refPos.z - 0.05) { distort2 = distort; }
//! ```
//!
//! **What this module supplies.** The depth buffer that test needs, at the moment
//! the screen copy was taken. Bevy hands a material no such thing: its own
//! transmissive shading reads `depth_prepass_texture`, which exists only under a
//! [`DepthPrepass`](bevy::core_pipeline::prepass::DepthPrepass) — and this viewer
//! deliberately has none, because a prepass would build depth pipelines for the
//! custom sky / terrain / water materials whose `specialize` pins bespoke vertex
//! layouts (see [`crate::underwater_fog`]). A prepass would also re-submit every
//! opaque draw for a second geometry pass.
//!
//! So instead of re-rendering the scene, this **copies the depth already
//! rendered**: one `copy_texture_to_texture` from the view's own
//! [`ViewDepthTexture`] into an [`Image`] the shared [`WaterMaterial`] binds,
//! issued in the `Core3d` schedule after the pre-water translucency pass
//! (`crate::transparency`'s `PreWaterPass`) and before Bevy's transmissive pass —
//! the same seam `view_transmission_texture` is filled at, so colour and depth are
//! the same instant of the frame. It is the trick Bevy's own prepass node uses to
//! make its depth sampleable, minus the geometry pass.
//!
//! Because the depth is copied rather than re-rendered, it holds **everything**
//! drawn up to that point — terrain, prims, avatars, the sky dome at the far plane
//! — not only the materials a prepass would have accepted.
//!
//! **The size is the gate, at both ends.** The copy is skipped unless source and
//! destination agree on size and sample count — they briefly do not while a window
//! resize works through — and the shader, for its part, uses the bound depth only
//! when it measures the same as the view being shaded. That second check is what
//! makes the whole thing safe for the views this pass does not serve: the material
//! bind group is shared by *every* view, so a reflection-probe capture and an
//! offline fixture scene (which never leaves the `1×1` placeholder) both see a
//! depth buffer that is not theirs, and both decline to read it. The failure mode
//! either way is one less rejected refraction sample, never a wrong pixel.

use bevy::asset::RenderAssetUsages;
use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::renderer::{RenderContext, ViewQuery};
use bevy::render::texture::GpuImage;
use bevy::render::view::ViewDepthTexture;
use sl_client_bevy::WaterMaterial;

use crate::transparency::PreWaterPass;
use crate::water::WaterState;
use crate::world_api::ViewerCamera;

/// The sample count of the scene-depth copy. It must equal the main view's, or the
/// copy is refused — and the main view's is pinned to 4× by
/// [`viewer_camera_bundle`](crate::viewer_camera::viewer_camera_bundle) (the
/// underwater-fog pass binds it as `texture_depth_2d_multisampled`), so this is
/// pinned to the same number rather than derived: the shader's binding is part of a
/// material bind group shared by every view and cannot vary per view.
pub(crate) const SCENE_DEPTH_SAMPLES: u32 = 4;

/// The depth format Bevy's 3-D views render into
/// (`bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT`); the copy's destination
/// must match it exactly.
pub(crate) const SCENE_DEPTH_FORMAT: TextureFormat = TextureFormat::Depth32Float;

/// The scene-depth copy's own scheduling: the main-world system that keeps the
/// destination image sized to the view, and the render-world copy itself.
#[derive(Debug, Default)]
pub struct WaterSceneDepthPlugin;

impl Plugin for WaterSceneDepthPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<WaterSceneDepthView>::default(),
            ExtractResourcePlugin::<WaterSceneDepthTarget>::default(),
        ))
        .init_resource::<WaterSceneDepthTarget>()
        .add_systems(Update, size_water_scene_depth);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.add_systems(
            Core3d,
            copy_water_scene_depth
                .in_set(Core3dSystems::MainPass)
                // After the pre-water translucency (which is itself after the opaque
                // pass), and before the transmissive pass the water draws in — the
                // seam Bevy fills `view_transmission_texture` at, so the depth copy
                // and the colour copy are the same instant.
                .after(PreWaterPass)
                .before(bevy::pbr::main_transmissive_pass_3d),
        );
    }
}

/// Marks the view whose depth the water refraction reads: the main
/// [`ViewerCamera`], and only it. Carried into the render world so the copy system
/// can tell the main view from a reflection-probe capture, which renders water too
/// but must not overwrite the copy.
#[derive(Debug, Default, Clone, Copy, Component, ExtractComponent)]
pub struct WaterSceneDepthView;

/// The [`Image`] the depth is copied into and the water material samples.
///
/// A resource as well as a material field because the copy runs in the render
/// world, where the material asset is a bind group and the handle is no longer
/// reachable.
#[derive(Debug, Default, Clone, Resource, ExtractResource)]
pub(crate) struct WaterSceneDepthTarget {
    /// The destination image, or the default handle before
    /// [`size_water_scene_depth`] has seen a camera.
    pub(crate) image: Handle<Image>,
}

/// A depth [`Image`] of `width × height` for the scene-depth copy: the same format
/// and sample count as the view depth texture it receives, and the usages a copy
/// destination that is also sampled needs.
///
/// `RENDER_ATTACHMENT` is not optional despite nothing rendering into it: WebGPU
/// requires it of any multisampled texture. There is no pixel data — the image
/// exists to own a GPU texture.
pub(crate) fn scene_depth_image(width: u32, height: u32) -> Image {
    Image {
        data: None,
        texture_descriptor: TextureDescriptor {
            label: Some("water_scene_depth"),
            size: Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: SCENE_DEPTH_SAMPLES,
            dimension: TextureDimension::D2,
            format: SCENE_DEPTH_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        asset_usage: RenderAssetUsages::default(),
        ..default()
    }
}

/// The `1×1` placeholder the water material is created with, before
/// [`size_water_scene_depth`] has a camera to size the real one from — and the
/// depth an offline fixture scene keeps for good, having no copy pass to fill one.
///
/// It is `1×1` so that it can never be mistaken for a view's depth: the shader
/// reads the bound depth only when it measures the same as the view being shaded,
/// so the placeholder rejects no refraction sample and the sea looks exactly as it
/// did before this pass existed.
pub(crate) fn placeholder_scene_depth_image() -> Image {
    scene_depth_image(1, 1)
}

/// Keep the scene-depth image the same size as the main view's depth texture, so
/// the render-world copy is legal and the shader's pixel lookup lands where it
/// means to.
///
/// On a size change this **replaces the image asset** rather than resizing it in
/// place. Resizing recreates the GPU texture behind the same handle, and nothing
/// re-prepares a material whose *image* changed — the material would keep a bind
/// group pointing at the freed texture for good. A new handle is a change to the
/// material itself, which does re-prepare it; and if the new image's GPU texture is
/// not ready yet, `AsBindGroup` answers `RetryNextUpdate` and Bevy tries again next
/// frame. Both paths converge; the in-place resize does not.
pub(crate) fn size_water_scene_depth(
    camera: Query<&Camera, With<ViewerCamera>>,
    water: Option<Res<WaterState>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut target: ResMut<WaterSceneDepthTarget>,
) {
    let (Ok(camera), Some(water)) = (camera.single(), water) else {
        return;
    };
    let Some(size) = camera.physical_target_size() else {
        return;
    };
    // Read before `get_mut`: a `get_mut` marks the material changed, and a material
    // marked changed every frame is a bind group rebuilt every frame.
    let Some(bound) = materials
        .get(water.material())
        .map(|m| m.scene_depth.clone())
    else {
        return;
    };
    let matches = images
        .get(&bound)
        .is_some_and(|image| image.width() == size.x && image.height() == size.y);
    if matches {
        return;
    }
    let fresh = images.add(scene_depth_image(size.x, size.y));
    if let Some(mut material) = materials.get_mut(water.material()) {
        material.scene_depth = fresh.clone();
    }
    target.image = fresh;
}

/// Render world: copy this view's depth texture into the scene-depth image, so the
/// water's refraction can reject a sample that lies in front of the surface.
///
/// Runs for the main view only ([`WaterSceneDepthView`]). The copy is skipped —
/// leaving whatever the image already holds — unless the source and destination
/// agree on size and sample count, which they briefly do not while a resize is
/// working its way through.
fn copy_water_scene_depth(
    view: ViewQuery<&ViewDepthTexture, With<WaterSceneDepthView>>,
    target: Res<WaterSceneDepthTarget>,
    images: Res<RenderAssets<GpuImage>>,
    mut ctx: RenderContext,
) {
    let source = view.into_inner();
    let Some(destination) = images.get(&target.image) else {
        return;
    };
    let source_size = source.texture.size();
    if source_size != destination.texture.size()
        || source.texture.sample_count() != destination.texture.sample_count()
    {
        return;
    }
    ctx.command_encoder().copy_texture_to_texture(
        source.texture.as_image_copy(),
        destination.texture.as_image_copy(),
        source_size,
    );
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use bevy::camera::RenderTargetInfo;
    use bevy::prelude::*;
    use bevy::render::render_resource::TextureUsages;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::WaterMaterial;

    use super::{
        SCENE_DEPTH_FORMAT, SCENE_DEPTH_SAMPLES, WaterSceneDepthTarget,
        placeholder_scene_depth_image, scene_depth_image, size_water_scene_depth,
    };
    use crate::water::WaterState;
    use crate::world_api::ViewerCamera;

    /// The copy destination must match the view depth texture Bevy renders into,
    /// or `copy_texture_to_texture` refuses it: same format, same sample count,
    /// and the usages a sampled copy destination needs. `RENDER_ATTACHMENT` is
    /// there because WebGPU demands it of a multisampled texture, not because
    /// anything renders into this one.
    #[test]
    fn the_copy_destination_matches_a_view_depth_texture() {
        let image = scene_depth_image(1920, 1080);
        let descriptor = &image.texture_descriptor;
        assert_eq!(descriptor.format, SCENE_DEPTH_FORMAT);
        assert_eq!(descriptor.sample_count, SCENE_DEPTH_SAMPLES);
        assert_eq!(descriptor.mip_level_count, 1);
        assert_eq!(descriptor.size.width, 1920);
        assert_eq!(descriptor.size.height, 1080);
        assert!(descriptor.usage.contains(TextureUsages::COPY_DST));
        assert!(descriptor.usage.contains(TextureUsages::TEXTURE_BINDING));
        assert!(descriptor.usage.contains(TextureUsages::RENDER_ATTACHMENT));
        assert!(
            image.data.is_none(),
            "a depth copy target carries no pixel data",
        );
    }

    /// A zero-sized target (a window minimised to nothing) must still produce a
    /// legal texture — wgpu rejects a zero extent outright.
    #[test]
    fn a_zero_size_still_yields_a_legal_texture() {
        let image = scene_depth_image(0, 0);
        assert_eq!(image.width(), 1);
        assert_eq!(image.height(), 1);
        assert_eq!(
            image.texture_descriptor.size,
            placeholder_scene_depth_image().texture_descriptor.size,
        );
    }

    /// Build an app with the shared water material wearing the `1×1` placeholder
    /// and a [`ViewerCamera`](crate::world_api::ViewerCamera) whose render target
    /// measures `size`, with the sizing system scheduled. Returns the app and the
    /// water material handle.
    ///
    /// The target size is written straight into `Camera::computed`, which is what
    /// `physical_target_size` reads: filling it the usual way would need the whole
    /// render app, and the system only ever asks the camera how big its target is.
    fn app_with_camera_target(size: UVec2) -> (App, Handle<WaterMaterial>) {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<Assets<WaterMaterial>>()
            .init_resource::<WaterSceneDepthTarget>()
            .add_systems(Update, size_water_scene_depth);

        let placeholder = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(placeholder_scene_depth_image());
        let material = app
            .world_mut()
            .resource_mut::<Assets<WaterMaterial>>()
            .add(WaterMaterial {
                params: crate::water::default_water_params(),
                normal_map: placeholder.clone(),
                normal_map_next: placeholder.clone(),
                exclusion_mask: placeholder.clone(),
                scene_depth: placeholder,
            });
        app.world_mut()
            .insert_resource(WaterState::material_only(material.clone()));
        let mut camera = Camera::default();
        camera.computed.target_info = Some(RenderTargetInfo {
            physical_size: size,
            scale_factor: 1.0,
        });
        app.world_mut().spawn((ViewerCamera, camera));
        (app, material)
    }

    /// The first frame with a camera swaps the placeholder for an image sized to
    /// the view, binds it into the material, and publishes it for the render-world
    /// copy — all three, or the copy is a no-op somewhere.
    #[test]
    fn sizes_the_target_to_the_view_and_binds_it() {
        let (mut app, material) = app_with_camera_target(UVec2::new(800, 600));
        app.update();

        let bound = app
            .world()
            .resource::<Assets<WaterMaterial>>()
            .get(&material)
            .map(|water| water.scene_depth.clone())
            .expect("the water material outlives the frame");
        let images = app.world().resource::<Assets<Image>>();
        let image = images.get(&bound).expect("the sized target exists");
        assert_eq!((image.width(), image.height()), (800, 600));
        assert_eq!(
            app.world().resource::<WaterSceneDepthTarget>().image,
            bound,
            "the render world is told which image to copy into",
        );
    }

    /// Once the target matches the view the system must leave the material alone:
    /// a `get_mut` every frame would rebuild the bind group every frame.
    #[test]
    fn a_matching_target_does_not_touch_the_material() {
        let (mut app, material) = app_with_camera_target(UVec2::new(800, 600));
        app.update();
        let first = app
            .world()
            .resource::<Assets<WaterMaterial>>()
            .get(&material)
            .map(|water| water.scene_depth.clone())
            .expect("the water material outlives the frame");

        app.update();

        let second = app
            .world()
            .resource::<Assets<WaterMaterial>>()
            .get(&material)
            .map(|water| water.scene_depth.clone())
            .expect("the water material outlives the frame");
        assert_eq!(first, second, "a matching target is left in place");
    }
}
