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
//! [`ReflectionProbe`](sl_client_bevy::ReflectionProbe) on `Object::extra.reflection_probe` (the two packed floats —
//! ambiance and clip distance — plus the flag byte: box-vs-sphere influence
//! volume, dynamic-object capture, and mirror). `reflection_probe_from_object`
//! lifts a present block onto an `ObjectReflectionProbe` component that
//! `apply_object` attaches to (or clears from) each object entity as its updates
//! arrive, exactly the way [`apply_flexi`](crate::flexi) /
//! [`apply_light`](sl_viewer_world_objects::objects) / [`apply_particles`](sl_viewer_world_objects::objects) do — a
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
//! (`CaptureRig`) per probe: six 90° cameras (one per cube face) that render the
//! scene into six `Rgba16Float` colour targets, which a render-world blit
//! (`copy_probe_faces`) copies into the six layers of a cube [`Image`], which a
//! [`GeneratedEnvironmentMapLight`] filters (irradiance + roughness-mipped radiance)
//! into the diffuse / specular maps the PBR shader samples. Six separate colour
//! targets plus a copy (rather than rendering straight into the cube's layers) keeps
//! camera sizing on Bevy's ordinary image-target path — a cube-layer render target
//! would need render-world manual texture views that the main-world camera-sizing
//! pass cannot resolve.
//!
//! Rig 0 is the **default probe**, captured around the viewpoint and bound globally
//! to the main view. Rigs `1..=``MAX_LOCAL_PROBES` are a **pool** handed to the
//! nearest local probes (`drive_local_probes`) — the budget local lights (P25.2)
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
//! `radius = scale.x * 0.5` sphere — softened by a `SPHERE_FALLOFF` taper, since
//! Bevy's light-probe volume is always a cuboid. Bevy then picks the nearest
//! applicable probe per fragment, falling back to the view's default probe outside
//! every volume, exactly the layering the reference shader does.
//!
//! **Capture cadence.** The capture is amortized (`CaptureSchedule`) and tiered,
//! mirroring the reference viewer's per-probe cadences rather than one flat period:
//! only one cube face anywhere in the scene is re-rendered per frame, in six-frame
//! bursts. The local probes run a continuous **oldest-first, distance-weighted**
//! round-robin (the reference's `age - mDistance * 0.1` priority), so a nearer /
//! staler probe refreshes first; the **default (ambient) probe** is environment-
//! only and refreshes only every `DEFAULT_PROBE_PERIOD_SECS`
//! (`RenderDefaultProbeUpdatePeriod`). Captures are **shadow-free** — the capture
//! cameras render the reflection-probe layers only, so the shadow-casting sun
//! builds no cascades for them (see [`crate::probe_layers`]). A freshly assigned
//! rig jumps the queue (`CaptureSchedule::urgent`) so a probe entering the budget
//! shows its own surroundings almost immediately instead of the previous tenant's.
//!
//! **Consistent image-based lighting.** Bevy applies the view environment map only
//! to `StandardMaterial` (prims, meshes, avatars). The viewer's custom sky / terrain
//! / water materials do not sample it, so — to avoid double-counting a flat ambient
//! on top of the probe's diffuse contribution — the sky scales the
//! `GlobalAmbientLight` it writes by `probe_ambient_scale` (`0.0`: dropped
//! entirely), and the terrain and water shaders sample the probe
//! themselves (terrain reads its diffuse irradiance for ambient; water reflects the
//! specular cube). Sky stays the source and is not itself lit by the probe.
//!
//! **Brightness calibration (P33.3).** A probe is calibrated when it *reproduces* the
//! surroundings it captured rather than re-scaling them: a mirror shows the world at
//! the radiance the eye sees it, and a diffuse surface's ambient is the irradiance
//! that world casts. That is one equation — `probe_intensity` — and it needs no
//! tuning constant, only the view's `Exposure`; the reference viewer likewise never
//! rescales a probe's radiance (`radscale` is 1). `PROBE_GAIN` /
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
//! `light_capture_cameras`: a capture camera is lit by the probe too, or it would
//! render a world with no image-based lighting at all — darker than the one beside it.
//! See also `probe_ambient_scale` / `SL_VIEWER_PROBE_AMBIENT_SCALE`.
//!
//! Deliberately not modelled: a probe's **ambiance**, which in the reference scales
//! only the irradiance half of its contribution and blends the flat sky ambient back
//! in below 1 (`tapIrradianceMap`). Bevy's probe has a *single* `intensity` over both
//! halves, so the irradiance cannot be scaled without scaling the reflection with it —
//! and the reflection must stay at unit gain. Every probe therefore runs at the
//! reference's ambiance-1 point (its `RenderSkyAutoAdjustLegacy` default), where the
//! probe's irradiance *is* the ambient and no flat fill is added — which is exactly
//! what a `probe_ambient_scale` of `0.0` arranges. A probe's **dynamic** flag is implicitly
//! always on (a rig re-renders the whole scene, avatars included); its **mirror** flag
//! (the reference's separate screen-space "hero" probe) is out of scope.
//!
//! `apply_object`: sl_viewer_world_objects::objects
//! [`GeneratedEnvironmentMapLight`]: bevy::light::GeneratedEnvironmentMapLight

use crate::probe_layers::{default_probe_camera_render_layers, local_probe_camera_render_layers};
use crate::settings::ViewerSettings;
use crate::world_api::{BOX_FALLOFF, MIN_NEAR_CLIP, ObjectReflectionProbe, ViewerCamera};
use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::CUBE_MAP_FACES;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Exposure, Hdr, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    CommandEncoder, Extent3d, Origin3d, TexelCopyTextureInfo, TextureAspect, TextureDimension,
    TextureFormat, TextureUsages, TextureViewDescriptor, TextureViewDimension,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderSystems};
use sl_settings::SettingValue;
use std::collections::VecDeque;
use std::f32::consts::FRAC_PI_2;

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
/// `MAX_LOCAL_LIGHTS`(crate::lights) budget.
///
/// Each one costs a capture rig: six scene re-renders per refresh plus a cubemap
/// filter, so the budget is deliberately small. Bevy in any case binds at most
/// `MAX_VIEW_LIGHT_PROBES` (8) reflection probes per view, and the reference viewer
/// likewise keeps only a bounded set of probes resident
/// (`LLReflectionMapManager::mReflectionProbeCount`).
const MAX_LOCAL_PROBES: usize = 4;

/// The total number of capture rigs: the default (global) probe plus the local pool.
const RIG_COUNT: usize = MAX_LOCAL_PROBES.saturating_add(1);

/// How long (seconds) the **default (ambient) probe** may go between refreshes —
/// the reference viewer's `RenderDefaultProbeUpdatePeriod` (default `2 s`). The
/// default probe is environment-only, so its contents change only with the sky /
/// sun and this lazy cadence is imperceptible while it keeps the default probe off
/// the per-frame round-robin the local probes run.
const DEFAULT_PROBE_PERIOD_SECS: f32 = 2.0;

/// Weight on a local probe's camera distance when picking the next rig to refresh,
/// mirroring the reference's oldest-first priority (`age - mDistance * 0.1`): a
/// nearer probe is refreshed a touch more eagerly than a distant one of the same
/// age.
const PROBE_DISTANCE_WEIGHT: f32 = 0.1;

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
///
/// This is a **factor of the ambient the sky asks for**, not an attenuation applied
/// to whatever the resource happens to hold: `crate::sky`'s `drive_sky` folds it into
/// the absolute brightness it writes. A `PostUpdate` system that multiplied the
/// resource down instead would decay geometrically toward zero on every frame the
/// sky did not rewrite it — and, because the product is never the value the sky
/// asked for, would make the sky's own write-on-change guard miss every frame. It is
/// idempotent only at the default `0.0`, which is what hid that for as long as the
/// knob was left alone.
///
/// Public because the sky is not the only producer of a flat ambient: the login-free
/// gallery lights a stage without one, and it has to split the ambient with the
/// probes by the same rule or its scenes are lit differently from the world they
/// stand in for.
#[must_use]
pub fn probe_ambient_scale() -> f32 {
    std::env::var("SL_VIEWER_PROBE_AMBIENT_SCALE")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(0.0)
}

/// The gain to apply to the probes' image-based lighting, overridable at runtime by
/// `SL_VIEWER_PROBE_GAIN` (an A/B knob — the calibrated value is `PROBE_GAIN`).
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
/// `GlobalAmbientLight` is dropped ([`probe_ambient_scale`]) precisely because the
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
    view: Query<&EnvironmentMapLight, With<ViewerCamera>>,
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

/// Keep every probe's intensity at the value `probe_intensity` calibrates, whatever
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
    camera: Query<&Exposure, With<ViewerCamera>>,
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
    /// The `CaptureRig` this camera belongs to, indexed as in
    /// [`ProbeRigs::rigs`] — `0` is the default (global) probe, `1..=`
    /// `MAX_LOCAL_PROBES` the local pool.
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
/// `drive_local_probes` hands to the nearest per-object probes. All are created
/// once, at startup, by [`setup_probe_rigs`] — a rig is *reassigned*, never rebuilt,
/// so no render-target churn happens as the camera moves through a scene.
struct CaptureRig {
    /// The cube [`Image`] (six `Rgba16Float` layers) the six face targets are
    /// copied into and that this probe's [`GeneratedEnvironmentMapLight`] filters.
    cube: Handle<Image>,
    /// The six face colour targets, in cube-layer order — kept so the render-world
    /// blit (`copy_probe_faces`) can name them by asset id.
    faces: [Handle<Image>; FACE_COUNT],
}

/// The local probe a pool rig is currently assigned to (P33.2).
struct LocalBinding {
    /// The probe **prim**'s object entity — the one carrying the
    /// `ObjectReflectionProbe`, whose world transform poses both the capture
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
/// `MAX_LOCAL_PROBES` rigs the nearest per-object probes are assigned.
#[derive(Resource)]
struct ProbeRigs {
    /// The rigs, index `0` the default probe and `1..=``MAX_LOCAL_PROBES` the
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

/// The amortized capture schedule: which rig is being re-rendered right now.
/// Only ever **one cube face in the whole scene** is re-rendered per frame, so the
/// scene re-render cost is bounded no matter how many probes are live.
///
/// A rig's six faces are captured over six consecutive frames (a *burst*); the
/// schedule then immediately starts the next rig's burst — the local probes cycle
/// continuously (no idle), oldest-first with a small distance weight
/// ([`select_next_rig`]), while the default probe is held back to
/// `DEFAULT_PROBE_PERIOD_SECS`. This reproduces the reference viewer's tiered
/// per-probe cadences instead of one flat period.
#[derive(Resource, Default)]
struct CaptureSchedule {
    /// The rig currently mid-burst and the next face of it to render, if any.
    active: Option<(usize, usize)>,
    /// Wall-clock time (seconds since startup) each rig's last burst began, indexed
    /// by rig. Drives the oldest-first priority and the default-probe period; grown
    /// to the rig count on first use, seeded to "never captured".
    last_captured: Vec<f32>,
    /// Rigs needing an out-of-turn capture — a pool rig just assigned to a new probe,
    /// whose cube still holds the *previous* tenant's surroundings. Drained ahead of
    /// the round-robin so a probe entering the budget shows its own environment
    /// within a few frames rather than after a full period.
    urgent: VecDeque<usize>,
}

/// One probe's face-target → cube-layer copy mapping, snapshotted for the render
/// world: the cube image and its six per-face source images, by asset id.
#[derive(Clone)]
struct ProbeCubeCopy {
    /// The destination cube image (its six array layers receive the faces).
    cube: AssetId<Image>,
    /// The six source face images, in cube-layer order.
    faces: [AssetId<Image>; FACE_COUNT],
    /// The per-face texel size the faces and cube were created at — [`CAPTURE_SIZE`]
    /// for the P33 probes, the `hero` resolution for a mirror. The blit's
    /// copy extent must match the actual texture size, since the two probe families
    /// no longer share one resolution.
    size: u32,
}

/// The render-world work-list of probe cubes to reassemble each frame, extracted
/// from the main world. `copy_probe_faces` walks it and blits each probe's six
/// captured face textures into its cube's six array layers.
///
/// Only the **live** rigs are listed (the default probe plus the assigned pool
/// rigs), so a free rig's stale faces are not re-blitted every frame.
#[derive(Resource, Clone, Default, ExtractResource)]
struct ProbeCubeCopies {
    /// One entry per live probe: the default probe, plus each assigned local probe.
    copies: Vec<ProbeCubeCopy>,
    /// The one face captured **this frame** — `(cube image, face index)`, published
    /// by [`drive_probe_captures`] — or `None` on a frame with nothing to capture.
    /// `copy_probe_faces` blits only this face: the other five faces of every
    /// rig are unchanged since their own capture frames, and re-blitting them all
    /// every frame dirtied every live cube (re-running the env-map filter) per
    /// frame. (The capture *cadence* itself is deliberately untouched: slower
    /// probe cycling lets Bevy purge the idle pipelines/bind groups and each
    /// late capture then pays a recalculation spike.)
    captured: Option<(AssetId<Image>, usize)>,
}

/// The reflection-probe plugin (Phase 33): captures scene environment cubemaps and
/// drives them as image-based lighting, supplying the scene-render half of reflection
/// probes that Bevy's [`GeneratedEnvironmentMapLight`] filter and
/// [`EnvironmentMapLight`] consumer expect but never produce themselves.
///
/// It installs the **default** probe (P33.1) — one cubemap captured around the
/// viewpoint, bound globally to the main view, the reference viewer's fallback probe
/// used wherever no nearer local probe applies — and the **per-object local** probes
/// (P33.2): the nearest `MAX_LOCAL_PROBES` probe prims each get a rig of their own
/// and a [`LightProbe`] volume (box or sphere, from the prim) that overrides the
/// default inside it.
///
/// It also drives the **realtime mirrors** (hero probes, viewer-realtime-mirrors): the
/// nearest `MAX_HERO_PROBES` `MIRROR`-flagged prims get a high-resolution rig
/// re-rendered every frame — see the hero section at the bottom of this module.
#[derive(Debug)]
pub struct ReflectionProbePlugin;

impl Plugin for ReflectionProbePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProbeCubeCopies>()
            .init_resource::<CaptureSchedule>()
            .init_resource::<ProbeTestSphere>()
            .init_resource::<ProbeDynamicContent>()
            .init_resource::<MirrorSettings>()
            .init_resource::<HeroCubeCopies>()
            .init_resource::<HeroSchedule>()
            .add_plugins(ExtractResourcePlugin::<ProbeCubeCopies>::default())
            .add_plugins(ExtractResourcePlugin::<HeroCubeCopies>::default())
            .add_systems(
                Startup,
                (
                    setup_probe_rigs,
                    register_probe_settings,
                    // Register the mirror settings before the hero rigs are built, so
                    // their resolution reads the registered default (or a file
                    // override) rather than falling back.
                    (register_mirror_settings, setup_hero_rigs).chain(),
                ),
            )
            .add_systems(
                Update,
                (
                    install_global_probe,
                    // The mirror settings must be current before the local pool decides
                    // which probes to exclude (mirrors) and before the hero systems run.
                    sync_mirror_settings,
                    drive_local_probes,
                    calibrate_probe_intensity,
                    light_capture_cameras,
                    sync_probe_dynamic_setting,
                    update_probe_camera_layers,
                    drive_probe_captures,
                    spawn_probe_test_sphere,
                )
                    .chain(),
            )
            // The hero (mirror) path, ordered after the shared settings sync. Kept out
            // of the P33 chain above so a hero capture is independent of the amortized
            // local-pool schedule — a mirror re-renders its whole cube every frame,
            // never one face at a time.
            .add_systems(
                Update,
                (
                    drive_hero_probes,
                    drive_hero_captures,
                    light_hero_capture_cameras,
                )
                    .chain()
                    .after(sync_mirror_settings),
            );

        // The face → cube-layer blit runs in the render world after the capture
        // cameras have drawn this frame's faces; the view's env-map filter reads the
        // reassembled cube on the following frame (a one-frame lag that is
        // imperceptible for a slowly re-captured environment). The hero blit runs the
        // same way for the mirror cubes.
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.add_systems(
            Render,
            (copy_probe_faces, copy_hero_faces).after(RenderSystems::Render),
        );
    }
}

/// Build a single-face colour target: a square `size²` `Rgba16Float` render texture
/// the capture camera draws HDR scene radiance into, also readable as a copy source so
/// the render-world blit can lift it into the cube's matching layer.
///
/// `size` is [`CAPTURE_SIZE`] for the P33 default / local probes and the (higher)
/// `hero` resolution for a mirror's hero probe.
fn create_face_image(images: &mut Assets<Image>, size: u32) -> Handle<Image> {
    let mut image = Image::new_target_texture(size, size, TextureFormat::Rgba16Float, None);
    // `new_target_texture` sets TEXTURE_BINDING | COPY_DST | RENDER_ATTACHMENT; the
    // blit additionally reads the face as a copy source.
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    images.add(image)
}

/// Build the destination cube [`Image`]: six `size²` `Rgba16Float` array layers viewed
/// as a cubemap, a copy destination for the per-face blit and a storage / sampled
/// source for [`GeneratedEnvironmentMapLight`]'s realtime filter.
fn create_cube_image(images: &mut Assets<Image>, size: u32) -> Handle<Image> {
    // A single `Rgba16Float` texel (four 16-bit floats = eight bytes) as the fill
    // pattern; `new_fill` replicates it across all six layers.
    let mut image = Image::new_fill(
        Extent3d {
            width: size,
            height: size,
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

/// Whether **local** reflection probes capture **dynamic content** (avatars, …)
/// in addition to static world geometry — the runtime mirror of the reference's
/// `RenderReflectionProbe*` controls.
///
/// Default **on** during development, to measure the full performance cost of a
/// faithful implementation; it may default off later, since dynamic content in a
/// probe both costs a per-frame re-render and defeats change-detection (an
/// animating avatar dirties its probe every frame). The **default (ambient)**
/// probe never captures dynamic content regardless — it is environment-only.
#[derive(Resource, Clone, Copy, Debug)]
pub(crate) struct ProbeDynamicContent {
    /// Whether local probes render the [`PROBE_DYNAMIC_LAYER`](crate::probe_layers)
    /// (avatars, …).
    pub(crate) include: bool,
}

impl Default for ProbeDynamicContent {
    /// Include dynamic content by default (see the type docs).
    fn default() -> Self {
        Self { include: true }
    }
}

/// The persistent settings key toggling dynamic-content capture in local probes
/// (`ProbeDynamicContent`). Grouped under `[render]` in the settings file.
pub const PROBE_DYNAMIC_SETTING: &str = "render_reflection_probe_dynamic_content";

/// Register the reflection-probe settings' declared defaults (startup). Guarded on
/// [`ViewerSettings`] existing, so the gallery / headless test apps that run the
/// probe plugin without a settings store are unaffected.
fn register_probe_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    settings.register_in(
        &["render"],
        PROBE_DYNAMIC_SETTING,
        SettingValue::Bool(ProbeDynamicContent::default().include),
        "Capture dynamic content (avatars) in local reflection probes. Costlier, and \
         it defeats probe change-detection (an animating avatar dirties its probe \
         every frame); the environment-only default probe is unaffected either way.",
    );
}

/// Mirror the persistent [`PROBE_DYNAMIC_SETTING`] into the `ProbeDynamicContent`
/// resource each frame (a no-op once they agree), so an edit in the settings file /
/// a bound preferences control takes effect without a restart.
fn sync_probe_dynamic_setting(
    settings: Option<Res<ViewerSettings>>,
    mut dynamic: ResMut<ProbeDynamicContent>,
) {
    let Some(settings) = settings else {
        return;
    };
    if let Ok(include) = settings.store().get_bool(PROBE_DYNAMIC_SETTING)
        && dynamic.include != include
    {
        dynamic.include = include;
    }
}

/// The render layers a capture camera for `rig` uses: the default probe (rig `0`)
/// captures the environment only; every local probe also captures static world
/// geometry, and dynamic content when `include_dynamic`.
fn capture_camera_render_layers(rig: usize, include_dynamic: bool) -> RenderLayers {
    if rig == 0 {
        default_probe_camera_render_layers()
    } else {
        local_probe_camera_render_layers(include_dynamic)
    }
}

/// Reconcile the local probe capture cameras' render layers with the current
/// `ProbeDynamicContent` setting whenever it changes (and once on startup). The
/// default probe's cameras (rig `0`) are always environment-only, so this only
/// flips the [`PROBE_DYNAMIC_LAYER`](crate::probe_layers) bit on the pool rigs.
fn update_probe_camera_layers(
    setting: Res<ProbeDynamicContent>,
    mut cameras: Query<(&ProbeCaptureCamera, &mut RenderLayers)>,
) {
    if !setting.is_changed() {
        return;
    }
    for (capture, mut layers) in &mut cameras {
        let desired = capture_camera_render_layers(capture.rig, setting.include);
        if *layers != desired {
            *layers = desired;
        }
    }
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
        // Render on the reflection-probe layers, never the main layer — so the
        // shadow-casting sun (on the main layer) builds no shadow cascades for
        // these cameras (viewer-perf-pipeline-specialization-stalls). The default
        // probe (rig 0) captures the environment only; local probes also capture
        // world geometry (and dynamic content, reconciled by
        // [`update_probe_camera_layers`] from `ProbeDynamicContent`). The
        // shadow-free mirror sun ([`SceneSunMirror`](crate::sky)) lights them.
        capture_camera_render_layers(rig, true),
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
    let cube = create_cube_image(images, CAPTURE_SIZE);
    let faces: [Handle<Image>; FACE_COUNT] =
        core::array::from_fn(|_| create_face_image(images, CAPTURE_SIZE));
    for face in 0..FACE_COUNT {
        // `.get(face)` (rather than `faces[face]`) to stay clear of the workspace
        // `indexing_slicing` lint; the loop index is always in range.
        let handle = faces.get(face).cloned().unwrap_or_default();
        spawn_capture_camera(commands, rig, face, handle);
    }
    CaptureRig { cube, faces }
}

/// Startup: create every capture rig — the default (global) probe's, plus the pool
/// of `MAX_LOCAL_PROBES` rigs the nearest per-object probes are handed. The rigs
/// exist for the process's lifetime; a probe entering or leaving the budget only
/// *rebinds* one (see `drive_local_probes`).
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
    camera: Query<(Entity, &Exposure), With<ViewerCamera>>,
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

/// Rank the probe prims for the `MAX_LOCAL_PROBES` budget: nearest first, by the
/// reference viewer's measure (`LLReflectionMapManager::update`'s
/// `mDistance = |eye - origin| - radius`), so a big probe whose volume the camera is
/// about to enter outranks a small one the same distance away.
fn rank_local_probes(
    eye: Vec3,
    probes: &Query<(Entity, &ObjectReflectionProbe, &GlobalTransform)>,
    exclude_mirrors: bool,
) -> Vec<Entity> {
    let mut ranked: Vec<(Entity, f32)> = probes
        .iter()
        // A mirror-flagged prim is claimed by the hero path when mirrors are on, so it
        // must not also take an amortized local rig (two coincident probe volumes over
        // the same surface — wasted capture, and Bevy would pick between them).
        .filter(|(_, probe, _)| !(exclude_mirrors && probe.is_mirror()))
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
/// sampling frame from the probe entity's **world rotation**, not from its
/// `rotation` field alone —
///
/// ```text
/// // bevy_pbr/src/light_probe/environment_map.rs
/// fn get_sample_rotation(&self, world_rotation: Quat) -> Quat {
///     (world_rotation * self.rotation).inverse()
/// }
/// ```
///
/// — and the shader rotates the world-space reflection direction by that
/// quaternion before sampling. But `copy_probe_faces` captures the cube in
/// **Bevy world space**. So any rotation the holder inherits rotates the
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
/// Only the **rotation** is in that composition, which is the point: the
/// holder's `Transform` (and so its scale) defines the influence volume and
/// nothing else. Stock Bevy 0.19 instead composed the `rotation` field into the
/// probe's whole affine, `*transform * Affine3A::from_quat(rotation)`, and used
/// the one matrix for both jobs — `R * S * R⁻¹`, which shears the volume of a
/// non-uniformly scaled probe off its prim's box and bends its reflected
/// directions anisotropically (worst on a mirror, whose volume is deliberately
/// flattened by [`hero_volume_scale`]). Our Bevy fork splits the two; see
/// `LightProbeComponent::get_sample_rotation` there.
///
/// The **default** probe needs no such thing: it hangs off the view, and Bevy takes
/// only the `rotation` field for a view environment map (`view_rotation`), never the
/// camera's transform.
fn sample_rotation(world_rotation: Quat) -> Quat {
    world_rotation.inverse()
}

/// Re-aim a bound holder's sampling frame after its prim turned (a spinning
/// mirror), writing [`sample_rotation`] to **both** of the components that carry
/// it.
///
/// The second write is the one that reaches the shader, and is easy to miss.
/// Bevy's [`GeneratedEnvironmentMapLight`] filter derives the
/// [`EnvironmentMapLight`] the light-probe pass actually samples through
/// (`gather_light_probes::<EnvironmentMapLight>`) **once** — its query is
/// `Without<EnvironmentMapLight>` — and never refreshes it, so re-aiming only the
/// source component leaves a turning prim reflecting the world at whatever angle
/// it happened to be bound at. [`calibrate_probe_intensity`] carries the same
/// both-components discipline for the intensity, and for the same reason.
fn reaim_sample_frame(commands: &mut Commands, holder: Entity, world_rotation: Quat) {
    let rotation = sample_rotation(world_rotation);
    let mut holder = commands.entity(holder);
    holder
        .entry::<GeneratedEnvironmentMapLight>()
        .and_modify(move |mut light| {
            light.rotation = rotation;
        });
    holder
        .entry::<EnvironmentMapLight>()
        .and_modify(move |mut light| {
            light.rotation = rotation;
        });
}

/// Hand the nearest probe prims the pool of capture rigs (P33.2).
///
/// Ranks every `ObjectReflectionProbe` by distance ([`rank_local_probes`]), frees
/// the rigs of probes that fell out of the `MAX_LOCAL_PROBES` budget (or whose prim
/// despawned), and binds a free rig to each newcomer — spawning its [`LightProbe`]
/// holder and queueing it for an immediate re-capture, since the rig's cube still
/// holds the previous tenant's surroundings. A probe that keeps its rig only has its
/// holder touched when its params or its prim's scale actually changed, so a settled
/// scene does no per-frame ECS churn (the same discipline as the P25.2 light budget).
///
/// Finally it republishes the render-world blit work-list ([`ProbeCubeCopies`]) —
/// the default probe plus exactly the bound local probes.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters; the mirror-exclusion setting is the one \
              added over the P33 original and does not group with the rest"
)]
fn drive_local_probes(
    mut commands: Commands,
    mut rigs: ResMut<ProbeRigs>,
    mut schedule: ResMut<CaptureSchedule>,
    mut copies: ResMut<ProbeCubeCopies>,
    mirrors: Res<MirrorSettings>,
    camera: Query<(&GlobalTransform, &Exposure), With<ViewerCamera>>,
    probes: Query<(Entity, &ObjectReflectionProbe, &GlobalTransform)>,
    mut last_bound: Local<usize>,
) {
    let Ok((view, exposure)) = camera.single() else {
        return;
    };
    let candidates = probes.iter().len();
    let selected = rank_local_probes(view.translation(), &probes, mirrors.enabled);

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
                    reaim_sample_frame(&mut commands, bound.holder, world_rotation);
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
                size: CAPTURE_SIZE,
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

/// Pick the next rig to (re)capture, mirroring the reference viewer's oldest-first,
/// distance-weighted priority (`age - mDistance * 0.1`): the live rig whose burst
/// began longest ago wins, minus a small penalty for camera distance so a nearer
/// probe of equal age refreshes first. The **default (ambient) probe** (rig `0`) is
/// only eligible once `DEFAULT_PROBE_PERIOD_SECS` has elapsed since its last
/// capture — it is environment-only and changes slowly, so it stays off the local
/// probes' continuous round-robin. Returns `None` when nothing is due (only the
/// default probe is live and it is still within its period).
fn select_next_rig(
    schedule: &CaptureSchedule,
    live: &[usize],
    rigs: &ProbeRigs,
    eye: Vec3,
    now: f32,
    probes: &Query<(Entity, &ObjectReflectionProbe, &GlobalTransform)>,
) -> Option<usize> {
    let candidates = live.iter().filter_map(|&rig| {
        let last = schedule
            .last_captured
            .get(rig)
            .copied()
            .unwrap_or(f32::NEG_INFINITY);
        let age = now - last;
        let distance = if rig == 0 {
            // The default probe is captured around the viewpoint — no camera
            // distance to weigh.
            0.0
        } else {
            // A local probe's distance is its prim's distance from the camera;
            // skip a probe whose prim vanished this frame.
            let (origin, _near) = rig_capture_pose(rig, rigs, eye, probes)?;
            eye.distance(origin)
        };
        Some((rig, age, distance))
    });
    pick_next_rig(candidates)
}

/// The pure oldest-first, distance-weighted selection: among `(rig, age,
/// distance)` candidates, the highest `age - distance * PROBE_DISTANCE_WEIGHT`
/// wins, with the default probe (rig `0`) eligible only once its `age` has passed
/// `DEFAULT_PROBE_PERIOD_SECS`. Split out from [`select_next_rig`] so the cadence
/// policy is unit-testable without an ECS world.
fn pick_next_rig(candidates: impl IntoIterator<Item = (usize, f32, f32)>) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (rig, age, distance) in candidates {
        // The default probe waits out its period; every local probe is always due.
        if rig == 0 && age < DEFAULT_PROBE_PERIOD_SECS {
            continue;
        }
        let priority = age - distance * PROBE_DISTANCE_WEIGHT;
        if best.is_none_or(|(_, best_priority)| priority > best_priority) {
            best = Some((rig, priority));
        }
    }
    best.map(|(rig, _priority)| rig)
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

/// Drive the tiered, amortized environment capture across every live rig.
///
/// At most **one** cube face in the whole scene is re-rendered per frame: a rig's six
/// faces are captured over six consecutive frames (a burst), then the schedule
/// immediately starts the next rig's burst. The next rig is chosen oldest-first with
/// a small distance weight ([`select_next_rig`]) — so the local probes cycle
/// continuously (near-real-time), while the default probe is held to
/// `DEFAULT_PROBE_PERIOD_SECS`. A rig just handed to a new probe jumps the queue
/// (`CaptureSchedule::urgent`), so it does not show the previous probe's
/// surroundings while it waits its turn. Captures render only the reflection-probe
/// layers, so no sun shadow cascade is built for their cameras (the periodic stall
/// this replaced — viewer-perf-pipeline-specialization-stalls).
fn drive_probe_captures(
    rigs: Res<ProbeRigs>,
    mut schedule: ResMut<CaptureSchedule>,
    mut copies: ResMut<ProbeCubeCopies>,
    time: Res<Time>,
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
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
    let now = time.elapsed_secs();

    // Seed / grow the per-rig capture timestamps to the current rig count.
    if schedule.last_captured.len() < rigs.rigs.len() {
        schedule
            .last_captured
            .resize(rigs.rigs.len(), f32::NEG_INFINITY);
    }

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

    // Pick the frame's work: continue the running burst, or start the next rig's
    // burst — a freshly bound (urgent) rig first, else the oldest-first pick.
    let burst = match schedule.active {
        Some(active) => Some(active),
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
            let next =
                urgent.or_else(|| select_next_rig(&schedule, &live, &rigs, eye, now, &probes));
            // Stamp the burst's start time so the priority does not re-pick it while
            // it is mid-burst and the default probe waits out its full period.
            if let Some(rig) = next
                && let Some(slot) = schedule.last_captured.get_mut(rig)
            {
                *slot = now;
            }
            next.map(|rig| (rig, 0))
        }
    };

    // Where the burst's rig captures from — `None` if it has nothing to capture (its
    // probe prim vanished this very frame), in which case no camera renders.
    let pose = burst.and_then(|(rig, _face)| rig_capture_pose(rig, &rigs, eye, &probes));
    let capturing = burst.zip(pose);

    // Publish the one face rendered this frame for the render-world blit, so
    // `copy_probe_faces` copies exactly it (and nothing on an idle frame).
    let captured = capturing
        .and_then(|((rig, face), _pose)| rigs.rigs.get(rig).map(|rig| (rig.cube.id(), face)));
    if copies.captured != captured {
        copies.captured = captured;
    }

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

    // Advance the burst: after the sixth face the rig is done and the next frame
    // starts the next rig's burst immediately (no idle) — the continuous
    // round-robin the local probes run.
    schedule.active = match burst {
        Some((rig, face)) => {
            let next = face.saturating_add(1);
            (next < FACE_COUNT).then_some((rig, next))
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
    camera: Query<Entity, With<ViewerCamera>>,
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
    // Only the face captured this frame is blitted (see [`ProbeCubeCopies::
    // captured`]); every other layer of every live cube already holds its own
    // capture and re-copying it would dirty the cube for nothing.
    let Some((cube, face)) = copies.captured else {
        return;
    };
    let Some(copy) = copies.copies.iter().find(|copy| copy.cube == cube) else {
        return;
    };
    let mut encoder = device.create_command_encoder(&default());
    if blit_one_face(&mut encoder, copy, face, &images) {
        queue.submit([encoder.finish()]);
    }
}

/// Blit each listed probe's six captured face textures into its cube's six array
/// layers, at that copy's own [`ProbeCubeCopy::size`]. Shared by the P33 probes
/// (`copy_probe_faces`) and the mirror hero probes ([`copy_hero_faces`]), which
/// keep separate work-lists but the identical per-cube copy.
///
/// Runs after the capture cameras have drawn, issuing its own command buffer (it does
/// not run beneath the render graph, so it cannot use `RenderContext`).
fn blit_cube_faces(
    copies: &[ProbeCubeCopy],
    images: &RenderAssets<GpuImage>,
    device: &RenderDevice,
    queue: &RenderQueue,
) {
    if copies.is_empty() {
        return;
    }
    let mut encoder = device.create_command_encoder(&default());
    let mut recorded = false;
    for copy in copies {
        for index in 0..copy.faces.len() {
            recorded |= blit_one_face(&mut encoder, copy, index, images);
        }
    }
    if recorded {
        queue.submit([encoder.finish()]);
    }
}

/// Record the copy of one captured face texture into its cube's matching array
/// layer, returning whether a copy was recorded (both textures resolved).
fn blit_one_face(
    encoder: &mut CommandEncoder,
    copy: &ProbeCubeCopy,
    index: usize,
    images: &RenderAssets<GpuImage>,
) -> bool {
    let Some(cube) = images.get(copy.cube) else {
        return false;
    };
    let Some(face) = copy.faces.get(index).and_then(|id| images.get(*id)) else {
        return false;
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
            width: copy.size,
            height: copy.size,
            depth_or_array_layers: 1,
        },
    );
    true
}

// ---------------------------------------------------------------------------
// Realtime mirrors — hero probes (viewer-realtime-mirrors)
// ---------------------------------------------------------------------------
//
// A **mirror** is a reflection-probe prim carrying the `MIRROR` flag
// ([`ObjectReflectionProbe::is_mirror`]). Where the P33 default / local probes are
// captured slowly and blurred by the roughness filter — right for image-based
// ambient, wrong for a bathroom or shop mirror — a mirror needs a **hero probe**:
// captured **sharp** (a much higher `RenderHeroProbeResolution` cube) and
// **live** (all six faces re-rendered every frame, `RenderHeroProbeUpdateRate`
// throttling it), including the **dynamic content** (avatars — so you see yourself)
// the local pool can be told to skip. It reuses the P33 `CaptureRig` /
// [`LocalBinding`] machinery — a hero probe is just a local reflection probe pinned
// to an every-frame cadence at high resolution — but keeps its own rigs, work-list
// and schedule so the two families never share a resolution or a budget. This is the
// reference viewer's `LLHeroProbeManager`, the dynamic cousin of
// `LLReflectionMapManager`.
//
// The reflection lands **on the glass** the same way every other reflection does:
// the hero rig's [`LightProbe`] volume sits at the mirror prim, so Bevy's per-fragment
// probe lookup finds the sharp hero cube for the mirror surface (a low-roughness PBR
// face), overriding the default probe there. To keep that from being fought over,
// mirror prims are **excluded from the P33 local pool** while mirrors are enabled
// ([`rank_local_probes`]'s `exclude_mirrors`), so a mirror is captured once, by the
// hero path, not twice.
//
// **Perf levers, all part of the feature** (a per-frame six-face render is expensive):
// the **instance cap** (`MAX_HERO_PROBES` — only the nearest mirror(s) get a rig),
// the **resolution** (`RenderHeroProbeResolution`, VRAM- and fill-bounded), and the
// **update rate** (`RenderHeroProbeUpdateRate` — every frame, or every Nth). A scene
// with no mirror in view pays nothing (the hero cameras sit inactive); toggling
// `RenderMirrors` off releases the rigs and lets the P33 pool reclaim the prims as
// ordinary (blurry) probes.
//
// **Rigid attachments move with the avatar in the mirror** — a bug the reference
// viewer has (its hero pass renders non-rigged attachments at a stale pose, so they
// float free of the avatar in the glass), and the mirror of one we had ourselves on
// the avatars themselves (pose_avatar_skeletons orphaning joint children). We avoid it
// **structurally**: the hero cameras render the *same live ECS entities* as the main
// view, at the same `GlobalTransform`s — there is no separate mirror-pose pass to fall
// behind. As long as the avatar posing (`drive_avatar_motion`) and the rigid-attachment
// re-placement (`pose_attachment_nodes`) have run before the render, which they have
// (both are `PostUpdate`, before extract), a rigid attachment is exactly where it is in
// the main view, so it tracks the avatar in the reflection too. Nothing here needs to
// do anything to get that right; it must only *not* introduce a second, stale posing
// path — so a hero capture must never pre-pose the scene.

/// The per-face resolution a hero probe's cube is captured at when the setting is
/// unset or the settings store is absent (the gallery / readback harnesses). Higher
/// than the P33 [`CAPTURE_SIZE`] so a mirror is sharp, but well under the reference's
/// 2048 default to keep the six-face-per-frame cost and the VRAM (six `size²`
/// `Rgba16Float` faces plus the cube, per active mirror) tractable.
const HERO_DEFAULT_RESOLUTION: u32 = 512;

/// The smallest hero-probe resolution the setting accepts (the P33 probe size — below
/// it a "mirror" is no sharper than an ordinary probe).
const HERO_MIN_RESOLUTION: u32 = 128;

/// The largest hero-probe resolution the setting accepts — the reference viewer's
/// `RenderHeroProbeResolution` default, and the VRAM ceiling (a 2048² cube plus six
/// face targets is ~400 MB of `Rgba16Float`).
const HERO_MAX_RESOLUTION: u32 = 2048;

/// The default `RenderHeroProbeUpdateRate`: re-render the mirror **every** frame, the
/// sharpest and most live setting. Raising it trades liveness for frame time.
const HERO_DEFAULT_UPDATE_RATE: u32 = 1;

/// How many mirrors are captured live at once — the **instance cap**, the nearest
/// mirror(s) winning. Each costs six full scene re-renders per update at the hero
/// resolution, so this is deliberately tiny; the reference likewise keeps a bounded
/// hero-probe set.
const MAX_HERO_PROBES: usize = 1;

/// The smallest full extent (metres) a hero probe's influence volume takes along each
/// axis. A mirror is usually a **flat** prim, whose own box volume is razor-thin — its
/// reflective face would sit right on the volume boundary, where Bevy's per-fragment
/// probe lookup may miss it. Flooring each axis gives the volume depth in front of the
/// glass so the reflection reliably lands on the surface.
const HERO_MIN_VOLUME_EXTENT: f32 = 1.0;

/// The persistent settings keys for the mirror feature, grouped under `[render]`, named
/// after the reference viewer's controls.
pub const RENDER_MIRRORS_SETTING: &str = "render_mirrors";
/// See [`RENDER_MIRRORS_SETTING`] — the hero-probe cube resolution.
pub const HERO_RESOLUTION_SETTING: &str = "render_hero_probe_resolution";
/// See [`RENDER_MIRRORS_SETTING`] — the hero-probe re-render cadence in frames.
pub const HERO_UPDATE_RATE_SETTING: &str = "render_hero_probe_update_rate";

/// The live mirror configuration, mirrored from the persistent `[render]` settings by
/// [`sync_mirror_settings`] so an edit in the settings file (or a bound preferences
/// control) takes effect without a restart — except the resolution, which sizes the
/// GPU targets and so is fixed at [`setup_hero_rigs`] (a restart applies a change).
#[derive(Resource, Clone, Copy, Debug)]
pub(crate) struct MirrorSettings {
    /// Whether realtime mirrors are enabled (`RenderMirrors`). Off releases the hero
    /// rigs and lets the P33 local pool reclaim mirror prims as ordinary probes.
    enabled: bool,
    /// How often a mirror re-renders, in frames (`RenderHeroProbeUpdateRate`, floored
    /// at 1): 1 = every frame, N = every Nth frame.
    update_rate: u32,
}

impl Default for MirrorSettings {
    /// Mirrors on, re-rendered every frame — the most faithful (and costliest)
    /// starting point, matching the P33 dynamic-content default's "measure the full
    /// cost" stance. Only a mirror prim actually in view incurs any of that cost.
    fn default() -> Self {
        Self {
            enabled: true,
            update_rate: HERO_DEFAULT_UPDATE_RATE,
        }
    }
}

/// Round a requested hero-probe resolution to a power of two within
/// `[HERO_MIN_RESOLUTION, HERO_MAX_RESOLUTION]`: [`GeneratedEnvironmentMapLight`]'s
/// filter requires a power-of-two cube, and the bounds cap the VRAM / fill cost.
fn normalize_hero_resolution(requested: u32) -> u32 {
    let clamped = requested.clamp(HERO_MIN_RESOLUTION, HERO_MAX_RESOLUTION);
    // `next_power_of_two` rounds up (and is a no-op on a power of two); a value just
    // under the max can round to just over it, so re-clamp.
    clamped
        .next_power_of_two()
        .clamp(HERO_MIN_RESOLUTION, HERO_MAX_RESOLUTION)
}

/// The influence-volume scale for a hero probe: the prim's own volume
/// ([`ObjectReflectionProbe::volume_scale`]) but floored per axis at
/// [`HERO_MIN_VOLUME_EXTENT`], so a flat mirror still has a volume that reaches its
/// reflective surface (see [`HERO_MIN_VOLUME_EXTENT`]).
fn hero_volume_scale(probe: &ObjectReflectionProbe) -> Vec3 {
    probe
        .volume_scale()
        .max(Vec3::splat(HERO_MIN_VOLUME_EXTENT))
}

/// A component on each hero capture camera marking which mirror rig and cube face it
/// renders — the hero counterpart of [`ProbeCaptureCamera`], kept distinct so the P33
/// capture / lighting systems never touch the hero cameras and vice versa.
#[derive(Component, Debug, Clone, Copy)]
struct HeroCaptureCamera {
    /// The hero rig this camera belongs to, indexed as in [`HeroProbeRigs::rigs`].
    rig: usize,
    /// The cube face (`0..6`) this camera renders, indexed as [`CUBE_MAP_FACES`].
    face: usize,
}

/// Every hero capture rig: a small pool of `MAX_HERO_PROBES` rigs handed to the
/// nearest mirror prims. Parallel to [`ProbeRigs`] but with no reserved global rig —
/// every hero rig is a pool rig bound to a mirror.
#[derive(Resource)]
struct HeroProbeRigs {
    /// The rigs, all pool rigs. Created once by [`setup_hero_rigs`].
    rigs: Vec<CaptureRig>,
    /// What each rig is bound to (`None` = free), indexed as [`rigs`](Self::rigs).
    bindings: Vec<Option<LocalBinding>>,
    /// The per-face resolution every rig's cube and faces were created at — fixed at
    /// setup (it sizes GPU targets), fed to the render-world blit.
    resolution: u32,
}

impl HeroProbeRigs {
    /// The rig currently bound to `object`'s mirror, if any.
    fn rig_of(&self, object: Entity) -> Option<usize> {
        self.bindings
            .iter()
            .position(|binding| binding.as_ref().is_some_and(|bound| bound.object == object))
    }

    /// The lowest-indexed **free** rig, or `None` when every rig is bound.
    fn free_rig(&self) -> Option<usize> {
        self.bindings.iter().position(Option::is_none)
    }
}

/// The render-world work-list of hero cubes to reassemble each frame, extracted from
/// the main world — the hero counterpart of [`ProbeCubeCopies`], kept separate so each
/// family owns its own rebuild. [`copy_hero_faces`] blits each mirror's six captured
/// faces into its cube.
#[derive(Resource, Clone, Default, ExtractResource)]
struct HeroCubeCopies {
    /// One entry per bound mirror.
    copies: Vec<ProbeCubeCopy>,
}

/// The hero-probe frame counter, driving the `RenderHeroProbeUpdateRate` cadence.
#[derive(Resource, Default)]
struct HeroSchedule {
    /// Frames elapsed since startup (wrapping), so a capture fires when
    /// `frame % update_rate == 0`.
    frame: u64,
}

/// Register the mirror feature's persistent `[render]` settings (startup). Guarded on
/// [`ViewerSettings`] existing, like [`register_probe_settings`], so the gallery /
/// headless harnesses are unaffected.
fn register_mirror_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    settings.register_in(
        &["render"],
        RENDER_MIRRORS_SETTING,
        SettingValue::Bool(MirrorSettings::default().enabled),
        "Enable realtime mirrors (hero probes): a mirror-flagged reflection-probe prim \
         reflects the scene — and you — sharp and live, re-rendered every frame. \
         Costlier than the P33 reflection probes, but only a mirror prim actually in \
         view pays for it.",
    );
    settings.register_in(
        &["render"],
        HERO_RESOLUTION_SETTING,
        SettingValue::U32(HERO_DEFAULT_RESOLUTION),
        "Per-face resolution of a mirror's hero-probe cubemap, in texels \
         (RenderHeroProbeResolution). Sharper and costlier when higher; rounded to a \
         power of two in [128, 2048]. Sizes GPU targets, so a change takes effect on \
         restart.",
    );
    settings.register_in(
        &["render"],
        HERO_UPDATE_RATE_SETTING,
        SettingValue::U32(HERO_DEFAULT_UPDATE_RATE),
        "How often a mirror re-renders, in frames (RenderHeroProbeUpdateRate): 1 = \
         every frame (most live), N = every Nth frame (cheaper, laggier). The main \
         performance lever for mirrors.",
    );
}

/// Mirror the persistent mirror settings into the [`MirrorSettings`] resource each
/// frame (a no-op once they agree). The resolution is deliberately not synced — it
/// sizes GPU targets fixed at [`setup_hero_rigs`].
fn sync_mirror_settings(
    settings: Option<Res<ViewerSettings>>,
    mut mirrors: ResMut<MirrorSettings>,
) {
    let Some(settings) = settings else {
        return;
    };
    if let Ok(enabled) = settings.store().get_bool(RENDER_MIRRORS_SETTING)
        && mirrors.enabled != enabled
    {
        mirrors.enabled = enabled;
    }
    if let Ok(rate) = settings.store().get_u32(HERO_UPDATE_RATE_SETTING) {
        let rate = rate.max(1);
        if mirrors.update_rate != rate {
            mirrors.update_rate = rate;
        }
    }
}

/// Spawn one hero cube-face capture camera: a 90°-FOV square HDR camera rendering the
/// full scene (environment + geometry + dynamic content — a mirror must show avatars)
/// into `face_image`, initially inactive ([`drive_hero_captures`] toggles it per the
/// update rate).
fn spawn_hero_camera(commands: &mut Commands, rig: usize, face: usize, face_image: Handle<Image>) {
    commands.spawn((
        Camera3d::default(),
        Camera {
            // Ahead of the main view; the blit reads the faces a frame later, so the
            // order among capture cameras is irrelevant.
            order: -1,
            is_active: false,
            ..default()
        },
        RenderTarget::Image(face_image.into()),
        Projection::Perspective(PerspectiveProjection {
            fov: FRAC_PI_2,
            aspect_ratio: 1.0,
            near: MIN_NEAR_CLIP,
            far: 4096.0,
            ..default()
        }),
        Transform::default(),
        // HDR, single-sampled, no tonemap: the cube holds linear scene radiance, like
        // the P33 capture cameras.
        Hdr,
        Msaa::Off,
        Tonemapping::None,
        // A mirror always captures dynamic content (you must see yourself), on the
        // probe layers only so the shadow sun builds no cascade for these cameras.
        local_probe_camera_render_layers(true),
        HeroCaptureCamera { rig, face },
    ));
}

/// Build one hero rig at `resolution`: its cube, its six face targets, and the six
/// cameras that render them (all initially idle).
fn create_hero_rig(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    rig: usize,
    resolution: u32,
) -> CaptureRig {
    let cube = create_cube_image(images, resolution);
    let faces: [Handle<Image>; FACE_COUNT] =
        core::array::from_fn(|_| create_face_image(images, resolution));
    for face in 0..FACE_COUNT {
        let handle = faces.get(face).cloned().unwrap_or_default();
        spawn_hero_camera(commands, rig, face, handle);
    }
    CaptureRig { cube, faces }
}

/// Startup: create the `MAX_HERO_PROBES` hero rigs at the configured
/// `RenderHeroProbeResolution` (defaulting when the settings store is absent). Like
/// the P33 rigs they exist for the process's lifetime; a mirror entering or leaving
/// the budget only *rebinds* one.
fn setup_hero_rigs(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    settings: Option<Res<ViewerSettings>>,
) {
    let resolution = settings
        .and_then(|settings| settings.store().get_u32(HERO_RESOLUTION_SETTING).ok())
        .map_or(HERO_DEFAULT_RESOLUTION, normalize_hero_resolution);
    let rigs: Vec<CaptureRig> = (0..MAX_HERO_PROBES)
        .map(|rig| create_hero_rig(&mut commands, &mut images, rig, resolution))
        .collect();
    commands.insert_resource(HeroProbeRigs {
        rigs,
        bindings: std::iter::repeat_with(|| None)
            .take(MAX_HERO_PROBES)
            .collect(),
        resolution,
    });
    debug!("hero probes: {MAX_HERO_PROBES} hero rig(s) set up at {resolution}² per face");
}

/// Rank the mirror prims for the `MAX_HERO_PROBES` budget: nearest first, by the same
/// `|eye - origin| - radius` measure the P33 local pool uses ([`rank_local_probes`]).
fn rank_hero_probes(
    eye: Vec3,
    probes: &Query<(Entity, &ObjectReflectionProbe, &GlobalTransform)>,
) -> Vec<Entity> {
    let mut ranked: Vec<(Entity, f32)> = probes
        .iter()
        .filter(|(_, probe, _)| probe.is_mirror())
        .map(|(entity, probe, transform)| {
            (
                entity,
                eye.distance(transform.translation()) - probe.radius(),
            )
        })
        .collect();
    ranked.sort_unstable_by(|a, b| a.1.total_cmp(&b.1));
    ranked.truncate(MAX_HERO_PROBES);
    ranked
        .into_iter()
        .map(|(entity, _distance)| entity)
        .collect()
}

/// Spawn a hero rig's [`LightProbe`] holder as a child of the mirror prim: the entity
/// carrying the (floored — [`hero_volume_scale`]) influence volume and binding the
/// rig's sharp cube as an [`EnvironmentMapLight`] over the glass. Like
/// [`spawn_probe_holder`] but with the hero volume, and a hard [`BOX_FALLOFF`] so the
/// reflection fills the volume rather than fading across it.
fn spawn_hero_holder(
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
                falloff: Vec3::splat(BOX_FALLOFF),
            },
            GeneratedEnvironmentMapLight {
                environment_map: cube,
                intensity,
                rotation: sample_rotation(world_rotation),
                affects_lightmapped_mesh_diffuse: true,
            },
            Transform::from_scale(hero_volume_scale(probe)),
            ChildOf(object),
        ))
        .id()
}

/// Hand the nearest mirror prims the hero rigs (the mirror counterpart of
/// `drive_local_probes`).
///
/// When `RenderMirrors` is off it releases every hero binding and clears the work-list
/// — the P33 local pool then reclaims the mirror prims as ordinary probes (they are no
/// longer excluded from [`rank_local_probes`]). When on, it ranks the mirrors nearest
/// first, frees rigs whose mirror fell out of the budget (or despawned), binds a free
/// rig to each newcomer (spawning its holder), refreshes a kept rig's holder only on a
/// real change, and republishes the render-world blit list.
fn drive_hero_probes(
    mut commands: Commands,
    settings: Res<MirrorSettings>,
    mut rigs: ResMut<HeroProbeRigs>,
    mut copies: ResMut<HeroCubeCopies>,
    camera: Query<(&GlobalTransform, &Exposure), With<ViewerCamera>>,
    probes: Query<(Entity, &ObjectReflectionProbe, &GlobalTransform)>,
    mut last_bound: Local<usize>,
) {
    let Ok((view, exposure)) = camera.single() else {
        return;
    };

    // Mirrors off: release everything and let the P33 pool take over.
    if !settings.enabled {
        for binding in &mut rigs.bindings {
            if let Some(bound) = binding.take() {
                commands.entity(bound.holder).try_despawn();
            }
        }
        if !copies.copies.is_empty() {
            copies.copies.clear();
        }
        if *last_bound != 0 {
            debug!("hero probes: RenderMirrors off — released every hero rig");
            *last_bound = 0;
        }
        return;
    }

    let selected = rank_hero_probes(view.translation(), &probes);

    // Free the rigs of mirrors that dropped out of the budget or whose prim is gone.
    for (index, binding) in rigs.bindings.iter_mut().enumerate() {
        let stale = binding
            .as_ref()
            .is_some_and(|bound| !selected.contains(&bound.object));
        if stale && let Some(bound) = binding.take() {
            commands.entity(bound.holder).try_despawn();
            debug!("hero probes: mirror released hero rig {index}");
        }
    }

    for object in selected {
        let Ok((_, probe, global)) = probes.get(object) else {
            continue;
        };
        let world_rotation = global.rotation();
        match rigs.rig_of(object) {
            // Already bound: refresh the holder only on a real change (a resized or
            // reshaped mirror), and re-derive the sampling correction if the prim
            // turned — a rest prim costs no churn.
            Some(index) => {
                let Some(Some(bound)) = rigs.bindings.get_mut(index) else {
                    continue;
                };
                if bound.applied != *probe {
                    commands.entity(bound.holder).insert((
                        LightProbe {
                            falloff: Vec3::splat(BOX_FALLOFF),
                        },
                        Transform::from_scale(hero_volume_scale(probe)),
                    ));
                    bound.applied = *probe;
                }
                if !bound.sample_rotation.abs_diff_eq(world_rotation, 1.0e-5) {
                    bound.sample_rotation = world_rotation;
                    reaim_sample_frame(&mut commands, bound.holder, world_rotation);
                }
            }
            // A newcomer: bind it to a free rig if the cap has one.
            None => {
                let Some(index) = rigs.free_rig() else {
                    continue;
                };
                let Some(cube) = rigs.rigs.get(index).map(|rig| rig.cube.clone()) else {
                    continue;
                };
                let holder = spawn_hero_holder(
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
                debug!("hero probes: mirror took hero rig {index}");
            }
        }
    }

    // Republish the blit list: exactly the bound mirrors, at the hero resolution.
    copies.copies.clear();
    for (index, rig) in rigs.rigs.iter().enumerate() {
        let live = rigs
            .bindings
            .get(index)
            .is_some_and(|binding| binding.is_some());
        if live {
            copies.copies.push(ProbeCubeCopy {
                cube: rig.cube.id(),
                faces: core::array::from_fn(|face| {
                    rig.faces.get(face).map(Handle::id).unwrap_or_default()
                }),
                size: rigs.resolution,
            });
        }
    }

    let bound = copies.copies.len();
    if bound != *last_bound {
        debug!("hero probes: {bound} mirror(s) captured live (cap {MAX_HERO_PROBES})");
        *last_bound = bound;
    }
}

/// Drive the live mirror capture: every [`MirrorSettings::update_rate`] frames, pose a
/// bound hero rig's six cameras at its mirror prim and activate them so the whole cube
/// re-renders **this** frame — the sharp, live reflection. Between those frames (and
/// when mirrors are off) the cameras idle, which is where the update rate buys its
/// frame time back.
fn drive_hero_captures(
    settings: Res<MirrorSettings>,
    rigs: Res<HeroProbeRigs>,
    mut schedule: ResMut<HeroSchedule>,
    probes: Query<(&ObjectReflectionProbe, &GlobalTransform)>,
    mut cameras: Query<(
        &HeroCaptureCamera,
        &mut Transform,
        &mut Camera,
        &mut Projection,
    )>,
) {
    schedule.frame = schedule.frame.wrapping_add(1);
    let due = settings.enabled
        && schedule
            .frame
            .is_multiple_of(u64::from(settings.update_rate.max(1)));

    for (capture, mut transform, mut camera, mut projection) in &mut cameras {
        // The mirror this rig is bound to, and where it is — `None` when the frame is
        // not due (or mirrors are off), when the rig is free, or when its prim is gone.
        let pose = if due {
            rigs.bindings
                .get(capture.rig)
                .and_then(|binding| binding.as_ref())
                .and_then(|bound| probes.get(bound.object).ok())
                .map(|(probe, global)| (global.translation(), probe.near_clip()))
        } else {
            None
        };
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
}

/// Light the hero capture cameras with the main view's environment map, so a mirror
/// re-renders the world as the eye sees it (image-based lighting included) rather than
/// unlit — the hero counterpart of `light_capture_cameras`.
fn light_hero_capture_cameras(
    mut commands: Commands,
    view: Query<&EnvironmentMapLight, With<ViewerCamera>>,
    cameras: Query<(Entity, Option<&EnvironmentMapLight>), With<HeroCaptureCamera>>,
) {
    let Ok(environment) = view.single() else {
        return;
    };
    for (entity, current) in &cameras {
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

/// Render world: blit each bound mirror's six captured faces into its hero cube — the
/// hero counterpart of `copy_probe_faces`, sharing [`blit_cube_faces`].
fn copy_hero_faces(
    copies: Res<HeroCubeCopies>,
    images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    blit_cube_faces(&copies.copies, &images, &device, &queue);
}

#[cfg(test)]
mod tests {
    use super::sample_rotation;
    use crate::coords::sl_to_bevy_rotation;
    use bevy::light::EnvironmentMapLight;
    use bevy::math::{Affine3A, EulerRot};
    use bevy::pbr::LightProbeComponent as _;
    use bevy::prelude::{Handle, Quat};

    /// A local probe must sample its cube in the space the cube was **captured**
    /// in — world space — however its prim is turned (R22i), and its influence
    /// volume must stay the prim's own box while it does.
    ///
    /// The failure this pins is not subtle once seen and was invisible until
    /// someone looked at a mirror: Bevy builds the sampling frame from the probe
    /// entity's *world rotation*, and every object entity carries the Second Life
    /// → Bevy basis change, so an identity `rotation` reflected the world rotated
    /// 90° about X — a neighbour below the prim appeared to one side, one behind
    /// appeared below.
    ///
    /// Both claims are put to **Bevy's own** composition functions rather than
    /// restated here, so this fails if a Bevy bump changes either rule (or drops
    /// our fork's split of them). `GeneratedEnvironmentMapLight` is filtered into
    /// an `EnvironmentMapLight` carrying the same `rotation`, which is the type
    /// those functions live on.
    ///
    /// The second assertion is the one the `Quat`-only version of this test
    /// structurally could not make: it feeds Bevy the **whole affine** it builds
    /// the influence volume from and pins the result to `R * S` — the prim's
    /// oriented box. Stock Bevy 0.19 folded the `rotation` field into that same
    /// affine, giving `R * S * R⁻¹`: a shear for any non-uniform `S`, which both
    /// moved the volume off the prim's box and bent every reflected direction.
    #[test]
    fn a_local_probe_samples_its_cube_in_world_space() {
        // A box probe on a 2 × 3 × 1 m prim: the anisotropy that made the shear
        // visible, and what `volume_scale` hands the holder.
        let volume_scale = Vec3::new(2.0, 3.0, 1.0);
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
            let probe = EnvironmentMapLight {
                diffuse_map: Handle::default(),
                specular_map: Handle::default(),
                intensity: 1.0,
                rotation: sample_rotation(world_rotation),
                affects_lightmapped_mesh_diffuse: true,
            };

            let sampling_frame = probe.get_sample_rotation(world_rotation);
            assert!(
                sampling_frame.abs_diff_eq(Quat::IDENTITY, 1.0e-5),
                "a probe whose prim is rotated by {world_rotation:?} must still sample its \
                 world-space cube unrotated, but the sampling frame came out \
                 {sampling_frame:?} — every reflection it casts is turned by that much"
            );

            // The holder's world transform: the object entity's rotation (the
            // prim's metre scale lives on the holder, as `volume_scale`
            // documents) times the holder's own `Transform::from_scale`.
            let prim_box = Affine3A::from_quat(world_rotation) * Affine3A::from_scale(volume_scale);
            let world_from_light = probe.get_world_from_light_matrix(&prim_box);
            assert!(
                world_from_light
                    .matrix3
                    .abs_diff_eq(prim_box.matrix3, 1.0e-5),
                "a probe's influence volume must stay its prim's oriented box, but the \
                 affine Bevy builds it from came out {:?} rather than {:?} for a prim \
                 rotated by {world_rotation:?} — the volume is sheared off the prim, and \
                 that scale lands in the sampling frame too",
                world_from_light.matrix3,
                prim_box.matrix3
            );
        }
    }
    use super::{
        BOX_FALLOFF, DEFAULT_PROBE_PERIOD_SECS, MIN_NEAR_CLIP, ObjectReflectionProbe, PROBE_GAIN,
        pick_next_rig, probe_intensity,
    };
    use crate::world_api::{SPHERE_FALLOFF, reflection_probe_from_object};
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

    /// The default (ambient) probe stays off the local probes' continuous
    /// round-robin until its period elapses: while it is younger than
    /// `DEFAULT_PROBE_PERIOD_SECS` it is never picked even when it is the oldest
    /// candidate, but once past the period it becomes eligible again.
    #[test]
    fn default_probe_waits_out_its_period() {
        // Default probe (rig 0) is the oldest but still within its period; a local
        // probe that is barely due wins instead.
        let within = DEFAULT_PROBE_PERIOD_SECS - 0.1;
        assert_eq!(
            pick_next_rig([(0, within, 0.0), (1, 0.05, 0.0)]),
            Some(1),
            "the default probe must not pre-empt a local probe within its period"
        );
        // Only the default probe is live and it is within its period: nothing is due.
        assert_eq!(pick_next_rig([(0, within, 0.0)]), None);
        // Past its period the default probe is eligible again.
        let past = DEFAULT_PROBE_PERIOD_SECS + 0.1;
        assert_eq!(pick_next_rig([(0, past, 0.0)]), Some(0));
    }

    /// Among local probes the oldest wins, but a nearer probe of equal age is
    /// preferred by the distance weight — the reference's `age - distance * 0.1`.
    #[test]
    fn local_probes_pick_oldest_then_nearest() {
        // Equal age: the nearer probe (smaller distance) has the higher priority.
        assert_eq!(pick_next_rig([(1, 1.0, 50.0), (2, 1.0, 5.0)]), Some(2));
        // A far older probe wins despite being farther: the distance weight is
        // small (0.1), so a 45 m gap costs ~4.5 s of age — a 9 s age lead beats it.
        assert_eq!(pick_next_rig([(1, 10.0, 50.0), (2, 1.0, 5.0)]), Some(1));
        // No candidates: nothing to capture.
        assert_eq!(pick_next_rig([]), None);
    }

    use super::{
        HERO_MAX_RESOLUTION, HERO_MIN_RESOLUTION, HERO_MIN_VOLUME_EXTENT, hero_volume_scale,
        normalize_hero_resolution,
    };

    /// The `MIRROR` flag is what routes a probe prim to the hero path.
    #[test]
    fn mirror_flag_is_detected() {
        let mut object = bare_object();
        object.extra.reflection_probe = Some(ReflectionProbe {
            ambiance: 0.0,
            clip_distance: 1.0,
            flags: ReflectionProbeFlags::MIRROR,
        });
        assert!(
            matches!(reflection_probe_from_object(&object), Some(probe) if probe.is_mirror()),
            "the MIRROR flag marks a hero probe"
        );

        // A plain (non-mirror) probe is not a hero probe.
        let mut plain = bare_object();
        plain.extra.reflection_probe = Some(ReflectionProbe {
            ambiance: 0.0,
            clip_distance: 1.0,
            flags: ReflectionProbeFlags::BOX_VOLUME,
        });
        assert!(
            matches!(reflection_probe_from_object(&plain), Some(probe) if !probe.is_mirror()),
            "a plain (non-mirror) probe is not a hero probe"
        );
    }

    /// The hero resolution is always a power of two within the accepted band — the
    /// env-map filter's requirement and the VRAM ceiling.
    #[test]
    fn hero_resolution_is_a_bounded_power_of_two() {
        // Already a power of two in range: unchanged.
        assert_eq!(normalize_hero_resolution(512), 512);
        // Rounded up to the next power of two.
        assert_eq!(normalize_hero_resolution(500), 512);
        assert_eq!(normalize_hero_resolution(1025), 2048);
        // Clamped to the band at both ends.
        assert_eq!(normalize_hero_resolution(1), HERO_MIN_RESOLUTION);
        assert_eq!(normalize_hero_resolution(100_000), HERO_MAX_RESOLUTION);
        // A value between max/2 and max still ends within the band (not above it).
        for requested in [1, 127, 200, 999, 2047, 2048, 5000] {
            let size = normalize_hero_resolution(requested);
            assert!(size.is_power_of_two(), "{size} must be a power of two");
            assert!((HERO_MIN_RESOLUTION..=HERO_MAX_RESOLUTION).contains(&size));
        }
    }

    /// A flat mirror's razor-thin box gets a volume floored to
    /// [`HERO_MIN_VOLUME_EXTENT`] on every axis, so the reflective face is inside the
    /// volume rather than on its boundary — while a large mirror keeps its own extent.
    #[test]
    fn hero_volume_is_floored_for_a_flat_mirror() {
        // A flat mirror: 2 m × 3 m panel, 2 cm thick (a box-volume probe).
        let flat = probe(
            [2.0, 3.0, 0.02],
            ReflectionProbeFlags::MIRROR.union(ReflectionProbeFlags::BOX_VOLUME),
        );
        let scale = hero_volume_scale(&flat);
        assert!(
            (scale.z - HERO_MIN_VOLUME_EXTENT).abs() < EPS,
            "the thin axis is floored to {HERO_MIN_VOLUME_EXTENT}, got {}",
            scale.z
        );
        // The already-large axes are untouched (box-volume mirror keeps its extent).
        assert!((scale.x - 2.0).abs() < EPS);
        assert!((scale.y - 3.0).abs() < EPS);

        // A large mirror is never shrunk by the floor.
        let big = probe(
            [4.0, 4.0, 4.0],
            ReflectionProbeFlags::MIRROR.union(ReflectionProbeFlags::BOX_VOLUME),
        );
        assert!(hero_volume_scale(&big).abs_diff_eq(Vec3::splat(4.0), EPS));
    }
}
