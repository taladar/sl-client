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

use bevy::camera::{Exposure, Hdr};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::cluster::{ClusterConfig, ClusterFarZMode, ClusterZConfig};
use bevy::prelude::*;
use bevy::render::render_resource::TextureUsages;

use crate::exposure::SlExposure;
use crate::glow::SlGlow;
use crate::tonemap::SlTonemap;
use crate::underwater_fog::UnderwaterFog;
use sl_viewer_world_api::ViewerCamera;

/// The viewer's main camera at `transform`: the 3D camera with a readable,
/// multisampled depth texture, the region-scale projection, the clustered
/// lighting configuration, the HDR target and the selectors of every full-frame
/// pass. Spawn it with whatever marks the view for its consumer (a `CameraRig`
/// in the viewer, a render target in a harness).
#[must_use]
pub fn viewer_camera_bundle(transform: Transform) -> impl Bundle {
    (
        // The underwater-fog post-process (P23.1) samples the scene depth, so make
        // the main-pass depth texture readable (`TEXTURE_BINDING`). MSAA is pinned
        // to 4× (the default) so that depth texture is multisampled to match the
        // fog pass's `texture_depth_2d_multisampled` binding.
        Camera3d {
            depth_texture_usages: (TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING)
                .into(),
            ..default()
        },
        // A close near plane (2 cm) so the camera can push right up to fine detail
        // — an avatar's face — without the surface clipping away, and a far plane
        // well beyond a region's diagonal so distant objects do not vanish.
        Projection::Perspective(PerspectiveProjection {
            near: 0.02,
            far: 4096.0,
            ..default()
        }),
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
        Tonemapping::None,
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
    )
}
