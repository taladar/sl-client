//! `scene.json`: the structured description of what the viewer was showing when
//! it took its frames.
//!
//! Pixels say two viewers disagree. They do not say why, and the four commonest
//! causes look identical in an image: a prim in the wrong place, a texture that
//! resolved to a different asset id, a mesh stuck at a coarser LOD, and a
//! material that never arrived. So each viewer writes this document beside its
//! frames, and the comparison that names the cause is a diff of two of them.
//!
//! # The schema is Firestorm's schema
//!
//! The patched Firestorm writes the same document (`fstestscenedump.cpp`), and
//! the two are compared field by field. Every key here is spelled the way it is
//! spelled there — including [`sl_client_bevy::pcode::describe`], which is the
//! reference's own `pCodeToString` — because a field named differently on the
//! two sides reads as a divergence in every object of every scene, and a reader
//! learns to ignore the diff. `schema_version` is `1`; a mismatch is an error at
//! the comparison rather than a confusing diff.
//!
//! The units are the contract: **Second Life region-local metres, Z up**,
//! whatever this viewer stores internally. That is why the numbers here go
//! through the inverse of the Second Life → Bevy basis change on the way out.
//!
//! # It reports what was drawn, not what arrived
//!
//! Every position and rotation is read back from the entity's
//! [`GlobalTransform`] — the pose the frame was actually rendered from — rather
//! than from the object update that produced it.
//!
//! That is the whole point. A dump built from the received wire values would
//! agree with Firestorm's *by construction*, because both viewers received the
//! same bytes; it would agree just as loudly when our own transform maths put
//! the prim somewhere else, which is exactly the bug a cross-check exists to
//! find. Reading the transform back means the dump can disagree with the wire,
//! and a dump that can disagree is the only kind worth diffing.
//!
//! The exception is **scale**, which no entity carries: an object's scale rides
//! its geometry holder, so the wire value is reported as the object's size.
//!
//! # An avatar, and what it wears, is placed as the reference places it
//!
//! Two kinds of thing the reference's document does not report the drawn pose
//! of, and both would otherwise read as a divergence on every avatar in every
//! scene.
//!
//! **An avatar.** The wire position of one is the centre of its physics capsule,
//! and both viewers lower the *skeleton* from there so the feet meet the ground
//! (`body_root_transform`'s `root_drop` here, `LLVOAvatar::updateCharacter`'s
//! `root_pos.z -= …` there). The reference's dump reports the **object's**
//! position, undropped, so this one does too.
//!
//! **An attachment.** `LLViewerObject::getPositionRegion` composes a child
//! against its parent **object** — for an attachment that is the avatar — while
//! the thing is *drawn* parented to a skeleton joint
//! (`LLViewerJointAttachment::setupDrawable`). So the reference reports the
//! wearer's position plus the wire offset, and the drawn place is a joint's
//! height further up. The first live pair caught exactly that on the fixture
//! NPC's skull box: 27.06 m here against 26.20 m there, both viewers drawing it
//! on the skull, the two numbers answering two different questions.
//!
//! A worn object is therefore composed here the way the reference composes one,
//! onto its wearer's own reported position — including the reference's quirk
//! that a *linked child* of an attachment is placed against its parent's
//! **local** rotation, so the wearer's turn is applied once rather than twice —
//! and a HUD attachment, drawn in screen space where a region position means
//! nothing at all, comes out meaning what the reference means by it.
//!
//! Nothing is given up by that. What was drawn is reported beside it, as
//! `drawn_position` / `drawn_rotation` — keys this viewer emits and the
//! reference does not, like `day_position` — so an attachment on the wrong
//! joint, or one never parented to the skeleton at all, is still a difference a
//! reader can see, on the side of the pair that can see it.
//!
//! # What an avatar is doing, not just where it is
//!
//! Each avatar lists the animations it is playing, in the order the viewer
//! applies them — most recently activated first, which is the order the
//! reference's motion controller keeps its active list in and the order our own
//! per-joint blend breaks a priority tie by. Order is half of what decides which
//! motion owns a joint; `priority` is the other half, and both are reported.
//!
//! Each entry says where that motion's clock has reached. `time` is seconds
//! since the viewer started playing it and is *not* comparable between two
//! viewers — they start at different moments — while `loop_time`, the same
//! number wrapped into the motion's own duration, is: it is the frame of the
//! animation the body was drawn at. A pair of frames photographed 0.5 s apart
//! out of a 2 s loop can differ wildly and mean nothing, and this section is
//! what says so.
//!
//! The two viewers list different *sets* here, deliberately. The reference
//! starts default motions on every avatar — head rotation, eye, body noise,
//! breathing, physics, hand pose, pelvis fix — which this viewer implements as
//! adjusters rather than as motions, so they appear only on that side. What is
//! worth comparing is the animations the **simulator** named: whether both
//! viewers play them, and where each one's clock has got to.
//!
//! # Which ids two viewers can agree on
//!
//! A comparison keys objects by id, and **not every id in a scene is a grid
//! id**. Three kinds are minted locally, differ between two viewers of one
//! scene — and between two runs of one viewer — and must never be matched or
//! reported as missing:
//!
//! - **Viewer-side scene objects.** The reference models its terrain patches,
//!   sky, water and clouds as objects with `local_id` 0, an `app-…` class and a
//!   freshly minted id (256 terrain patches alone). This viewer does not model
//!   them as objects at all, so they appear only on that side; they are scenery
//!   the viewer built, not content the grid sent.
//! - **Control avatars.** An animesh rides a headless avatar with no grid
//!   identity; the reference gives it a local UUID. This dump instead reports
//!   one by **the object it rides**, which is the only thing about it two
//!   viewers can agree on — so a control avatar is matched by
//!   `is_control_avatar` plus the animesh object, never by id.
//! - **Baked avatar textures.** A bake's id is minted by whoever baked it — a
//!   client bake on upload, a server bake per bake run — so two viewers can hold
//!   different texture ids for the *same* appearance, and the same viewer can on
//!   the next run. A baked slot's id is evidence that a bake arrived, never
//!   evidence that two viewers rendered the same one.
//!
//! Everything else here is a grid id: object keys, mesh and sculpt assets, and
//! the texture ids of ordinary (non-baked) faces.
//!
//! # When it is written
//!
//! Once, at the end of a capture run, from a system in [`Last`] — after
//! transform propagation, so the poses are this frame's rather than last
//! frame's. The screenshot harness raises [`SceneDumpRequest`] at the same
//! moment it writes its status file, so the dump describes the scene the last
//! frame was taken from.

use std::collections::HashMap;
use std::path::PathBuf;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde::Serialize;
use sl_client_bevy::{
    AgentKey, MAX_FACES, RegionHandle, Rotation, ScopedObjectId, SlIdentity, SlRegionIdentity,
    TextureFace, decode_texture_entry, pcode,
};
use sl_viewer_world_api::{
    AvatarState, MAX_PARENT_WALK, ObjectCategory, ObjectState, SceneObject, TrackedObject,
    ViewerCamera,
};
use sl_viewer_world_avatar::animations::{AnimationManager, AnimationPlayback, PlayingAnimation};
use sl_viewer_world_avatar::animesh::ControlAvatarState;
use sl_viewer_world_avatar::avatars::AvatarBodyPart;
use sl_viewer_world_scene::environment::EnvironmentState;

use crate::coords::{
    bevy_to_sl_vec, metres_to_f32, region_offset_bevy, sl_rotation_to_quat, sl_to_bevy_rotation,
};
use crate::objects::ObjectSlMotion;
use crate::settings::ViewerSettings;

/// The schema both viewers write. Bumped only when a field changes meaning; a
/// comparison refuses two dumps that disagree about it.
pub const SCHEMA_VERSION: u32 = 1;

/// What this viewer calls itself in a dump's `context`.
pub const VIEWER_NAME: &str = "sl-client";

/// The build identity a dump names itself with, handed in by the binary (which
/// is what knows its channel, its version and which grid it was told to log
/// into).
#[derive(Debug, Clone, Resource)]
pub struct DumpIdentity {
    /// The viewer channel reported to the grid.
    pub channel: String,
    /// The viewer version reported to the grid.
    pub version: String,
    /// The grid, as the operator named it — a login URI's `host:port` for a
    /// local or fake grid, which is what Firestorm's grid manager calls it too.
    pub grid: String,
}

/// Where a scene dump is written, and whether one has been asked for.
///
/// A resource rather than an argument because the request comes from the
/// screenshot harness — a different system, one frame earlier — and the write
/// must happen after transform propagation.
#[derive(Debug, Resource)]
pub struct SceneDumpRequest {
    /// Where the dump goes.
    pub path: PathBuf,
    /// Whether a dump has been asked for and not yet written.
    pending: bool,
    /// Whether one has already been written, so the request is honoured once
    /// even though the harness re-raises it on every frame of its logout.
    written: bool,
}

impl SceneDumpRequest {
    /// A request that will write to `path` when raised.
    #[must_use]
    pub const fn new(path: PathBuf) -> Self {
        Self {
            path,
            pending: false,
            written: false,
        }
    }

    /// Ask for the dump to be written at the end of this frame. Idempotent: a
    /// run writes one dump however often it asks.
    pub const fn request(&mut self) {
        if !self.written {
            self.pending = true;
        }
    }
}

/// Writes `scene.json` when the capture asks for it.
#[derive(Debug)]
pub struct SceneDumpPlugin {
    /// Where the dump goes.
    pub path: PathBuf,
    /// What the dump says produced it.
    pub identity: DumpIdentity,
}

impl Plugin for SceneDumpPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SceneDumpRequest::new(self.path.clone()))
            .insert_resource(self.identity.clone())
            // `Last`, so every `GlobalTransform` this reads is the one the frame
            // was rendered with rather than the previous frame's.
            .add_systems(Last, write_requested_scene_dump);
    }
}

/// A point in Second Life coordinates, as the reference emits one: three
/// numbers, `[x, y, z]`.
type Point = [f32; 3];

/// A quaternion in Second Life coordinates, as the reference emits one:
/// `[x, y, z, w]`.
type Quaternion = [f32; 4];

/// A whole scene dump.
#[derive(Debug, Serialize)]
pub struct SceneDump {
    /// The schema this document is written to.
    pub schema_version: u32,
    /// What produced it, and where.
    pub context: Context,
    /// The framing of the shot.
    pub camera: CameraDump,
    /// The lighting the frame was rendered under.
    pub environment: EnvironmentDump,
    /// The render settings that decide what the frame could contain at all.
    pub render: RenderDump,
    /// Every object in the agent's own region, sorted by id.
    pub objects: Vec<ObjectDump>,
    /// Every avatar the viewer is showing.
    pub avatars: Vec<AvatarDump>,
}

/// Identity and build of the viewer, and where it was.
#[derive(Debug, Serialize)]
pub struct Context {
    /// Which viewer wrote this.
    pub viewer: String,
    /// Its channel.
    pub channel: String,
    /// Its version.
    pub version: String,
    /// When the dump was taken, as `YYYY-MM-DDTHH:MM:SS`.
    pub time: String,
    /// The grid.
    pub grid: String,
    /// The agent's region.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region_name: Option<String>,
    /// Its id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region_id: Option<String>,
    /// Its handle. A string, not a number: a `u64` does not survive every JSON
    /// reader intact, and a handle is an identity rather than an arithmetic
    /// quantity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region_handle: Option<String>,
}

/// Where the camera was and what it could see.
#[derive(Debug, Serialize)]
pub struct CameraDump {
    /// The eye, in region metres.
    pub origin_region: Point,
    /// The eye, in Second Life global metres.
    pub origin_global: Point,
    /// What it looks at, in region metres.
    pub focus_region: Point,
    /// What it looks at, in Second Life global metres.
    pub focus_global: Point,
    /// The view axis.
    pub at_axis: Point,
    /// The camera's up axis.
    pub up_axis: Point,
    /// The camera's left axis.
    pub left_axis: Point,
    /// The vertical field of view, in radians.
    pub fov_radians: f32,
    /// The frame's aspect ratio.
    pub aspect: f32,
    /// The near clip plane, in metres.
    pub near_clip: f32,
    /// The far clip plane, in metres.
    pub far_clip: f32,
}

/// The sky the frame was lit by.
#[derive(Debug, Serialize)]
pub struct EnvironmentDump {
    /// The position in the day cycle the frame was rendered at, `0.0..=1.0`.
    ///
    /// Firestorm does not emit this yet — its dump names the sun's rotation
    /// instead — so it is expected to be absent on that side of a comparison
    /// rather than different.
    pub day_position: f32,
    /// The sun's direction, in Second Life coordinates.
    pub sun_direction: Point,
    /// The moon's direction.
    pub moon_direction: Point,
    /// The sun's orientation.
    pub sun_rotation: Quaternion,
    /// The name of the sky frame in force.
    pub sky_name: String,
    /// The name of the water settings in force.
    pub water_name: String,
}

/// The render settings in force.
///
/// Every field is optional because it is read from the settings store by the
/// reference's own name: a setting this viewer does not have yet is absent
/// rather than invented, and appears in the comparison the day it is added.
#[derive(Debug, Default, Serialize)]
pub struct RenderDump {
    /// `RenderFarClip`: how far the simulator streams content toward the agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draw_distance: Option<f32>,
    /// `RenderQualityPerformance`: the graphics preset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_level: Option<i32>,
    /// `RenderShadowDetail`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow_detail: Option<i32>,
    /// `RenderVolumeLODFactor`: the mesh / prim level-of-detail multiplier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh_lod_boost: Option<f32>,
    /// `RenderMaxTextureResolution`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_texture_res: Option<i32>,
    /// `RenderReflectionProbeDetail`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reflection_detail: Option<i32>,
}

/// One in-world object.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the schema is the reference viewer's, and its object entry states four independent               yes/no facts about a prim; folding them into an enum here would rename fields the               comparison matches by name"
)]
#[derive(Debug, Serialize)]
pub struct ObjectDump {
    /// The object's grid-wide id.
    pub id: String,
    /// Its region-local id.
    pub local_id: u32,
    /// Its class, in the reference's spelling.
    pub pcode: String,
    /// Where it is, in region metres — read back from what was drawn, except for
    /// an object **worn on an avatar**, which is composed against its wearer the
    /// way the reference composes one (see [the module docs](self)).
    pub position: Point,
    /// How it is turned, from the same source as [`position`](Self::position).
    pub rotation: Quaternion,
    /// Where a **worn** object was actually drawn, in region metres.
    ///
    /// Present only for an attachment, whose [`position`](Self::position) is the
    /// reference's composition rather than the drawn pose; for everything else
    /// the two are the same number and this is absent. The reference emits no
    /// such key, so it is expected to be absent on that side of a comparison
    /// rather than different.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drawn_position: Option<Point>,
    /// How a worn object was actually drawn, beside
    /// [`drawn_position`](Self::drawn_position).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drawn_rotation: Option<Quaternion>,
    /// Its size in metres, as the simulator described it.
    pub scale: Point,
    /// How many faces this viewer **drew** — its tessellated prim faces or its
    /// mesh's submeshes.
    ///
    /// The reference reports the object's *declared* texture-entry count
    /// (`getNumTEs`) instead. The two normally agree, and where they do not the
    /// difference is worth seeing: one viewer drawing five faces of a six-sided
    /// prim, or not having built an object the other has, is exactly the kind of
    /// divergence a frame shows and cannot explain.
    pub num_faces: usize,
    /// Those faces.
    pub faces: Vec<FaceDump>,
    /// Whether it is being drawn.
    pub visible: bool,
    /// Whether its shape comes from a mesh asset.
    pub is_mesh: bool,
    /// That asset, when it does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh_id: Option<String>,
    /// Whether its shape comes from a sculpt map.
    pub is_sculpt: bool,
    /// The level of detail it is currently tessellated at.
    pub lod: i32,
    /// Whether the object **declares** itself flexible (it carries a flexible
    /// extra-parameter block).
    ///
    /// The reference reports whether it is *drawing* one instead, and the two
    /// disagree wherever it declines to: a declared-flexi prim that it renders
    /// rigid reads `true` here and `false` there, which is the difference worth
    /// seeing rather than a spelling to paper over.
    pub is_flexible: bool,
    /// Whether it emits light.
    pub is_light: bool,
}

/// One face of an object — where "the texture is wrong" actually lives.
#[derive(Debug, Serialize)]
pub struct FaceDump {
    /// The face index.
    pub index: usize,
    /// Its texture asset id.
    pub texture: String,
    /// Its tint, RGBA in `0.0..=1.0` (the reference emits floats; the wire
    /// carries bytes).
    pub color: [f32; 4],
    /// Horizontal repeats.
    pub scale_s: f32,
    /// Vertical repeats.
    pub scale_t: f32,
    /// Horizontal offset.
    pub offset_s: f32,
    /// Vertical offset.
    pub offset_t: f32,
    /// Texture rotation, in radians.
    pub rotation: f32,
    /// The bump-map code.
    pub bump: u8,
    /// The shininess code.
    pub shiny: u8,
    /// Whether the face is unlit.
    pub fullbright: bool,
    /// The glow amount.
    pub glow: f32,
    /// A legacy Blinn-Phong material, when the face has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material_id: Option<String>,
}

/// One avatar.
#[derive(Debug, Serialize)]
pub struct AvatarDump {
    /// The agent's id.
    pub id: String,
    /// Whether this is the logged-in agent.
    pub is_self: bool,
    /// Where the avatar is, in region metres: the position the simulator sent
    /// for its **object**, which is what the reference's document reports, and
    /// not the drawn body root (see [the module docs](self)).
    pub position: Point,
    /// How it is turned, from the same source as [`position`](Self::position).
    pub rotation: Quaternion,
    /// Where this viewer drew the avatar's body root — a `root_drop` below
    /// [`position`](Self::position), because the wire position of an avatar is
    /// the centre of its physics capsule and the skeleton hangs from its feet.
    ///
    /// Absent for a presence this viewer has only a position for (a coarse dot,
    /// an animesh's control avatar), whose [`position`](Self::position) is the
    /// drawn one. The reference emits no such key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drawn_position: Option<Point>,
    /// How the drawn body root was turned, beside
    /// [`drawn_position`](Self::drawn_position).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drawn_rotation: Option<Quaternion>,
    /// Whether this is an animesh's control avatar rather than a resident.
    ///
    /// A control avatar has no grid identity, so [`id`](Self::id) here is the
    /// **animesh object's** key rather than an avatar id — see [the module
    /// docs](self). The reference mints a local UUID instead, which is why the
    /// two are matched by this flag and the object, never by id.
    pub is_control_avatar: bool,
    /// What it is playing, in the order the viewer applies it — see
    /// [`AnimationDump`].
    pub animations: Vec<AnimationDump>,
    /// Whether this viewer has drawn it as a body rather than a placeholder.
    ///
    /// The nearest thing this viewer has to the reference's `is_fully_loaded`,
    /// and deliberately named for what it measures: a body is present when the
    /// rigged base body has been built for this avatar. An avatar still shown as
    /// a sphere reads `false` here and `false` there, which is the comparison
    /// that matters.
    pub has_body: bool,
}

/// The three resources that say what every avatar is playing and when.
///
/// One [`SystemParam`] rather than three parameters because a dump is a
/// photograph of the whole world and the system that writes it is already at
/// Bevy's parameter limit — see [`write_requested_scene_dump`].
#[derive(SystemParam)]
struct AnimationInputs<'w> {
    /// What each avatar is playing.
    playback: Res<'w, AnimationPlayback>,
    /// The decoded motions, for each animation's duration and priority.
    manager: Res<'w, AnimationManager>,
    /// The clock the playback times are measured against.
    time: Res<'w, Time>,
}

/// One animation an avatar is playing.
///
/// The list is in **the order the viewer applies it**, most recently activated
/// first: the reference's motion controller pushes each newly started motion to
/// the front of its active list, and our own per-joint blend breaks a priority
/// tie the same way. Order matters because it is half of what decides which
/// motion owns a joint; `priority` is the other half.
///
/// Two viewers list different *sets* here, and that is not a divergence. The
/// reference starts default motions on every avatar (head rotation, eye, body
/// noise, breathing, physics, hand pose, pelvis fix) which this viewer
/// implements as adjusters rather than as motions, so they appear only on that
/// side. What is worth comparing is the animations the **simulator** named:
/// whether both viewers are playing them, and where each one's clock has
/// reached.
#[derive(Debug, Serialize)]
pub struct AnimationDump {
    /// The animation's asset id.
    pub id: String,
    /// The simulator's per-avatar sequence number, when the simulator is what
    /// asked for this animation. Absent for a motion the viewer plays of its
    /// own accord.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<i32>,
    /// Seconds since the motion started, on the clock it is sampled at.
    pub time: f32,
    /// Where in the motion that lands, in seconds — `time` wrapped by
    /// [`duration`](Self::duration) for a looping motion, and clamped to it for
    /// one that plays once. This is the "which frame" of the animation, and the
    /// number two viewers of one scene can actually be compared on: `time` runs
    /// from whenever each viewer started playing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_time: Option<f32>,
    /// The motion's length in seconds, when the viewer has its asset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f32>,
    /// Whether it loops.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub looping: Option<bool>,
    /// Its base priority: what decides which motion owns a joint when several
    /// animate it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    /// Whether it has been stopped and is easing out.
    pub stopping: bool,
}

/// Where `time` lands inside a motion: wrapped for a looping one, clamped for
/// one that plays once, and `None` when the viewer does not have the asset and
/// so does not know how long it is.
fn loop_time(time: f32, duration: Option<f32>, looping: Option<bool>) -> Option<f32> {
    let duration = duration.filter(|duration| *duration > 0.0)?;
    let time = time.max(0.0);
    if looping == Some(true) {
        Some(time % duration)
    } else {
        Some(time.min(duration))
    }
}

/// Collect the dump. See [the module docs](self) for what the numbers mean.
#[expect(
    clippy::too_many_arguments,
    reason = "a dump is a photograph of every part of the world at once; each argument is one \
              of the sections it has to describe"
)]
fn build(
    identity: &SlIdentity,
    dump_identity: &DumpIdentity,
    objects: &ObjectState,
    avatars: &AvatarState,
    animesh: &ControlAvatarState,
    environment: &EnvironmentState,
    settings: &ViewerSettings,
    playback: &AnimationPlayback,
    animation_manager: &AnimationManager,
    now: f32,
    region: Option<&SlRegionIdentity>,
    camera: Option<(&GlobalTransform, Option<&Projection>)>,
    motions: &Query<'_, '_, &ObjectSlMotion>,
    transforms: &Query<'_, '_, &GlobalTransform>,
    bodies: &Query<'_, '_, &AvatarBodyPart>,
    scene_objects: &Query<'_, '_, &SceneObject>,
) -> SceneDump {
    let origin = objects.origin;
    let handle = identity.region_handle;
    let offset = handle.map_or(Vec3::ZERO, |handle| region_offset_bevy(handle, origin));
    SceneDump {
        schema_version: SCHEMA_VERSION,
        context: build_context(dump_identity, region, handle),
        camera: build_camera(camera, offset, handle),
        environment: build_environment(environment),
        render: build_render(settings),
        objects: build_objects(
            identity,
            objects,
            avatars,
            motions,
            transforms,
            scene_objects,
            offset,
        ),
        avatars: build_avatars(
            identity,
            avatars,
            animesh,
            objects,
            playback,
            animation_manager,
            now,
            motions,
            transforms,
            bodies,
            offset,
        ),
    }
}

/// Component-wise vector subtract, avoiding the glam `-` operator the workspace
/// `arithmetic_side_effects` lint trips on (the idiom `crate::camera` uses).
const fn vsub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Component-wise vector add, for the same reason as [`vsub`].
const fn vadd(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

/// A Bevy world position back in Second Life region metres.
fn region_point(world: Vec3, offset: Vec3) -> Point {
    let local = bevy_to_sl_vec(vsub(world, offset));
    [local.x, local.y, local.z]
}

/// A Bevy world rotation back in Second Life coordinates, undoing the single
/// basis change every object's transform carries.
///
/// Emitted with a **non-negative real part**. A quaternion and its negation are
/// the same rotation, and two viewers that reached it by different routes
/// routinely disagree about the sign — so without this every second object reads
/// as a rotation difference in a comparison that is looking for exactly that.
fn region_rotation(world: Quat) -> Quaternion {
    canonical_rotation(sl_to_bevy_rotation().inverse().mul_quat(world))
}

/// A Second Life rotation as the dump emits one: `[x, y, z, w]`, with a
/// non-negative real part. See [`region_rotation`] for why the sign is pinned.
fn canonical_rotation(sl: Quat) -> Quaternion {
    if sl.w < 0.0 {
        [-sl.x, -sl.y, -sl.z, -sl.w]
    } else {
        [sl.x, sl.y, sl.z, sl.w]
    }
}

/// A Bevy direction back in Second Life coordinates.
fn region_direction(world: Vec3) -> Point {
    let sl = bevy_to_sl_vec(world);
    [sl.x, sl.y, sl.z]
}

/// A region-local point in Second Life **global** metres: the region's
/// south-west corner plus the local offset.
fn global_point(local: Point, handle: Option<RegionHandle>) -> Point {
    let Some(handle) = handle else {
        return local;
    };
    // Whole region metres, so exact in `f32` through the 16-bit split.
    let (east, north) = handle.global_coordinates();
    [
        local[0] + metres_to_f32(east),
        local[1] + metres_to_f32(north),
        local[2],
    ]
}

/// The `context` section.
fn build_context(
    identity: &DumpIdentity,
    region: Option<&SlRegionIdentity>,
    handle: Option<RegionHandle>,
) -> Context {
    Context {
        viewer: VIEWER_NAME.to_owned(),
        channel: identity.channel.clone(),
        version: identity.version.clone(),
        time: timestamp(),
        grid: identity.grid.clone(),
        region_name: region.and_then(|region| region.0.sim_name.as_ref().map(ToString::to_string)),
        region_id: region.map(|region| region.0.region_id.to_string()),
        region_handle: handle.map(|handle| handle.get().to_string()),
    }
}

/// The `camera` section.
fn build_camera(
    camera: Option<(&GlobalTransform, Option<&Projection>)>,
    offset: Vec3,
    handle: Option<RegionHandle>,
) -> CameraDump {
    let (transform, projection) = match camera {
        Some((transform, projection)) => (*transform, projection),
        None => (GlobalTransform::IDENTITY, None),
    };
    let perspective = match projection {
        Some(Projection::Perspective(perspective)) => perspective.clone(),
        _other => PerspectiveProjection::default(),
    };
    let eye = transform.translation();
    let at = transform.forward().as_vec3();
    // The point the view axis meets at the rig's own working distance. This
    // viewer keeps no focus *point* — a pinned harness camera is a pose, not a
    // target — so the comparable part of the pair is the axis; the focus is
    // stated so a reader can see where the two views converge.
    let focus = Vec3::new(
        eye.x + at.x * FOCUS_DISTANCE_M,
        eye.y + at.y * FOCUS_DISTANCE_M,
        eye.z + at.z * FOCUS_DISTANCE_M,
    );
    let origin_region = region_point(eye, offset);
    let focus_region = region_point(focus, offset);
    CameraDump {
        origin_global: global_point(origin_region, handle),
        focus_global: global_point(focus_region, handle),
        origin_region,
        focus_region,
        at_axis: region_direction(at),
        up_axis: region_direction(transform.up().as_vec3()),
        left_axis: region_direction(vsub(Vec3::ZERO, transform.right().as_vec3())),
        fov_radians: perspective.fov,
        aspect: perspective.aspect_ratio,
        near_clip: perspective.near,
        far_clip: perspective.far,
    }
}

/// How far along the view axis the reported focus point sits. Arbitrary, and
/// stated rather than hidden: see [`build_camera`].
const FOCUS_DISTANCE_M: f32 = 10.0;

/// The `environment` section.
fn build_environment(environment: &EnvironmentState) -> EnvironmentDump {
    let position = sl_viewer_world_scene::sky::day_position(environment);
    let sky = environment.settings.blended_sky_settings(0.0, position);
    let water = environment.settings.blended_water_settings(position);
    let direction = |rotation: &Rotation| {
        // The reference's own derivation (`LLSettingsSky::getSunDirection`): the
        // body's orientation applied to the Second Life X axis.
        let vector = crate::coords::sl_rotation_to_quat(rotation).mul_vec3(Vec3::X);
        [vector.x, vector.y, vector.z]
    };
    EnvironmentDump {
        day_position: position,
        sun_direction: sky
            .as_ref()
            .map_or([0.0; 3], |sky| direction(&sky.sun_rotation)),
        moon_direction: sky
            .as_ref()
            .map_or([0.0; 3], |sky| direction(&sky.moon_rotation)),
        sun_rotation: sky.as_ref().map_or([0.0, 0.0, 0.0, 1.0], |sky| {
            [
                sky.sun_rotation.x,
                sky.sun_rotation.y,
                sky.sun_rotation.z,
                sky.sun_rotation.s,
            ]
        }),
        sky_name: sky.map_or_else(String::new, |sky| sky.name),
        water_name: water.map_or_else(String::new, |water| water.name),
    }
}

/// The `render` section, read from the settings store by the reference's own
/// setting names.
fn build_render(settings: &ViewerSettings) -> RenderDump {
    let store = settings.store();
    RenderDump {
        draw_distance: store.get_f32("RenderFarClip").ok(),
        // Read as `u32` and reported as `i32`: this viewer stores a preset level
        // as an unsigned setting while the reference's dump emits a signed one,
        // and a comparison that reads `2` on one side and nothing on the other
        // would report a missing setting rather than an equal one.
        quality_level: level(store, "RenderQualityPerformance"),
        shadow_detail: level(store, "RenderShadowDetail"),
        mesh_lod_boost: store.get_f32("RenderVolumeLODFactor").ok(),
        max_texture_res: level(store, "RenderMaxTextureResolution"),
        reflection_detail: level(store, "RenderReflectionProbeDetail"),
    }
}

/// One render setting that names a level, however this viewer happens to store
/// it: `u32` here, `i32` in the reference's document, and absent from both when
/// the setting does not exist.
fn level(store: &sl_settings::SettingsStore, name: &str) -> Option<i32> {
    store
        .get_u32(name)
        .ok()
        .and_then(|value| i32::try_from(value).ok())
        .or_else(|| store.get_i32(name).ok())
}

/// The `objects` section: every object on the agent's **own** circuit, sorted by
/// id.
///
/// Own circuit rather than everything the viewer holds, matching the reference's
/// own-region filter: two viewers may have cached different neighbours, and a
/// dump that includes them compares their caching rather than their rendering.
/// Sorted because an unstable iteration order turns every dump into a diff
/// against itself and teaches its reader to ignore the comparison.
fn build_objects(
    identity: &SlIdentity,
    objects: &ObjectState,
    avatars: &AvatarState,
    motions: &Query<'_, '_, &ObjectSlMotion>,
    transforms: &Query<'_, '_, &GlobalTransform>,
    scene_objects: &Query<'_, '_, &SceneObject>,
    offset: Vec3,
) -> Vec<ObjectDump> {
    let mut dumped: Vec<ObjectDump> = objects
        .objects
        .iter()
        .filter(|(scoped, _tracked)| identity.circuit_id == Some(scoped.circuit))
        .filter(|(_scoped, tracked)| {
            // Avatars are reported separately, with their appearance state.
            scene_objects
                .get(tracked.entity)
                .is_ok_and(|object| object.category != ObjectCategory::Avatar)
        })
        .map(|(scoped, tracked)| {
            dump_object(
                scoped, tracked, objects, avatars, motions, transforms, offset,
            )
        })
        .collect();
    dumped.sort_by(|left, right| left.id.cmp(&right.id));
    dumped
}

/// One object's entry.
fn dump_object(
    scoped: &ScopedObjectId,
    tracked: &TrackedObject,
    objects: &ObjectState,
    avatars: &AvatarState,
    motions: &Query<'_, '_, &ObjectSlMotion>,
    transforms: &Query<'_, '_, &GlobalTransform>,
    offset: Vec3,
) -> ObjectDump {
    let transform = transforms.get(tracked.entity).ok();
    let faces = dump_faces(&tracked.texture_entry, tracked.face_entities.len());
    let sculpt = tracked.extra.sculpt.as_ref();
    let is_mesh = sculpt.is_some_and(|sculpt| sculpt.texture.is_mesh());
    // What was drawn, and — for a worn object, whose drawn pose the reference's
    // document is not reporting — the reference's own composition beside it.
    let drawn = transform.map(|transform| {
        (
            region_point(transform.translation(), offset),
            region_rotation(transform.rotation()),
        )
    });
    let worn = worn_placement(scoped, objects, avatars, motions, transforms, offset)
        .map(ReferencePose::emitted);
    let (position, rotation) = worn.or(drawn).unwrap_or(([0.0; 3], [0.0, 0.0, 0.0, 1.0]));
    let drawn_pose = worn.and(drawn);
    ObjectDump {
        id: tracked.full_key.to_string(),
        local_id: scoped.id.get(),
        pcode: pcode::describe(tracked.shape.pcode()),
        position,
        rotation,
        drawn_position: drawn_pose.map(|(position, _rotation)| position),
        drawn_rotation: drawn_pose.map(|(_position, rotation)| rotation),
        scale: [tracked.scale.x, tracked.scale.y, tracked.scale.z],
        num_faces: faces.len(),
        faces,
        // An entity with no transform is one that has been despawned out from
        // under the map; anything else the viewer holds is being drawn.
        visible: transform.is_some(),
        is_mesh,
        mesh_id: is_mesh
            .then(|| sculpt.map(|sculpt| sculpt.texture.to_string()))
            .flatten(),
        // As the reference means it (`LLVOVolume::isSculpted`): a mesh *is* a
        // sculpt whose type says mesh, so a mesh object reports both. Reading
        // this as "a legacy sculpt and not a mesh" made every mesh in the scene
        // a difference.
        is_sculpt: sculpt.is_some(),
        lod: i32::from(tracked.prim_lod.index()),
        is_flexible: tracked.extra.flexible.is_some(),
        is_light: tracked.extra.light.is_some(),
    }
}

/// The placement the reference's document reports for one object, and the
/// rotation its children are composed against.
#[derive(Debug, Clone, Copy)]
struct ReferencePose {
    /// Where the object is, in region metres (`getPositionRegion`).
    position: Vec3,
    /// How it is turned, in region coordinates (`getRotationRegion`) — what the
    /// dump reports.
    rotation: Quat,
    /// The object's own **local** rotation (`getRotation`), which is what the
    /// reference composes a child against and is *not* the region rotation for
    /// anything below a root.
    local_rotation: Quat,
}

impl ReferencePose {
    /// The pose of a root object, whose local rotation is its region rotation.
    const fn root(position: Vec3, rotation: Quat) -> Self {
        Self {
            position,
            rotation,
            local_rotation: rotation,
        }
    }

    /// This pose as the dump emits one.
    fn emitted(self) -> (Point, Quaternion) {
        (
            [self.position.x, self.position.y, self.position.z],
            canonical_rotation(self.rotation),
        )
    }
}

/// One link of a parent chain: an object's wire-space pose relative to its
/// parent, in Second Life coordinates — an attachment's offset from its
/// attachment point, a seated avatar's offset from its seat, or a linked child's
/// offset from its linkset root.
#[derive(Debug, Clone, Copy)]
struct LocalPose {
    /// The parent-relative position, in metres.
    position: Vec3,
    /// The parent-relative rotation.
    rotation: Quat,
}

impl LocalPose {
    /// The wire pose an object last reported, as the object layer mirrors it
    /// onto the entity.
    fn of(motion: &ObjectSlMotion) -> Self {
        Self {
            position: Vec3::new(motion.position.x, motion.position.y, motion.position.z),
            rotation: sl_rotation_to_quat(&motion.rotation),
        }
    }
}

/// One link of the reference's own composition:
///
/// ```text
/// mPositionRegion = parent->getPositionRegion() + getPosition() * parent->getRotation()
/// rotationRegion  = getRotation() * parent->getRotation()
/// ```
///
/// Literal about it, quirk included: a link is composed against its parent's
/// **local** rotation, so on a linked child of an attachment the wearer's turn is
/// applied once rather than twice. Matching the reference is the point; a
/// comparison that is right about a grandchild and disagrees with the document it
/// is diffing has bought nothing.
fn compose_link(parent: ReferencePose, link: LocalPose) -> ReferencePose {
    ReferencePose {
        position: vadd(
            parent.position,
            parent.local_rotation.mul_vec3(link.position),
        ),
        rotation: parent.local_rotation.mul_quat(link.rotation),
        local_rotation: link.rotation,
    }
}

/// A whole chain composed onto a base pose, outermost link first.
fn compose_worn(base: ReferencePose, chain: &[LocalPose]) -> ReferencePose {
    chain
        .iter()
        .fold(base, |parent, link| compose_link(parent, *link))
}

/// The reference's placement for an object **worn on an avatar**, or `None` when
/// it is not worn — in which case what was drawn is what the dump reports.
///
/// Walks the parent chain up to the wearing avatar, collecting each link's wire
/// pose, and composes them onto that avatar's own reference placement — the same
/// number the `avatars` section reports, so the two sections agree with each
/// other as well as with the reference. The walk is bounded exactly like the
/// object layer's own ([`MAX_PARENT_WALK`]), against a malformed parent cycle.
fn worn_placement(
    scoped: &ScopedObjectId,
    objects: &ObjectState,
    avatars: &AvatarState,
    motions: &Query<'_, '_, &ObjectSlMotion>,
    transforms: &Query<'_, '_, &GlobalTransform>,
    offset: Vec3,
) -> Option<ReferencePose> {
    let mut chain: Vec<LocalPose> = Vec::new();
    let mut current = *scoped;
    let mut wearer = None;
    for _ in 0..MAX_PARENT_WALK {
        let tracked = objects.objects.get(&current)?;
        // A linkset root standing in the world is not worn by anybody, and is
        // reported exactly as it was drawn.
        if tracked.is_root {
            return None;
        }
        chain.push(LocalPose::of(motions.get(tracked.entity).ok()?));
        if avatars.agent_of(tracked.parent).is_some() {
            wearer = avatar_placement(tracked.parent, objects, motions, transforms, offset);
            break;
        }
        current = tracked.parent;
    }
    chain.reverse();
    Some(compose_worn(wearer?, &chain))
}

/// The reference's placement for an **avatar object**: the position the
/// simulator sent for it, or — when it is sitting — that offset composed onto its
/// seat.
///
/// Not the drawn body root, which is a different quantity by design. The wire
/// position of an avatar is the centre of its physics capsule, and both viewers
/// lower the *skeleton* from there so the feet meet the ground
/// (`body_root_transform`'s `root_drop` here, `LLVOAvatar::updateCharacter`'s
/// `root_pos.z -= …` there). The reference's document reports the object's
/// position, so a dump that reported the drawn root instead would differ from it
/// by that drop on every avatar in the scene — and, through the wearer, on every
/// attachment as well.
fn avatar_placement(
    scoped: ScopedObjectId,
    objects: &ObjectState,
    motions: &Query<'_, '_, &ObjectSlMotion>,
    transforms: &Query<'_, '_, &GlobalTransform>,
    offset: Vec3,
) -> Option<ReferencePose> {
    let tracked = objects.objects.get(&scoped)?;
    let local = LocalPose::of(motions.get(tracked.entity).ok()?);
    if tracked.is_root {
        return Some(ReferencePose::root(local.position, local.rotation));
    }
    // Sitting: the wire pose is relative to the seat, which is an ordinary
    // in-world object and is reported as it was drawn.
    let seat = objects.objects.get(&tracked.parent)?;
    let seat = drawn_pose(transforms.get(seat.entity).ok()?, offset);
    Some(compose_link(seat, local))
}

/// What was drawn, as a [`ReferencePose`]: the entity's own global transform
/// back in region coordinates. Its local rotation is its region rotation, which
/// holds for the roots this is used on.
fn drawn_pose(transform: &GlobalTransform, offset: Vec3) -> ReferencePose {
    let position = region_point(transform.translation(), offset);
    let rotation = region_rotation(transform.rotation());
    ReferencePose::root(
        Vec3::new(position[0], position[1], position[2]),
        Quat::from_xyzw(rotation[0], rotation[1], rotation[2], rotation[3]),
    )
}

/// The faces of a decoded texture entry, as many as the object actually has.
///
/// `count` is the object's own face count — its tessellated prim faces or its
/// mesh's submeshes — which is what the reference reports (`getNumTEs`). It is
/// *not* the entry's own capacity: a texture entry states a default that applies
/// to every face, so decoding one with the 64-face maximum yields 64 faces for a
/// six-sided box and turns every object in the comparison into a difference.
///
/// An object that has not been tessellated yet has no faces, and says so rather
/// than inventing them — one viewer having built an object and the other not is
/// a difference worth seeing.
fn dump_faces(entry: &[u8], count: usize) -> Vec<FaceDump> {
    let decoded = decode_texture_entry(entry, count.min(MAX_FACES));
    decoded
        .faces
        .iter()
        .enumerate()
        .map(|(index, face)| dump_face(index, face))
        .collect()
}

/// One face's entry.
fn dump_face(index: usize, face: &TextureFace) -> FaceDump {
    let channel = |value: u8| f32::from(value) / 255.0;
    FaceDump {
        index,
        texture: face.texture_id.to_string(),
        color: [
            channel(face.color[0]),
            channel(face.color[1]),
            channel(face.color[2]),
            channel(face.color[3]),
        ],
        scale_s: face.scale_s,
        scale_t: face.scale_t,
        offset_s: face.offset_s,
        offset_t: face.offset_t,
        rotation: face.rotation,
        bump: face.bumpmap(),
        shiny: face.shininess(),
        fullbright: face.fullbright(),
        glow: face.glow,
        material_id: face.material_id.map(|id| id.to_string()),
    }
}

/// The `avatars` section.
#[expect(
    clippy::too_many_arguments,
    reason = "one avatar entry states its identity, its placement and its appearance state, and \
              each of those is read from a different resource"
)]
fn build_avatars(
    identity: &SlIdentity,
    avatars: &AvatarState,
    animesh: &ControlAvatarState,
    objects: &ObjectState,
    playback: &AnimationPlayback,
    animation_manager: &AnimationManager,
    now: f32,
    motions: &Query<'_, '_, &ObjectSlMotion>,
    transforms: &Query<'_, '_, &GlobalTransform>,
    bodies: &Query<'_, '_, &AvatarBodyPart>,
    offset: Vec3,
) -> Vec<AvatarDump> {
    let with_bodies: Vec<AgentKey> = bodies.iter().map(AvatarBodyPart::agent).collect();
    // The avatar *object* behind each agent, which is what the reference reports
    // a position for.
    let objects_of: HashMap<AgentKey, ScopedObjectId> = avatars
        .by_scoped
        .iter()
        .map(|(scoped, agent)| (*agent, *scoped))
        .collect();
    let residents = avatars
        .objects
        .iter()
        .chain(avatars.coarse.iter())
        .map(|(agent, entities)| {
            let placement = objects_of
                .get(agent)
                .and_then(|scoped| avatar_placement(*scoped, objects, motions, transforms, offset));
            dump_avatar(
                agent.to_string(),
                identity.agent_id == Some(*agent),
                false,
                with_bodies.contains(agent),
                placement,
                transforms.get(entities.anchor).ok(),
                &playback.playing_animations(*agent, now, animation_manager),
                offset,
            )
        });
    // An animesh's control avatar, reported by the object it rides: it has no
    // grid identity of its own, and the reference's locally minted one cannot be
    // matched against anything. Whether the animesh rezzed *as* an animesh is
    // worth comparing, so it is listed rather than dropped.
    let animated = animesh.animated_objects().map(|object| {
        let anchor = objects
            .entity_of(object)
            .and_then(|entity| transforms.get(entity).ok());
        dump_avatar(
            object.to_string(),
            false,
            true,
            true,
            None,
            anchor,
            &animesh.playing_animations(object, now, animation_manager),
            offset,
        )
    });
    let mut dumped: Vec<AvatarDump> = residents.chain(animated).collect();
    dumped.sort_by(|left, right| left.id.cmp(&right.id));
    dumped
}

/// One avatar's entry.
///
/// `placement` is where the reference's document puts it — the avatar object's
/// own position — and `transform` is the body root this viewer drew, which sits
/// a `root_drop` lower (see [`avatar_placement`]). A resident has both; a coarse
/// dot, or an animesh's control avatar, has only what was drawn.
#[expect(
    clippy::too_many_arguments,
    reason = "an avatar entry is its identity, its two placements, what it is playing and its \
              appearance state; a struct of them would only move the same arguments one call up"
)]
fn dump_avatar(
    id: String,
    is_self: bool,
    is_control_avatar: bool,
    has_body: bool,
    placement: Option<ReferencePose>,
    transform: Option<&GlobalTransform>,
    playing: &[PlayingAnimation],
    offset: Vec3,
) -> AvatarDump {
    let drawn = transform.map(|transform| {
        (
            region_point(transform.translation(), offset),
            region_rotation(transform.rotation()),
        )
    });
    let placed = placement.map(ReferencePose::emitted);
    let (position, rotation) = placed.or(drawn).unwrap_or(([0.0; 3], [0.0, 0.0, 0.0, 1.0]));
    let drawn_pose = placed.and(drawn);
    AvatarDump {
        id,
        is_self,
        is_control_avatar,
        position,
        rotation,
        drawn_position: drawn_pose.map(|(position, _rotation)| position),
        drawn_rotation: drawn_pose.map(|(_position, rotation)| rotation),
        animations: playing.iter().map(dump_animation).collect(),
        has_body,
    }
}

/// One playing animation's entry.
fn dump_animation(playing: &PlayingAnimation) -> AnimationDump {
    AnimationDump {
        id: playing.id.to_string(),
        // The simulator numbers what it asks for from one; a motion the viewer
        // started itself carries no number of the simulator's.
        sequence: (playing.sequence > 0).then_some(playing.sequence),
        time: playing.time,
        loop_time: loop_time(playing.time, playing.duration, playing.loops),
        duration: playing.duration,
        looping: playing.loops,
        priority: playing.priority,
        stopping: playing.stopping,
    }
}

/// The dump's timestamp, in the reference's own `YYYY-MM-DDTHH:MM:SS` form
/// (UTC), so the two viewers' dumps stamp themselves the same way.
fn timestamp() -> String {
    let format = time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");
    time::OffsetDateTime::now_utc()
        .format(&format)
        .unwrap_or_default()
}

/// Write the dump when one has been asked for.
#[expect(
    clippy::too_many_arguments,
    reason = "the dump describes every part of the world at once, and each parameter is one of \
              its sections; splitting the system would only split the photograph"
)]
fn write_requested_scene_dump(
    mut request: ResMut<SceneDumpRequest>,
    dump_identity: Res<DumpIdentity>,
    identity: Res<SlIdentity>,
    objects: Res<ObjectState>,
    avatars: Res<AvatarState>,
    animesh: Res<ControlAvatarState>,
    environment: Res<EnvironmentState>,
    settings: Res<ViewerSettings>,
    animations: AnimationInputs,
    regions: Query<&SlRegionIdentity, With<sl_client_bevy::SlCurrentRegion>>,
    cameras: Query<(&GlobalTransform, Option<&Projection>), With<ViewerCamera>>,
    motions: Query<&ObjectSlMotion>,
    transforms: Query<&GlobalTransform>,
    bodies: Query<&AvatarBodyPart>,
    scene_objects: Query<&SceneObject>,
) {
    if !request.pending {
        return;
    }
    request.pending = false;
    request.written = true;
    let dump = build(
        &identity,
        &dump_identity,
        &objects,
        &avatars,
        &animesh,
        &environment,
        &settings,
        &animations.playback,
        &animations.manager,
        animations.time.elapsed_secs(),
        regions.iter().next(),
        cameras.iter().next(),
        &motions,
        &transforms,
        &bodies,
        &scene_objects,
    );
    match serde_json::to_string_pretty(&dump) {
        Ok(json) => match fs_err::write(&request.path, json.as_bytes()) {
            Ok(()) => info!("scene dump written to {}", request.path.display()),
            Err(error) => error!("scene dump: {error}"),
        },
        Err(error) => error!("scene dump: cannot serialise: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{Rotation, TextureFace, TextureKey, Uuid, Vector};

    use sl_viewer_world_avatar::animations::PlayingAnimation;

    use super::{
        FaceDump, LocalPose, ObjectDump, Point, ReferencePose, compose_worn, dump_avatar,
        dump_face, dump_faces, loop_time, region_direction, region_point, region_rotation,
    };
    use crate::coords::{sl_to_bevy_object_rotation, sl_to_bevy_vec};

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// Whether two points agree to within a millimetre — the tolerance the
    /// comparison itself uses on floats, and the only kind of equality a
    /// coordinate that has been through two basis changes deserves.
    fn near(left: [f32; 3], right: [f32; 3]) -> bool {
        left.iter()
            .zip(right.iter())
            .all(|(left, right)| (left - right).abs() < 1e-3)
    }

    /// The dump's coordinate conversion is the **inverse of the object layer's
    /// own**: a Second Life position placed on an entity the way `objects` places
    /// one comes back out of the dump unchanged.
    ///
    /// This is the check with teeth. The conversion could be plausible and wrong
    /// in four ways (a swapped axis, a missing negation, the basis change applied
    /// twice, the region offset forgotten), and every one of them would produce a
    /// dump that looks like a rendering divergence in every object of the scene.
    #[test]
    fn a_position_survives_the_trip_out_and_back() {
        let sl = Vector {
            x: 124.0,
            y: 136.5,
            z: 25.25,
        };
        // Exactly what the object layer does to place a root object.
        let placed = sl_to_bevy_vec(&sl);
        let dumped = region_point(placed, Vec3::ZERO);
        assert!(near(dumped, [sl.x, sl.y, sl.z]), "dumped {dumped:?}");
    }

    /// A rotation and its negation are the same rotation, and two viewers that
    /// reached one by different routes disagree about the sign as often as not.
    /// The dump emits the non-negative-real form, so a comparison sees one
    /// rotation rather than two.
    #[test]
    fn a_rotation_is_emitted_in_its_canonical_sign() {
        // A three-quarter turn about Second Life up, whose composed quaternion
        // comes out with a negative real part.
        let sl = Rotation {
            x: 0.0,
            y: 0.0,
            z: -core::f32::consts::FRAC_1_SQRT_2,
            s: -core::f32::consts::FRAC_1_SQRT_2,
        };
        let [x, y, z, w] = region_rotation(sl_to_bevy_object_rotation(&sl));
        assert!(w >= 0.0, "the real part came out {w}");
        // Still the same rotation: every component negated together.
        let close = |left: f32, right: f32| (left - right).abs() < 1e-5;
        assert!(
            close(x, -sl.x) && close(y, -sl.y) && close(z, -sl.z) && close(w, -sl.s),
            "expected the negation of {sl:?}, dumped [{x}, {y}, {z}, {w}]"
        );
    }

    /// The same for a rotation, through the single basis change a root object's
    /// transform carries.
    #[test]
    fn a_rotation_survives_the_trip_out_and_back() {
        // A quarter turn about the Second Life up axis (Z).
        let sl = Rotation {
            x: 0.0,
            y: 0.0,
            z: core::f32::consts::FRAC_1_SQRT_2,
            s: core::f32::consts::FRAC_1_SQRT_2,
        };
        let placed = sl_to_bevy_object_rotation(&sl);
        let [x, y, z, w] = region_rotation(placed);
        let close = |left: f32, right: f32| (left - right).abs() < 1e-5;
        assert!(
            close(x, sl.x) && close(y, sl.y) && close(z, sl.z) && close(w, sl.s),
            "expected {sl:?}, dumped [{x}, {y}, {z}, {w}]"
        );
    }

    /// A region offset is subtracted, so an object in a neighbour region reports
    /// *its own* region-local position rather than one measured from the scene
    /// origin — the mistake that makes a neighbour look 256 m out of place.
    #[test]
    fn a_region_offset_is_taken_back_off() {
        let sl = Vector {
            x: 10.0,
            y: 20.0,
            z: 30.0,
        };
        let offset = sl_to_bevy_vec(&Vector {
            x: 256.0,
            y: 0.0,
            z: 0.0,
        });
        let placed = sl_to_bevy_vec(&Vector {
            x: sl.x + 256.0,
            y: sl.y,
            z: sl.z,
        });
        let dumped = region_point(placed, offset);
        assert!(near(dumped, [sl.x, sl.y, sl.z]), "dumped {dumped:?}");
    }

    /// The camera's axes come out in Second Life coordinates: Bevy's forward
    /// (`-Z`) is Second Life's `+Y` (north), and Bevy's up (`+Y`) is Second
    /// Life's `+Z`.
    #[test]
    fn the_camera_axes_are_in_second_life_coordinates() {
        let north: Point = region_direction(Vec3::NEG_Z);
        let up: Point = region_direction(Vec3::Y);
        assert!(near(north, [0.0, 1.0, 0.0]), "north came out {north:?}");
        assert!(near(up, [0.0, 0.0, 1.0]), "up came out {up:?}");
    }

    /// A texture entry states a default that applies to every face, so decoding
    /// one with the wire's 64-face maximum gives a six-sided box sixty-four
    /// faces — and turns every object in a comparison into a difference. The
    /// object's own face count is what bounds it.
    #[test]
    fn a_box_has_six_faces_not_sixty_four() {
        let entry = sl_client_bevy::encode_texture_entry(&sl_client_bevy::TextureEntry {
            faces: vec![TextureFace::new(TextureKey::from(Uuid::nil())); 6],
        });
        assert_eq!(dump_faces(&entry, 6).len(), 6);
        // An object that has not been tessellated says it has no faces rather
        // than inventing them.
        assert_eq!(dump_faces(&entry, 0).len(), 0);
    }

    /// A face's tint is bytes on the wire and floats in the document, because
    /// that is what the reference emits: comparing `255` against `1.0` reads as a
    /// divergence on every textured face in the scene.
    #[test]
    fn a_face_tint_is_emitted_as_the_reference_emits_it() {
        let face = TextureFace::new(TextureKey::from(Uuid::nil()));
        let dumped = dump_face(0, &face);
        assert!(
            near(
                [dumped.color[0], dumped.color[1], dumped.color[2]],
                [1.0, 1.0, 1.0]
            ) && (dumped.color[3] - 1.0).abs() < 1e-3,
            "an untinted face came out {:?}",
            dumped.color
        );
    }

    /// Bump, shininess and full-bright share one wire byte and three fields in
    /// the document. Unpacking them wrongly is invisible in a frame and obvious
    /// in a diff, so it is pinned here.
    #[test]
    fn the_packed_face_byte_is_unpacked_into_three_fields() {
        let mut face = TextureFace::new(TextureKey::from(Uuid::nil()));
        // bump 3, full-bright set (bit 5), shininess 2 (top two bits).
        face.bump_shiny_fullbright = 3 | 0x20 | (2 << 6);
        let dumped = dump_face(1, &face);
        assert_eq!(dumped.bump, 3);
        assert!(dumped.fullbright);
        assert_eq!(dumped.shiny, 2);
        assert_eq!(dumped.index, 1);
    }

    /// A control avatar has no grid identity: the reference mints it a local
    /// UUID, which differs between two viewers of one scene and between two runs
    /// of one viewer. Ours reports the animesh **object** instead, because that
    /// is the only thing about it the two sides can agree on — and it is flagged,
    /// so a comparison never matches one against a resident.
    #[test]
    fn a_control_avatar_is_reported_by_the_object_it_rides() {
        let object = "00000000-0000-0000-0000-00000ca71011";
        let dumped = dump_avatar(
            object.to_owned(),
            false,
            true,
            true,
            None,
            None,
            &[],
            Vec3::ZERO,
        );
        assert!(dumped.is_control_avatar);
        assert!(!dumped.is_self);
        assert_eq!(dumped.id, object);
    }

    /// A one-faced object entry, for the tests that ask what a serialised entry
    /// looks like rather than what is in it.
    fn an_object_entry() -> ObjectDump {
        ObjectDump {
            id: "id".to_owned(),
            local_id: 1,
            pcode: "volume-0".to_owned(),
            position: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            drawn_position: None,
            drawn_rotation: None,
            scale: [1.0; 3],
            num_faces: 1,
            faces: vec![FaceDump {
                index: 0,
                texture: "texture".to_owned(),
                color: [1.0; 4],
                scale_s: 1.0,
                scale_t: 1.0,
                offset_s: 0.0,
                offset_t: 0.0,
                rotation: 0.0,
                bump: 0,
                shiny: 0,
                fullbright: false,
                glow: 0.0,
                material_id: None,
            }],
            visible: true,
            is_mesh: false,
            mesh_id: None,
            is_sculpt: false,
            lod: 3,
            is_flexible: false,
            is_light: false,
        }
    }

    /// The recorded divergence itself, in numbers: the cross-check's fixture NPC
    /// stands at `z = 25.95` wearing a box a quarter metre above its skull point,
    /// and the reference's document places that box at **26.20 m** — the wearer's
    /// position plus the wire offset — while it is *drawn* on the skull joint,
    /// 27.06 m up. Composing the reference's way is what makes the two documents
    /// comparable at all.
    #[test]
    fn a_worn_object_is_placed_where_the_reference_places_it() {
        let wearer = ReferencePose::root(Vec3::new(128.0, 136.0, 25.95), Quat::IDENTITY);
        let box_on_the_skull = LocalPose {
            position: Vec3::new(0.0, 0.0, 0.25),
            rotation: Quat::IDENTITY,
        };
        let placed = compose_worn(wearer, &[box_on_the_skull]);
        assert!(near(placed.position.to_array(), [128.0, 136.0, 26.20]));
        assert!(placed.rotation.abs_diff_eq(Quat::IDENTITY, 1e-5));
    }

    /// The wearer's own turn carries its attachment around with it: the offset is
    /// rotated by the avatar's rotation, and the attachment's orientation is its
    /// own rotation composed onto the avatar's.
    #[test]
    fn a_wearers_turn_carries_its_attachment_round() {
        // A quarter turn left, which sends a half metre "forward" (+x) to +y.
        let quarter = Quat::from_rotation_z(core::f32::consts::FRAC_PI_2);
        let wearer = ReferencePose::root(Vec3::new(100.0, 100.0, 25.0), quarter);
        let held = LocalPose {
            position: Vec3::new(0.5, 0.0, 0.0),
            rotation: Quat::IDENTITY,
        };
        let placed = compose_worn(wearer, &[held]);
        assert!(near(placed.position.to_array(), [100.0, 100.5, 25.0]));
        assert!(placed.rotation.abs_diff_eq(quarter, 1e-5));
    }

    /// A linked child of an attachment, composed the way
    /// `LLViewerObject::getPositionRegion` composes one — **against its parent's
    /// local rotation**, so the wearer's turn is applied once rather than twice.
    ///
    /// That is the reference's own arithmetic rather than the pose the child is
    /// drawn at, and it is deliberate: this document exists to be diffed against
    /// the reference's, and being right about a grandchild while disagreeing with
    /// the document under comparison buys nothing. The drawn pose is reported
    /// beside it instead.
    #[test]
    fn a_linked_child_of_an_attachment_follows_the_references_composition() {
        let quarter = Quat::from_rotation_z(core::f32::consts::FRAC_PI_2);
        let wearer = ReferencePose::root(Vec3::new(100.0, 100.0, 25.0), quarter);
        let root = LocalPose {
            position: Vec3::new(0.0, 0.0, 0.25),
            rotation: quarter,
        };
        let child = LocalPose {
            position: Vec3::new(1.0, 0.0, 0.0),
            rotation: Quat::IDENTITY,
        };
        let placed = compose_worn(wearer, &[root, child]);
        // The root lands a quarter metre above the wearer (its offset is along
        // the shared z), and the child a metre along the *root's* own x — which
        // the root's local quarter turn sends to +y — rather than along the
        // twice-turned axis the child is drawn on.
        assert!(near(placed.position.to_array(), [100.0, 101.0, 25.25]));
        assert!(placed.rotation.abs_diff_eq(quarter, 1e-5));
    }

    /// An avatar is reported where the simulator put its **object**, with the
    /// body root this viewer drew beside it.
    ///
    /// The two are a `root_drop` apart — the wire position is the centre of the
    /// physics capsule and the skeleton hangs from the feet — so reporting the
    /// drawn root made every avatar in the scene differ from the reference's
    /// document by that drop, and every attachment with them.
    #[test]
    fn an_avatar_is_reported_at_its_object_position() -> Result<(), TestError> {
        let placement = ReferencePose::root(Vec3::new(104.0, 136.0, 25.95), Quat::IDENTITY);
        let drawn = GlobalTransform::from(Transform::from_translation(sl_to_bevy_vec(&Vector {
            x: 104.0,
            y: 136.0,
            z: 25.009,
        })));
        let dumped = dump_avatar(
            "id".to_owned(),
            false,
            false,
            true,
            Some(placement),
            Some(&drawn),
            &[],
            Vec3::ZERO,
        );
        assert!(near(dumped.position, [104.0, 136.0, 25.95]));
        assert!(near(
            dumped
                .drawn_position
                .ok_or("an avatar with a body reports what was drawn")?,
            [104.0, 136.0, 25.009]
        ));
        Ok(())
    }

    /// A looping motion's clock is reported as **where in the motion** it
    /// landed, not as how long it has been running: two viewers start playing at
    /// different moments, so the raw elapsed time is never comparable and the
    /// wrapped one always is.
    #[test]
    fn a_motions_clock_is_reported_inside_the_motion() {
        // The catalogue's own twist: two seconds, looping.
        assert_eq!(loop_time(5.5, Some(2.0), Some(true)), Some(1.5));
        assert_eq!(loop_time(0.25, Some(2.0), Some(true)), Some(0.25));
        // A motion that plays once holds its last frame rather than wrapping
        // back to its first.
        assert_eq!(loop_time(5.5, Some(2.0), Some(false)), Some(2.0));
        // Nothing is invented for a motion whose asset has not arrived, or one
        // that declares no length.
        assert_eq!(loop_time(5.5, None, Some(true)), None);
        assert_eq!(loop_time(5.5, Some(0.0), Some(true)), None);
    }

    /// An avatar's entry lists what it is playing, with the simulator's sequence
    /// number where the simulator asked for it — and the list is emitted in the
    /// order it was handed in, which is the order the viewer applies it.
    #[test]
    fn an_avatar_entry_lists_what_it_is_playing() -> Result<(), TestError> {
        let playing = [
            PlayingAnimation {
                id: Uuid::from_u128(0x57A2),
                sequence: 2,
                time: 3.5,
                duration: Some(2.0),
                loops: Some(true),
                priority: Some(4),
                stopping: false,
            },
            PlayingAnimation {
                id: Uuid::from_u128(0x57A3),
                sequence: 0,
                time: 0.5,
                duration: None,
                loops: None,
                priority: None,
                stopping: true,
            },
        ];
        let dumped = dump_avatar(
            "id".to_owned(),
            true,
            false,
            true,
            None,
            None,
            &playing,
            Vec3::ZERO,
        );
        let first = dumped.animations.first().ok_or("no animation reported")?;
        assert_eq!(first.id, Uuid::from_u128(0x57A2).to_string());
        assert_eq!(first.sequence, Some(2));
        assert_eq!(first.loop_time, Some(1.5));
        assert_eq!(first.priority, Some(4));
        assert!(!first.stopping);
        // The viewer's own motion carries no simulator sequence number, and an
        // animation whose asset has not arrived invents no length or priority.
        let second = dumped.animations.get(1).ok_or("no second animation")?;
        assert_eq!(second.sequence, None);
        assert_eq!(second.duration, None);
        assert_eq!(second.loop_time, None);
        assert!(second.stopping);
        Ok(())
    }

    /// A worn object's entry carries the drawn pose beside the composed one, and
    /// an unworn object's does not — the reference emits neither key, so their
    /// presence must be exactly the case where the two poses answer different
    /// questions.
    #[test]
    fn only_a_worn_entry_carries_the_drawn_pose() -> Result<(), TestError> {
        let keys = |object: &ObjectDump| -> Result<Vec<String>, TestError> {
            let value: serde_json::Value = serde_json::from_str(&serde_json::to_string(object)?)?;
            let map = value
                .as_object()
                .ok_or("an object entry is a JSON object")?
                .clone();
            Ok(map.keys().cloned().collect())
        };
        let mut object = an_object_entry();
        assert!(!keys(&object)?.contains(&"drawn_position".to_owned()));
        object.drawn_position = Some([1.0, 2.0, 3.0]);
        object.drawn_rotation = Some([0.0, 0.0, 0.0, 1.0]);
        let worn = keys(&object)?;
        assert!(worn.contains(&"drawn_position".to_owned()));
        assert!(worn.contains(&"drawn_rotation".to_owned()));
        Ok(())
    }

    /// Every key the comparison matches on, spelled as `fstestscenedump.cpp`
    /// spells it. A rename here is not a schema change but a silent divergence
    /// in every object of every scene, which is why the list is written out.
    #[test]
    fn an_object_entry_carries_the_reference_key_set() -> Result<(), TestError> {
        let object = an_object_entry();
        let value: serde_json::Value = serde_json::from_str(&serde_json::to_string(&object)?)?;
        let map = value
            .as_object()
            .ok_or("an object entry is a JSON object")?;
        let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "faces",
                "id",
                "is_flexible",
                "is_light",
                "is_mesh",
                "is_sculpt",
                "local_id",
                "lod",
                "num_faces",
                "pcode",
                "position",
                "rotation",
                "scale",
                "visible",
            ]
        );
        let face = map
            .get("faces")
            .and_then(|faces| faces.get(0))
            .and_then(serde_json::Value::as_object)
            .ok_or("a face is a JSON object")?;
        let mut face_keys: Vec<&str> = face.keys().map(String::as_str).collect();
        face_keys.sort_unstable();
        assert_eq!(
            face_keys,
            vec![
                "bump",
                "color",
                "fullbright",
                "glow",
                "index",
                "offset_s",
                "offset_t",
                "rotation",
                "scale_s",
                "scale_t",
                "shiny",
                "texture",
            ]
        );
        Ok(())
    }
}
