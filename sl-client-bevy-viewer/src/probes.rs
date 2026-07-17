//! Reflection probes (Phase 33): fold a prim's `LLReflectionProbeParams`
//! extra-param block into the scene mirror, and drive both the scene-wide
//! **default** reflection probe and the **per-object local** probes — each a
//! real-time captured environment cubemap — the way the reference viewer's
//! `LLReflectionMapManager` does.
//!
//! **Detect (ingest, P33.1).** A reflection probe is not a `PrimFlags` bit — a prim
//! is a probe exactly when it carries the `LLReflectionProbeParams` extra-param block
//! (`ExtraParams` type `0x90`), the way `LLViewerObject::getReflectionProbeParams`
//! keys off the block's presence. sl-proto already decodes that block into a
//! [`ReflectionProbe`] on `Object::extra.reflection_probe` (the two packed floats —
//! ambiance and clip distance — plus the flag byte: box-vs-sphere influence
//! volume, dynamic-object capture, and mirror). [`reflection_probe_from_object`]
//! lifts a present block onto an [`ObjectReflectionProbe`] component that
//! [`apply_object`] attaches to (or clears from) each object entity as its updates
//! arrive, exactly the way [`apply_flexi`](crate::flexi) /
//! [`apply_light`](crate::lights) / [`apply_particles`](crate::particles) do — a
//! prim toggled probe on or off in-world flips the block present / absent, so the
//! component is refreshed every update. The component also carries the prim's metre
//! scale, from which the local probe's influence volume is derived.
//!
//! **Capture (P33.1 / P33.2).** Bevy 0.19 has the sink side of reflection probes — a
//! [`GeneratedEnvironmentMapLight`] on the view is the "global" probe that lights
//! every PBR surface (the reference viewer's default probe), and the same component
//! beside a [`LightProbe`] on an ordinary entity is a *local* reflection probe whose
//! cuboid influence volume overrides it inside. What Bevy lacks is the *source*: it
//! never renders the scene into a cubemap. This module supplies that missing half —
//! mirroring `LLReflectionMapManager`'s real-time capture — with a **capture rig**
//! ([`CaptureRig`]) per probe: six 90° cameras (one per cube face) that render the
//! scene into six `Rgba16Float` colour targets, which a render-world blit
//! ([`copy_probe_faces`]) copies into the six layers of a cube [`Image`], which a
//! [`GeneratedEnvironmentMapLight`] filters (irradiance + roughness-mipped radiance)
//! into the diffuse / specular maps the PBR shader samples. Six separate colour
//! targets plus a copy (rather than rendering straight into the cube's layers) keeps
//! camera sizing on Bevy's ordinary image-target path — a cube-layer render target
//! would need render-world manual texture views that the main-world camera-sizing
//! pass cannot resolve.
//!
//! Rig 0 is the **default probe**, captured around the viewpoint and bound globally
//! to the main view. Rigs `1..=`[`MAX_LOCAL_PROBES`] are a **pool** handed to the
//! nearest local probes ([`drive_local_probes`]) — the budget local lights (P25.2)
//! spend the same way, and the reason the pool is small: each rig costs six scene
//! re-renders per refresh, so the probes that cannot influence what is on screen must
//! not pay for one.
//!
//! **Local probe volumes (P33.2).** A rig's holder is a [`LightProbe`] entity
//! parented to the probe prim, so it rides the prim's position and rotation, with a
//! local scale that reproduces the reference viewer's influence volume: for a
//! **box**-volume probe the prim's own metre scale (`LLReflectionMap::getBox` uses
//! `scale * 0.5` as the box half-extents); for a **sphere**-volume probe a cube of
//! side `scale.x` — the smallest one containing the reference's
//! `radius = scale.x * 0.5` sphere — softened by a [`SPHERE_FALLOFF`] taper, since
//! Bevy's light-probe volume is always a cuboid. Bevy then picks the nearest
//! applicable probe per fragment, falling back to the view's default probe outside
//! every volume, exactly the layering the reference shader does.
//!
//! **Capture cadence.** The capture is amortized ([`CaptureSchedule`]): only one cube
//! face anywhere in the scene is re-rendered per frame, in six-frame bursts, so a rig
//! is refreshed every [`CAPTURE_PERIOD_FRAMES`] and the total cost stays proportional
//! to the number of *live* probes rather than to the frame rate. A freshly assigned
//! rig jumps the queue ([`CaptureSchedule::urgent`]) so a probe entering the budget
//! shows its own surroundings almost immediately instead of the previous tenant's.
//!
//! **Consistent image-based lighting.** Bevy applies the view environment map only
//! to `StandardMaterial` (prims, meshes, avatars). The viewer's custom sky / terrain
//! / water materials do not sample it, so — to avoid double-counting a flat ambient
//! on top of the probe's diffuse contribution — [`suppress_global_ambient`] drops the
//! sky-set `GlobalAmbientLight`, and the terrain and water shaders sample the probe
//! themselves (terrain reads its diffuse irradiance for ambient; water reflects the
//! specular cube). Sky stays the source and is not itself lit by the probe.
//!
//! **Brightness calibration (P33.3).** A probe is calibrated when it *reproduces* the
//! surroundings it captured rather than re-scaling them: a mirror shows the world at
//! the radiance the eye sees it, and a diffuse surface's ambient is the irradiance
//! that world casts. That is one equation — [`probe_intensity`] — and it needs no
//! tuning constant, only the view's `Exposure`; the reference viewer likewise never
//! rescales a probe's radiance (`radscale` is 1). [`PROBE_GAIN`] /
//! `SL_VIEWER_PROBE_GAIN` is therefore an A/B knob, not a look control, and
//! `SL_VIEWER_PROBE_TEST_SPHERE=1` spawns a mirror ball to check the result against
//! the scene behind it.
//!
//! What made this a task of its own is that the equation only *closes* if the eye and
//! the capture see the same scene. They did not: the viewer's camera used to render to
//! an 8-bit target, which is Bevy's cue to tonemap `StandardMaterial` in the mesh
//! shader while the custom sky / terrain / water materials (which never call Bevy's
//! tonemapper) were merely clipped at 1.0 — so the sky the eye saw was flattened to
//! white where the probes' HDR capture cameras recorded its true radiance, and the
//! probes lit the world several times too brightly. P33.3 gives the camera an HDR
//! target and one tone mapper at the end ([`tonemap`](crate::tonemap), the reference
//! viewer's own), which puts every material in the single linear space the probes
//! capture. The other half of "the eye and the capture see the same scene" is
//! [`light_capture_cameras`]: a capture camera is lit by the probe too, or it would
//! render a world with no image-based lighting at all — darker than the one beside it.
//! See also [`probe_ambient_scale`] / `SL_VIEWER_PROBE_AMBIENT_SCALE`.
//!
//! Deliberately not modelled: a probe's **ambiance**, which in the reference scales
//! only the irradiance half of its contribution and blends the flat sky ambient back
//! in below 1 (`tapIrradianceMap`). Bevy's probe has a *single* `intensity` over both
//! halves, so the irradiance cannot be scaled without scaling the reflection with it —
//! and the reflection must stay at unit gain. Every probe therefore runs at the
//! reference's ambiance-1 point (its `RenderSkyAutoAdjustLegacy` default), where the
//! probe's irradiance *is* the ambient and no flat fill is added — which is exactly
//! what [`suppress_global_ambient`] arranges. A probe's **dynamic** flag is implicitly
//! always on (a rig re-renders the whole scene, avatars included); its **mirror** flag
//! (the reference's separate screen-space "hero" probe) is out of scope.
//!
//! [`apply_object`]: crate::objects
//! [`GeneratedEnvironmentMapLight`]: bevy::light::GeneratedEnvironmentMapLight

use crate::camera::FlyCamera;
use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::CUBE_MAP_FACES;
use bevy::camera::{Exposure, Hdr, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    Extent3d, Origin3d, TexelCopyTextureInfo, TextureAspect, TextureDimension, TextureFormat,
    TextureUsages, TextureViewDescriptor, TextureViewDimension,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderSystems};
use sl_client_bevy::{Object, ReflectionProbe, ReflectionProbeFlags};
use std::collections::VecDeque;
use std::f32::consts::FRAC_PI_2;

/// A component marking an object entity as a **reflection probe**, carrying the
/// decoded `LLReflectionProbeParams` parameters (in Second Life semantics) plus
/// the prim's metre scale — the inputs the capture / volume side needs.
///
/// Attached to (and refreshed / cleared on) each object entity by
/// [`apply_object`](crate::objects) as its updates arrive. See
/// [`reflection_probe_from_object`] for the present-vs-absent lift.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct ObjectReflectionProbe {
    /// The decoded reflection-probe parameters: the ambiance (irradiance) scale,
    /// the reflection-capture near-clip distance in metres, and the flag set
    /// (box-vs-sphere volume, dynamic capture, mirror).
    pub(crate) data: ReflectionProbe,
    /// The prim's Second Life metre scale, refreshed every update so a **resized**
    /// probe's influence volume (a box of these half-extents, or a sphere of the
    /// bounding radius) stays correct. The reference viewer likewise derives the
    /// probe volume from the prim's dimensions, not from the probe params.
    pub(crate) scale: [f32; 3],
}

impl ObjectReflectionProbe {
    /// Whether this probe's influence volume is a **box** (the prim's oriented
    /// bounding box) rather than a **sphere** — the `BOX_VOLUME` flag, which the
    /// reference reads as `LLVOVolume::getReflectionProbeIsBox`.
    const fn is_box_volume(&self) -> bool {
        self.data.flags.contains(ReflectionProbeFlags::BOX_VOLUME)
    }

    /// The influence volume as a scale for Bevy's unit-cube [`LightProbe`] volume,
    /// in the prim's **local** frame (the frame below the object entity, i.e. still
    /// Second Life axes — the object entity carries the basis change, exactly as the
    /// geometry holder's scale does).
    ///
    /// A **box** probe scales the unit cube by the prim's metre scale, so the volume
    /// is the prim's own oriented box (`LLReflectionMap::getBox`: half-extents
    /// `scale * 0.5`). A **sphere** probe has no cuboid counterpart in Bevy, so it
    /// becomes the smallest cube containing the reference's sphere — whose radius is
    /// `scale.x * 0.5`, the *X* extent alone (`LLReflectionMap::update`) — and the
    /// corners the cube adds beyond that sphere are taken back out by
    /// [`SPHERE_FALLOFF`].
    const fn volume_scale(&self) -> Vec3 {
        let [x, y, z] = self.scale;
        if self.is_box_volume() {
            Vec3::new(x, y, z)
        } else {
            Vec3::splat(x)
        }
    }

    /// The [`LightProbe`] falloff (per axis, as a fraction of the volume) this
    /// probe's influence tapers over: a hard-edged [`BOX_FALLOFF`] for a box volume,
    /// the far softer [`SPHERE_FALLOFF`] for a sphere approximated by a cube.
    const fn falloff(&self) -> Vec3 {
        if self.is_box_volume() {
            Vec3::splat(BOX_FALLOFF)
        } else {
            Vec3::splat(SPHERE_FALLOFF)
        }
    }

    /// The probe's influence radius in metres, as `LLReflectionMap::update` computes
    /// it: the half-diagonal of the prim's box for a box volume, half the prim's *X*
    /// extent for a sphere. Used to rank probes by distance (the reference's
    /// `mDistance = |eye - origin| - radius`), so a large probe the camera is just
    /// outside of outranks a tiny one the same distance away.
    fn radius(&self) -> f32 {
        let [x, y, z] = self.scale;
        if self.is_box_volume() {
            Vec3::new(x * 0.5, y * 0.5, z * 0.5).length()
        } else {
            x * 0.5
        }
    }

    /// The near-clip distance the probe's capture cameras render with — the probe's
    /// own clip distance, floored at [`MIN_NEAR_CLIP`] the way
    /// `LLReflectionMap::getNearClip` floors it at `MINIMUM_NEAR_CLIP`. It is how a
    /// probe inside a room excludes the walls of the prim (or the furniture) it sits
    /// in from its own reflection.
    const fn near_clip(&self) -> f32 {
        self.data.clip_distance.max(MIN_NEAR_CLIP)
    }
}

/// Lift an object's reflection-probe block onto an [`ObjectReflectionProbe`], or
/// `None` when the object is not (or is no longer) a probe.
///
/// Mirrors the reference viewer's `LLViewerObject::getReflectionProbeParams`: a
/// prim is a probe exactly when it carries a reflection-probe extra-param block, so
/// this is a straight `Option` lift with no sentinel to reject.
pub(crate) fn reflection_probe_from_object(object: &Object) -> Option<ObjectReflectionProbe> {
    object
        .extra
        .reflection_probe
        .map(|data| ObjectReflectionProbe {
            data,
            scale: [object.scale.x, object.scale.y, object.scale.z],
        })
}

/// Reconcile an object entity's [`ObjectReflectionProbe`] component with its
/// current reflection-probe block: insert / refresh it when the prim is a probe,
/// remove it when the prim was changed to non-probe in-world (the block dropped) or
/// never was one. Called on both the spawn and update paths so a prim toggled probe
/// on or off between updates is tracked, the way [`apply_flexi`](crate::flexi) /
/// [`apply_light`](crate::lights) / [`apply_particles`](crate::particles) are.
pub(crate) fn apply_reflection_probe(
    entity: Entity,
    probe: Option<ObjectReflectionProbe>,
    commands: &mut Commands,
) {
    match probe {
        Some(probe) => {
            let data = &probe.data;
            debug!(
                "object reflection probe: ambiance={:.2} clip_distance={:.2}m \
                 box_volume={} dynamic={} mirror={}",
                data.ambiance,
                data.clip_distance,
                data.flags.contains(ReflectionProbeFlags::BOX_VOLUME),
                data.flags.contains(ReflectionProbeFlags::DYNAMIC),
                data.flags.contains(ReflectionProbeFlags::MIRROR),
            );
            commands.entity(entity).insert(probe);
        }
        None => {
            commands.entity(entity).remove::<ObjectReflectionProbe>();
        }
    }
}

/// The per-face cubemap capture resolution, in texels. Must be a power of two
/// (and ≤ 8192) for [`GeneratedEnvironmentMapLight`]'s filter to accept the cube.
/// 128² per face matches the reference viewer's default probe resolution — enough
/// for a convincing reflection once roughness-filtered, cheap enough to re-render
/// six faces on a slow cadence.
const CAPTURE_SIZE: u32 = 128;

/// The number of cube faces (and therefore capture cameras) per probe.
const FACE_COUNT: usize = 6;

/// How many **per-object local** probes (P33.2) are captured and bound at once —
/// the nearest ones win, the way the nearest / brightest prim lights win the P25.2
/// [`MAX_LOCAL_LIGHTS`](crate::lights) budget.
///
/// Each one costs a capture rig: six scene re-renders per refresh plus a cubemap
/// filter, so the budget is deliberately small. Bevy in any case binds at most
/// `MAX_VIEW_LIGHT_PROBES` (8) reflection probes per view, and the reference viewer
/// likewise keeps only a bounded set of probes resident
/// (`LLReflectionMapManager::mReflectionProbeCount`).
const MAX_LOCAL_PROBES: usize = 4;

/// The total number of capture rigs: the default (global) probe plus the local pool.
const RIG_COUNT: usize = MAX_LOCAL_PROBES.saturating_add(1);

/// Frames between two refreshes of the *same* rig. The six faces of a rig are
/// re-rendered one per frame in a burst, then the schedule moves on to the next rig
/// — so the expensive six-face scene re-render (each with its own shadow pass) is
/// paid only in brief bursts, and the whole scene's probes are refreshed on this
/// period regardless of how many are live. The environment changes slowly, so the
/// resulting staleness is imperceptible while the frame rate stays near the un-probed
/// baseline.
const CAPTURE_PERIOD_FRAMES: usize = 180;

/// The smallest near-clip distance a probe's capture cameras may use, in metres —
/// `LLReflectionMap::getNearClip`'s `MINIMUM_NEAR_CLIP`.
const MIN_NEAR_CLIP: f32 = 0.1;

/// The [`LightProbe`] falloff of a **box**-volume local probe: the fraction of the
/// volume over which its influence tapers out toward the faces of the box. Small, so
/// a box probe's reflection fills the room it bounds (as the reference's box probes
/// do) and only blends out right at the boundary rather than fading across it.
const BOX_FALLOFF: f32 = 0.1;

/// The [`LightProbe`] falloff of a **sphere**-volume local probe. Bevy's influence
/// volume is always a cuboid, so a sphere probe is bound as the cube circumscribing
/// its sphere; a broad taper pulls the influence back in toward the sphere, so the
/// corners the cube adds contribute little.
const SPHERE_FALLOFF: f32 = 0.5;

/// The **gain** on a probe's image-based lighting (P33.3): how bright its
/// contribution is relative to the scene radiance it captured. `1.0` is the
/// calibrated value — a mirror then reflects the surroundings at exactly the radiance
/// the eye sees of them, and a diffuse surface's ambient is exactly the irradiance
/// they cast — which is also what the reference viewer does (`radscale` is 1 in
/// `LLReflectionMapManager::updateUniforms`, and a probe's radiance is never
/// rescaled). Anything else is a lie about the environment, so this is not a
/// look-tuning knob; `SL_VIEWER_PROBE_GAIN` exists only to make the miscalibration
/// visible in an A/B capture.
const PROBE_GAIN: f32 = 1.0;

/// How much of the sky-driven [`GlobalAmbientLight`] to keep once the reflection
/// probe is providing image-based ambient to both PBR objects and the terrain.
/// Default `0.0` drops it entirely (the probe is the single ambient source, so it
/// is not double-counted); overridable by `SL_VIEWER_PROBE_AMBIENT_SCALE`.
fn probe_ambient_scale() -> f32 {
    std::env::var("SL_VIEWER_PROBE_AMBIENT_SCALE")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(0.0)
}

/// Attenuate the sky-set [`GlobalAmbientLight`] each frame (after the sky system
/// re-sets it) so the reflection probe's image-based lighting is not stacked on top
/// of a second flat ambient term — the double-count that otherwise over-brightens
/// probe-lit PBR surfaces.
fn suppress_global_ambient(mut ambient: ResMut<GlobalAmbientLight>) {
    ambient.brightness *= probe_ambient_scale();
}

/// The gain to apply to the probes' image-based lighting, overridable at runtime by
/// `SL_VIEWER_PROBE_GAIN` (an A/B knob — the calibrated value is [`PROBE_GAIN`]).
fn probe_gain() -> f32 {
    std::env::var("SL_VIEWER_PROBE_GAIN")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(PROBE_GAIN)
}

/// The [`EnvironmentMapLight`] intensity that gives the probes a [`probe_gain`] gain
/// under this view's `exposure` — the whole of P33.3's calibration, in one line.
///
/// Bevy's image-based lighting is *photometric*: a probe's sampled cubemap is scaled
/// by `intensity` (nominally cd/m²) and the sum of a surface's light is then scaled by
/// the view's `exposure` on its way to the framebuffer. But the cubemap here is not
/// photometric — it is a **render of the scene** in whatever linear space the viewer's
/// materials write, already through their own `exposure`. Undoing the exposure the
/// image-based path re-applies is therefore what makes a probe reproduce, rather than
/// re-scale, the radiance it captured: `intensity * exposure == gain`.
///
/// This is what ties the probes to the exposure model instead of to a magic number
/// (P33.1 shipped a hand-tuned `1200`, which is `1 / exposure` for Bevy's default
/// `Exposure::BLENDER` to within the eye's ability to tune it — the constant was
/// *measuring* this, and now it is derived). It also means the custom terrain / water
/// shaders, which sample the probe and scale by `intensity_for_view * view.exposure`
/// (they are not themselves exposed), land on the same gain — one calibration for both
/// material families.
fn probe_intensity(exposure: &Exposure) -> f32 {
    let scale = exposure.exposure();
    // A zero/denormal exposure would blow the intensity up to infinity; fall back to
    // Bevy's default rather than emit a NaN into the light probes.
    if scale > f32::EPSILON {
        probe_gain() / scale
    } else {
        probe_gain() / Exposure::default().exposure()
    }
}

/// Light the capture cameras with the default probe's environment map, so a rig
/// re-renders the scene as the eye *sees* it rather than as it would look with no
/// image-based lighting at all (P33.3).
///
/// A capture camera is an ordinary view, and Bevy lights a view's surfaces from that
/// view's own [`EnvironmentMapLight`] — which a capture camera has none of. Left
/// alone, then, every rig renders a world with no ambient whatsoever: the sky-set
/// `GlobalAmbientLight` is dropped ([`suppress_global_ambient`]) precisely because the
/// probe replaces it, so a prim's shadowed side comes out black and the terrain shader
/// falls back to its flat no-probe fill. That darker world is what the cubemap would
/// then hold, and what a mirror would show — visibly *not* the world beside it.
///
/// Sharing the main view's already-filtered maps (rather than giving each capture
/// camera a [`GeneratedEnvironmentMapLight`] of its own, which would set a whole
/// filter chain running per camera) costs nothing and makes the capture see the same
/// lighting the eye does. It is a feedback loop by construction — this frame's cube is
/// lit by the last one's — which is exactly how the reference viewer accumulates
/// bounced light across probe updates, and it converges rather than runs away because
/// each bounce is attenuated by the surfaces' albedo.
fn light_capture_cameras(
    mut commands: Commands,
    view: Query<&EnvironmentMapLight, With<FlyCamera>>,
    cameras: Query<(Entity, Option<&EnvironmentMapLight>), With<ProbeCaptureCamera>>,
) {
    let Ok(environment) = view.single() else {
        return;
    };
    for (entity, current) in &cameras {
        // Only write when it would actually change — the handles are stable for the
        // process's lifetime, so after the first frame this is a pure read.
        let stale = current.is_none_or(|current| {
            current.diffuse_map != environment.diffuse_map
                || current.specular_map != environment.specular_map
                || (current.intensity - environment.intensity).abs() > f32::EPSILON
        });
        if stale {
            commands.entity(entity).insert(environment.clone());
        }
    }
}

/// Keep every probe's intensity at the value [`probe_intensity`] calibrates, whatever
/// the view's exposure currently is.
///
/// Two reasons this is a system and not a one-off at insert time. Bevy's
/// [`GeneratedEnvironmentMapLight`] filter derives an [`EnvironmentMapLight`] from the
/// component **once** (its query is `Without<EnvironmentMapLight>`) and never refreshes
/// the derived intensity, so a later exposure change would leave every probe stale; and
/// the local probes' holders are spawned as probes enter the budget, long after startup.
/// Only entities whose intensity actually differs are touched, so a settled scene does
/// no change-detection churn.
fn calibrate_probe_intensity(
    camera: Query<&Exposure, With<FlyCamera>>,
    mut probes: Query<(
        &mut GeneratedEnvironmentMapLight,
        Option<&mut EnvironmentMapLight>,
    )>,
) {
    let Ok(exposure) = camera.single() else {
        return;
    };
    let intensity = probe_intensity(exposure);
    for (mut generated, filtered) in &mut probes {
        if (generated.intensity - intensity).abs() > f32::EPSILON {
            generated.intensity = intensity;
        }
        if let Some(mut filtered) = filtered
            && (filtered.intensity - intensity).abs() > f32::EPSILON
        {
            filtered.intensity = intensity;
        }
    }
}

/// A component on each capture camera marking it as one face of one probe's cubemap
/// (which rig it belongs to and which face it renders), so the capture driver can
/// pose and toggle the six cameras of the rig whose turn it is.
#[derive(Component, Debug, Clone, Copy)]
struct ProbeCaptureCamera {
    /// The [`CaptureRig`] this camera belongs to, indexed as in
    /// [`ProbeRigs::rigs`] — `0` is the default (global) probe, `1..=`
    /// [`MAX_LOCAL_PROBES`] the local pool.
    rig: usize,
    /// The cube face (array layer, `0..6`) this camera renders — indexed the same
    /// as [`CUBE_MAP_FACES`], so the camera's look direction and the cube layer the
    /// copy writes agree.
    face: usize,
}

/// One probe's **capture rig**: everything needed to re-render the scene around a
/// point into an environment cubemap — the destination cube [`Image`] and the six
/// per-face colour targets the rig's six capture cameras draw into. The cameras
/// themselves are found through their [`ProbeCaptureCamera`] component (which names
/// the rig and face each belongs to), not held here.
///
/// Rig `0` is the default (global) probe; the rest are the pool
/// [`drive_local_probes`] hands to the nearest per-object probes. All are created
/// once, at startup, by [`setup_probe_rigs`] — a rig is *reassigned*, never rebuilt,
/// so no render-target churn happens as the camera moves through a scene.
struct CaptureRig {
    /// The cube [`Image`] (six `Rgba16Float` layers) the six face targets are
    /// copied into and that this probe's [`GeneratedEnvironmentMapLight`] filters.
    cube: Handle<Image>,
    /// The six face colour targets, in cube-layer order — kept so the render-world
    /// blit ([`copy_probe_faces`]) can name them by asset id.
    faces: [Handle<Image>; FACE_COUNT],
}

/// The local probe a pool rig is currently assigned to (P33.2).
struct LocalBinding {
    /// The probe **prim**'s object entity — the one carrying the
    /// [`ObjectReflectionProbe`], whose world transform poses both the capture
    /// cameras and the influence volume.
    object: Entity,
    /// The [`LightProbe`] holder entity spawned as a child of `object`: it carries
    /// the influence volume (its local scale) and the rig's
    /// [`GeneratedEnvironmentMapLight`].
    holder: Entity,
    /// The probe parameters last applied to the holder, so an unchanged probe costs
    /// no per-frame component churn (the same trick the P25.2 light budget plays).
    applied: ObjectReflectionProbe,
    /// The prim's world rotation the holder's [`sample_rotation`] correction was
    /// last derived from, so a prim at rest likewise costs no churn — and a prim
    /// that turns has the correction re-derived.
    sample_rotation: Quat,
}

/// Every capture rig in the scene: the default (global) probe's, plus the pool of
/// [`MAX_LOCAL_PROBES`] rigs the nearest per-object probes are assigned.
#[derive(Resource)]
struct ProbeRigs {
    /// The rigs, index `0` the default probe and `1..=`[`MAX_LOCAL_PROBES`] the
    /// local pool. Created once by [`setup_probe_rigs`].
    rigs: Vec<CaptureRig>,
    /// What each rig is currently bound to, indexed the same as
    /// [`rigs`](Self::rigs): `None` for the global probe (index `0`, which is bound
    /// to the view, not to an object) and for a free pool rig.
    bindings: Vec<Option<LocalBinding>>,
    /// Whether the global [`GeneratedEnvironmentMapLight`] has been installed on the
    /// main view yet (it is deferred until the fly-camera entity exists).
    installed: bool,
}

impl ProbeRigs {
    /// The pool rig currently assigned to `object`'s probe, if it holds one.
    fn rig_of(&self, object: Entity) -> Option<usize> {
        self.bindings
            .iter()
            .position(|binding| binding.as_ref().is_some_and(|bound| bound.object == object))
    }

    /// The lowest-indexed **free** pool rig, or `None` when the whole pool is spoken
    /// for. Rig `0` is the global probe and is never free.
    fn free_rig(&self) -> Option<usize> {
        self.bindings
            .iter()
            .enumerate()
            .skip(1)
            .find_map(|(index, binding)| binding.is_none().then_some(index))
    }
}

/// The amortized capture schedule: which rig is being re-rendered right now, and
/// which is next. Only ever **one cube face in the whole scene** is re-rendered per
/// frame, so the scene re-render cost is bounded no matter how many probes are live.
///
/// A rig's six faces are captured over six consecutive frames (a *burst*); the
/// schedule then idles ([`idle_frames`]) before starting the next rig's burst, sized
/// so that each live rig comes round again every [`CAPTURE_PERIOD_FRAMES`].
#[derive(Resource, Default)]
struct CaptureSchedule {
    /// The rig currently mid-burst and the next face of it to render, if any.
    active: Option<(usize, usize)>,
    /// Frames still to idle before the next burst begins.
    idle: usize,
    /// Rigs needing an out-of-turn capture — a pool rig just assigned to a new probe,
    /// whose cube still holds the *previous* tenant's surroundings. Drained ahead of
    /// the round-robin so a probe entering the budget shows its own environment
    /// within a few frames rather than after a full period.
    urgent: VecDeque<usize>,
    /// The round-robin cursor over the rig indices, for the routine refresh.
    cursor: usize,
}

/// One probe's face-target → cube-layer copy mapping, snapshotted for the render
/// world: the cube image and its six per-face source images, by asset id.
#[derive(Clone)]
struct ProbeCubeCopy {
    /// The destination cube image (its six array layers receive the faces).
    cube: AssetId<Image>,
    /// The six source face images, in cube-layer order.
    faces: [AssetId<Image>; FACE_COUNT],
}

/// The render-world work-list of probe cubes to reassemble each frame, extracted
/// from the main world. [`copy_probe_faces`] walks it and blits each probe's six
/// captured face textures into its cube's six array layers.
///
/// Only the **live** rigs are listed (the default probe plus the assigned pool
/// rigs), so a free rig's stale faces are not re-blitted every frame.
#[derive(Resource, Clone, Default, ExtractResource)]
struct ProbeCubeCopies {
    /// One entry per live probe: the default probe, plus each assigned local probe.
    copies: Vec<ProbeCubeCopy>,
}

/// The reflection-probe plugin (Phase 33): captures scene environment cubemaps and
/// drives them as image-based lighting, supplying the scene-render half of reflection
/// probes that Bevy's [`GeneratedEnvironmentMapLight`] filter and
/// [`EnvironmentMapLight`] consumer expect but never produce themselves.
///
/// It installs the **default** probe (P33.1) — one cubemap captured around the
/// viewpoint, bound globally to the main view, the reference viewer's fallback probe
/// used wherever no nearer local probe applies — and the **per-object local** probes
/// (P33.2): the nearest [`MAX_LOCAL_PROBES`] probe prims each get a rig of their own
/// and a [`LightProbe`] volume (box or sphere, from the prim) that overrides the
/// default inside it.
pub(crate) struct ReflectionProbePlugin;

impl Plugin for ReflectionProbePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProbeCubeCopies>()
            .init_resource::<CaptureSchedule>()
            .init_resource::<ProbeTestSphere>()
            .add_plugins(ExtractResourcePlugin::<ProbeCubeCopies>::default())
            .add_systems(Startup, setup_probe_rigs)
            .add_systems(
                Update,
                (
                    install_global_probe,
                    drive_local_probes,
                    calibrate_probe_intensity,
                    light_capture_cameras,
                    drive_probe_captures,
                    spawn_probe_test_sphere,
                )
                    .chain(),
            )
            // Runs after the sky system (Update) re-sets the ambient each frame.
            .add_systems(PostUpdate, suppress_global_ambient);

        // The face → cube-layer blit runs in the render world after the capture
        // cameras have drawn this frame's faces; the view's env-map filter reads the
        // reassembled cube on the following frame (a one-frame lag that is
        // imperceptible for a slowly re-captured environment).
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.add_systems(Render, copy_probe_faces.after(RenderSystems::Render));
    }
}

/// Build a single-face colour target: a square `Rgba16Float` render texture the
/// capture camera draws HDR scene radiance into, also readable as a copy source so
/// the render-world blit can lift it into the cube's matching layer.
fn create_face_image(images: &mut Assets<Image>) -> Handle<Image> {
    let mut image =
        Image::new_target_texture(CAPTURE_SIZE, CAPTURE_SIZE, TextureFormat::Rgba16Float, None);
    // `new_target_texture` sets TEXTURE_BINDING | COPY_DST | RENDER_ATTACHMENT; the
    // blit additionally reads the face as a copy source.
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    images.add(image)
}

/// Build the destination cube [`Image`]: six `Rgba16Float` array layers viewed as a
/// cubemap, a copy destination for the per-face blit and a storage / sampled source
/// for [`GeneratedEnvironmentMapLight`]'s realtime filter.
fn create_cube_image(images: &mut Assets<Image>) -> Handle<Image> {
    // A single `Rgba16Float` texel (four 16-bit floats = eight bytes) as the fill
    // pattern; `new_fill` replicates it across all six layers.
    let mut image = Image::new_fill(
        Extent3d {
            width: CAPTURE_SIZE,
            height: CAPTURE_SIZE,
            depth_or_array_layers: u32::try_from(FACE_COUNT).unwrap_or(6),
        },
        TextureDimension::D2,
        &[0u8; 8],
        TextureFormat::Rgba16Float,
        RenderAssetUsages::all(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::STORAGE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC;
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    images.add(image)
}

/// Spawn one cube-face capture camera: a 90°-FOV square HDR camera rendering the
/// world into `face_image`, initially inactive (the schedule toggles it on when its
/// rig's turn to re-capture comes round).
fn spawn_capture_camera(
    commands: &mut Commands,
    rig: usize,
    face: usize,
    face_image: Handle<Image>,
) {
    commands.spawn((
        Camera3d::default(),
        Camera {
            // Render before the main view (order 0). The env-map filter reads the
            // cube a frame later, so ordering among the capture cameras is
            // irrelevant; a single negative order keeps them all ahead of the view.
            order: -1,
            // Toggled on by `drive_probe_captures` only when this face is due for
            // re-capture, so the six-face scene re-render is amortized.
            is_active: false,
            ..default()
        },
        // A 2D colour target (not a window), so camera sizing resolves from the
        // image and no manual texture-view plumbing is needed.
        RenderTarget::Image(face_image.into()),
        Projection::Perspective(PerspectiveProjection {
            fov: FRAC_PI_2,
            aspect_ratio: 1.0,
            near: MIN_NEAR_CLIP,
            far: 4096.0,
            ..default()
        }),
        Transform::default(),
        // The face target is `Rgba16Float`, so the camera must render HDR and
        // single-sampled (the target is not multisampled), and must not tonemap —
        // the cube holds linear scene radiance for image-based lighting.
        Hdr,
        Msaa::Off,
        Tonemapping::None,
        ProbeCaptureCamera { rig, face },
    ));
}

/// Build one capture rig: its cube, its six face colour targets, and the six cameras
/// that render them (all initially idle).
fn create_capture_rig(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    rig: usize,
) -> CaptureRig {
    let cube = create_cube_image(images);
    let faces: [Handle<Image>; FACE_COUNT] = core::array::from_fn(|_| create_face_image(images));
    for face in 0..FACE_COUNT {
        // `.get(face)` (rather than `faces[face]`) to stay clear of the workspace
        // `indexing_slicing` lint; the loop index is always in range.
        let handle = faces.get(face).cloned().unwrap_or_default();
        spawn_capture_camera(commands, rig, face, handle);
    }
    CaptureRig { cube, faces }
}

/// Startup: create every capture rig — the default (global) probe's, plus the pool
/// of [`MAX_LOCAL_PROBES`] rigs the nearest per-object probes are handed. The rigs
/// exist for the process's lifetime; a probe entering or leaving the budget only
/// *rebinds* one (see [`drive_local_probes`]).
fn setup_probe_rigs(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let rigs: Vec<CaptureRig> = (0..RIG_COUNT)
        .map(|rig| create_capture_rig(&mut commands, &mut images, rig))
        .collect();
    commands.insert_resource(ProbeRigs {
        rigs,
        bindings: std::iter::repeat_with(|| None).take(RIG_COUNT).collect(),
        installed: false,
    });
    debug!(
        "reflection probes: {RIG_COUNT} capture rig(s) set up at {CAPTURE_SIZE}² per face \
         (1 default + {MAX_LOCAL_PROBES} local)"
    );
}

/// Install the default probe's [`GeneratedEnvironmentMapLight`] on the main view
/// once the fly-camera exists, so every PBR surface receives the captured
/// environment as image-based lighting. Runs each frame until it succeeds, then
/// idles (the flag guards against re-inserting).
fn install_global_probe(
    mut commands: Commands,
    mut probes: ResMut<ProbeRigs>,
    camera: Query<(Entity, &Exposure), With<FlyCamera>>,
) {
    if probes.installed {
        return;
    }
    let Ok((view, exposure)) = camera.single() else {
        return;
    };
    let Some(global) = probes.rigs.first() else {
        return;
    };
    commands.entity(view).insert(GeneratedEnvironmentMapLight {
        environment_map: global.cube.clone(),
        intensity: probe_intensity(exposure),
        // The cube is captured directly in Bevy world space, so it samples with no
        // extra reorientation.
        rotation: Quat::IDENTITY,
        affects_lightmapped_mesh_diffuse: true,
    });
    probes.installed = true;
    debug!("reflection probes: installed default environment map on the main view");
}

/// Rank the probe prims for the [`MAX_LOCAL_PROBES`] budget: nearest first, by the
/// reference viewer's measure (`LLReflectionMapManager::update`'s
/// `mDistance = |eye - origin| - radius`), so a big probe whose volume the camera is
/// about to enter outranks a small one the same distance away.
fn rank_local_probes(
    eye: Vec3,
    probes: &Query<(Entity, &ObjectReflectionProbe, &GlobalTransform)>,
) -> Vec<Entity> {
    let mut ranked: Vec<(Entity, f32)> = probes
        .iter()
        .map(|(entity, probe, transform)| {
            let distance = eye.distance(transform.translation()) - probe.radius();
            (entity, distance)
        })
        .collect();
    ranked.sort_unstable_by(|a, b| a.1.total_cmp(&b.1));
    ranked.truncate(MAX_LOCAL_PROBES);
    ranked
        .into_iter()
        .map(|(entity, _distance)| entity)
        .collect()
}

/// Spawn a pool rig's [`LightProbe`] holder as a child of the probe prim: the entity
/// that carries the influence volume (its local scale — see
/// [`ObjectReflectionProbe::volume_scale`]) and binds the rig's captured cube as an
/// [`EnvironmentMapLight`] over it. Parenting to the prim is what makes the volume
/// track the prim's position and rotation for free.
///
/// `world_rotation` is the **prim's** world rotation, and it is passed in to be
/// **undone** — see [`sample_rotation`].
fn spawn_probe_holder(
    commands: &mut Commands,
    object: Entity,
    cube: Handle<Image>,
    probe: &ObjectReflectionProbe,
    intensity: f32,
    world_rotation: Quat,
) -> Entity {
    commands
        .spawn((
            LightProbe {
                falloff: probe.falloff(),
            },
            GeneratedEnvironmentMapLight {
                environment_map: cube,
                intensity,
                rotation: sample_rotation(world_rotation),
                affects_lightmapped_mesh_diffuse: true,
            },
            Transform::from_scale(probe.volume_scale()),
            ChildOf(object),
        ))
        .id()
}

/// The [`GeneratedEnvironmentMapLight::rotation`] that makes a **local** probe
/// sample its cube in the space the cube was captured in — the inverse of the
/// holder's world rotation (R22i).
///
/// The subtlety, which cost a visibly wrong reflection: Bevy builds a probe's
/// sampling frame from the probe entity's **world transform**, not from its
/// `rotation` field alone —
///
/// ```text
/// // bevy_pbr/src/light_probe/environment_map.rs
/// fn get_world_from_light_matrix(&self, original_transform: &Affine3A) -> Affine3A {
///     *original_transform * Affine3A::from_quat(self.rotation)
/// }
/// ```
///
/// — and the shader transforms the reflection direction *into* that frame
/// (`light_from_world`) before sampling. But [`copy_probe_faces`] captures the cube
/// in **Bevy world space**. So any rotation the holder inherits rotates the
/// reflection.
///
/// It always inherits one. The holder is a child of the prim's object entity, and
/// every root object entity carries the Second Life → Bevy basis change in its world
/// rotation (`sl_to_bevy_object_rotation` = `sl_to_bevy_rotation() * the prim's own
/// rotation`). So with an identity `rotation` — as this was first written — every
/// local probe reflected the world turned 90° about X: a neighbour below the prim
/// appeared to one side, one behind appeared below. Undoing the holder's world
/// rotation here cancels the sampling frame back to world space while leaving the
/// [`Transform`] — and therefore the **influence volume** — still tracking the prim,
/// which is the whole reason the holder is parented to it.
///
/// The **default** probe needs no such thing: it hangs off the view, and Bevy takes
/// only the `rotation` field for a view environment map (`view_rotation`), never the
/// camera's transform.
fn sample_rotation(world_rotation: Quat) -> Quat {
    world_rotation.inverse()
}

/// Hand the nearest probe prims the pool of capture rigs (P33.2).
///
/// Ranks every [`ObjectReflectionProbe`] by distance ([`rank_local_probes`]), frees
/// the rigs of probes that fell out of the [`MAX_LOCAL_PROBES`] budget (or whose prim
/// despawned), and binds a free rig to each newcomer — spawning its [`LightProbe`]
/// holder and queueing it for an immediate re-capture, since the rig's cube still
/// holds the previous tenant's surroundings. A probe that keeps its rig only has its
/// holder touched when its params or its prim's scale actually changed, so a settled
/// scene does no per-frame ECS churn (the same discipline as the P25.2 light budget).
///
/// Finally it republishes the render-world blit work-list ([`ProbeCubeCopies`]) —
/// the default probe plus exactly the bound local probes.
fn drive_local_probes(
    mut commands: Commands,
    mut rigs: ResMut<ProbeRigs>,
    mut schedule: ResMut<CaptureSchedule>,
    mut copies: ResMut<ProbeCubeCopies>,
    camera: Query<(&GlobalTransform, &Exposure), With<FlyCamera>>,
    probes: Query<(Entity, &ObjectReflectionProbe, &GlobalTransform)>,
    mut last_bound: Local<usize>,
) {
    let Ok((view, exposure)) = camera.single() else {
        return;
    };
    let candidates = probes.iter().len();
    let selected = rank_local_probes(view.translation(), &probes);

    // Free the rigs of probes that dropped out of the budget or whose prim is gone.
    // Bevy's hierarchy already despawns a holder whose parent object despawned, so
    // `try_despawn` covers that race.
    for (index, binding) in rigs.bindings.iter_mut().enumerate().skip(1) {
        let stale = binding
            .as_ref()
            .is_some_and(|bound| !selected.contains(&bound.object));
        if stale && let Some(bound) = binding.take() {
            commands.entity(bound.holder).try_despawn();
            debug!("reflection probes: local probe released capture rig {index}");
        }
    }

    for object in selected {
        // The entity came straight from this frame's query, so the lookup cannot
        // miss; skip defensively rather than unwrap.
        let Ok((_, probe, global)) = probes.get(object) else {
            continue;
        };
        let world_rotation = global.rotation();
        match rigs.rig_of(object) {
            // Already bound: refresh the holder only when the probe actually changed
            // (a resized prim, or one switched between a box and a sphere volume) —
            // or when the prim **turned**, which re-aims the sampling frame the
            // cube must be read through (`sample_rotation`).
            Some(index) => {
                let Some(Some(bound)) = rigs.bindings.get_mut(index) else {
                    continue;
                };
                if bound.applied != *probe {
                    commands.entity(bound.holder).insert((
                        LightProbe {
                            falloff: probe.falloff(),
                        },
                        Transform::from_scale(probe.volume_scale()),
                    ));
                    bound.applied = *probe;
                }
                // A rotating probe prim (a spinning mirror) turns its holder with
                // it, so the correction is re-derived rather than set once at bind.
                // `abs_diff_eq` so a prim at rest does no per-frame churn.
                if !bound.sample_rotation.abs_diff_eq(world_rotation, 1.0e-5) {
                    bound.sample_rotation = world_rotation;
                    commands
                        .entity(bound.holder)
                        .entry::<GeneratedEnvironmentMapLight>()
                        .and_modify(move |mut light| {
                            light.rotation = sample_rotation(world_rotation);
                        });
                }
            }
            // A newcomer: bind it to a free rig, if the budget still has one.
            None => {
                let Some(index) = rigs.free_rig() else {
                    continue;
                };
                let Some(cube) = rigs.rigs.get(index).map(|rig| rig.cube.clone()) else {
                    continue;
                };
                let holder = spawn_probe_holder(
                    &mut commands,
                    object,
                    cube,
                    probe,
                    probe_intensity(exposure),
                    world_rotation,
                );
                if let Some(slot) = rigs.bindings.get_mut(index) {
                    *slot = Some(LocalBinding {
                        object,
                        holder,
                        applied: *probe,
                        sample_rotation: world_rotation,
                    });
                }
                // Its cube still holds the last probe's environment: re-capture now
                // rather than at this rig's next turn in the round-robin.
                schedule.urgent.push_back(index);
                debug!("reflection probes: local probe took capture rig {index}");
            }
        }
    }

    // Republish the blit work-list: the default probe, plus every bound local probe.
    copies.copies.clear();
    for (index, rig) in rigs.rigs.iter().enumerate() {
        let live = index == 0
            || rigs
                .bindings
                .get(index)
                .is_some_and(|binding| binding.is_some());
        if live {
            copies.copies.push(ProbeCubeCopy {
                cube: rig.cube.id(),
                faces: core::array::from_fn(|face| {
                    rig.faces.get(face).map(Handle::id).unwrap_or_default()
                }),
            });
        }
    }

    let bound = copies.copies.len().saturating_sub(1);
    if bound != *last_bound {
        debug!(
            "local reflection probes: {bound} of {candidates} probe prim(s) captured \
             (budget {MAX_LOCAL_PROBES})"
        );
        *last_bound = bound;
    }
}

/// The frames to idle between two capture bursts, given how many rigs are `live`
/// (the default probe plus the bound local probes). Sized so each live rig comes
/// round again every [`CAPTURE_PERIOD_FRAMES`]: with `live` rigs each taking
/// [`FACE_COUNT`] render frames plus this idle, one full cycle is
/// `live * (FACE_COUNT + idle)` frames. So the capture cost scales with the number of
/// *live* probes, and every probe refreshes on the same wall-clock cadence no matter
/// how many there are.
fn idle_frames(live: usize) -> usize {
    CAPTURE_PERIOD_FRAMES
        .checked_div(live.max(1))
        .unwrap_or(CAPTURE_PERIOD_FRAMES)
        .saturating_sub(FACE_COUNT)
}

/// Where a rig's capture cameras sit and how near they clip: the viewpoint (and the
/// default near clip) for the default probe; the probe prim's world origin (and the
/// probe's own near clip, which is how a probe excludes the prim or the furniture it
/// sits inside from its own reflection) for a bound local probe.
fn rig_capture_pose(
    rig: usize,
    rigs: &ProbeRigs,
    eye: Vec3,
    probes: &Query<(Entity, &ObjectReflectionProbe, &GlobalTransform)>,
) -> Option<(Vec3, f32)> {
    match rigs.bindings.get(rig) {
        Some(Some(bound)) => {
            let (_, probe, transform) = probes.get(bound.object).ok()?;
            Some((transform.translation(), probe.near_clip()))
        }
        // Rig 0 (the default probe) is bound to the view, not to an object; a free
        // pool rig has nothing to capture.
        _other if rig == 0 => Some((eye, MIN_NEAR_CLIP)),
        _other => None,
    }
}

/// Drive the amortized environment capture across every live rig.
///
/// At most **one** cube face in the whole scene is re-rendered per frame: a rig's six
/// faces are captured over six consecutive frames (a burst), then the schedule idles
/// ([`idle_frames`]) before moving on to the next live rig, so every probe is
/// refreshed every [`CAPTURE_PERIOD_FRAMES`] and the costly scene re-render (each
/// with its own shadow pass) never spikes a frame with more than one face. A rig just
/// handed to a new probe jumps the queue, so it does not show the previous probe's
/// surroundings for a whole period.
fn drive_probe_captures(
    rigs: Res<ProbeRigs>,
    mut schedule: ResMut<CaptureSchedule>,
    camera: Query<&GlobalTransform, With<FlyCamera>>,
    probes: Query<(Entity, &ObjectReflectionProbe, &GlobalTransform)>,
    mut cameras: Query<(
        &ProbeCaptureCamera,
        &mut Transform,
        &mut Camera,
        &mut Projection,
    )>,
) {
    let Ok(view) = camera.single() else {
        return;
    };
    let eye = view.translation();

    // The rigs worth capturing: the default probe and every bound local probe.
    let live: Vec<usize> = (0..rigs.rigs.len())
        .filter(|&rig| {
            rig == 0
                || rigs
                    .bindings
                    .get(rig)
                    .is_some_and(|binding| binding.is_some())
        })
        .collect();

    // Pick the frame's work: continue the running burst, idle, or start the next
    // rig's burst (a freshly bound rig first, else the round-robin).
    let burst = match schedule.active {
        Some(active) => Some(active),
        None if schedule.idle > 0 => {
            schedule.idle = schedule.idle.saturating_sub(1);
            None
        }
        None => {
            let urgent = loop {
                match schedule.urgent.pop_front() {
                    // A rig queued for an urgent re-capture may have been freed again
                    // before its turn came; drop those.
                    Some(rig) if live.contains(&rig) => break Some(rig),
                    Some(_freed) => continue,
                    None => break None,
                }
            };
            let next = urgent.or_else(|| {
                let cursor = schedule.cursor.checked_rem(live.len()).unwrap_or(0);
                schedule.cursor = cursor.saturating_add(1);
                live.get(cursor).copied()
            });
            next.map(|rig| (rig, 0))
        }
    };

    // Where the burst's rig captures from — `None` if it has nothing to capture (its
    // probe prim vanished this very frame), in which case no camera renders.
    let pose = burst.and_then(|(rig, _face)| rig_capture_pose(rig, &rigs, eye, &probes));
    let capturing = burst.zip(pose);

    // Only the one face being captured this frame renders; every other camera idles.
    // The components are touched only when something actually changes, so the idle
    // cameras cost no change-detection churn.
    for (capture, mut transform, mut camera, mut projection) in &mut cameras {
        let pose = capturing.and_then(|((rig, face), pose)| {
            (capture.rig == rig && capture.face == face).then_some(pose)
        });
        if let Some((origin, near)) = pose {
            if let Some(face) = CUBE_MAP_FACES.get(capture.face) {
                *transform = Transform::from_translation(origin).looking_to(face.target, face.up);
            }
            if let Projection::Perspective(perspective) = projection.as_mut() {
                perspective.near = near;
            }
        }
        let active = pose.is_some();
        if camera.is_active != active {
            camera.is_active = active;
        }
    }

    // Advance the burst: after the sixth face the rig is done, and the schedule idles
    // long enough that every live rig is refreshed once per capture period.
    schedule.active = match burst {
        Some((rig, face)) => {
            let next = face.saturating_add(1);
            if next < FACE_COUNT {
                Some((rig, next))
            } else {
                schedule.idle = idle_frames(live.len());
                None
            }
        }
        None => None,
    };
}

/// Whether the reflection-probe diagnostic mirror ball is enabled
/// (`SL_VIEWER_PROBE_TEST_SPHERE=1`). Off by default; a debug affordance to *see* the
/// captured environment, since ordinary Second Life / OpenSim content rarely carries
/// the metallic PBR materials a probe visibly reflects.
fn probe_test_sphere_enabled() -> bool {
    std::env::var("SL_VIEWER_PROBE_TEST_SPHERE")
        .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

/// Tracks whether the diagnostic mirror ball has been spawned yet (it is deferred
/// until the fly-camera entity exists, then spawned once).
#[derive(Resource, Default)]
struct ProbeTestSphere {
    /// Whether the ball has already been spawned.
    spawned: bool,
}

/// Spawn a perfectly-mirrored sphere parented to the main view (a "mirror ball") so
/// the captured environment cubemap is directly visible as its reflection — a
/// diagnostic for the whole capture → copy → filter → image-based-lighting chain,
/// enabled only by [`probe_test_sphere_enabled`]. A metallic, near-zero-roughness
/// `StandardMaterial` renders black without an environment map, so a lit ball
/// confirms the probe works (and its content confirms the orientation).
fn spawn_probe_test_sphere(
    mut commands: Commands,
    mut state: ResMut<ProbeTestSphere>,
    camera: Query<Entity, With<FlyCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if state.spawned || !probe_test_sphere_enabled() {
        return;
    }
    let Ok(view) = camera.single() else {
        return;
    };
    let mesh = meshes.add(Sphere::new(0.35));
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        metallic: 1.0,
        perceptual_roughness: 0.0,
        ..default()
    });
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        // A little right of, below, and ahead of the camera, so it stays framed as
        // the view moves (Bevy camera space: +X right, +Y up, −Z forward).
        Transform::from_xyz(0.55, -0.25, -1.6),
        ChildOf(view),
    ));
    state.spawned = true;
    debug!("reflection probes: spawned diagnostic mirror ball on the main view");
}

/// Render world: blit each probe's six captured face textures into its cube's six
/// array layers, so the view's [`GeneratedEnvironmentMapLight`] filter reads a
/// complete environment cubemap. Runs after the capture cameras have drawn, issuing
/// its own command buffer (it does not run beneath the render graph, so it cannot use
/// `RenderContext`).
fn copy_probe_faces(
    copies: Res<ProbeCubeCopies>,
    images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    if copies.copies.is_empty() {
        return;
    }
    let mut encoder = device.create_command_encoder(&default());
    let mut recorded = false;
    for copy in &copies.copies {
        let Some(cube) = images.get(copy.cube) else {
            continue;
        };
        for (index, face_id) in copy.faces.iter().enumerate() {
            let Some(face) = images.get(*face_id) else {
                continue;
            };
            let layer = u32::try_from(index).unwrap_or(0);
            encoder.copy_texture_to_texture(
                TexelCopyTextureInfo {
                    texture: &face.texture,
                    mip_level: 0,
                    origin: Origin3d::ZERO,
                    aspect: TextureAspect::All,
                },
                TexelCopyTextureInfo {
                    texture: &cube.texture,
                    mip_level: 0,
                    origin: Origin3d {
                        x: 0,
                        y: 0,
                        z: layer,
                    },
                    aspect: TextureAspect::All,
                },
                Extent3d {
                    width: CAPTURE_SIZE,
                    height: CAPTURE_SIZE,
                    depth_or_array_layers: 1,
                },
            );
            recorded = true;
        }
    }
    if recorded {
        queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use super::sample_rotation;
    use crate::coords::sl_to_bevy_rotation;
    use bevy::math::EulerRot;
    use bevy::prelude::Quat;

    /// A local probe must sample its cube in the space the cube was **captured**
    /// in — world space — however its prim is turned (R22i).
    ///
    /// The failure this pins is not subtle once seen and was invisible until
    /// someone looked at a mirror: Bevy builds the sampling frame from the probe
    /// entity's *world transform*, and every object entity carries the Second Life
    /// → Bevy basis change, so an identity `rotation` reflected the world rotated
    /// 90° about X — a neighbour below the prim appeared to one side, one behind
    /// appeared below.
    ///
    /// Asserting the composition Bevy actually performs
    /// (`world_from_light = world_transform * rotation`) resolves to identity is
    /// the whole claim, and it holds for any prim rotation rather than only the
    /// basis change.
    #[test]
    fn a_local_probe_samples_its_cube_in_world_space() {
        for world_rotation in [
            // The basis change alone: an unrotated prim.
            sl_to_bevy_rotation(),
            // The basis change with a prim rotation on top, which is what a real
            // probe prim carries (`sl_to_bevy_object_rotation`).
            sl_to_bevy_rotation().mul_quat(Quat::from_rotation_z(0.7)),
            // A prim turned every which way.
            Quat::from_euler(EulerRot::XYZ, 0.3, -1.1, 2.4),
            Quat::IDENTITY,
        ] {
            // Exactly Bevy's `get_world_from_light_matrix`.
            let world_from_light = world_rotation.mul_quat(sample_rotation(world_rotation));
            assert!(
                world_from_light.abs_diff_eq(Quat::IDENTITY, 1.0e-5),
                "a probe whose prim is rotated by {world_rotation:?} must still sample its \
                 world-space cube unrotated, but the sampling frame came out \
                 {world_from_light:?} — every reflection it casts is turned by that much"
            );
        }
    }
    use super::{
        BOX_FALLOFF, CAPTURE_PERIOD_FRAMES, FACE_COUNT, MIN_NEAR_CLIP, ObjectReflectionProbe,
        PROBE_GAIN, SPHERE_FALLOFF, idle_frames, probe_intensity, reflection_probe_from_object,
    };
    use bevy::camera::Exposure;
    use bevy::prelude::Vec3;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{Object, ReflectionProbe, ReflectionProbeFlags, Vector};

    /// A minimal plain prim object with no extra params — the fixture the probe
    /// tests decorate.
    fn bare_object() -> Object {
        use sl_client_bevy::{
            CircuitId, ObjectMotion, RegionHandle, RegionLocalObjectId, Rotation, Uuid,
        };
        const fn zero() -> Vector {
            Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }
        }
        Object {
            region_handle: RegionHandle(0),
            local_id: RegionLocalObjectId(1),
            circuit: CircuitId::new(1),
            full_id: Uuid::from_u128(1).into(),
            parent_id: RegionLocalObjectId(0),
            pcode: 9,
            state: 0,
            crc: 0,
            material: 0,
            click_action: 0,
            update_flags: 0,
            scale: Vector {
                x: 2.0,
                y: 4.0,
                z: 6.0,
            },
            motion: ObjectMotion {
                position: zero(),
                velocity: zero(),
                acceleration: zero(),
                rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                angular_velocity: zero(),
                collision_plane: None,
            },
            owner_id: Uuid::from_u128(0),
            sound: Uuid::from_u128(0),
            gain: 0.0,
            sound_flags: 0,
            sound_radius: 0.0,
            text: String::new(),
            text_color: [0; 4],
            name_value: String::new(),
            media_url: None,
            texture_entry: Vec::new(),
            texture_anim: Vec::new(),
            texture_animation: None,
            shape: sl_client_bevy::PrimShapeParams::default(),
            particle_system: Vec::new(),
            particles: None,
            data: Vec::new(),
            extra_params: Vec::new(),
            extra: sl_client_bevy::ObjectExtraParams::default(),
            properties: None,
            joint_type: 0,
            joint_pivot: zero(),
            joint_axis_or_anchor: zero(),
        }
    }

    /// An object with no reflection-probe block is not a probe.
    #[test]
    fn no_probe_block_is_none() {
        assert_eq!(reflection_probe_from_object(&bare_object()), None);
    }

    /// A prim carrying a reflection-probe block lifts into a component holding it
    /// and the prim's scale (for the influence volume).
    #[test]
    fn probe_block_becomes_a_component() {
        let mut object = bare_object();
        let data = ReflectionProbe {
            ambiance: 0.5,
            clip_distance: 3.0,
            flags: ReflectionProbeFlags::BOX_VOLUME,
        };
        object.extra.reflection_probe = Some(data);
        assert_eq!(
            reflection_probe_from_object(&object),
            Some(ObjectReflectionProbe {
                data,
                scale: [2.0, 4.0, 6.0],
            })
        );
    }

    /// Build a probe component on a prim of `scale` with the given flags.
    fn probe(scale: [f32; 3], flags: ReflectionProbeFlags) -> ObjectReflectionProbe {
        ObjectReflectionProbe {
            data: ReflectionProbe {
                ambiance: 0.0,
                clip_distance: 1.0,
                flags,
            },
            scale,
        }
    }

    /// Tolerance for the float comparisons below (the workspace denies strict float
    /// equality).
    const EPS: f32 = 1.0e-6;

    /// A **box**-volume probe's influence volume is the prim's own box: Bevy's unit
    /// cube scaled by the prim's metre scale, i.e. half-extents `scale * 0.5` — the
    /// reference viewer's `LLReflectionMap::getBox`. Its ranking radius is that box's
    /// half-diagonal.
    #[test]
    fn box_volume_is_the_prim_box() {
        let probe = probe([2.0, 4.0, 6.0], ReflectionProbeFlags::BOX_VOLUME);
        assert!(probe.is_box_volume());
        assert!(
            probe
                .volume_scale()
                .abs_diff_eq(Vec3::new(2.0, 4.0, 6.0), EPS)
        );
        assert!(probe.falloff().abs_diff_eq(Vec3::splat(BOX_FALLOFF), EPS));
        // |(1, 2, 3)| = sqrt(14).
        assert!((probe.radius() - 14.0_f32.sqrt()).abs() < EPS);
    }

    /// A **sphere**-volume probe (no box flag) takes its radius from the prim's *X*
    /// extent alone (`LLReflectionMap::update`), and — Bevy having only cuboid probe
    /// volumes — is bound as the cube circumscribing that sphere, softened by the
    /// broader sphere falloff.
    #[test]
    fn sphere_volume_uses_the_x_extent() {
        let probe = probe([2.0, 4.0, 6.0], ReflectionProbeFlags::empty());
        assert!(!probe.is_box_volume());
        assert!(probe.volume_scale().abs_diff_eq(Vec3::splat(2.0), EPS));
        assert!(
            probe
                .falloff()
                .abs_diff_eq(Vec3::splat(SPHERE_FALLOFF), EPS)
        );
        assert!((probe.radius() - 1.0).abs() < EPS);
    }

    /// The capture near clip is the probe's own clip distance, floored at the
    /// reference's `MINIMUM_NEAR_CLIP` — so the common "unset" zero does not make the
    /// capture cameras degenerate.
    #[test]
    fn near_clip_is_floored() {
        let mut zero_clip = probe([1.0, 1.0, 1.0], ReflectionProbeFlags::empty());
        zero_clip.data.clip_distance = 0.0;
        assert!((zero_clip.near_clip() - MIN_NEAR_CLIP).abs() < EPS);

        let mut far_clip = zero_clip;
        far_clip.data.clip_distance = 2.5;
        assert!((far_clip.near_clip() - 2.5).abs() < EPS);
    }

    /// The calibration itself (P33.3): whatever the view's exposure, a probe's
    /// intensity is the value that cancels it, so the image-based lighting comes out at
    /// the gain — `intensity * exposure == gain` — and the captured surroundings are
    /// reproduced rather than re-scaled. This is also the product the custom terrain /
    /// water shaders form when they sample the probe.
    #[test]
    fn intensity_cancels_the_view_exposure() {
        for ev100 in [
            Exposure::EV100_INDOOR,
            Exposure::EV100_OVERCAST,
            Exposure::EV100_SUNLIGHT,
            Exposure::default().ev100,
        ] {
            let exposure = Exposure { ev100 };
            let gain = probe_intensity(&exposure) * exposure.exposure();
            assert!(
                (gain - PROBE_GAIN).abs() < 1.0e-3,
                "ev100={ev100} gain={gain}"
            );
        }
    }

    /// A degenerate exposure (a zero or denormal scale — nothing sets one, but the
    /// component is public and a division by it would send every probe to infinity)
    /// falls back to Bevy's default rather than poisoning the light probes with a NaN.
    #[test]
    fn a_degenerate_exposure_falls_back() {
        // `exposure()` is `exp2(-ev100) / 1.2`, so a huge ev100 underflows it to zero.
        let degenerate = Exposure { ev100: 1000.0 };
        let intensity = probe_intensity(&degenerate);
        assert!(intensity.is_finite());
        assert!((intensity - probe_intensity(&Exposure::default())).abs() < 1.0e-3);
    }

    /// However many probes are live, each one's rig comes round again once per
    /// capture period: a rig's cycle is its six face frames plus the idle the
    /// schedule waits between bursts, and all live rigs take a turn within it.
    #[test]
    fn every_live_rig_refreshes_once_per_period() {
        for live in 1..=super::RIG_COUNT {
            let cycle = FACE_COUNT
                .saturating_add(idle_frames(live))
                .saturating_mul(live);
            // Integer division of the period among the rigs loses at most one frame
            // per rig, so the cycle lands just at or below the nominal period.
            assert!(cycle <= CAPTURE_PERIOD_FRAMES, "live={live} cycle={cycle}");
            assert!(
                cycle > CAPTURE_PERIOD_FRAMES.saturating_sub(live),
                "live={live} cycle={cycle}"
            );
        }
    }
}
