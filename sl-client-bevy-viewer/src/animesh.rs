//! Animesh (animated-object) rendering — Phase 29.
//!
//! A scripted linkset whose root carries the `ExtendedMesh`
//! `ANIMATED_MESH_ENABLED` flag is an *animated object*: it drives its own
//! skeleton — the reference viewer's `LLControlAvatar`, a headless avatar with no
//! wearer — so its rigged meshes deform under `ObjectAnimation` the way a worn
//! rigged mesh deforms under its avatar's `AvatarAnimation`. Without this an
//! animesh renders as a static, un-posed rigged mesh stuck at its bind pose.
//!
//! The control avatar reuses the standard avatar skeleton ([`AvatarBody`]) and the
//! Phase 18 blend driver:
//!
//! - [`ControlAvatarState::ensure_spawned`] instances the skeleton joints as a
//!   child of the animesh root object entity, so the whole skeleton follows the
//!   object as it moves (the reference viewer's `matchVolumeTransform` pins the
//!   control avatar to the root prim's render transform);
//! - [`apply_rigged_attachments`](crate::objects::apply_rigged_attachments) binds
//!   the linkset's rigged submeshes to those joints (the animesh branch of the
//!   worn-rigged-mesh bind), recording the rig's joint position overrides on the
//!   control avatar rather than on any wearer;
//! - [`ingest_object_animations`] fetches each signalled animation's motion,
//!   [`drive_control_avatars`] folds each object's `ObjectAnimation` set into a
//!   blended per-joint [`AnimationPose`], and [`pose_control_avatars`] writes that
//!   pose into the control avatar's joint world matrices.
//!
//! The two driver systems mirror
//! [`drive_avatar_skeletons`](crate::animations::drive_avatar_skeletons) /
//! [`pose_avatar_skeletons`](crate::animations::pose_avatar_skeletons) exactly, but
//! keyed by the animesh object rather than an avatar and against a rest
//! (un-shaped) skeleton — an animated object has no visual-param shape, only the
//! joint position overrides its own rigged meshes impose.

use std::collections::HashMap;

use bevy::math::Affine3A;
use bevy::prelude::*;
use sl_client_bevy::{
    AnimationPose, AssetKey, JointOverrides, ObjectKey, SkeletalDeformations, SlEvent,
    SlSessionEvent, Uuid, VolumeDeformations,
};

use crate::animations::{
    AnimationManager, PlayState, reconcile_playing, resolve_pose, retain_active,
};
use crate::avatar_assets::AvatarAssetLibrary;
use crate::avatars::AvatarBody;

/// One animesh's control avatar: the skeleton-instance root and joint entities,
/// plus the joint position overrides its own rigged meshes impose (R1).
struct ControlAvatar {
    /// The skeleton root anchor — an identity child of the animesh root object
    /// entity, so its world transform (and therefore the whole skeleton) tracks
    /// the object as it moves. Composed with each joint's Second Life world matrix
    /// to place the posed joints in Bevy world space.
    root: Entity,
    /// The skeleton-instance joint entities, in joint order (parallel to
    /// [`AvatarBody`]'s joint tables) — the entities the linkset's rigged submeshes
    /// bind to and the pose driver writes each frame.
    joints: Vec<Entity>,
    /// The joint position overrides each of the linkset's rigged meshes imposes on
    /// this control avatar's skeleton (R1), keyed by the contributing mesh asset id
    /// — the animesh counterpart of [`AvatarState`](crate::avatars::AvatarState)'s
    /// per-avatar `joint_overrides`. Merged (highest mesh id wins per joint) into
    /// the effective set the pose driver folds into the skeletal recurrence.
    overrides: HashMap<Uuid, JointOverrides>,
}

/// Viewer-side animesh bookkeeping (P29): the control avatar per animated object,
/// plus its animation playback state — which animations each signalled part is
/// playing, their timing / activation order, and the per-joint pose the driver
/// blended this frame for [`pose_control_avatars`] to write.
///
/// **Two different keys (P29.2).** The control avatars and poses are keyed by
/// the animesh **root**'s full [`ObjectKey`] (the flagged animated object the
/// skeleton hangs off). The signalled animations are keyed by the **part** the
/// sim named in `ObjectAnimation.Sender.ID` — the linkset prim holding the
/// animations (the one the script runs in), which is *often a child, not the
/// root*. The drivers resolve each signalled part up its linkset
/// ([`crate::objects::animesh_root`]) and merge every part's set into the
/// root's control avatar, exactly as the reference's
/// `LLControlAvatar::updateAnimations` merges the signalled maps of every
/// volume in the linkset. The playback half mirrors
/// [`AnimationPlayback`](crate::animations::AnimationPlayback) but per part.
#[derive(Resource, Default)]
pub(crate) struct ControlAvatarState {
    /// The control avatar per animesh root object.
    avatars: HashMap<ObjectKey, ControlAvatar>,
    /// The currently-playing animations per **signalled part** (the
    /// `ObjectAnimation` sender), keyed by animation id. Persistent across the
    /// part being untracked: an `ObjectAnimation` routinely arrives *before*
    /// the part's first `ObjectUpdate`, and the reference keeps its signalled
    /// map for the whole session (`LLObjectSignaledAnimationMap`) — see
    /// [`bound_signalled`](Self::bound_signalled) for the safety cap.
    playing: HashMap<ObjectKey, HashMap<Uuid, PlayState>>,
    /// The next activation-recency stamp to hand out (see
    /// [`AnimationPlayback`](crate::animations::AnimationPlayback)).
    next_order: u64,
    /// Each root object's resolved per-joint pose this frame (only roots with a
    /// drivable animation and a spawned control avatar appear).
    poses: HashMap<ObjectKey, AnimationPose>,
}

/// The signalled-part cap: above this many parts with live animation sets, the
/// never-tracked ones are dropped ([`ControlAvatarState::bound_signalled`]). Far
/// above any real region's animesh count — a memory backstop for a long session
/// wandering many regions, since a part that is never tracked also never sends
/// the stop that would empty its set.
const MAX_SIGNALLED_PARTS: usize = 4096;

impl ControlAvatarState {
    /// Ensure a control avatar exists for the animesh root `object` (whose scene
    /// entity is `object_entity`), spawning the standard skeleton as an identity
    /// child of the object entity on first call. Returns the skeleton root and the
    /// joint entities the caller binds the linkset's rigged submeshes to.
    ///
    /// The root is parented under the object entity so the whole skeleton follows
    /// the object's world transform (which already carries the Second Life → Bevy
    /// basis change and the object's world placement / rotation) and despawns with
    /// it. The joint local transforms do not place the final geometry — the pose
    /// driver overwrites each joint's world matrix in `PostUpdate` — but they seed
    /// the hierarchy so the joints exist to bind to.
    pub(crate) fn ensure_spawned(
        &mut self,
        object: ObjectKey,
        object_entity: Entity,
        body: &AvatarBody,
        commands: &mut Commands,
    ) -> (Entity, Vec<Entity>) {
        if let Some(control) = self.avatars.get(&object) {
            return (control.root, control.joints.clone());
        }
        let root = commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                ChildOf(object_entity),
            ))
            .id();
        let joints = body.spawn_bare_skeleton(root, commands);
        debug!(
            "animesh {object}: spawned control avatar ({} joints)",
            joints.len()
        );
        let _prev = self.avatars.insert(
            object,
            ControlAvatar {
                root,
                joints: joints.clone(),
                overrides: HashMap::new(),
            },
        );
        (root, joints)
    }

    /// The parts with a live signalled animation set (the `ObjectAnimation`
    /// senders). Used to spawn a control avatar early — as soon as any part of
    /// an animesh linkset has an animation — rather than waiting for its mesh
    /// to bind, so an animation that arrives before the (much later) mesh
    /// decode is not lost (P29); the caller resolves each part to its flagged
    /// root ([`crate::objects::animesh_root`]).
    pub(crate) fn signalled_parts(&self) -> std::collections::HashSet<ObjectKey> {
        self.playing.keys().copied().collect()
    }

    /// Record the joint position overrides that rigged `mesh` imposes on `object`'s
    /// control-avatar skeleton (R1), replacing any previous contribution from that
    /// mesh. A no-op for an object with no spawned control avatar. Mirrors
    /// [`AvatarState::record_joint_overrides`](crate::avatars::AvatarState).
    pub(crate) fn record_overrides(
        &mut self,
        object: ObjectKey,
        mesh: Uuid,
        overrides: JointOverrides,
    ) {
        let Some(control) = self.avatars.get_mut(&object) else {
            return;
        };
        if control.overrides.get(&mesh) == Some(&overrides) {
            return;
        }
        if overrides.is_empty() {
            let _prev = control.overrides.remove(&mesh);
        } else {
            let _prev = control.overrides.insert(mesh, overrides);
        }
    }

    /// The effective joint position overrides for `object`'s control avatar (R1):
    /// the per-joint winner across every one of the linkset's rigged meshes,
    /// resolved to the highest mesh id on a conflict (the reference viewer's
    /// `findActiveOverride`). Empty when the linkset carries no position-bearing rig.
    fn effective_overrides(&self, object: ObjectKey) -> JointOverrides {
        let Some(control) = self.avatars.get(&object) else {
            return JointOverrides::default();
        };
        // Merge in ascending mesh-id order so the highest mesh id wins each joint.
        let mut meshes: Vec<(&Uuid, &JointOverrides)> = control.overrides.iter().collect();
        meshes.sort_by_key(|(mesh, _)| **mesh);
        let mut effective = JointOverrides::default();
        for (_mesh, overrides) in meshes {
            effective.merge(overrides);
        }
        effective
    }

    /// Drop the control avatar and pose for every animesh root that is no longer
    /// live (`keep(object)` is `false`). The skeleton entities despawn with
    /// their object entity (Bevy's recursive hierarchy despawn), so only the
    /// bookkeeping is dropped here. The signalled-animation sets are **not**
    /// touched (P29.2): they key by part, arrive before tracking, and must
    /// survive it — see [`bound_signalled`](Self::bound_signalled).
    pub(crate) fn retain(&mut self, keep: impl Fn(ObjectKey) -> bool) {
        self.avatars.retain(|&object, _| keep(object));
        self.poses.retain(|&object, _| keep(object));
    }

    /// The memory backstop on the persistent signalled-animation map: once more
    /// than [`MAX_SIGNALLED_PARTS`] parts hold a set, drop the ones that are not
    /// currently tracked (`keep(part)` is `false`) — the never-streamed
    /// attachments of hidden avatars are the bulk of those. A no-op below the
    /// cap, so the ordinary early-arrival buffer is never disturbed.
    pub(crate) fn bound_signalled(&mut self, keep: impl Fn(ObjectKey) -> bool) {
        if self.playing.len() <= MAX_SIGNALLED_PARTS {
            return;
        }
        self.playing.retain(|&part, _| keep(part));
    }
}

/// Ingest each `ObjectAnimation` update and request every signalled animation's
/// motion, so it is fetched and decoded ready for the control-avatar driver — the
/// animesh counterpart of
/// [`ingest_avatar_animations`](crate::animations::ingest_avatar_animations),
/// sharing the same [`AnimationManager`]. The request is idempotent.
pub(crate) fn ingest_object_animations(
    mut events: MessageReader<SlEvent>,
    mut manager: ResMut<AnimationManager>,
) {
    for event in events.read() {
        if let SlSessionEvent::ObjectAnimation { animations, .. } = &event.0 {
            for animation in animations {
                manager.request(AssetKey::from(animation.anim_id.uuid()));
            }
        }
    }
}

/// Resolve each animesh control avatar's per-joint animation pose from the motions
/// its linkset is playing (P29.2), the animesh mirror of
/// [`drive_avatar_skeletons`](crate::animations::drive_avatar_skeletons).
///
/// Each frame it folds the latest `ObjectAnimation` updates into the
/// per-**part** playback clock (the sim keys the message by the linkset prim
/// holding the animations, not the flagged root), drops fully-eased-out
/// motions, resolves every signalled part up its linkset to the animesh root
/// ([`crate::objects::animesh_root`]) — merging the sets of all parts of one
/// linkset, as the reference's `LLControlAvatar::updateAnimations` does — then
/// blends each root's motions into an [`AnimationPose`] against the standard
/// skeleton (a control avatar has no visual-param shape, so joint names resolve
/// through the shared [`AvatarBody::joint_index`]). A root with no spawned
/// control avatar or no drivable motion is omitted, so it keeps its bind-pose
/// rest.
pub(crate) fn drive_control_avatars(
    time: Res<Time>,
    mut events: MessageReader<SlEvent>,
    manager: Res<AnimationManager>,
    state: Res<crate::objects::ObjectState>,
    mut control: ResMut<ControlAvatarState>,
    body: Option<Res<AvatarBody>>,
) {
    let now = time.elapsed_secs();
    let control = control.as_mut();
    // Reconcile the playback clock with each authoritative animation set. The
    // key is the *sender part*, kept even while the part is untracked — the
    // message routinely precedes the part's first `ObjectUpdate`.
    for event in events.read() {
        if let SlSessionEvent::ObjectAnimation {
            object_id,
            animations,
        } = &event.0
        {
            let pairs: Vec<(Uuid, i32)> = animations
                .iter()
                .map(|animation| (animation.anim_id.uuid(), animation.sequence_id))
                .collect();
            let entry = control.playing.entry(*object_id).or_default();
            reconcile_playing(entry, &mut control.next_order, &pairs, now);
        }
    }
    // Drop fully-eased-out motions; forget parts whose set emptied.
    control.playing.retain(|_part, anims| {
        retain_active(anims, now, &manager);
        !anims.is_empty()
    });
    // Without the avatar asset library there is no skeleton to resolve names for.
    let Some(body) = body else {
        control.poses.clear();
        return;
    };
    // Resolve each signalled part to its animesh root and merge the linkset's
    // sets. A part that is untracked, or whose chain reaches no flagged root,
    // contributes nothing (its set stays buffered for when tracking catches up).
    let parts: std::collections::HashSet<ObjectKey> = control.playing.keys().copied().collect();
    let scoped_by_full = state.scoped_by_full_keys(&parts);
    let mut merged: HashMap<ObjectKey, HashMap<Uuid, PlayState>> = HashMap::new();
    for (&part, anims) in &control.playing {
        let Some(&scoped) = scoped_by_full.get(&part) else {
            continue;
        };
        let Some((root, _entity)) = crate::objects::animesh_root(&state, scoped) else {
            continue;
        };
        let entry = merged.entry(root).or_default();
        for (&anim, play) in anims {
            // Two parts of one linkset playing the same animation id is
            // degenerate; the first part wins, matching the reference's map
            // merge.
            let _prev = entry.entry(anim).or_insert(*play);
        }
    }
    let mut poses: HashMap<ObjectKey, AnimationPose> = HashMap::new();
    for (&root, anims) in &merged {
        // Only a root with a spawned control avatar can be posed.
        if !control.avatars.contains_key(&root) {
            continue;
        }
        if let Some(pose) = resolve_pose(anims, now, &manager, |name| body.joint_index(name)) {
            let _prev = poses.insert(root, pose);
        }
    }
    // Edge-triggered logging: an object starting / stopping being posed is the live
    // signal that a keyframe motion decoded and drove its control avatar.
    for &object in poses.keys() {
        if !control.poses.contains_key(&object) {
            debug!("animesh: posing control avatar for object {object}");
        }
    }
    for &object in control.poses.keys() {
        if !poses.contains_key(&object) {
            debug!("animesh: released control avatar for object {object} back to rest");
        }
    }
    control.poses = poses;
}

/// Write each posed animesh control avatar's animated joint world matrices straight
/// into its joint entities' `GlobalTransform`s (P29.2), the animesh mirror of
/// [`pose_avatar_skeletons`](crate::animations::pose_avatar_skeletons).
///
/// Runs in `PostUpdate` **after** transform propagation, so it overwrites the
/// just-propagated joint globals with the animated ones for the frame's skinning.
/// For each posed object it re-runs the Second Life skeletal recurrence with the
/// resolved [`AnimationPose`] and the linkset's effective joint overrides folded in
/// (against a rest [`SkeletalDeformations`] — an animated object has no shape),
/// composes each joint's Second Life world matrix with the control-avatar-root
/// global (its object's Bevy world transform), and writes it to the joint entity.
///
/// A control avatar with no pose this frame is still written each frame at its rest
/// (empty) pose, so a stopped animation returns it to its bind pose and overlapping
/// animations compose without a per-animation reset (Bevy's dirty-bit propagation
/// cannot un-freeze a joint global the driver overwrote).
pub(crate) fn pose_control_avatars(
    control: Res<ControlAvatarState>,
    library: Option<Res<AvatarAssetLibrary>>,
    mut globals: Query<&mut GlobalTransform>,
) {
    let Some(library) = library else {
        return;
    };
    // A control avatar (animesh) has no visual params, so both the skeletal and the
    // collision-volume deformations are the rest ones.
    let rest = SkeletalDeformations::default();
    let rest_volumes = VolumeDeformations::default();
    let empty = AnimationPose::default();
    for (&object, avatar) in &control.avatars {
        let pose = control.poses.get(&object).unwrap_or(&empty);
        let overrides = control.effective_overrides(object);
        let world =
            library
                .skeleton()
                .deformed_world_matrices(&rest, &rest_volumes, &overrides, pose);
        // The control-avatar-root global carries the object's Bevy world transform
        // (the SL → Bevy basis change + world placement); each joint's Bevy global
        // is that composed with its Second Life world matrix. Copied out so the
        // mutable joint writes below do not overlap the read.
        let Ok(root_global) = globals.get(avatar.root) else {
            continue;
        };
        // `mul_mat4` (a method, not the `*` operator) keeps clear of the workspace
        // `arithmetic_side_effects` lint the glam operators trip.
        let root_matrix = root_global.to_matrix();
        for (entity, matrix) in avatar.joints.iter().zip(world.iter()) {
            if let Ok(mut global) = globals.get_mut(*entity) {
                *global = GlobalTransform::from(Affine3A::from_mat4(root_matrix.mul_mat4(matrix)));
            }
        }
    }
}
