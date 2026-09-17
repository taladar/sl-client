//! World state the world's own layers share.
//!
//! Where the camera is and what it is doing, how the avatar is moving, which
//! region's terrain is loaded, what has been derendered: state produced by one
//! part of the world layer and read by the others. It sits here so those parts
//! can be separate crates that describe the same world without depending on
//! each other to name it.

use crate::object_flags::FLAGS_SERVER_AUTOPILOT;
use crate::settings::{CAMERA_OFFSET, MAX_DISTANCE, MAX_PITCH, MOUSELOOK_CROSS_DISTANCE};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sl_client_bevy::{
    AgentKey, Object, ObjectKey, ParticleSystem, Priority, RegionHandle, Rotation, ScopedObjectId,
    Uuid, Vector,
};

/// The camera mode: one of the three the [`ViewerCamera`] cycles between. See the
/// [module documentation](self).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CameraMode {
    /// First-person: at the eyes, mouse aims, cursor captured.
    Mouselook,
    /// Orbiting third-person around a `FocusTarget` (the default).
    #[default]
    ThirdPerson,
    /// Free 6-DOF spectator camera (the promoted debug fly-camera).
    Flycam,
}

/// Whether the own avatar's body stays drawn while the camera is in
/// [`CameraMode::Mouselook`] — the reference's `FirstPersonAvatarVisible`, kept
/// current from the settings store by the camera & movement preferences tab.
///
/// Either way the **head** is not drawn from inside it: with the body shown, the
/// head, hair, eyelashes and eyeballs leave the view (still casting their
/// shadow) and so does whatever is worn on a head attachment point; with it
/// hidden, nothing of the avatar is drawn at all.
///
/// Defaults to shown, which is this project's default for the setting (the
/// reference hides the body), so a run with no settings store keeps a body in
/// mouselook.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirstPersonAvatarVisible(pub bool);

impl Default for FirstPersonAvatarVisible {
    /// Shown — see the type's documentation.
    fn default() -> Self {
        Self(true)
    }
}

/// A request to enter [`CameraMode::Flycam`], or to leave it for third person —
/// the same toggle the 6-DOF device's first button pulses, written by anything
/// outside the world layer that offers the user a way in or out (the menu bar's
/// **Joystick Flycam** entry and its `Alt+Shift+F` accelerator).
///
/// A message rather than a direct `CameraMode` write because entering and
/// leaving are not just a mode assignment: the rig's aim is seeded from the
/// current view on the way in and the smoothing is resnapped on the way out, so
/// the pose is continuous entering and does not glide through the scene leaving.
/// `sl_viewer_world_view::camera`'s `switch_camera_mode` owns that, and this is
/// how it is asked.
#[derive(Message, Debug, Clone, Copy)]
pub struct ToggleFlycam;

/// The marker on the one main viewer camera entity — the camera every world
/// system means by "the camera", as opposed to the reflection-probe, mirror and
/// minimap cameras that also carry `Camera3d`. Mode-agnostic: the same entity is
/// the camera in mouselook, third person and flycam.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ViewerCamera;

/// Marks a camera that draws one **overlay layer** over the world camera's
/// frame, and names which layer it draws.
///
/// The composited window frame is four passes in a fixed order — the world
/// (order 0), the edit gizmos (1), the HUD attachments (2, which is also
/// `bevy_ui`'s default camera and so carries the viewer's UI) — and the capture
/// harness routes each one into a captured frame or leaves it on the window
/// *independently*, so a comparison can ask about HUD rendering without the UI
/// in the way, or about the UI without the HUD.
///
/// It lives here, rather than each overlay's own crate, because the harness
/// (`sl-viewer-world-view`) must address a camera spawned by a crate above it
/// (`sl-viewer-edit`'s gizmo overlay) without depending on it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayCamera {
    /// The edit-tool gizmo overlay: the move / rotate / scale handles and the
    /// selection outlines, on their own render layer.
    Gizmos,
    /// The HUD-attachment layer — and, because that camera carries
    /// `IsDefaultUiCamera`, every `bevy_ui` node too. The two are one camera, so
    /// a capture that wants one and not the other hides the other's content.
    HudAndUi,
}

/// Marks an entity that draws a **second copy of another entity's skinned
/// geometry** and names the entity it copies — so the avatar layer poses the two
/// alike.
///
/// A viewer draws the same posed mesh twice in more than one place: the waterline
/// split gives a straddling face a twin clipped to the other side
/// (`sl-viewer-world-scene`'s `water_clip`), and the build tool's selection
/// highlight outlines a rigged face with a wireframe of its own geometry
/// (`sl-viewer-edit`'s `selection_wireframe`). Sharing the mesh asset is not
/// enough to share the *pose*, because this viewer skins on the GPU: the palette
/// of a rigged draw is written per **entity** by the avatar pipeline, from a
/// binding that entity carries. A copy that has only cloned `SkinnedMesh` falls
/// back to Bevy's own skin extract, which reads the placeholder joints a
/// GPU-posed rig binds, and draws the geometry collapsed.
///
/// So the copy carries this marker instead, and the crate that owns the skinning
/// (`sl-viewer-world-avatar`) copies the pose binding across for as long as the
/// source has one. It lives here because neither the water layer nor the build
/// tool may depend on the avatar layer, and none of them may reach the other two.
///
/// The copy still needs the `SkinnedMesh` itself — that is what makes Bevy
/// allocate it a palette at all, and its absence is a wgpu validation error, not
/// an artifact.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkinPoseTwin {
    /// The skinned entity whose pose this one must be drawn at.
    pub source: Entity,
}

/// The drivable state of the [`ViewerCamera`], shared by every mode.
///
/// Third person reads the orbit fields (`azimuth` /
/// `elevation` / `distance`); mouselook and
/// flycam read the aim fields (`yaw` / `pitch` /
/// `roll`). The flycam's *position* is the entity `Transform`'s
/// translation, not stored here — so a debug focus system that writes the
/// transform moves the flycam directly. The smoothed pose eases toward the mode's
/// desired pose so mode changes glide.
#[derive(Component, Debug, Clone)]
pub struct CameraRig {
    /// Third-person horizontal orbit offset from dead-behind the avatar, radians
    /// (`0` = rear view). Only a mouse-drag moves it — never the arrow keys.
    pub azimuth: f32,
    /// Third-person vertical orbit angle, radians (positive looks down onto the
    /// avatar). Seeded from `CAMERA_OFFSET`'s elevation.
    pub elevation: f32,
    /// Third-person camera distance from the focus, metres, clamped between
    /// [`MOUSELOOK_CROSS_DISTANCE`] and the tunable maximum
    /// (`CameraTuning::max_distance`, default `MAX_DISTANCE`).
    pub distance: f32,
    /// Mouselook / flycam yaw about Bevy up (`+Y`), radians.
    pub yaw: f32,
    /// Mouselook / flycam pitch about the camera's local right, radians, clamped
    /// to `±MAX_PITCH`.
    pub pitch: f32,
    /// Flycam roll about the camera's local forward, radians (only `CameraSpin`
    /// roll moves it).
    pub roll: f32,
    /// The world-space offset from a `FocusTarget::Point` focus to the camera
    /// eye, used only in focus-on-object. Captured at alt-click so the camera does
    /// not jump, and orbited / zoomed since. Unlike the avatar rear-view orbit
    /// (which follows the heading) this is fixed in the world, as the reference's
    /// object focus is.
    pub point_offset: Vec3,
    /// The last rendered eye position, eased toward the mode's desired eye.
    pub smoothed_eye: Vec3,
    /// The last rendered look-at point, eased toward the mode's desired focus.
    pub smoothed_focus: Vec3,
    /// Whether the smoothed pose has been seeded yet (so the first valid frame
    /// snaps rather than gliding in from an arbitrary origin).
    pub seeded: bool,
}

impl Default for CameraRig {
    /// The reference rear-view orbit: dead behind, tilted and distanced by
    /// `CAMERA_OFFSET`.
    fn default() -> Self {
        let horizontal =
            (CAMERA_OFFSET.x * CAMERA_OFFSET.x + CAMERA_OFFSET.y * CAMERA_OFFSET.y).sqrt();
        Self {
            azimuth: 0.0,
            elevation: CAMERA_OFFSET.z.atan2(horizontal),
            distance: CAMERA_OFFSET.length(),
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            point_offset: Vec3::ZERO,
            smoothed_eye: Vec3::ZERO,
            smoothed_focus: Vec3::ZERO,
            seeded: false,
        }
    }
}

impl CameraRig {
    /// Reset the third-person orbit to the default rear view — the reference's
    /// `Escape` "reset camera". Leaves the aim / smoothing alone (the caller snaps
    /// via the mode change).
    pub fn reset_orbit(&mut self) {
        let default = Self::default();
        self.azimuth = default.azimuth;
        self.elevation = default.elevation;
        self.distance = default.distance;
    }

    /// Seed the third-person orbit from the debug framing environment variables,
    /// so the offline screenshot harness can frame the avatar from a chosen angle
    /// (the same `SL_VIEWER_CAMERA_*` knobs the old login-snap read). A no-op when
    /// none are set — the default rear view stands.
    ///
    /// `SL_VIEWER_CAMERA_ORBIT_DEG` swings the azimuth (90 = a side view),
    /// `_ELEV_DEG` the elevation (positive looks down), `_DISTANCE` the zoom.
    pub fn seed_orbit_from_env(&mut self) {
        let env_f32 = |key: &str| -> Option<f32> {
            std::env::var(key).ok().and_then(|value| value.parse().ok())
        };
        if let Some(orbit) = env_f32("SL_VIEWER_CAMERA_ORBIT_DEG") {
            self.azimuth = orbit.to_radians();
        }
        if let Some(elevation) = env_f32("SL_VIEWER_CAMERA_ELEV_DEG") {
            self.elevation = elevation.to_radians().clamp(-MAX_PITCH, MAX_PITCH);
        }
        if let Some(distance) = env_f32("SL_VIEWER_CAMERA_DISTANCE") {
            self.distance = distance.clamp(MOUSELOOK_CROSS_DISTANCE, MAX_DISTANCE);
        }
    }

    /// Reset the smoothing so the next frame snaps to the mode's desired pose
    /// rather than gliding — called after a region-origin shift
    /// (`crate::terrain::recenter_terrain`) so the eased pose does not drift
    /// across the 256 m rebase (the reference's sideways-after-crossing bug).
    pub const fn resnap(&mut self) {
        self.seeded = false;
    }

    /// Aim the flycam / mouselook along `direction` (Bevy Y-up space) by setting
    /// the yaw/pitch, so the aim survives the next frame's re-derivation. A zero
    /// direction is ignored. Yaw is measured so `-Z` gives yaw `0`; pitch is the
    /// elevation, clamped to `±MAX_PITCH`.
    pub fn aim_along(&mut self, direction: Vec3) {
        let dir = direction.normalize_or_zero();
        if dir == Vec3::ZERO {
            return;
        }
        self.yaw = (-dir.x).atan2(-dir.z);
        self.pitch = dir.y.asin().clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// The rotation the rig's current yaw/pitch aims along (roll excluded) — the
    /// same reconstruction mouselook uses, so `rotation * NEG_Z` is the aim
    /// direction. A fixed flycam bakes this into its entity transform at spawn:
    /// `drive_flycam` integrates input deltas onto the transform and never reads
    /// the rig, so without this the transform keeps its identity (SL-north)
    /// orientation and `--camera-look-at` has no effect.
    #[must_use]
    pub fn aim_quat(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }

    /// Place the focus-on-point eye offset directly (world space, eye = point +
    /// offset). The media-controls **Zoom** (`crate::media_controls`) uses this
    /// to park the camera squarely in front of a media face — the counterpart of
    /// `focus_on_object` capturing the offset at an alt-click.
    pub const fn set_point_offset(&mut self, offset: Vec3) {
        self.point_offset = offset;
    }
}

/// The authoritative kinematic motion of a full-object avatar (`pcode` 47) as of
/// its last `ObjectUpdate`, attached to the avatar's anchor entity by
/// `apply_object`(crate::avatars) and change-detected: a fresh insert on every
/// update reseeds the interpolation. Its presence marks the avatar anchors
/// `drive_avatar_motion` dead-reckons between updates. Coarse (minimap-only)
/// avatars carry no velocity and so get no [`AvatarMotion`].
#[derive(Debug, Component, Clone)]
pub struct AvatarMotion {
    /// Region-local position (metres, Second Life Z-up frame).
    pub position: Vector,
    /// Linear velocity (metres/second).
    pub velocity: Vector,
    /// Linear acceleration (metres/second²).
    pub acceleration: Vector,
    /// Orientation (a Second Life unit quaternion).
    pub rotation: Rotation,
    /// Angular velocity (rotation axis scaled by radians/second).
    pub angular_velocity: Vector,
    /// The region this avatar lives in, for the region-edge / neighbour lookups.
    pub region_handle: RegionHandle,
    /// The avatar's bounding-box height (object scale Z), for the ground floor.
    pub height: f32,
    /// Whether the anchor applies the object's orientation (a rigged body root) or
    /// stays upright (a placeholder sphere, which does not visibly rotate).
    pub apply_rotation: bool,
    /// The **collision (foot) plane** the simulator reports for this avatar: the
    /// surface its physics capsule is resting on, as the plane equation
    /// `[nx, ny, nz, w]` (a unit normal and a distance) in the region-local
    /// Second Life frame — `n · p = w`. `None` when the update carried no plane
    /// (a placeholder sphere, or a compressed update). This is the simulator's
    /// authoritative ground under the avatar — it already accounts for prims the
    /// avatar stands on, unlike a terrain-only lookup — and is what
    /// `crate::ground` resolves the foot-IK ground from, exactly as the
    /// reference viewer's `getGround` / `mFootPlane` do.
    collision_plane: Option<[f32; 4]>,
    /// Whether the update carried [`FLAGS_SERVER_AUTOPILOT`]: the simulator is
    /// steering this agent, so its reported facing is authoritative even for the
    /// own avatar, whose facing is otherwise the viewer's held heading.
    server_autopilot: bool,
}

impl AvatarMotion {
    /// The avatar's current heading (yaw about the Second Life up axis, radians),
    /// extracted from its reported orientation. The viewer's movement controls
    /// (`crate::movement`) seed the walk heading from this so the first step does
    /// not snap the avatar to an arbitrary facing.
    #[must_use]
    pub fn yaw(&self) -> f32 {
        let Rotation { x, y, z, s } = &self.rotation;
        // Yaw about Z from a unit quaternion (`atan2(2(sz + xy), 1 - 2(y² + z²))`).
        let siny_cosp = 2.0 * (s * z + x * y);
        let cosy_cosp = 1.0 - 2.0 * (y * y + z * z);
        siny_cosp.atan2(cosy_cosp)
    }

    /// The avatar's vertical (Second Life Z-up) velocity component (metres/second):
    /// positive climbing, negative descending / falling. The client-side locomotion
    /// fallback (`crate::locomotion`) reads this to pick the ascend / descend /
    /// fall states — the only states with no advertised control-flag intent.
    #[must_use]
    pub const fn vertical_speed(&self) -> f32 {
        self.velocity.z
    }

    /// The region this avatar is in — the frame the terrain queries and its reported
    /// position are expressed in.
    #[must_use]
    pub const fn region(&self) -> RegionHandle {
        self.region_handle
    }

    /// The avatar's reported linear velocity (Second Life Z-up metres/second, region
    /// frame). The walk-adjust foot-slip servo (P31.14) matches the walk animation's
    /// playback speed to this.
    #[must_use]
    pub const fn sl_velocity(&self) -> Vec3 {
        Vec3::new(self.velocity.x, self.velocity.y, self.velocity.z)
    }

    /// The avatar's reported angular velocity (rotation axis scaled by radians/second,
    /// region frame). The fly-adjust bank (P31.14) rolls the pelvis into a turn by its
    /// Z component, exactly as the reference's `LLFlyAdjustMotion` does.
    #[must_use]
    pub const fn sl_angular_velocity(&self) -> Vec3 {
        Vec3::new(
            self.angular_velocity.x,
            self.angular_velocity.y,
            self.angular_velocity.z,
        )
    }

    /// Build the authoritative motion from an avatar's object update. `apply_rotation`
    /// is `true` for a rigged body root (whose anchor carries the object rotation)
    /// and `false` for a placeholder sphere.
    #[must_use]
    pub fn from_object(object: &Object, apply_rotation: bool) -> Self {
        Self {
            position: object.motion.position.clone(),
            velocity: object.motion.velocity.clone(),
            acceleration: object.motion.acceleration.clone(),
            rotation: object.motion.rotation.clone(),
            angular_velocity: object.motion.angular_velocity.clone(),
            region_handle: object.region_handle,
            height: object.scale.z,
            apply_rotation,
            collision_plane: object.motion.collision_plane,
            server_autopilot: object.update_flags & FLAGS_SERVER_AUTOPILOT != 0,
        }
    }

    /// The simulator's collision (foot) plane for this avatar (region-local
    /// `[nx, ny, nz, w]`), or `None` when the last update carried none. The ground
    /// probe (`crate::ground`) resolves the foot-IK ground from it.
    #[must_use]
    pub const fn collision_plane(&self) -> Option<[f32; 4]> {
        self.collision_plane
    }

    /// Whether the simulator is steering this agent ([`FLAGS_SERVER_AUTOPILOT`]),
    /// which makes the facing this update reports the one the own avatar adopts.
    #[must_use]
    pub const fn is_server_autopiloted(&self) -> bool {
        self.server_autopilot
    }
}

/// What kind of thing a blacklist entry names — the reference's `LLAssetType`,
/// narrowed to the kinds a viewer can actually refuse.
///
/// The two **in-world** kinds are what the derender menus produce and what the
/// scene mirror gates on. The three **asset** kinds are refused at their own
/// point of use instead — a blacklisted sound is never played, a blacklisted
/// animation never runs, a blacklisted texture is never fetched — which is
/// exactly where the reference refuses them. Their producers are the explorer
/// floaters (the sound explorer feeds `Sound`, the animation explorer
/// `Animation`); until those land, an asset entry comes from the per-account
/// file itself, which is also how the reference's distributed blacklist data
/// (`fsdata`) feeds textures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DerenderKind {
    /// An in-world object (the reference's `AT_OBJECT`).
    Object,
    /// An avatar (the reference's `AT_PERSON`).
    Resident,
    /// A sound asset, never played (`AT_SOUND`).
    Sound,
    /// An animation asset, never run (`AT_ANIMATION`).
    Animation,
    /// A texture asset, never fetched (`AT_TEXTURE`).
    Texture,
}

impl DerenderKind {
    /// The Fluent key naming this kind in the blacklist's Type column.
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Object => "derender-type-object",
            Self::Resident => "derender-type-resident",
            Self::Sound => "derender-type-sound",
            Self::Animation => "derender-type-animation",
            Self::Texture => "derender-type-texture",
        }
    }

    /// A stable sort rank, so the Type column orders deterministically.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Object => 0,
            Self::Resident => 1,
            Self::Sound => 2,
            Self::Animation => 3,
            Self::Texture => 4,
        }
    }

    /// Whether this kind names something the **scene mirror** suppresses (as
    /// opposed to an asset refused at its point of use).
    #[must_use]
    pub const fn is_in_world(self) -> bool {
        matches!(self, Self::Object | Self::Resident)
    }
}

/// A component marking an object entity as a **particle source**, carrying the
/// decoded `LLPartSysData` particle-system parameters in Second Life semantics —
/// ready for P30.2 to drive a CPU particle simulation and render its particles as
/// camera-facing billboards.
///
/// Attached to (and refreshed / cleared on) each object entity by
/// `apply_object`(crate::objects) as its updates arrive. Only a *live* system
/// (non-zero CRC) is carried; see `particles_from_object`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct ObjectParticleSystem {
    /// The decoded particle system: the source parameters (pattern, burst / age
    /// timing, emission angles / radius / speed, angular velocity, acceleration,
    /// texture, target) plus the template particle parameters it emits
    /// (per-particle age, start / end colour and scale, glow, blend).
    pub system: ParticleSystem,
}

/// The top of the pixel-area priority range: [`Priority::from_pixel_area`]
/// saturates here (`FULL_RESOLUTION_PIXEL_AREA` = `2048 * 2048`). Boost
/// priorities sit *strictly above* this, so a boosted asset always outranks even
/// the closest, largest prim face rather than merely tying with it on a region
/// dense with max-pixel-area content — mirroring how the reference viewer's
/// `BOOST_*` levels force a texture ahead of ordinary pixel-area-ranked content.
pub const PIXEL_AREA_CAP: u32 = 2048 * 2048;

/// The fixed boost priority for a region's four terrain detail textures
/// (`LLGLTexture::BOOST_TERRAIN`): one step into the boost band, so the ground is
/// not starved behind nearer prims (the terrain textures are few and always
/// under the camera, and the on-screen face pass does not rank them — terrain is
/// a custom material, not a tessellated prim face).
pub const TERRAIN_BOOST_PRIORITY: Priority = Priority::new(PIXEL_AREA_CAP + 1);

/// The fixed boost priority for the sky's referenced textures — the rainbow /
/// halo (and, later, sun / moon / cloud / bloom) maps the atmospheric sky dome
/// samples (`LLGLTexture::BOOST_HIGH`). In the boost band so a sky texture
/// resolves ahead of ordinary pixel-area-ranked scene faces (the sky is drawn
/// behind everything and, like terrain, is a custom material the on-screen face
/// pass cannot rank), one step above the avatar boost.
pub const SKY_BOOST_PRIORITY: Priority = Priority::new(PIXEL_AREA_CAP + 3);

/// The fixed boost priority for an avatar's textures and server-side bakes
/// (`LLGLTexture::BOOST_AVATAR` / `BOOST_AVATAR_BAKED`): above terrain, so the
/// avatars the camera is looking at resolve first even on a region dense with
/// max-pixel-area prims. The avatar is a skinned mesh, not a tessellated prim
/// face, so the on-screen face pass does not rank it — this boost is what keeps
/// its bakes ahead of the surrounding scene.
pub const AVATAR_BOOST_PRIORITY: Priority = Priority::new(PIXEL_AREA_CAP + 2);

/// A GPU-posed pose slot's identity (§5): either a rigged **avatar** keyed by
/// its wearer agent, or an **animesh** control avatar keyed by its animated-
/// object root. The registry, the feed and pass D's staging all key their
/// per-slot state on this, so avatars and animesh share the one passes-A–D
/// pipeline and one dense slot space.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PoseSlotKey {
    /// A rigged avatar, keyed by its wearer agent.
    Avatar(AgentKey),
    /// An animesh control avatar, keyed by its animated-object root
    /// ([`ObjectKey`]) — it has no wearer agent.
    Animesh(ObjectKey),
    /// A synthetic **debug-crowd copy** of the local avatar
    /// (`SL_VIEWER_CROWD`, `gpu_avatars::crowd`), keyed by its crowd
    /// index. It carries no real agent: it reuses the local avatar's shape,
    /// clips and body submesh handles but stages its own slot, so passes A–D
    /// run at crowd scale for perf measurement. Never allocated when the env is
    /// unset (the crowd resource is empty), so a normal run never sees it.
    Crowd(u32),
}

/// One blacklist entry: what was derendered, where and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerenderEntry {
    /// The derendered thing's persistent id (an object's full id, an avatar's
    /// agent id).
    pub id: Uuid,
    /// Its name, as the surface that derendered it knew it (may be empty when
    /// the object-properties reply had not landed yet).
    pub name: String,
    /// The region it was derendered in (empty when unknown).
    pub region: String,
    /// What kind of thing it is.
    pub kind: DerenderKind,
    /// Whether it survives a teleport and a relog (the "Blacklist" slice) or is
    /// a session-only "Temporary" derender.
    pub permanent: bool,
    /// When it was added, as Unix epoch seconds (stored as a plain integer so
    /// the file needs no date parser).
    pub added_epoch_secs: i64,
}

/// Why a region-scoped id is suppressed — which release frees it again.
///
/// Two sources share one suppression index (and therefore one ingest gate, one
/// transitive parent walk, one purge and one re-fetch): the **blacklist**, keyed
/// by the entry's id, and the **friends-only filter**, keyed by the non-friend
/// agent it hides. Keeping the source on each entry is what lets a release be
/// exact — un-blacklisting one object, or befriending one avatar, frees that
/// subtree and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenBy {
    /// The blacklist entry with this id ([`DerenderList::remove`] frees it).
    Blacklist(Uuid),
    /// The friends-only filter, hiding this non-friend agent (turning the filter
    /// off — or befriending them — frees it).
    FriendsOnly(Uuid),
}

impl HiddenBy {
    /// The persistent id at the root of this suppression — a blacklist entry's
    /// id, or the hidden agent's.
    #[must_use]
    pub const fn id(self) -> Uuid {
        match self {
            Self::Blacklist(id) | Self::FriendsOnly(id) => id,
        }
    }
}

/// The viewer's derender / blacklist state, and the friends-only filter that
/// shares its suppression machinery.
#[derive(Resource, Debug, Default)]
pub struct DerenderList {
    /// The entries, newest last.
    pub entries: Vec<DerenderEntry>,
    /// The blacklisted ids and what each is blacklisted **as**, derived from
    /// [`Self::entries`] — the hot-path index every check goes through. Keyed by
    /// id alone: an id is one thing, so a second entry for it would be a
    /// contradiction, and the kind rides along so a sound check never matches an
    /// object entry.
    ids: HashMap<Uuid, DerenderKind>,
    /// The region-scoped ids currently suppressed, each mapped to **what hides
    /// it**: an entry's own object maps to its own source, a linkset child or
    /// attachment to its root's. Keeping the source is what lets a single
    /// release free exactly its own subtree (see `Self::release`).
    /// Session-derived, never persisted.
    pub hidden_scoped: HashMap<ScopedObjectId, HiddenBy>,
    /// Suppressions whose scene entities still need despawning, by source: a
    /// fresh blacklist entry, or an avatar the friends-only filter just started
    /// hiding.
    pub pending_ids: Vec<HiddenBy>,
    /// Scoped ids whose scene entities still need despawning (an object that
    /// was already tracked when its parent became hidden).
    pub pending_scoped: Vec<ScopedObjectId>,
    /// Scoped ids just **released** from suppression, to be re-fetched from the
    /// simulator so an un-derendered object comes back at once
    /// (`refetch_released_objects`).
    pub pending_refetch: Vec<ScopedObjectId>,
    /// Bumped on every change to [`Self::entries`], so the floater rebuilds
    /// exactly when the list moved.
    revision: u64,
    /// The per-account store path, resolved at login; `None` until then (and
    /// when the platform has no per-avatar directory, disabling persistence).
    pub path: Option<PathBuf>,
    /// Whether the on-disk list has been read — a once-per-session load.
    pub loaded: bool,
    /// Whether the **permanent** entries changed since the last flush.
    pub dirty: bool,
    /// Whether the **friends-only** filter is on (`viewer-render-friends-only`,
    /// the reference's `FSRenderFriendsOnly`): while it is, every avatar that is
    /// not a friend and not the agent itself is suppressed exactly as a
    /// derendered one is.
    pub friends_only: bool,
    /// The agent's own id, which the filter never hides.
    pub own_agent: Option<Uuid>,
    /// The friends the filter spares, mirrored from
    /// `sl_viewer_social::FriendsModel` so the per-object gate stays
    /// one hash lookup.
    pub friends: HashSet<Uuid>,
}

impl DerenderList {
    /// Whether `id` is blacklisted **as** `kind` — the query each point of use
    /// runs (a sound before playing it, an animation before running it, a
    /// texture before fetching it).
    #[must_use]
    pub fn blacklists(&self, id: Uuid, kind: DerenderKind) -> bool {
        self.ids.get(&id) == Some(&kind)
    }

    /// Whether `id` names an in-world thing this viewer must not draw — a
    /// blacklisted object / avatar, or an avatar the friends-only filter hides.
    /// The hot-path query the scene mirror runs per streamed object.
    #[must_use]
    pub fn hides_in_world(&self, id: Uuid) -> bool {
        self.blacklists_in_world(id) || self.friends_only_hides(id)
    }

    /// Whether `id` is on the **blacklist** as an in-world kind (as opposed to
    /// being hidden by the friends-only filter).
    #[must_use]
    pub fn blacklists_in_world(&self, id: Uuid) -> bool {
        self.ids.get(&id).is_some_and(|kind| kind.is_in_world())
    }

    /// Whether the friends-only filter hides the avatar `agent`: the filter is
    /// on, and they are neither the agent itself nor a friend. Animesh
    /// ("control") avatars are exempt for free — they are ordinary mesh objects
    /// on the wire, never `pcode` 47, so this gate never sees them, which is the
    /// reference's `!avatar->isControlAvatar()` by construction.
    #[must_use]
    pub fn friends_only_hides(&self, agent: Uuid) -> bool {
        self.friends_only && self.own_agent != Some(agent) && !self.friends.contains(&agent)
    }

    /// Every blacklisted id of `kind` — how a consumer that cannot consult the
    /// list per item (the texture store, whose fetch gate is not a Bevy system)
    /// mirrors the set it needs.
    #[must_use]
    pub fn ids_of_kind(&self, kind: DerenderKind) -> HashSet<Uuid> {
        self.ids
            .iter()
            .filter(|(_id, held)| **held == kind)
            .map(|(id, _held)| *id)
            .collect()
    }

    /// Whether the object with region-scoped id `scoped` must not be mirrored
    /// into the scene: it is blacklisted itself, or it hangs off something that
    /// is (a linkset child, an attachment). Maintained by
    /// `index_derendered_objects`.
    #[must_use]
    pub fn is_suppressed(&self, scoped: ScopedObjectId) -> bool {
        self.hidden_scoped.contains_key(&scoped)
    }

    /// What suppresses `scoped`, if anything — the source an inherited
    /// suppression is inherited from.
    #[must_use]
    pub fn suppressing_root(&self, scoped: ScopedObjectId) -> Option<HiddenBy> {
        self.hidden_scoped.get(&scoped).copied()
    }

    /// Record every id in `removed` as suppressed by the blacklisted `root`.
    ///
    /// The scene purge calls this with what it despawned, because those ids are
    /// often the *only* record of them: the simulator streams a static object
    /// once, so an object derendered long after it was streamed never produces
    /// another update for `index_derendered_objects` to learn from — and
    /// without the record, un-derendering it would have nothing to re-fetch.
    pub fn note_hidden(
        &mut self,
        removed: impl IntoIterator<Item = ScopedObjectId>,
        root: HiddenBy,
    ) {
        for scoped in removed {
            let _prior = self.hidden_scoped.insert(scoped, root);
        }
    }

    /// The whole list, in insertion order.
    #[must_use]
    pub fn entries(&self) -> &[DerenderEntry] {
        &self.entries
    }

    /// The list revision — a view stores the value it last built at and rebuilds
    /// when it advances.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Add (or replace) an entry, marking the scene for a purge of its id.
    /// Re-derendering an id already listed **upgrades** it: a temporary entry
    /// that is blacklisted becomes permanent, never the other way round, which
    /// is what the reference's `addNewItemToBlacklist` overwrite amounts to for
    /// the only two paths that reach it.
    pub fn add(&mut self, entry: DerenderEntry) {
        if let Some(existing) = self.entries.iter_mut().find(|held| held.id == entry.id) {
            let upgraded = entry.permanent && !existing.permanent;
            existing.permanent |= entry.permanent;
            if existing.name.is_empty() {
                existing.name.clone_from(&entry.name);
            }
            if upgraded {
                self.dirty = true;
                self.revision = self.revision.wrapping_add(1);
            }
            return;
        }
        self.dirty |= entry.permanent;
        self.pending_ids.push(HiddenBy::Blacklist(entry.id));
        self.entries.push(entry);
        self.reindex();
    }

    /// Drop the entry for `id`, if held, releasing everything it suppressed and
    /// queueing those objects for a re-fetch (see `Self::release`).
    pub fn remove(&mut self, id: Uuid) {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        if self.entries.len() == before {
            return;
        }
        self.dirty = true;
        self.reindex();
        // Release exactly what this entry was suppressing — its own object and
        // everything that inherited the suppression from it — and nothing else:
        // another blacklisted root's children (and anything the friends-only
        // filter hides) must stay hidden.
        self.release(|root| root == HiddenBy::Blacklist(id));
    }

    /// Drop every temporary entry (a teleport, or the floater's Clear temporary).
    pub fn clear_temporary(&mut self) {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.permanent);
        if self.entries.len() == before {
            return;
        }
        self.reindex();
        // Every suppression whose blacklist entry just left the list is
        // released; the permanent entries — and the friends-only filter — keep
        // theirs.
        let live: HashSet<Uuid> = self.ids.keys().copied().collect();
        self.release(|root| match root {
            HiddenBy::Blacklist(id) => !live.contains(&id),
            HiddenBy::FriendsOnly(_agent) => false,
        });
    }

    /// Drop every suppression whose root `released` accepts, and queue the
    /// freed region-scoped ids for a re-fetch.
    ///
    /// The re-fetch is what makes "Re-render" mean it: the simulator streams an
    /// object once, and the viewer dropped every update for it while it was
    /// suppressed, so simply forgetting the entry would leave the object absent
    /// until the region streamed it again (a teleport away and back — which is
    /// all the reference does). Because the index kept the object's *region-local*
    /// id the whole time, we can instead ask for it back right now
    /// (`RequestMultipleObjects`, a full cache miss).
    fn release(&mut self, released: impl Fn(HiddenBy) -> bool) {
        let freed: Vec<ScopedObjectId> = self
            .hidden_scoped
            .iter()
            .filter(|(_scoped, root)| released(**root))
            .map(|(scoped, _root)| *scoped)
            .collect();
        for scoped in &freed {
            let _dropped = self.hidden_scoped.remove(scoped);
        }
        self.pending_refetch.extend(freed);
    }

    /// Re-apply the friends-only filter after its inputs moved (the toggle
    /// flipped, the friends list changed, or the own agent became known): free
    /// everyone it no longer hides — queuing their re-fetch, so they come back
    /// without a relog — and queue a purge for every avatar it now does.
    ///
    /// `known` is the agents this viewer currently tracks; only they can have
    /// anything in the scene to purge, and anyone streaming in later is caught
    /// by the ingest gate instead.
    pub fn resync_friends_only(&mut self, known: &[Uuid]) {
        let spared: HashSet<Uuid> = self
            .hidden_scoped
            .values()
            .filter_map(|hidden| match hidden {
                HiddenBy::FriendsOnly(agent) => Some(*agent),
                HiddenBy::Blacklist(_id) => None,
            })
            .filter(|agent| !self.friends_only_hides(*agent))
            .collect();
        if !spared.is_empty() {
            self.release(
                |root| matches!(root, HiddenBy::FriendsOnly(agent) if spared.contains(&agent)),
            );
        }
        for agent in known {
            if self.friends_only_hides(*agent) {
                self.pending_ids.push(HiddenBy::FriendsOnly(*agent));
            }
        }
    }

    /// Rebuild the derived id index and bump the revision.
    fn reindex(&mut self) {
        self.ids = self
            .entries
            .iter()
            .map(|entry| (entry.id, entry.kind))
            .collect();
        self.revision = self.revision.wrapping_add(1);
    }
}

/// The agent's own region-local position, folded from its own-avatar object
/// updates (`SlSessionEvent::ObjectAdded` / `SlSessionEvent::ObjectUpdated`
/// whose `full_id` is the agent id). `None` before the own avatar arrives. This
/// is the region-local `⟨x, y, z⟩` the location read-out shows, the same source
/// the reference viewer's `LLAgentUI::buildLocationString` reads
/// (`gAgent.getPositionAgent`).
#[derive(Resource, Debug, Clone, Default)]
pub struct AgentRegionPosition {
    /// The region-local position in metres, or `None` before the own avatar
    /// object arrives.
    pub position: Option<Vector>,
}

impl AgentRegionPosition {
    /// The agent's region-local position in metres, or `None` before the own
    /// avatar object arrives. Read by the About Land Options tab to set a
    /// parcel's landing point to where the agent stands.
    #[must_use]
    pub const fn position(&self) -> Option<&Vector> {
        self.position.as_ref()
    }
}
