//! Object-entity components the world's ingest path attaches, and the state
//! the input / motion drivers keep.
//!
//! Each is *described* here and *produced* above in the world layer: the
//! object update path lifts a light / particle / probe / physics block onto
//! its component, and the movement, picking and HUD drivers own the
//! resources.

use crate::world_state::{AvatarMotion, ObjectParticleSystem};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use sl_client_bevy::{
    ControlFlags, DecodedTexture, LightData, Object, ObjectKey, PrimFaceId, PrimShapeParams,
    Priority, ReflectionProbe, ReflectionProbeFlags, RegionHandle, Rotation, ScopedObjectId,
    SurfaceInfo, TextureFace, TextureKey, Uuid, Vector, texture_face_uv_transform, to_bevy_image,
};
use sl_viewer_kit::coords::{sl_rotation_to_quat, sl_to_bevy_rotation};

/// A component marking an object entity as a **reflection probe**, carrying the
/// decoded `LLReflectionProbeParams` parameters (in Second Life semantics) plus
/// the prim's metre scale — the inputs the capture / volume side needs.
///
/// Attached to (and refreshed / cleared on) each object entity by
/// `apply_object` (the object ingest path) as its updates arrive. See
/// `reflection_probe_from_object` for the present-vs-absent lift.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ObjectReflectionProbe {
    /// The decoded reflection-probe parameters: the ambiance (irradiance) scale,
    /// the reflection-capture near-clip distance in metres, and the flag set
    /// (box-vs-sphere volume, dynamic capture, mirror).
    pub data: ReflectionProbe,
    /// The prim's Second Life metre scale, refreshed every update so a **resized**
    /// probe's influence volume (a box of these half-extents, or a sphere of the
    /// bounding radius) stays correct. The reference viewer likewise derives the
    /// probe volume from the prim's dimensions, not from the probe params.
    pub scale: [f32; 3],
}

impl ObjectReflectionProbe {
    /// Whether this probe's influence volume is a **box** (the prim's oriented
    /// bounding box) rather than a **sphere** — the `BOX_VOLUME` flag, which the
    /// reference reads as `LLVOVolume::getReflectionProbeIsBox`.
    #[must_use]
    pub const fn is_box_volume(&self) -> bool {
        self.data.flags.contains(ReflectionProbeFlags::BOX_VOLUME)
    }

    /// Whether this probe drives a **realtime mirror** — the `MIRROR` flag, which the
    /// reference reads as `LLVOVolume::isMirror` to hand the prim to the hero-probe
    /// manager. A mirror is captured sharp and live (all six faces every frame,
    /// dynamic content included) by the `hero` path rather than the amortized
    /// P33 local pool; see the reflection-probe plugin.
    #[must_use]
    pub const fn is_mirror(&self) -> bool {
        self.data.flags.contains(ReflectionProbeFlags::MIRROR)
    }

    /// The influence volume as a scale for Bevy's unit-cube `LightProbe` volume,
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
    /// `SPHERE_FALLOFF`.
    #[must_use]
    pub const fn volume_scale(&self) -> Vec3 {
        let [x, y, z] = self.scale;
        if self.is_box_volume() {
            Vec3::new(x, y, z)
        } else {
            Vec3::splat(x)
        }
    }

    /// The `LightProbe` falloff (per axis, as a fraction of the volume) this
    /// probe's influence tapers over: a hard-edged [`BOX_FALLOFF`] for a box volume,
    /// the far softer `SPHERE_FALLOFF` for a sphere approximated by a cube.
    #[must_use]
    pub const fn falloff(&self) -> Vec3 {
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
    #[must_use]
    pub fn radius(&self) -> f32 {
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
    #[must_use]
    pub const fn near_clip(&self) -> f32 {
        self.data.clip_distance.max(MIN_NEAR_CLIP)
    }
}

/// Lift an object's reflection-probe block onto an `ObjectReflectionProbe`, or
/// `None` when the object is not (or is no longer) a probe.
///
/// Mirrors the reference viewer's `LLViewerObject::getReflectionProbeParams`: a
/// prim is a probe exactly when it carries a reflection-probe extra-param block, so
/// this is a straight `Option` lift with no sentinel to reject.
#[must_use]
pub fn reflection_probe_from_object(object: &Object) -> Option<ObjectReflectionProbe> {
    object
        .extra
        .reflection_probe
        .map(|data| ObjectReflectionProbe {
            data,
            scale: [object.scale.x, object.scale.y, object.scale.z],
        })
}

/// The smallest near-clip distance a probe's capture cameras may use, in metres —
/// `LLReflectionMap::getNearClip`'s `MINIMUM_NEAR_CLIP`.
pub const MIN_NEAR_CLIP: f32 = 0.1;

/// The `LightProbe` falloff of a **box**-volume local probe: the fraction of the
/// volume over which its influence tapers out toward the faces of the box. Small, so
/// a box probe's reflection fills the room it bounds (as the reference's box probes
/// do) and only blends out right at the boundary rather than fading across it.
pub const BOX_FALLOFF: f32 = 0.1;

/// The `LightProbe` falloff of a **sphere**-volume local probe. Bevy's influence
/// volume is always a cuboid, so a sphere probe is bound as the cube circumscribing
/// its sphere; a broad taper pulls the influence back in toward the sphere, so the
/// corners the cube adds contribute little.
pub const SPHERE_FALLOFF: f32 = 0.5;

/// The projector parameters of a **spotlight** — a light that carries a
/// light-image ([`LightImage`](sl_client_bevy::LightImage)) extra-param and so
/// projects a texture within a cone (`LLVOVolume::isLightSpotlight`). A plain
/// point light has none of this.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightProjection {
    /// The projected texture id (`LLLightImageParams::getLightTexture`).
    pub texture: TextureKey,
    /// The projector cone field-of-view, in radians (`params.mV[0]`).
    pub fov: f32,
    /// The projector focus / blur (`params.mV[1]`).
    pub focus: f32,
    /// The projector ambiance — the diffuse spill outside the cone
    /// (`params.mV[2]`).
    pub ambiance: f32,
}

/// A component marking an object entity as a **light source**, carrying the
/// decoded `LLLightParams` (and, for a spotlight, `LLLightImageParams`)
/// parameters in Second Life semantics — ready for P25.2 to convert into a Bevy
/// `PointLight` / `SpotLight`.
///
/// Attached to (and refreshed / cleared on) each object entity by
/// `apply_object` (the object ingest path) as its updates arrive.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ObjectLight {
    /// The light's **linear** RGB colour, each channel in `0.0..=1.0`. The wire
    /// bytes are the linear (not gamma-corrected) colour — Firestorm's
    /// `LLLightParams::unpack` feeds them straight into `setLinearColor` — so no
    /// sRGB decode is applied here.
    pub linear_color: [f32; 3],
    /// The light intensity in `0.0..=1.0` — the alpha channel of the wire colour
    /// (`LLVOVolume::getLightIntensity` reads `getLinearColor().mV[3]`). The
    /// effective emitted colour is `linear_color * intensity`.
    pub intensity: f32,
    /// The light radius, in metres (`LIGHT_MIN_RADIUS`..=`LIGHT_MAX_RADIUS`,
    /// i.e. `0.0..=20.0`).
    pub radius: f32,
    /// The falloff exponent (`LIGHT_MIN_FALLOFF`..=`LIGHT_MAX_FALLOFF`, i.e.
    /// `0.0..=2.0`): how sharply the light dims toward its radius.
    pub falloff: f32,
    /// The spotlight cutoff cone half-angle, in degrees
    /// (`LIGHT_MIN_CUTOFF`..=`LIGHT_MAX_CUTOFF`, i.e. `0.0..=180.0`). Sent for
    /// every light but only meaningful for a projector.
    pub cutoff: f32,
    /// The projector parameters when this is a **spotlight** (it carries a
    /// light-image block); `None` for a plain point light.
    pub projection: Option<LightProjection>,
}

impl ObjectLight {
    /// Whether this light is a **spotlight** (projector) rather than a plain
    /// point light — true exactly when it carries projector parameters, mirroring
    /// `LLVOVolume::isLightSpotlight` (a light-image block is present).
    #[must_use]
    pub const fn is_spotlight(&self) -> bool {
        self.projection.is_some()
    }

    /// The light's effective emitted linear colour: its base colour scaled by its
    /// intensity, mirroring `LLVOVolume::getLightLinearColor`
    /// (`color * color.mV[3]`).
    #[must_use]
    pub const fn effective_linear_color(&self) -> [f32; 3] {
        [
            self.linear_color[0] * self.intensity,
            self.linear_color[1] * self.intensity,
            self.linear_color[2] * self.intensity,
        ]
    }
}

/// Convert one wire colour byte to a normalized `0.0..=1.0` float. The workspace
/// denies `as` casts, so the widening goes through [`f32::from`].
fn channel(byte: u8) -> f32 {
    f32::from(byte) / 255.0
}

/// Decode an object's light extra-params into an [`ObjectLight`], or `None` if the
/// object is not a light source (it carries no `LLLightParams` block).
///
/// A spotlight additionally carries a light-image block; when present it becomes
/// the [`projection`](ObjectLight::projection).
#[must_use]
pub fn light_from_object(object: &Object) -> Option<ObjectLight> {
    let light: LightData = object.extra.light?;
    let projection = object
        .extra
        .light_image
        .as_ref()
        .map(|image| LightProjection {
            texture: image.texture,
            fov: image.params.x,
            focus: image.params.y,
            ambiance: image.params.z,
        });
    Some(ObjectLight {
        linear_color: [
            channel(light.color[0]),
            channel(light.color[1]),
            channel(light.color[2]),
        ],
        intensity: channel(light.color[3]),
        radius: light.radius,
        falloff: light.falloff,
        cutoff: light.cutoff,
        projection,
    })
}

/// Lift a live particle system off an object into an [`ObjectParticleSystem`], or
/// `None` when the object is not (or is no longer) a particle source.
///
/// Returns `None` in the two cases the reference viewer treats as "no source":
/// the object carries no particle-system block at all (`Object::particles` is
/// `None` — sl-proto already yields `None` for an empty `PSBlock`, matching
/// `isNullPS`'s zero-size check), or it carries a **null** system whose CRC is
/// zero (`LLPartSysData::isNullPS` — the `llParticleSystem([])` stop sentinel).
#[must_use]
pub fn particles_from_object(object: &Object) -> Option<ObjectParticleSystem> {
    let system = object.particles.clone()?;
    // A zero-CRC system is the reference viewer's "null" particle system: the
    // sentinel a script sends to stop emitting. `isNullPS` rejects it, so it is
    // not a live source.
    if system.crc == 0 {
        return None;
    }
    Some(ObjectParticleSystem { system })
}

/// The authoritative kinematic state of a server-flagged physical root prim as of
/// its last `ObjectUpdate`, attached to the object entity by `apply_physics` and
/// change-detected: a fresh insert on every update reseeds the interpolation. The
/// component is absent on any object that is not a physical root, so its presence
/// alone marks the entities `drive_physical_objects` gives a kinematic body.
#[derive(Component, Clone, Debug)]
pub struct PhysicalObject {
    /// The object's full (grid-wide) key — the id the `GetObjectPhysicsData`
    /// capability request and its reply use, and the key
    /// `ObjectPhysicsShapes` stores this object's physics data under.
    pub full_key: ObjectKey,
    /// Region-local position (metres, Second Life Z-up frame).
    pub position: Vector,
    /// Linear velocity (metres/second).
    pub velocity: Vector,
    /// Linear acceleration (metres/second²) — usually gravity for a falling prim.
    pub acceleration: Vector,
    /// Orientation (a Second Life unit quaternion).
    pub rotation: Rotation,
    /// Angular velocity (rotation axis scaled by radians/second).
    pub angular_velocity: Vector,
    /// The region this object lives in, for the region-edge / neighbour lookups.
    pub region_handle: RegionHandle,
    /// The object's size (metres per axis), the source for its cuboid collider.
    pub scale: Vector,
}

/// The evolving dead-reckoning prediction shared by the object
/// (`PhysicsInterp`) and avatar ([`AvatarInterp`]) motion drivers: the
/// extrapolated (predicted) region-local pose plus the motion state that
/// `advance_motion` steps forward each frame between authoritative server
/// updates. All of it is in Second Life space (Z-up, pre basis-change), so the
/// same math serves both paths — they differ only in the ground floor they apply
/// (permissive for objects, stricter for avatars).
#[derive(Debug)]
pub struct MotionState {
    /// The predicted region-local position (Second Life Z-up metres).
    pub position: [f32; 3],
    /// The predicted orientation, in Second Life space (pre basis-change).
    pub rotation: Quat,
    /// The current linear velocity (metres/second), decaying under the phase-out.
    pub velocity: [f32; 3],
    /// The current linear acceleration (metres/second²); zeroed on a region cross
    /// or an empty-edge clip, matching the reference viewer.
    pub acceleration: [f32; 3],
    /// The angular velocity (axis·radians/second).
    pub angular_velocity: [f32; 3],
    /// The object's / avatar's region, for the region-edge / neighbour lookups.
    pub region_handle: RegionHandle,
    /// While predicted to be crossing a border, the elapsed-seconds deadline after
    /// which motion is stopped (`mRegionCrossExpire`); `None` when not crossing.
    pub region_cross_expire: Option<f64>,
}

impl MotionState {
    /// Seed the prediction from an authoritative update's motion fields.
    #[must_use]
    pub fn new(
        position: &Vector,
        velocity: &Vector,
        acceleration: &Vector,
        rotation: &Rotation,
        angular_velocity: &Vector,
        region_handle: RegionHandle,
    ) -> Self {
        Self {
            position: vector_to_array(position),
            rotation: sl_rotation_to_quat(rotation),
            velocity: vector_to_array(velocity),
            acceleration: vector_to_array(acceleration),
            angular_velocity: vector_to_array(angular_velocity),
            region_handle,
            region_cross_expire: None,
        }
    }
}

/// A [`Vector`]'s components as a plain `[f32; 3]` for the per-component
/// dead-reckoning math (Bevy's `Vec3` arithmetic operators are forbidden by the
/// workspace `arithmetic_side_effects` lint).
const fn vector_to_array(vector: &Vector) -> [f32; 3] {
    [vector.x, vector.y, vector.z]
}

/// The Bevy-world orientation of a predicted motion: its Second Life-space rotation
/// composed with the Second Life → Bevy basis change, matching the root transform
/// `body_root_transform` (the avatar path) writes on an authoritative update.
#[must_use]
pub fn bevy_rotation_of(motion: &MotionState) -> Quat {
    sl_to_bevy_rotation().mul_quat(motion.rotation)
}

/// The viewer-side interpolation state for one avatar, owned entirely by
/// `drive_avatar_motion`: the shared dead-reckoning prediction plus the avatar's
/// ground-floor height and whether its anchor carries the object rotation. Unlike
/// the object path, this driver moves the anchor by the *delta* between successive
/// predictions, so the root-drop vertical render offset (R23, owned by
/// `apply_object` (the avatar path) and refreshed by the appearance path) is left
/// untouched.
#[derive(Debug, Component)]
pub struct AvatarInterp {
    /// The shared dead-reckoning prediction (pose + motion) advanced each frame.
    pub motion: MotionState,
    /// Elapsed seconds when the last server update was ingested.
    pub last_message_secs: f64,
    /// Elapsed seconds at the last interpolation step.
    pub last_interp_secs: f64,
    /// The avatar's bounding-box height, for the stricter ground floor.
    pub height: f32,
    /// Whether to write the predicted orientation onto the anchor (a rigged body).
    pub apply_rotation: bool,
    /// The orientation actually written to the anchor this frame (Bevy space), eased
    /// toward the authoritative / dead-reckoned facing each frame rather than snapped
    /// to it (P31.7). This decouples the rendered turn from the sparse authoritative
    /// rotation updates — the own avatar's facing arrives only as terse
    /// `ObjectUpdate`s echoing the client-driven `SetRotation` (P31.5), so without
    /// this easing a turn snaps between those updates while translation stays smooth.
    pub rendered_rotation: Quat,
    /// The anchor **translation** actually written this frame (Bevy space,
    /// including the R23 root-drop offset baked in by the avatar path),
    /// eased toward the authoritative / dead-reckoned position each update rather
    /// than snapped to it. This is the translation counterpart of
    /// [`rendered_rotation`](Self::rendered_rotation): on each terse `ObjectUpdate`
    /// the authoritative position jumps a little (fast motion, sparse updates), and
    /// snapping the anchor to it made the world visibly shake against a rigid
    /// follow camera — easing spreads the correction across a few frames. A
    /// region crossing / teleport still snaps (see `TRANSLATION_SNAP_DISTANCE_M`).
    pub rendered_translation: Vec3,
    /// The **authoritative / dead-reckoned** anchor translation (Bevy space, with
    /// the root-drop offset) that [`rendered_translation`](Self::rendered_translation)
    /// eases toward every frame: captured from the anchor on each server update and
    /// advanced by the prediction delta between updates. Tracking it separately (vs.
    /// easing only on update frames) is what lets a short teleport that leaves the
    /// avatar standing still converge fully to the destination instead of freezing
    /// part-way once updates stop arriving.
    pub target_translation: Vec3,
}

impl AvatarInterp {
    /// Seed the interpolation state from an authoritative update at time `now`,
    /// starting the eased translation at the anchor's current position `anchor`
    /// (already placed by the avatar path).
    #[must_use]
    pub fn seeded(motion: &AvatarMotion, now: f64, anchor: Vec3) -> Self {
        let motion_state = MotionState::new(
            &motion.position,
            &motion.velocity,
            &motion.acceleration,
            &motion.rotation,
            &motion.angular_velocity,
            motion.region_handle,
        );
        // Start the eased orientation at the authoritative facing so the avatar does
        // not visibly rotate into place from identity on its first frame.
        let rendered_rotation = bevy_rotation_of(&motion_state);
        Self {
            motion: motion_state,
            last_message_secs: now,
            last_interp_secs: now,
            height: motion.height,
            apply_rotation: motion.apply_rotation,
            rendered_rotation,
            rendered_translation: anchor,
            target_translation: anchor,
        }
    }

    /// Re-base the eased translation onto a moved scene origin: a region crossing
    /// (or a teleport to an already-connected region) shifts every origin-anchored
    /// entity by the same `delta`, so shift both the rendered and target
    /// translations to keep the avatar in the same world spot across the re-base
    /// (`recenter_avatars`). The region-local
    /// [`motion`](Self::motion) is unaffected — its dead-reckoned deltas are
    /// origin-invariant.
    pub fn rebase(&mut self, delta: Vec3) {
        // Per-component to avoid the `arithmetic_side_effects` lint on the glam
        // `Vec3` operator.
        self.rendered_translation.x += delta.x;
        self.rendered_translation.y += delta.y;
        self.rendered_translation.z += delta.z;
        self.target_translation.x += delta.x;
        self.target_translation.y += delta.y;
        self.target_translation.z += delta.z;
    }

    /// Re-seed the predicted pose to a fresh authoritative update at time `now`,
    /// snapping the prediction back to the server truth and restarting the timers.
    pub fn reseed(&mut self, motion: &AvatarMotion, now: f64) {
        self.motion = MotionState::new(
            &motion.position,
            &motion.velocity,
            &motion.acceleration,
            &motion.rotation,
            &motion.angular_velocity,
            motion.region_handle,
        );
        self.last_message_secs = now;
        self.last_interp_secs = now;
        self.height = motion.height;
        self.apply_rotation = motion.apply_rotation;
    }
}

/// The minimum interval, in seconds, between the body-rotation `AgentUpdate`s sent
/// while turning (~20 Hz), so a held turn key does not flood the circuit — the
/// heading still advances every frame client-side, it is just broadcast at this
/// rate.
pub const ROTATION_SEND_INTERVAL_SECS: f32 = 0.05;

/// The per-key state of the tap-tap-hold-to-run detector: how recently the key
/// was last tapped and whether a double-tap's run is currently latched (held).
#[derive(Debug, Clone)]
pub struct DoubleTapRun {
    /// Seconds since the key was last freshly pressed; starts beyond the window
    /// so the first tap of a session can never pair with "before the session".
    pub since_last_tap: f32,
    /// Whether the second tap of a double-tap is still held, running the avatar.
    pub latched: bool,
}

impl Default for DoubleTapRun {
    fn default() -> Self {
        Self {
            since_last_tap: f32::INFINITY,
            latched: false,
        }
    }
}

/// The persistent state of the avatar movement controls: the client-tracked walk
/// heading, whether flying is toggled on, and the bookkeeping that keeps the viewer
/// from re-sending an unchanged intent every frame.
#[derive(Debug, Resource)]
pub struct AvatarControls {
    /// The walk heading (yaw about the Second Life up axis, radians) the body faces;
    /// seeded once from the own avatar's reported facing so the first step does not
    /// snap it.
    pub yaw: f32,
    /// Whether flying is toggled on ([`ControlFlags::FLY`] is advertised).
    pub flying: bool,
    /// Whether `yaw` has been seeded from the own avatar yet.
    pub seeded: bool,
    /// Whether the seeded heading has been advertised to the simulator at least
    /// once, so a walk before the first turn moves in the right direction.
    pub sent_initial_rotation: bool,
    /// The control-flag set last advertised, so a [`sl_client_bevy::Command::SetControls`] is emitted
    /// only when the flags actually change.
    pub last_controls: ControlFlags,
    /// Seconds accumulated since the last rotation send, for the turning throttle.
    pub rotation_send_accum: f32,
    /// Seconds the ascend key has been held while not flying, for the P31.16
    /// hold-to-take-off; reset whenever that precondition lapses. The reference's
    /// `gKeyboard->getCurKeyElapsedTime()` in `agent_jump`.
    pub ascend_hold_secs: f32,
    /// Frames the ascend key has been held while not flying, counted alongside
    /// [`ascend_hold_secs`](Self::ascend_hold_secs) and reset with it. The
    /// reference's `getCurKeyElapsedFrameCount()`: a take-off needs *both* the
    /// elapsed time and a handful of frames, so a single very long frame (a
    /// stutter, a texture-decode hitch) cannot turn a tap into a take-off.
    pub ascend_hold_frames: u32,
    /// The tap-tap-hold-to-run detector for the walk-forward key.
    pub tap_run_forward: DoubleTapRun,
    /// The tap-tap-hold-to-run detector for the walk-backward key.
    pub tap_run_backward: DoubleTapRun,
    /// A heading (radians about the Second Life up axis) something has *told*
    /// the avatar to face rather than turned it towards: RLV's
    /// `@setrot:<radians>=force`, or the facing a teleport's arrival placed the
    /// agent at (the arrival slam). Taken by the movement driver on the next
    /// frame it runs, which replaces the tracked heading with it and advertises
    /// it at once.
    pub forced_heading: Option<f32>,
}

impl AvatarControls {
    /// The heading (radians about the Second Life up axis) the **own** avatar is
    /// drawn facing and the third-person camera follows, once it is known.
    ///
    /// The viewer's own heading, not the facing the simulator echoes back. That
    /// is the reference's rule (`LLVOAvatar::updateOrientation` takes the self
    /// avatar's forward direction from `gAgent`'s at-axis), and it matters
    /// because the echo is not the heading: the simulator turns the body towards
    /// a sent rotation over several updates and parks it short, 1.5–3° off on
    /// aditi, so a body drawn from the echo sits visibly off the heading it was
    /// turned to, and any small step the simulator chose to send would turn it
    /// with no key pressed. `None` until the heading has been seeded from the
    /// avatar's first report, when the echo is all there is.
    #[must_use]
    pub const fn held_heading(&self) -> Option<f32> {
        if self.seeded { Some(self.yaw) } else { None }
    }

    /// The [`ControlFlags`] set last advertised to the simulator (walk / run /
    /// fly / ascend / descend). The client-side locomotion fallback
    /// (the `locomotion` module) reads the same advertised intent that moves the
    /// avatar to pick which built-in animation to play for immediate feedback.
    ///
    /// The set includes [`ControlFlags::FLY`] while flying is toggled on, so the
    /// locomotion fallback reads the fly / hover states straight off it.
    #[must_use]
    pub const fn advertised(&self) -> ControlFlags {
        self.last_controls
    }
}

impl Default for AvatarControls {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            flying: false,
            seeded: false,
            sent_initial_rotation: false,
            last_controls: ControlFlags::empty(),
            rotation_send_accum: ROTATION_SEND_INTERVAL_SECS,
            ascend_hold_secs: 0.0,
            ascend_hold_frames: 0,
            tap_run_forward: DoubleTapRun::default(),
            tap_run_backward: DoubleTapRun::default(),
            forced_heading: None,
        }
    }
}

/// Who input belongs to this frame.
///
/// Derived from `InputFocus` by `compute_input_context`; never assigned by
/// hand.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InputContext {
    /// Nothing in the UI holds focus: the world has the keyboard and the mouse.
    ///
    /// The seam the camera / movement modes (mouselook, third-person, sitting —
    /// Firestorm's `keys.xml` modes) subdivide when they arrive.
    #[default]
    World,
    /// A focusable UI node that does not take text holds focus — a button, a
    /// checkbox. `Enter` / `Space` activate it, and the world gets no keys.
    UiWidget,
    /// A text-accepting node holds focus. Characters, the arrows and `Backspace`
    /// are all its; the world gets nothing.
    TextEntry,
    /// An in-world **media face** holds keyboard focus
    /// ([`crate::MediaFocus`]): keys go to the embedded page, so
    /// the world gets nothing — the reference's `LLViewerMediaFocus` taking
    /// `gFocusMgr`'s keyboard focus.
    Media,
}

impl InputContext {
    /// Whether the world owns input right now.
    #[must_use]
    pub const fn is_world(self) -> bool {
        matches!(self, Self::World)
    }
}

/// A run condition: true while the world owns the keyboard.
///
/// Put this on every system that reads a key a focused UI could want — which is
/// all of them bar the `F`-key overlay toggles. See
/// `sl_viewer_world_view::input_context` for why the arrow keys are in that set.
///
/// It lives here, beside [`InputContext`] itself, rather than with the system
/// that computes the context: a gate stated in terms of shared vocabulary can be
/// applied by any layer, and every layer that reads a key needs it.
#[must_use]
pub fn world_has_keyboard(context: Res<InputContext>) -> bool {
    context.is_world()
}

/// The spawned HUD point nodes, keyed by raw attachment-point id, so an
/// attachment can be routed to the node for its point.
///
/// Empty when the run has no avatar assets (no `--viewer-assets`): the HUD point
/// offsets come from `avatar_lad.xml`, so without it there is no HUD screen and a
/// HUD attachment is hidden rather than routed (the same degradation that leaves
/// avatars as placeholder spheres).
#[derive(Resource, Debug, Default)]
pub struct HudState {
    /// The HUD point node entities, keyed by raw attachment-point id.
    pub points: HashMap<u8, Entity>,
}

impl HudState {
    /// The node entity a HUD attachment worn on `point_id` parents to, or `None`
    /// if there is no HUD screen (no avatar assets) or the id is not a HUD point.
    #[must_use]
    pub fn point_entity(&self, point_id: u8) -> Option<Entity> {
        self.points.get(&point_id).copied()
    }
}

/// Component-wise vector subtraction (`a - b`), avoiding the glam `-` operator the
/// workspace `arithmetic_side_effects` lint trips on.
#[must_use]
pub fn vsub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Component-wise vector scaling (`v * s`).
#[must_use]
pub fn vscale(v: Vec3, s: f32) -> Vec3 {
    Vec3::new(v.x * s, v.y * s, v.z * s)
}

/// Build the [`SurfaceInfo`] a touch carries from a ray hit, the picked face, and
/// the touched object's world transform — the viewer's `LLPickInfo::getSurfaceInfo`.
///
/// - **Face** is the Linden face index the ray struck (`-1` when the hit is not on
///   a textured face, the reference's "no intersection" value).
/// - **ST** is the face's own `[0, 1]` surface coordinate: the mesh's stored
///   texture coordinate, un-flipped from the bottom-up→top-down convention this
///   viewer bakes into `ATTRIBUTE_UV_0` back into Second Life's bottom-up space.
/// - **UV** is `ST` with the face's texture placement (repeats / offset /
///   rotation, [`texture_face_uv_transform`]) applied — the coordinate as the
///   texture is actually sampled, matching the reference's `surfaceToTexture`.
/// - **Position / normal / binormal** are given in the object's own Second Life
///   frame (its global's inverse carries the world hit back into it). A HUD lives
///   in screen space with no meaningful region position, so the object-local
///   frame is the sensible finite choice; the reference instead reports region /
///   HUD-matrix coordinates, a deliberate simplification here. The binormal is
///   derived geometrically (perpendicular to the normal, along the hit triangle)
///   rather than from a texture tangent the ray hit does not carry.
#[must_use]
pub fn surface_info_from_hit(
    hit: &bevy::picking::mesh_picking::ray_cast::RayMeshHit,
    face_id: Option<PrimFaceId>,
    texture_face: Option<&TextureFace>,
    object_global: &GlobalTransform,
) -> SurfaceInfo {
    let inverse = object_global.affine().inverse();
    // The hit point / normal in the object's own Second Life frame (the object
    // subtree lives in Second Life space under the root's basis change, so the
    // inverse of its global lands here directly).
    let position = inverse.transform_point3(hit.point);
    let normal = inverse.transform_vector3(hit.normal).normalize_or_zero();

    // The binormal: perpendicular to the normal and along the surface, derived
    // from the hit triangle's first edge projected off the normal.
    let binormal = hit
        .triangle
        .map(|tri| {
            let edge = inverse.transform_vector3(vsub(tri[1], tri[0]));
            let along = vsub(edge, vscale(normal, edge.dot(normal)));
            normal.cross(along).normalize_or_zero()
        })
        .filter(|binormal| *binormal != Vec3::ZERO)
        .unwrap_or_else(|| normal.any_orthonormal_vector());

    // ST: the mesh's stored surface coordinate, back in Second Life bottom-up
    // space (this viewer flips `v` when building the Bevy mesh).
    let bevy_uv = hit.uv.unwrap_or(Vec2::ZERO);
    let st = Vec2::new(bevy_uv.x, 1.0 - bevy_uv.y);
    // UV: ST with the face's texture placement applied, as sampled — the
    // `uv_transform` acts in the Bevy (flipped) UV space, so flip back after.
    let placed = texture_face.map_or(bevy_uv, |tf| {
        texture_face_uv_transform(tf).transform_point2(bevy_uv)
    });
    let uv = Vec2::new(placed.x, 1.0 - placed.y);

    SurfaceInfo {
        uv: [uv.x, uv.y],
        st: [st.x, st.y],
        face_index: face_id.map_or(-1, |face| i32::from(face.get())),
        position: Vector {
            x: position.x,
            y: position.y,
            z: position.z,
        },
        normal: Vector {
            x: normal.x,
            y: normal.y,
            z: normal.z,
        },
        binormal: Vector {
            x: binormal.x,
            y: binormal.y,
            z: binormal.z,
        },
    }
}

/// Whether the pointer is over a **blocking** UI element — a hovered `bevy_ui`
/// node that occludes what is behind it.
///
/// A node **without** a [`Pickable`] component blocks by default in `bevy_ui`
/// (`should_block_lower` defaults to `true`) — and most pane content (the pane
/// column, the group-list body, the transcript text) has no explicit `Pickable`,
/// so it must count as blocking. Only nodes that opt **out** with an explicit
/// `Pickable { should_block_lower: false, .. }` — the full-window
/// UI root and the (empty) dock host — are transparent to the pick,
/// so an empty-UI click still touches the world / HUD through them.
///
/// A hovered entry only occludes if it is an **actual UI node with positive
/// area** — it has a [`ComputedNode`] whose laid-out size is non-zero. Two kinds
/// of hover-map entry are *not* a UI surface and must never suppress a world pick:
/// a hover entry that is not a `bevy_ui` node at all (it has no `ComputedNode`),
/// and a degenerate zero-area node (e.g. an empty, collapsed text node). Without
/// this guard such an entry — hovered everywhere, covering nothing — reported the
/// whole world as "blocked", silently killing every world pick (touch, and the
/// avatar context menu's body pick).
#[must_use]
pub fn pointer_over_blocking_ui(
    hover_map: &HoverMap,
    pickables: &Query<&Pickable>,
    sizes: &Query<&ComputedNode>,
) -> bool {
    hover_map
        .values()
        .flat_map(|hits| hits.keys())
        .any(|entity| {
            let blocks = pickables
                .get(*entity)
                .map_or(true, |pickable| pickable.should_block_lower);
            let has_area = sizes
                .get(*entity)
                .is_ok_and(|computed| computed.size().x > 0.0 && computed.size().y > 0.0);
            blocks && has_area
        })
}

// ---- moved down from the object layer (step 19) ----
/// The broad render classification of an in-world object, decided from its
/// `pcode` and sculpt/mesh extra parameters. It routes the object to the right
/// (later-phase) rendering path; P5.1 only records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectCategory {
    /// An avatar (`pcode` 47) — a placeholder sphere in Phase 10.
    Avatar,
    /// A plain volume prim — tessellated with `sl_prim` in Phase 5.2.
    Prim,
    /// A sculpted prim (its shape comes from a sculpt texture) — Phase 9.
    Sculpt,
    /// A mesh object (its shape comes from a mesh asset) — Phase 7.
    Mesh,
    /// A Linden tree (`PCODE_TREE` / `PCODE_NEW_TREE`) — its branch / leaf
    /// geometry is generated procedurally from its species (P26.2).
    Tree,
    /// A Linden grass clump (`PCODE_GRASS`) — its crossed-quad blade geometry is
    /// generated procedurally from its species and scale (P26.3).
    Grass,
    /// Anything else (particle-system object, …); not rendered by the current
    /// phases.
    Other,
}

/// A marker component tagging an entity as an in-world object, carrying its
/// scoped id and render classification for the rendering phases to query — the
/// `pick_object` crosshair tool (both fields) and the `drive_render_priority`
/// prim LOD pass (P21.3, keyed off the classification and scoped id).
///
/// Both readers live in the object layer (`sl_viewer_world_objects`, modules
/// `objects` and `render_priority`), which depends on this crate rather than
/// the other way round, so they cannot be linked from here.
#[derive(Component, Debug, Clone, Copy)]
pub struct SceneObject {
    /// The object's scoped (circuit + region-local) id.
    pub scoped_id: ScopedObjectId,
    /// The object's render classification.
    pub category: ObjectCategory,
}

/// Debug identity carried on each object's root entity so the `pick_object`
/// crosshair tool (in the object layer) can report exactly what the camera is
/// looking at — the object's
/// full id, its mesh/sculpt asset id (the thing to fetch and decode offline when
/// its geometry looks wrong), and its Second Life scale/position.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ObjectDebugInfo {
    /// The object's full (asset) id.
    full_id: Uuid,
    /// The mesh or sculpt-map asset id, when the object has one.
    asset: Option<Uuid>,
    /// The object's Second Life scale (metres per axis).
    scale: [f32; 3],
    /// The object's Second Life region-local position.
    position: [f32; 3],
    /// The object's quantized prim shape parameters, so a wrongly tessellated plain
    /// prim can be reproduced offline exactly as the simulator described it.
    shape: PrimShapeParams,
}

impl ObjectDebugInfo {
    /// The object's mesh or sculpt-map asset id, or `None` for a plain prim. Used
    /// by the P20.2 render-priority driver to rank a mesh object's still-fetching
    /// geometry (or a sculpt's map) from the object's on-screen size before its
    /// face entities exist.
    #[must_use]
    pub const fn render_asset(&self) -> Option<Uuid> {
        self.asset
    }

    /// Build the debug identity for an object's root entity from what the
    /// simulator described: its full id, its mesh / sculpt asset id if it has
    /// one, and its Second Life scale, position and prim shape.
    ///
    /// The fields stay private so the object layer records this identity
    /// through one call rather than reaching into five fields from another
    /// crate.
    #[must_use]
    pub const fn new(
        full_id: Uuid,
        asset: Option<Uuid>,
        scale: [f32; 3],
        position: [f32; 3],
        shape: PrimShapeParams,
    ) -> Self {
        Self {
            full_id,
            asset,
            scale,
            position,
            shape,
        }
    }

    /// The object's Second Life scale (metres per axis), whose half-diagonal is
    /// its bounding radius for the P20.2 pixel-area computation.
    #[must_use]
    pub const fn scale(&self) -> [f32; 3] {
        self.scale
    }

    /// The object's full (asset) id, as the crosshair pick tool reports it.
    #[must_use]
    pub const fn full_id(&self) -> Uuid {
        self.full_id
    }

    /// The object's Second Life region-local position.
    #[must_use]
    pub const fn position(&self) -> [f32; 3] {
        self.position
    }

    /// The object's quantized prim shape parameters, so a wrongly tessellated
    /// plain prim can be reproduced offline exactly as the simulator described
    /// it.
    #[must_use]
    pub const fn shape(&self) -> PrimShapeParams {
        self.shape
    }
}

/// Whether the own avatar is currently typing into local chat — driven by the
/// nearby-chat bar (`crate::nearby_chat_bar`) through [`set`](Self::set): active
/// while the bar is focused and holds a draft, inactive on send / blur.
#[derive(Debug, Resource, Default)]
pub struct TypingState {
    /// Whether typing is active this frame.
    active: bool,
    /// The `active` value last advertised to the simulator, so a `StartTyping` /
    /// `StopTyping` `ChatFromViewer` is emitted only on the *edge* rather than every
    /// frame — the simulator holds the state until the opposite signal arrives.
    advertised: bool,
}

impl TypingState {
    /// Whether the own avatar is typing this frame.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Set the typing state (the nearby-chat bar calls this while a draft is being
    /// typed, and clears it on send / blur). The wire edge is reconciled by the
    /// object layer's typing driver, so this only records intent.
    pub const fn set(&mut self, active: bool) {
        self.active = active;
    }

    /// Take the un-advertised typing edge, if there is one: `Some(active)` the
    /// first time the state differs from what the simulator was last told, and
    /// `None` on every frame after. The simulator holds each state between
    /// signals, so re-sending every frame would flood the circuit.
    ///
    /// Taking the edge records it as advertised, so the caller must actually
    /// send the wire signals when this returns `Some`.
    pub const fn take_edge(&mut self) -> Option<bool> {
        if self.active == self.advertised {
            None
        } else {
            self.advertised = self.active;
            Some(self.active)
        }
    }
}

/// The id a Second Life GLTF material override uses for "no texture here" (the
/// reference viewer's `LLGLTFMaterial::GLTF_OVERRIDE_NULL_UUID`): a face
/// carrying it has no diffuse texture to fetch, so it is treated exactly like
/// the nil id rather than endlessly re-requested (it is not a fetchable asset
/// and 503s).
const GLTF_OVERRIDE_NULL_UUID: Uuid = Uuid::from_u128(u128::MAX);

/// Whether a face texture id denotes "no diffuse texture" — the nil id or the
/// GLTF override-null sentinel — so it should neither be fetched nor treated as
/// a textured face.
#[must_use]
pub fn is_absent_texture(id: TextureKey) -> bool {
    let uuid = id.uuid();
    uuid.is_nil() || uuid == GLTF_OVERRIDE_NULL_UUID
}

/// Upload a decoded texture as a Bevy image with Second Life's wrap behaviour:
/// prim faces repeat their texture, which is not Bevy's default.
#[must_use]
pub fn build_prim_image(decoded: &Arc<DecodedTexture>) -> Image {
    let mut image = to_bevy_image(decoded);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// The state of a face's diffuse image — see [`DecodedTextures::diffuse_image`].
#[derive(Debug, Clone)]
pub enum DiffuseImage {
    /// The face carries no texture (a nil / null-sentinel id); a material should
    /// clear its `base_color_texture` and show the flat tint.
    Absent,
    /// The texture exists but has not decoded yet — keep the current image and
    /// try again once it lands.
    Pending,
    /// The decoded image, uploaded as a fresh Bevy image ready to paint onto a
    /// material.
    Ready(Handle<Image>),
}

/// Successfully decoded textures by id, shared across every consumer so a
/// texture is fetched and decoded once no matter how many faces, avatars or UI
/// panes show it.
///
/// This is the *result* half of texture fetching, deliberately split from the
/// machinery that produces it. The object layer's texture manager owns the
/// in-flight requests, priorities, retry state and level-of-detail budgets, and
/// records what it decodes here; everything that merely wants to *show* a
/// texture reads this resource instead, and so does not depend on the fetch
/// pipeline or rebuild when it changes.
///
/// Ask for a texture that has not been fetched with [`BoostTexture`].
#[derive(Resource, Default, Debug)]
pub struct DecodedTextures {
    /// Decoded images by texture id.
    decoded: HashMap<TextureKey, Arc<DecodedTexture>>,
}

impl DecodedTextures {
    /// The decoded image for `id`, once it has been fetched, or `None` if it is
    /// still in flight or the fetch failed.
    #[must_use]
    pub fn get(&self, id: TextureKey) -> Option<&Arc<DecodedTexture>> {
        self.decoded.get(&id)
    }

    /// Whether `id` has decoded.
    #[must_use]
    pub fn contains(&self, id: TextureKey) -> bool {
        self.decoded.contains_key(&id)
    }

    /// How many textures have decoded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.decoded.len()
    }

    /// Whether nothing has decoded yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.decoded.is_empty()
    }

    /// Every decoded texture, for the diagnostics and cost models that walk the
    /// whole store.
    pub fn iter(&self) -> impl Iterator<Item = (&TextureKey, &Arc<DecodedTexture>)> {
        self.decoded.iter()
    }

    /// Record a freshly decoded image, returning the one it replaced (a texture
    /// re-decoded at a finer level of detail replaces its coarser image).
    pub fn insert(
        &mut self,
        id: TextureKey,
        image: Arc<DecodedTexture>,
    ) -> Option<Arc<DecodedTexture>> {
        self.decoded.insert(id, image)
    }

    /// Drop a texture's decoded image, returning it if there was one.
    pub fn remove(&mut self, id: TextureKey) -> Option<Arc<DecodedTexture>> {
        self.decoded.remove(&id)
    }

    /// Classify a diffuse texture for a consumer that paints it onto a material
    /// directly (the build tool's live texture preview), uploading a fresh Bevy
    /// image when ready. Distinguishes a genuinely **absent** texture (a nil /
    /// null-sentinel id — the material should clear its `base_color_texture` and
    /// show the flat tint) from one that simply has not **decoded** yet (keep the
    /// old image and wait), which a bare `Option<Handle>` would conflate.
    pub fn diffuse_image(&self, id: TextureKey, images: &mut Assets<Image>) -> DiffuseImage {
        if is_absent_texture(id) {
            DiffuseImage::Absent
        } else if let Some(decoded) = self.decoded.get(&id) {
            DiffuseImage::Ready(images.add(build_prim_image(decoded)))
        } else {
            DiffuseImage::Pending
        }
    }
}

/// Ask the object layer's texture manager to fetch a texture at a raised
/// priority, for a surface that is showing it right now — a profile picture, an
/// inventory thumbnail, the texture picker's grid.
///
/// This is a request rather than a call so that the crates which merely *show*
/// textures do not depend on the fetch machinery. It is a priority hint on an
/// operation that already spans many frames, so being served on the next frame
/// rather than this one is not observable.
#[derive(Message, Debug, Clone, Copy)]
pub struct BoostTexture {
    /// The texture to fetch.
    pub key: TextureKey,
    /// The priority to fetch it at.
    pub priority: Priority,
}

/// A positive per-frame budget from `var`, or `default` when it is unset /
/// unparsable / zero.
#[must_use]
pub fn env_budget(var: &str, default: usize) -> usize {
    std::env::var(var)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

/// The label the texture store's pipeline figures are published under — the same
/// short name the overlay prints them beside, so neither side keeps a mapping
/// table of its own. The labels live here rather than beside the stores because
/// they are what the two layers agree on.
pub const TEXTURE_LABEL: &str = "tex";
/// The mesh store's published label (see [`TEXTURE_LABEL`]).
pub const MESH_LABEL: &str = "mesh";
/// The animation store's published label (see [`TEXTURE_LABEL`]).
pub const ANIMATION_LABEL: &str = "anim";
/// The wearable-asset store's published label (see [`TEXTURE_LABEL`]).
pub const WEARABLE_LABEL: &str = "wear";
/// The glTF render-material store's published label (see [`TEXTURE_LABEL`]).
pub const MATERIAL_LABEL: &str = "gmat";

/// Every store label the overlay expects to find published.
///
/// The publishers are **split across two crates** — the object layer's
/// `asset_stats` covers texture / mesh / material and the avatar layer's
/// `avatar_asset_stats` covers animation / wearable — so neither one can check
/// its own completeness, and a whole publisher going missing (a plugin dropped
/// from the composition root, a store added to one half and not listed) would
/// only show up as two quietly absent lines on the `F3` panel. This is the list
/// the two halves are checked against together.
pub const PIPELINE_LABELS: &[&str] = &[
    TEXTURE_LABEL,
    MESH_LABEL,
    ANIMATION_LABEL,
    WEARABLE_LABEL,
    MATERIAL_LABEL,
];

/// One asset store's live pipeline figures, as its own layer publishes them.
///
/// The formatting is the reader's business; this is just the numbers.
#[derive(Debug, Clone, Copy, Default)]
pub struct StorePipelineStats {
    /// Per-stage entry counts, footprint and cumulative cache / GC counters.
    pub stats: sl_client_bevy::StoreStats,
    /// The admission gate's in-flight / capacity / waiting figures.
    pub gate: sl_client_bevy::GateStats,
    /// How many requests are parked or retrying.
    pub deferred: usize,
}

/// Every asset store's live pipeline figures, **published by the layer that owns
/// the store** and read by whoever wants to show them.
///
/// The pipeline-status overlay (`F3`) used to take one `Res<…Manager>` per store,
/// which meant the scene layer named four of the object layer's asset stores for
/// no reason but to read three numbers off each. Publishing here inverts that: the
/// object layer states its own figures in vocabulary this crate defines, and the
/// overlay reads one resource.
///
/// [`wanted`](Self::wanted) is the demand side of the same inversion: publishing
/// costs nothing while nothing is looking, so the reader says when it is looking
/// and the publishers order themselves against that rather than against a
/// visibility flag that lives in the reader's crate.
#[derive(Debug, Resource, Default)]
pub struct PipelineStats {
    /// Whether anything is currently displaying these figures. Publishers skip
    /// their work entirely while this is `false`.
    wanted: bool,
    /// The published figures, by the short label the overlay prints them under.
    /// A `BTreeMap` so the overlay's line order is stable frame to frame without
    /// the reader having to sort.
    by_label: BTreeMap<&'static str, StorePipelineStats>,
}

impl PipelineStats {
    /// State whether anything is displaying these figures, so the publishers know
    /// whether to bother. Idempotent — call it every frame from the reader.
    pub const fn set_wanted(&mut self, wanted: bool) {
        self.wanted = wanted;
    }

    /// Whether a publisher should do its work this frame.
    #[must_use]
    pub const fn wanted(&self) -> bool {
        self.wanted
    }

    /// Run condition for a publisher system: is anything looking?
    #[must_use]
    pub fn pipeline_stats_wanted(stats: Res<Self>) -> bool {
        stats.wanted
    }

    /// Publish one store's figures under `label`.
    pub fn publish(&mut self, label: &'static str, stats: StorePipelineStats) {
        let _previous = self.by_label.insert(label, stats);
    }

    /// One store's published figures, or `None` when its layer has not published
    /// any yet (the frame the overlay is first shown).
    #[must_use]
    pub fn get(&self, label: &str) -> Option<StorePipelineStats> {
        self.by_label.get(label).copied()
    }

    /// Every published store's label and figures, in label order.
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, StorePipelineStats)> {
        self.by_label.iter().map(|(&label, &stats)| (label, stats))
    }
}
