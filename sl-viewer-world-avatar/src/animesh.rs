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
//! - `ControlAvatarState::ensure_spawned` spawns the control avatar's root as
//!   a child of the animesh root object entity, so it (and the rigged submeshes
//!   parented to it) follows the object as it moves (the reference viewer's
//!   `matchVolumeTransform` pins the control avatar to the root prim's render
//!   transform);
//! - [`apply_rigged_attachments`](crate::rigged_attachments::apply_rigged_attachments)
//!   binds
//!   the linkset's rigged submeshes to those joints (the animesh branch of the
//!   worn-rigged-mesh bind), recording the rig's joint position overrides on the
//!   control avatar rather than on any wearer;
//! - `ingest_object_animations` fetches each signalled animation's motion and
//!   `drive_control_avatars` reconciles each object's `ObjectAnimation` set
//!   into a merged per-root playing set, which — via
//!   `publish_control_avatars` publishing the object's root matrix to the
//!   GPU-avatar feed — the shared passes-A–D pipeline samples, blends and
//!   FK-poses in place (§5), exactly as it does an avatar's clips.
//!
//! Phase 4 removed the per-object joint entities and the CPU skinner: an animesh
//! is a GPU pose slot (`Animesh`)
//! keyed by its object rather than an avatar, posed against a rest (un-shaped)
//! skeleton — an animated object has no visual-param shape, only the joint
//! position overrides its own rigged meshes impose.

use std::collections::HashMap;

/// The animesh control-avatar pose publish's own scheduling (P29.2).
///
/// Publish each animesh control avatar's pose slot to the GPU feed (its object
/// world matrix + empty corrections) after transform propagation, so the GPU
/// samples / blends / FK-poses it in place (Phase 4 §5) — no per-object joint
/// entities remain.
#[derive(Debug, Default)]
pub struct AnimeshPosePlugin;

impl Plugin for AnimeshPosePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            publish_control_avatars.after(TransformSystems::Propagate),
        );
    }
}

use bevy::prelude::*;
use sl_client_bevy::{
    AnimationPose, AssetKey, JointOverrides, ObjectKey, ScopedObjectId, SlEvent, SlSessionEvent,
    Uuid,
};

use crate::animations::{
    AnimationManager, PlayState, reconcile_playing, resolve_pose, retain_active,
};
use crate::avatars::AvatarBody;
use crate::world_api::ObjectState;

/// Whether worn rigged meshes' joint position overrides (R1) are applied to the
/// avatar skeleton. On by default; `SL_VIEWER_JOINT_OVERRIDES=0` disables it, so the
/// pre-override skeleton behaviour can be compared side by side in one session.
#[must_use]
pub fn joint_overrides_enabled() -> bool {
    std::env::var("SL_VIEWER_JOINT_OVERRIDES").as_deref() != Ok("0")
}

/// A guard on the linkset-chain walk in [`animesh_root`], against a malformed
/// parent cycle.
const MAX_LINKSET_DEPTH: usize = 32;

/// The animesh linkset root that `scoped` belongs to (P29): walk its parent chain
/// up to the object carrying the animated-object flag and return that root's full
/// [`ObjectKey`] (the key its control avatar is filed under) and scene entity
/// (the control-avatar skeleton parents to it so it follows the object). `None`
/// if the chain reaches no animated-object root (not an animesh).
///
/// This walk is also how a signalled animation finds its control avatar
/// (P29.2): the sim keys `ObjectAnimation` by the linkset **part** holding the
/// animations (the prim the script runs in) — often a *child*, not the flagged
/// root — and the reference merges every part's signalled set into the root's
/// control avatar (`LLControlAvatar::updateAnimations` over the whole linkset).
#[must_use]
pub fn animesh_root(state: &ObjectState, scoped: ScopedObjectId) -> Option<(ObjectKey, Entity)> {
    let mut current = scoped;
    for _ in 0..MAX_LINKSET_DEPTH {
        let tracked = state.objects.get(&current)?;
        if tracked.animated {
            return Some((tracked.full_key, tracked.entity));
        }
        // A root's `parent` is its own scoped id; stop before looping forever.
        if tracked.parent == current {
            return None;
        }
        current = tracked.parent;
    }
    None
}

/// One animesh's control avatar (§5): the skeleton root the linkset's rigged
/// submeshes parent to, plus the joint position overrides its own rigged meshes
/// impose (R1). Phase 4 removed the per-object joint entities — the submeshes
/// bind the shared dummy joint and are GPU-posed in place on an
/// `Animesh` pose slot.
#[derive(Debug)]
struct ControlAvatar {
    /// The skeleton root anchor — an identity child of the animesh root object
    /// entity, so its world transform tracks the object as it moves. The rigged
    /// submeshes parent to it, and its `GlobalTransform` is the pose slot's root
    /// matrix (the SL → Bevy basis change + the object's world placement).
    root: Entity,
    /// The joint position overrides each of the linkset's rigged meshes imposes on
    /// this control avatar's skeleton (R1), keyed by the contributing mesh asset id
    /// — the animesh counterpart of [`AvatarState`](crate::world_api::AvatarState)'s
    /// per-avatar `joint_overrides`. Merged (highest mesh id wins per joint) into
    /// the effective set the GPU rest solve folds into the skeleton.
    overrides: HashMap<Uuid, JointOverrides>,
    /// Bumped whenever [`overrides`](Self::overrides) changes, so the GPU
    /// staging re-composes this slot's rest rows only on a real change (the
    /// animesh counterpart of an avatar's `pose_inputs_generation`).
    overrides_generation: u64,
}

/// Viewer-side animesh bookkeeping (P29): the control avatar per animated object,
/// plus its animation playback state — which animations each signalled part is
/// playing, their timing / activation order, and the merged per-root playing set
/// the GPU-avatar scheduler samples and blends this frame.
///
/// **Two different keys (P29.2).** The control avatars and poses are keyed by
/// the animesh **root**'s full [`ObjectKey`] (the flagged animated object the
/// skeleton hangs off). The signalled animations are keyed by the **part** the
/// sim named in `ObjectAnimation.Sender.ID` — the linkset prim holding the
/// animations (the one the script runs in), which is *often a child, not the
/// root*. The drivers resolve each signalled part up its linkset
/// ([`animesh_root`]) and merge every part's set into the
/// root's control avatar, exactly as the reference's
/// `LLControlAvatar::updateAnimations` merges the signalled maps of every
/// volume in the linkset. The playback half mirrors
/// [`AnimationPlayback`](crate::animations::AnimationPlayback) but per part.
#[derive(Debug, Resource, Default)]
pub struct ControlAvatarState {
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
    /// drivable animation and a spawned control avatar appear). Kept for the
    /// edge-triggered posing log only — the GPU samples/blends the clips itself.
    poses: HashMap<ObjectKey, AnimationPose>,
    /// Each root object's **merged** playing set this frame (the union of its
    /// linkset parts' sets), keyed by animation id — the animesh counterpart of
    /// [`AnimationPlayback::merged_active`](crate::animations::AnimationPlayback::merged_active).
    /// The GPU-avatar scheduler builds this slot's playback rows + sample jobs
    /// from it, so passes A+B blend the same motions the CPU resolver would.
    merged: HashMap<ObjectKey, HashMap<Uuid, PlayState>>,
}

/// The signalled-part cap: above this many parts with live animation sets, the
/// never-tracked ones are dropped (`ControlAvatarState::bound_signalled`). Far
/// above any real region's animesh count — a memory backstop for a long session
/// wandering many regions, since a part that is never tracked also never sends
/// the stop that would empty its set.
const MAX_SIGNALLED_PARTS: usize = 4096;

impl ControlAvatarState {
    /// Ensure a control avatar exists for the animesh root `object` (whose scene
    /// entity is `object_entity`), spawning its identity **root** as a child of
    /// the object entity on first call. Returns that root — the caller parents
    /// the linkset's rigged submeshes to it (§5: they carry no joint entities;
    /// the GPU poses them in place off the `Animesh` pose slot).
    ///
    /// The root is parented under the object entity so it follows the object's
    /// world transform (which already carries the Second Life → Bevy basis change
    /// and the object's world placement / rotation) and despawns with it. Its
    /// `GlobalTransform` is the pose slot's root matrix.
    pub(crate) fn ensure_spawned(
        &mut self,
        object: ObjectKey,
        object_entity: Entity,
        commands: &mut Commands,
    ) -> Entity {
        if let Some(control) = self.avatars.get(&object) {
            return control.root;
        }
        let root = commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                ChildOf(object_entity),
            ))
            .id();
        debug!("animesh {object}: spawned control avatar root");
        let _prev = self.avatars.insert(
            object,
            ControlAvatar {
                root,
                overrides: HashMap::new(),
                overrides_generation: 0,
            },
        );
        root
    }

    /// Every animesh root that has a spawned control avatar — the GPU staging
    /// enumerates these as its animesh pose slots.
    pub(crate) fn animesh_roots(&self) -> impl Iterator<Item = ObjectKey> + '_ {
        self.avatars.keys().copied()
    }

    /// `object`'s override generation, bumped whenever its effective joint
    /// overrides change (the GPU rest-row re-compose trigger). `0` for an object
    /// with no control avatar.
    pub(crate) fn overrides_generation(&self, object: ObjectKey) -> u64 {
        self.avatars
            .get(&object)
            .map_or(0, |control| control.overrides_generation)
    }

    /// `object`'s merged playing set as owned `(animation id, play state)` pairs
    /// — the animesh counterpart of
    /// [`AnimationPlayback::merged_active`](crate::animations::AnimationPlayback::merged_active),
    /// which the GPU scheduler builds this slot's playback rows + sample jobs from.
    #[must_use]
    pub(crate) fn merged_active(&self, object: ObjectKey) -> Vec<(Uuid, PlayState)> {
        self.merged
            .get(&object)
            .into_iter()
            .flat_map(|set| set.iter().map(|(&anim, play)| (anim, *play)))
            .collect()
    }

    /// The parts with a live signalled animation set (the `ObjectAnimation`
    /// senders). Used to spawn a control avatar early — as soon as any part of
    /// an animesh linkset has an animation — rather than waiting for its mesh
    /// to bind, so an animation that arrives before the (much later) mesh
    /// decode is not lost (P29); the caller resolves each part to its flagged
    /// root ([`animesh_root`]).
    pub(crate) fn signalled_parts(&self) -> std::collections::HashSet<ObjectKey> {
        self.playing.keys().copied().collect()
    }

    /// Record the joint position overrides that rigged `mesh` imposes on `object`'s
    /// control-avatar skeleton (R1), replacing any previous contribution from that
    /// mesh. A no-op for an object with no spawned control avatar. Mirrors
    /// [`AvatarState::record_joint_overrides`](crate::world_api::AvatarState).
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
        // A real change: bump the generation so the GPU staging re-composes this
        // slot's rest rows (its skeleton was repositioned by the rig).
        control.overrides_generation = control.overrides_generation.wrapping_add(1);
    }

    /// The effective joint position overrides for `object`'s control avatar (R1):
    /// the per-joint winner across every one of the linkset's rigged meshes,
    /// resolved to the highest mesh id on a conflict (the reference viewer's
    /// `findActiveOverride`). Empty when the linkset carries no position-bearing rig.
    pub(crate) fn effective_overrides(&self, object: ObjectKey) -> JointOverrides {
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
        self.merged.retain(|&object, _| keep(object));
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
/// ([`animesh_root`]) — merging the sets of all parts of one
/// linkset, as the reference's `LLControlAvatar::updateAnimations` does — then
/// blends each root's motions into an [`AnimationPose`] against the standard
/// skeleton (a control avatar has no visual-param shape, so joint names resolve
/// through the shared `AvatarBody::joint_index`). A root with no spawned
/// control avatar or no drivable motion is omitted, so it keeps its bind-pose
/// rest.
pub(crate) fn drive_control_avatars(
    time: Res<Time>,
    mut events: MessageReader<SlEvent>,
    manager: Res<AnimationManager>,
    state: Res<crate::world_api::ObjectState>,
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
        control.merged.clear();
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
        let Some((root, _entity)) = animesh_root(&state, scoped) else {
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
    // Keep only the merged sets of roots with a spawned control avatar, so the
    // GPU scheduler (`crate::gpu_avatars`) iterates exactly the animesh slots it
    // can pose. The set is what passes A+B sample and blend GPU-side.
    let mut root_merged: HashMap<ObjectKey, HashMap<Uuid, PlayState>> = HashMap::new();
    for (&root, anims) in &merged {
        // Only a root with a spawned control avatar can be posed.
        if !control.avatars.contains_key(&root) {
            continue;
        }
        let _prev = root_merged.insert(root, anims.clone());
        if let Some(pose) = resolve_pose(anims, now, &manager, |name| body.joint_index(name)) {
            let _prev = poses.insert(root, pose);
        }
    }
    // The GPU staging reads this every frame, so no write-on-change guard is
    // needed here (`PlayState` is not `PartialEq`).
    control.merged = root_merged;
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
    // Kept for the edge-triggered posing log above (the GPU does the real
    // blend); write-on-change so an idle animesh does not re-dirty the resource.
    if control.poses != poses {
        control.poses = poses;
    }
}

/// Publish each animesh control avatar's pose slot to the GPU feed (§5): its
/// root matrix (the object's Bevy world transform, the SL → Bevy basis change +
/// world placement) and — since an animesh has no procedural adjusters — an
/// empty correction list. The GPU then samples, blends and FK-poses the
/// skeleton in place (passes A–D), exactly like an avatar; there are no per-
/// object joint entities to write.
///
/// Runs in `PostUpdate` **after** transform propagation (so the control-avatar
/// root's `GlobalTransform` is current) and **before**
/// [`stage_gpu_avatars`](crate::gpu_avatars) reads the feed. A no-op on a
/// downlevel device, where the GPU pipeline is inactive.
pub(crate) fn publish_control_avatars(
    control: Res<ControlAvatarState>,
    mut feed: ResMut<crate::gpu_avatars::GpuAvatarPoseFeed>,
    mode: Option<Res<crate::gpu_avatars::GpuAvatarsMode>>,
    globals: Query<&GlobalTransform>,
) {
    if !mode.is_some_and(|mode| mode.active) {
        return;
    }
    for (&object, avatar) in &control.avatars {
        let Ok(root_global) = globals.get(avatar.root) else {
            continue;
        };
        feed.publish_real(
            crate::world_api::PoseSlotKey::Animesh(object),
            root_global.to_matrix(),
            Vec::new(),
        );
    }
}
