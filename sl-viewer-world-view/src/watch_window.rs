//! The `--watch` window of a headless viewer: a real window that shows the
//! off-screen one's frames, for a person to follow an automated run.
//!
//! A headless viewer's primary window is an off-screen one (Bevy's
//! `OffscreenWindow`): every camera draws into a texture nothing presents, and
//! the synthetic input injector is the only thing that moves the viewer. To
//! watch that, this plugin spawns a **second** window — winit-backed, and
//! deliberately *not* primary, so no reader of the primary window's cursor
//! follows the real mouse over it — carrying Bevy's `ViewOnlyWindow`, which
//! makes `bevy_winit` drop every input event arriving on it. Moving the mouse or
//! typing over the watch window changes nothing in the run.
//!
//! What it shows is the frame preview the capture harness already uses
//! ([`spawn_frame_preview`]: an unlit quad in front of a camera of its own,
//! letterboxed). The quad samples an ordinary [`Image`], into which the render
//! world copies the off-screen window's texture after each frame is rendered
//! (`copy_frame_to_preview`) — the off-screen texture itself lives only in
//! the render world, so it cannot be named by a material. The watch window is
//! one frame behind the run, which a person cannot see.
//!
//! Closing the watch window is a quit request like any window's, so a watched
//! run ends with the same graceful logout.

use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::TextureFormat;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::view::ExtractedWindows;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::window::{PresentMode, ViewOnlyWindow, WindowRef};

use crate::screenshot::{CaptureSize, FramePreview, spawn_frame_preview};

/// Shows a headless viewer's off-screen primary window on a view-only window
/// of its own — see [the module docs](self).
#[derive(Debug, Clone, Copy)]
pub struct WatchWindowPlugin {
    /// The off-screen window's size, which the preview image must match
    /// exactly for the render world's copy.
    pub frame: CaptureSize,
}

impl Plugin for WatchWindowPlugin {
    fn build(&self, app: &mut App) {
        let frame = self.frame;
        app.add_plugins(ExtractResourcePlugin::<WatchPreview>::default())
            .add_systems(
                Startup,
                move |mut commands: Commands,
                      mut images: ResMut<Assets<Image>>,
                      mut meshes: ResMut<Assets<Mesh>>,
                      mut materials: ResMut<Assets<StandardMaterial>>| {
                    spawn_watch_window(
                        &mut commands,
                        frame,
                        &mut images,
                        &mut meshes,
                        &mut materials,
                    );
                },
            );
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            // After `Render`, where the frame is rendered and submitted: this
            // copy is submitted after it, so it copies the finished frame.
            render_app.add_systems(Render, copy_frame_to_preview.in_set(RenderSystems::Cleanup));
        }
    }
}

/// The image the watch window's preview quad samples, and the render world's
/// copy destination.
#[derive(Resource, Debug, Clone, ExtractResource)]
pub struct WatchPreview {
    /// The preview image, the off-screen window's size and format.
    image: Handle<Image>,
}

/// Spawn the view-only watch window, the image its preview samples, and the
/// preview itself.
fn spawn_watch_window(
    commands: &mut Commands,
    frame: CaptureSize,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let window = commands
        .spawn((
            Window {
                title: "sl-client-bevy-viewer (watching a headless run)".to_owned(),
                // The viewer's own app-id, so a compositor's rules for the viewer
                // apply to the window watching it too.
                name: Some("sl-client-bevy-viewer".to_owned()),
                // Presenting must never wait on the compositor: a watch window the
                // compositor stops scheduling frames for (another workspace, a
                // minimised window) would otherwise stall the run it watches.
                present_mode: PresentMode::AutoNoVsync,
                ..default()
            },
            ViewOnlyWindow,
            Name::new("watch-window"),
        ))
        .id();
    // The copy destination: the off-screen window's exact size and format
    // (`Rgba8UnormSrgb`), which a texture-to-texture copy requires. A render
    // target image is also sampleable and a copy destination.
    let image = images.add(Image::new_target_texture(
        frame.width,
        frame.height,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    spawn_frame_preview(
        commands,
        FramePreview {
            frame: &image,
            size: frame,
            window: WindowRef::Entity(window),
        },
        meshes,
        materials,
    );
    commands.insert_resource(WatchPreview { image });
    info!("watch: showing the off-screen window's {frame} frames on a view-only window");
}

/// Render world: copy the finished off-screen frame into the watch preview's
/// image.
///
/// Its own encoder and submission, after the frame's: the queue executes them
/// in order, so the copy reads what the frame wrote.
fn copy_frame_to_preview(
    preview: Option<Res<WatchPreview>>,
    windows: Res<ExtractedWindows>,
    images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let Some(preview) = preview else {
        return;
    };
    let Some(source) = windows
        .primary
        .and_then(|primary| windows.windows.get(&primary))
        .and_then(|window| window.offscreen_texture.as_ref())
    else {
        return;
    };
    let Some(destination) = images.get(&preview.image) else {
        return;
    };
    if source.size() != destination.texture.size() {
        // The watch plugin sized its image from the off-screen window's size,
        // which nothing resizes; a mismatch is a wiring mistake, and a copy
        // would be a validation error.
        warn_once!(
            "watch: the off-screen window is {:?} but the preview is {:?}; nothing is shown",
            source.size(),
            destination.texture.size()
        );
        return;
    }
    let mut encoder = device.create_command_encoder(&default());
    encoder.copy_texture_to_texture(
        source.as_image_copy(),
        destination.texture.as_image_copy(),
        source.size(),
    );
    queue.submit([encoder.finish()]);
}

#[cfg(test)]
mod tests {
    use bevy::camera::RenderTarget;
    use bevy::prelude::*;
    use bevy::window::{PrimaryWindow, ViewOnlyWindow, WindowRef};
    use pretty_assertions::assert_eq;

    use super::{WatchPreview, spawn_watch_window};
    use crate::screenshot::CaptureSize;

    /// The watch window is view-only and **not** primary — the primary window
    /// is the off-screen one every cursor reader follows — and the preview is
    /// drawn on it, never on the primary window, where it would land inside
    /// the very frame it previews. The preview image is the frame's size,
    /// which the render world's copy needs.
    #[test]
    fn the_watch_window_is_view_only_and_carries_the_preview() -> Result<(), String> {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::mesh::MeshPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>();
        let frame = CaptureSize {
            width: 640,
            height: 360,
        };
        app.add_systems(
            Startup,
            move |mut commands: Commands,
                  mut images: ResMut<Assets<Image>>,
                  mut meshes: ResMut<Assets<Mesh>>,
                  mut materials: ResMut<Assets<StandardMaterial>>| {
                spawn_watch_window(
                    &mut commands,
                    frame,
                    &mut images,
                    &mut meshes,
                    &mut materials,
                );
            },
        );
        app.update();
        let world = app.world_mut();
        let windows: Vec<(Entity, bool, bool)> = world
            .query::<(Entity, Has<ViewOnlyWindow>, Has<PrimaryWindow>)>()
            .iter(world)
            .filter(|(entity, ..)| world.get::<Window>(*entity).is_some())
            .collect();
        let [(watch, true, false)] = windows.as_slice() else {
            return Err(format!(
                "want one view-only, non-primary window, got {windows:?}"
            ));
        };
        let targets: Vec<RenderTarget> = world
            .query_filtered::<&RenderTarget, With<Camera>>()
            .iter(world)
            .cloned()
            .collect();
        assert!(
            matches!(
                targets.as_slice(),
                [RenderTarget::Window(WindowRef::Entity(target))] if target == watch
            ),
            "the preview camera must target the watch window, got {targets:?}"
        );
        let image = world.resource::<WatchPreview>().image.clone();
        let size = world
            .resource::<Assets<Image>>()
            .get(&image)
            .map(Image::size)
            .ok_or("the preview image was not stored")?;
        assert_eq!(size, UVec2::new(640, 360));
        Ok(())
    }
}
