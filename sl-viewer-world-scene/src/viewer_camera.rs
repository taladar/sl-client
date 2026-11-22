//! The main camera's component bundle — the one definition of what the viewer's
//! view carries, shared by the viewer's own `setup_scene`, the scene gallery and
//! the headless readback rig.
//!
//! It is a bundle rather than a spawn site because half of these components are
//! *selectors*: the underwater fog, exposure, tone mapper and glow passes each
//! find the view they run on by the component that carries their settings. A
//! camera spawned without them renders, and every one of those passes silently
//! does nothing — which is exactly how a harness ends up asserting on a frame
//! the viewer would never show.

use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::camera::{Exposure, Hdr, ShadowLodOrigin};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::cluster::{ClusterConfig, ClusterFarZMode, ClusterZConfig};
use bevy::prelude::*;
use bevy::render::render_resource::TextureUsages;

use crate::exposure::SlExposure;
use crate::glow::SlGlow;
use crate::tonemap::SlTonemap;
use crate::underwater_fog::UnderwaterFog;
use crate::water_scene_depth::WaterSceneDepthView;
use sl_viewer_kit::probe_layers::main_camera_render_layers;
use sl_viewer_world_api::{ViewerCamera, WorldUiVisibility};

/// The reference viewer's default vertical field of view, in radians
/// (`DEFAULT_FIELD_OF_VIEW`, `llcamera.h`): **60°**, and the default of its
/// persisted `CameraAngle` setting (`1.047197551`).
///
/// This is a fidelity number, not a taste one. How far away things look, how
/// much of a room fits on screen and how a camera offset feels are all read off
/// it, and a Second Life user's eye is calibrated to this value by every other
/// viewer. Bevy's own default is 45°, which framed noticeably less of the world:
/// the first two-viewer cross-check put both cameras at the same pose and
/// photographed five prims of a fixture row here against three there.
pub const DEFAULT_FIELD_OF_VIEW: f32 = core::f32::consts::PI / 3.0;

/// The narrowest vertical field of view the reference accepts, in radians
/// (`MIN_FIELD_OF_VIEW`): 5°. Also the lower bound of the preferences tab's
/// field-of-view slider, which reads it from here rather than restating it.
pub const MIN_FIELD_OF_VIEW: f32 = 5.0 * core::f32::consts::PI / 180.0;

/// The widest vertical field of view the reference accepts, in radians
/// (`MAX_FIELD_OF_VIEW`): 175°. Also the upper bound of the preferences tab's
/// field-of-view slider.
pub const MAX_FIELD_OF_VIEW: f32 = 175.0 * core::f32::consts::PI / 180.0;

/// A vertical field of view clamped the way the reference clamps one
/// (`LLCamera::getMinView` / `getMaxView`), which is **aspect-dependent**: the
/// limits bound the *horizontal* extent, so a wide view has its maximum divided
/// by the aspect ratio and a narrow one has its minimum divided by it.
///
/// Ported rather than simplified because the asymmetry is the whole content of
/// it: without the scaling, a 21:9 window admits a vertical field of view whose
/// horizontal span is past 175° and the projection turns inside out at the edges.
#[must_use]
pub fn clamp_field_of_view(fov: f32, aspect: f32) -> f32 {
    if !fov.is_finite() || aspect <= 0.0 || !aspect.is_finite() {
        return DEFAULT_FIELD_OF_VIEW;
    }
    let (min, max) = if aspect > 1.0 {
        (MIN_FIELD_OF_VIEW, MAX_FIELD_OF_VIEW / aspect)
    } else {
        (MIN_FIELD_OF_VIEW / aspect, MAX_FIELD_OF_VIEW)
    };
    fov.clamp(min.min(max), max.max(min))
}

/// The viewer's perspective projection: a close near plane (2 cm) so the camera
/// can push right up to fine detail — an avatar's face — without the surface
/// clipping away, a far plane well beyond a region's diagonal so distant objects
/// do not vanish, and the reference's own [`DEFAULT_FIELD_OF_VIEW`].
///
/// The far plane is deliberately **not** the reference's. It sets its far clip
/// to the draw distance (`LLAgentCamera` → `setFar(mDrawDistance)`) and draws its
/// sky in a pass of its own that the clip never reaches; ours is one scene, whose
/// sky dome is 3 km out and whose cloud dome is 15 km, so a far plane at the
/// draw distance would clip the sky away entirely. Matching it there is a change
/// to how the sky is drawn, not a change to a number.
///
/// A function rather than a literal inside [`viewer_camera_bundle`] so that code
/// which has to *reproduce* this framing without a camera — the render tier's
/// CPU projection of a subject's centre onto the readback frame — projects
/// through the very same numbers rather than a copy that can drift.
#[must_use]
pub fn viewer_projection() -> PerspectiveProjection {
    PerspectiveProjection {
        near: 0.02,
        far: 4096.0,
        fov: DEFAULT_FIELD_OF_VIEW,
        ..default()
    }
}

/// The viewer's main camera at `transform`: the 3D camera with a readable,
/// multisampled depth texture, the region-scale projection, the clustered
/// lighting configuration, the HDR target and the selectors of every full-frame
/// pass. Spawn it with whatever marks the view for its consumer (a `CameraRig`
/// in the viewer, a render target in a harness).
#[must_use]
pub fn viewer_camera_bundle(transform: Transform) -> impl Bundle {
    (
        // The underwater-fog post-process (P23.1) samples the scene depth, so make
        // the main-pass depth texture readable (`TEXTURE_BINDING`), and the water's
        // refraction reads a *copy* of it taken mid-frame
        // ([`crate::water_scene_depth`]), so it must also be copyable (`COPY_SRC` —
        // Bevy only adds that itself for a camera carrying a `DepthPrepass`, which
        // this one deliberately does not). MSAA is pinned to 4× (the default) so
        // that depth texture is multisampled to match the fog pass's
        // `texture_depth_2d_multisampled` binding and the water's own.
        Camera3d {
            depth_texture_usages: (TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC)
                .into(),
            ..default()
        },
        // See `viewer_projection`.
        Projection::Perspective(viewer_projection()),
        transform,
        ViewerCamera,
        // A clustered-forward Z config tuned for a viewer that pushes the camera
        // right up to avatars wearing small local lights (facelights). Bevy's
        // default `ClusterZConfig` keeps a **special first Z-slice** spanning
        // `[near_plane, first_slice_depth=5 m]`, and its default
        // `MaxClusterableObjectRange` far mode derives the grid's far plane from the
        // visible lights' own reach. Together those drop a worn light out of a
        // mid-distance band: the light and the surface it lights sit inside that 5 m
        // special slice, whose light handling fails, so a facelight only reaches the
        // face when the camera is inside the light sphere (a separate special case)
        // and goes dark across the rest of the near field. Shrinking the special
        // slice to `0.5 m` puts the whole avatar-viewing range into ordinary
        // well-conditioned logarithmic slices (which light correctly), and pinning
        // the far plane to a constant stops a lone small light from collapsing the
        // grid's depth range. The XY/Z counts stay at Bevy's defaults.
        ClusterConfig::FixedZ {
            total: 4096,
            z_slices: 24,
            z_config: ClusterZConfig {
                first_slice_depth: 0.5,
                far_z_mode: ClusterFarZMode::Constant(512.0),
            },
            dynamic_resizing: true,
        },
        Msaa::Sample4,
        // P33.3: render the scene into a floating-point target and tonemap it once,
        // at the end, with the reference viewer's own tone mapper (`tonemap`).
        //
        // Without `Hdr` the view target is 8-bit, which Bevy takes as the cue to
        // tonemap `StandardMaterial` inside the mesh shader — leaving the viewer's
        // custom sky / terrain / water materials (which never call Bevy's tonemapper)
        // merely *clipped* at 1.0 instead, two different transfers in one frame. The
        // reflection probes capture the scene linear and un-tonemapped, so that split
        // also made a probe's cubemap disagree with what the eye saw of the very same
        // surroundings — the miscalibration P33.3 exists to fix. One HDR target plus
        // one tone mapper at the end puts every material in the one linear space the
        // probes capture.
        Hdr,
        // Bevy's tonemapping is switched off: `SlTonemap` (the pass and its settings,
        // mirroring the reference's `RenderTonemapType` / `RenderTonemapMix` /
        // `RenderExposure`) is this viewer's tone mapper, and two would double up.
        (
            Tonemapping::None,
            bevy::core_pipeline::tonemapping::DebandDither::Disabled,
        ),
        SlTonemap::default(),
        // The reference's dynamic exposure inputs (the `exp_min`/`exp_max` range is
        // filled per frame from the active sky by `refresh_exposure`). Only on the
        // main camera — the reflection-probe capture cameras stay linear.
        SlExposure::default(),
        // The reference's glow pass inputs (disabled by default; see `glow.rs`).
        // Only on the main camera.
        SlGlow::default(),
        // Bevy's *photometric* exposure: what turns the sun's illuminance (lux) and a
        // prim light's lumens into the linear radiance the frame is composed in. It is
        // a distinct thing from the reference's `RenderExposure` (a plain scale on the
        // finished linear frame, carried by `SlTonemap`), and it is spelled out rather
        // than left implicit because the reflection probes read it: their intensity is
        // derived from it (`probes::probe_intensity`), so a probe reproduces the
        // radiance it captured instead of re-scaling it.
        Exposure::default(),
        // The Second Life / Firestorm glow pass (`RenderGlow*`) is [`SlGlow`] above
        // (the faithful alpha-mask separable-Gaussian glow, `glow.rs`), which runs
        // after the tone mapper as the reference does — it replaced the Bevy
        // screen-space `Bloom` this camera used to carry.
        //
        // The `UnderwaterFog` component both carries the per-frame fog parameters
        // and selects this camera for the fog pass.
        UnderwaterFog::default(),
        // Selects this camera as the one whose depth the water's refraction reads:
        // the copy pass serves exactly one view, and a reflection-probe capture
        // (which renders water too) must not overwrite it.
        WaterSceneDepthView,
        // The point / spot light shadow LOD follows the viewpoint. Without the
        // marker Bevy takes a window camera, else *any* camera — a reflection
        // probe's capture camera, when this one renders into an image (the
        // render tiers) or no window is primary — and warns that it had to.
        //
        // Paired with the layers only because the bundle tuple is at its limit:
        // the world, plus the world-anchored UI (hover text, name tags), which
        // [`apply_world_ui_visibility`] takes off again whenever a frame leaves
        // the UI out.
        (ShadowLodOrigin, main_camera_render_layers(true)),
    )
}

/// Keeps the main camera's layers in step with [`WorldUiVisibility`]: the
/// world-anchored UI's layer is on the [`ViewerCamera`] exactly while no hider
/// holds it off.
///
/// Add it to any app whose frames can leave the UI out — the capture harness's
/// world-only runs and the snapshot floater's shots without the UI — so hover
/// text and name tags leave the frame with the rest of the UI, as in the
/// reference.
#[derive(Debug, Default, Clone, Copy)]
pub struct WorldUiLayerPlugin;

impl Plugin for WorldUiLayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldUiVisibility>().add_systems(
            PostUpdate,
            apply_world_ui_visibility.before(VisibilitySystems::CheckVisibility),
        );
    }
}

/// Put the world-anchored UI's layer on the main camera, or take it off, to
/// match [`WorldUiVisibility`].
///
/// Every frame rather than on a change to the resource, so a camera spawned
/// after the last change is put right too; the comparison keeps an unchanged
/// camera from being marked changed. In `PostUpdate` before Bevy decides what
/// each camera sees, so the frame a hider takes is already without the UI.
pub fn apply_world_ui_visibility(
    visibility: Res<WorldUiVisibility>,
    mut cameras: Query<&mut RenderLayers, With<ViewerCamera>>,
) {
    let wanted = main_camera_render_layers(visibility.shown());
    for mut layers in &mut cameras {
        layers.set_if_neq(wanted.clone());
    }
}

#[cfg(test)]
mod tests {
    use bevy::camera::visibility::RenderLayers;
    use bevy::prelude::*;
    use sl_viewer_kit::probe_layers::{main_camera_render_layers, world_ui_render_layers};
    use sl_viewer_world_api::{ViewerCamera, WorldUiHider, WorldUiVisibility};

    use super::{
        DEFAULT_FIELD_OF_VIEW, MAX_FIELD_OF_VIEW, MIN_FIELD_OF_VIEW, WorldUiLayerPlugin,
        clamp_field_of_view, viewer_projection,
    };

    /// Whether the app's one viewer camera draws the world-anchored UI after a
    /// frame.
    fn camera_draws_world_ui(app: &mut App) -> bool {
        app.update();
        let mut cameras = app
            .world_mut()
            .query_filtered::<&RenderLayers, With<ViewerCamera>>();
        cameras
            .iter(app.world())
            .all(|layers| layers.intersects(&world_ui_render_layers()))
    }

    /// Set the app's world-UI holds.
    fn hold(app: &mut App, change: impl FnOnce(&mut WorldUiVisibility)) {
        change(&mut app.world_mut().resource_mut::<WorldUiVisibility>());
    }

    /// The camera draws hover text and name tags by default, drops them while
    /// any hider holds them off, and draws them again only once every hider has
    /// let go — a snapshot restoring inside a world-only capture run leaves the
    /// run world-only.
    #[test]
    fn the_world_ui_leaves_the_view_while_any_hider_holds_it() {
        let mut app = App::new();
        app.add_plugins(WorldUiLayerPlugin);
        app.world_mut()
            .spawn((ViewerCamera, main_camera_render_layers(true)));
        assert!(camera_draws_world_ui(&mut app), "drawn with nothing held");

        hold(&mut app, |ui| ui.hide(WorldUiHider::Capture));
        assert!(
            !camera_draws_world_ui(&mut app),
            "a world-only capture draws it"
        );

        hold(&mut app, |ui| ui.hide(WorldUiHider::Snapshot));
        hold(&mut app, |ui| ui.release(WorldUiHider::Snapshot));
        assert!(
            !camera_draws_world_ui(&mut app),
            "the snapshot's restore showed it inside a world-only capture"
        );

        hold(&mut app, |ui| ui.release(WorldUiHider::Capture));
        assert!(camera_draws_world_ui(&mut app), "not drawn once released");
    }

    /// A camera spawned while the UI is held off (a capture run hides it before
    /// the viewer's camera exists) is put right too.
    #[test]
    fn a_camera_spawned_while_the_ui_is_held_off_does_not_draw_it() {
        let mut app = App::new();
        app.add_plugins(WorldUiLayerPlugin);
        hold(&mut app, |ui| ui.hide(WorldUiHider::Capture));
        app.update();
        app.world_mut()
            .spawn((ViewerCamera, main_camera_render_layers(true)));
        assert!(!camera_draws_world_ui(&mut app));
    }

    /// The default is the reference's 60°, not the engine's 45°.
    ///
    /// A fidelity pin rather than a taste one: the two-viewer cross-check found
    /// this by photographing one scene from one pose and framing five prims of a
    /// row here against three there. Anything that "tidies" this back to a
    /// framework default is changing how far away the whole world looks.
    #[test]
    fn the_default_field_of_view_is_the_references() {
        let degrees = DEFAULT_FIELD_OF_VIEW.to_degrees();
        assert!(
            (degrees - 60.0).abs() < 1.0e-3,
            "the default field of view is {degrees}°, not 60°"
        );
        assert!(
            (viewer_projection().fov - DEFAULT_FIELD_OF_VIEW).abs() < 1.0e-6,
            "the camera bundle does not use the default field of view"
        );
    }

    /// The reference's clamp is **asymmetric in the aspect ratio**, because the
    /// 5°–175° bounds limit the *horizontal* extent: a wide view therefore
    /// admits a narrower vertical field than a square one. Without the scaling a
    /// 21:9 window opens past 175° horizontally and the projection turns inside
    /// out at the edges.
    #[test]
    fn a_wide_view_may_not_open_as_far_as_a_square_one() {
        let square = clamp_field_of_view(MAX_FIELD_OF_VIEW, 1.0);
        let wide = clamp_field_of_view(MAX_FIELD_OF_VIEW, 21.0 / 9.0);
        assert!(
            (square - MAX_FIELD_OF_VIEW).abs() < 1.0e-6,
            "a square view keeps the full maximum"
        );
        assert!(
            wide < square,
            "a wide view should be held below the square maximum, got {wide} against {square}"
        );
        assert!(
            (wide - MAX_FIELD_OF_VIEW / (21.0 / 9.0)).abs() < 1.0e-6,
            "a wide view's maximum is the bound divided by its aspect"
        );
    }

    /// And the mirror image: a taller-than-wide view cannot close as far,
    /// because the minimum is what bounds its width.
    #[test]
    fn a_narrow_view_may_not_close_as_far_as_a_square_one() {
        let narrow = clamp_field_of_view(MIN_FIELD_OF_VIEW, 0.5);
        assert!(
            narrow > MIN_FIELD_OF_VIEW,
            "a narrow view should be held above the square minimum, got {narrow}"
        );
        assert!((narrow - MIN_FIELD_OF_VIEW / 0.5).abs() < 1.0e-6);
    }

    /// An ordinary field of view passes through untouched at every ordinary
    /// aspect — the clamp is a guard, not a policy.
    #[test]
    fn an_ordinary_field_of_view_is_left_alone() {
        for aspect in [1.0, 4.0 / 3.0, 16.0 / 9.0, 21.0 / 9.0] {
            let clamped = clamp_field_of_view(DEFAULT_FIELD_OF_VIEW, aspect);
            assert!(
                (clamped - DEFAULT_FIELD_OF_VIEW).abs() < 1.0e-6,
                "60° was moved to {clamped} at aspect {aspect}"
            );
        }
    }

    /// A view with no aspect yet — a camera whose target has not been sized, or
    /// one that has been handed a zero or a not-a-number — falls back to the
    /// default rather than propagating the nonsense into a projection matrix.
    #[test]
    fn a_nonsense_aspect_falls_back_to_the_default() {
        for aspect in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let clamped = clamp_field_of_view(DEFAULT_FIELD_OF_VIEW, aspect);
            assert!((clamped - DEFAULT_FIELD_OF_VIEW).abs() < 1.0e-6);
        }
        assert!(
            (clamp_field_of_view(f32::NAN, 1.0) - DEFAULT_FIELD_OF_VIEW).abs() < 1.0e-6,
            "a not-a-number field of view falls back too"
        );
    }
}
