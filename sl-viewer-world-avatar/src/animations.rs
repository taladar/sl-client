//! Resolve an avatar-animation UUID to its decoded keyframe [`Motion`] (P18.2).
//!
//! When the simulator signals that an avatar is playing an animation
//! ([`SlSessionEvent::AvatarAnimation`]), the viewer needs the animation's
//! playable [`Motion`] to pose that avatar's skeleton (P18.3). This module owns
//! the resolver that turns each signalled UUID into a decoded, cached motion,
//! mirroring the texture / mesh / wearable-asset managers.
//!
//! Resolution follows the reference viewer's split (see [`sl_anim::registry`]):
//!
//! - A **procedural** built-in (walk / run / stand / turn / the `LLEmote`
//!   expressions / the always-on adjusters) has no downloadable asset, so it is
//!   recorded as unavailable and never fetched — driving it is the synthesis
//!   work deferred past this MVP.
//! - A **downloadable built-in** (the waves / bows / dances) or an **uploaded**
//!   animation is fetched as an ordinary `.anim` asset: first from a
//!   `<uuid>.anim` file under the `--viewer-assets` directory (a
//!   pre-provisioned built-in), and otherwise over the `ViewerAsset` capability
//!   (the same generic-asset store the wearable fetch uses). Stock viewers ship
//!   no such local `.anim` files, so in practice both built-in and uploaded
//!   downloadable animations arrive over `ViewerAsset`; the local path is the
//!   escape hatch for a hand-populated built-in library.
//!
//! The fetched bytes are decoded off the render thread on Bevy's [`IoTaskPool`]
//! and the resulting [`Motion`] is cached by UUID, shared across every avatar
//! playing it.
//!
//! The module also owns the P18.3 skeleton driver: `drive_avatar_skeletons`
//! folds each avatar's `AvatarAnimation` set into a playback clock and resolves a
//! per-joint [`AnimationPose`] from the playing motions, and
//! `pose_avatar_skeletons` writes that pose into the skeleton-instance joints'
//! world matrices (in `PostUpdate`, after transform propagation) — recomputing the
//! Second Life skeletal recurrence so a shaped avatar's limbs keep their length
//! under animation rather than shearing.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use sl_anim::{
    HandPose, JointContribution, JointPriority, KeyframeMotionClass, Motion, blend_joint,
    builtin_animation,
};
use sl_client_bevy::{
    AgentKey, AnimationPose, AssetCacheLimits, AssetKey, AssetStore, AssetType, BevyAssetFetcher,
    BevySkeleton, BlobFetcher, CAP_VIEWER_ASSET, GateStats, JointOverrides, SkeletalDeformations,
    SlCapabilities, SlEvent, SlSessionEvent, StoreStats, Uuid, VolumeDeformations, sample_motion,
};

use crate::avatar_assets::AvatarAssetLibrary;
use crate::avatars::{AvatarBody, AvatarBodyPart, AvatarRuntimeMorphs};
use crate::body_physics::{BodyPhysicsInput, BodyPhysicsMotion};
use crate::ground::AvatarGround;
use crate::locomotion_ik::{AdjustInput, AdjusterAnims, LegJoints, LocomotionAdjust};
use crate::look_at::{
    BLINK_LEFT_PARAM, BLINK_RIGHT_PARAM, LookAtJoints, LookAtMotion, LookAtTargets,
};
use crate::reach::{PointAtTargets, ReachInput, ReachJoints, ReachMotion};
use crate::world_api::AvatarState;
use crate::world_api::{AvatarMotion, DerenderKind, WorldPhase, world_has_keyboard};

/// The avatar animation pipeline's own scheduling.
///
/// Keep the animation store's `ViewerAsset` cap current, request a motion for
/// every animation each nearby avatar is playing, and fold finished resolves
/// into the shared motion cache (P18.2); then drive each rigged avatar's
/// skeleton from its playing motions, overlaying the sampled keyframe poses onto
/// the appearance rest pose (P18.3, hence after
/// [`WorldPhase::AvatarAppearanceApplied`]).
///
/// The settled playing set is published as [`WorldPhase::AvatarSkeletonsDriven`]
/// so the name-tag composer — which also waits on the group store, a crate above
/// this one — can order against it from up there.
#[derive(Debug, Default)]
pub struct AvatarAnimationPlugin;

impl Plugin for AvatarAnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                update_animation_caps,
                ingest_avatar_animations,
                poll_animations,
                // Client-side locomotion / state animations for the own avatar
                // (P31.6): derive its movement state from the P31.4 velocity +
                // P31.5 controls and play the matching built-in animation when
                // the simulator is silent about it. After the controls (so it
                // reads the freshly advertised intent) and before the skeleton
                // driver (so its client-driven set is reconciled into the same
                // frame's pose).
                crate::locomotion::drive_own_locomotion
                    .after(WorldPhase::AvatarControlsDriven)
                    .before(WorldPhase::AvatarSkeletonsDriven)
                    .run_if(world_has_keyboard),
                // Typing state animation for the own avatar (P31.9): reconcile
                // the typing state the nearby-chat bar drives, play
                // `ANIM_AGENT_TYPE` locally, and broadcast a `StartTyping` /
                // `StopTyping` `ChatFromViewer`. Not gated on
                // `world_has_keyboard` — typing happens while the *chat field*
                // holds the keyboard (the TextEntry context), so that gate would
                // suppress it. Like locomotion it must reconcile its
                // client-driven set before the skeleton driver folds it into the
                // frame's pose.
                crate::typing::drive_own_typing.before(WorldPhase::AvatarSkeletonsDriven),
                drive_avatar_skeletons
                    .in_set(WorldPhase::AvatarSkeletonsDriven)
                    .after(WorldPhase::AvatarAppearanceApplied),
                // Hand-pose morph (P31.13): cross-fade each avatar's hands into
                // the pose its highest-priority playing animation asks for.
                // After the skeleton driver (whose playing set it reads) and
                // before the runtime-morph fold, so the cross-faded weights reach
                // the GPU in the same frame.
                crate::hand_pose::drive_hand_poses
                    .after(WorldPhase::AvatarSkeletonsDriven)
                    .before(WorldPhase::AvatarMorphsFolded),
                // Head & eye look-at tracking (P31.12): derive the own avatar's
                // look-at target from the fly-camera, and ingest nearby avatars'
                // `ViewerEffect` look-at gaze hints. The pose pass (PostUpdate)
                // reads both.
                crate::look_at::update_own_look_at_target,
                crate::look_at::receive_look_at_effects,
                // Activity-driven reach & aim (P31.15): the own avatar's object
                // selection (the E key) and the point-at effect it publishes,
                // other avatars' point-at effects, and the G key that plays an
                // aim animation through the simulator so the targeting motion
                // engages the way a scripted weapon would drive it. The pose pass
                // (PostUpdate) reads the resulting targets.
                (
                    crate::reach::select_object_under_crosshair.run_if(world_has_keyboard),
                    crate::reach::drive_own_point_at
                        .after(crate::reach::select_object_under_crosshair),
                    crate::reach::receive_point_at_effects,
                    crate::reach::drive_aim_animation.run_if(world_has_keyboard),
                ),
                // Avatar ground probe (P31.14): resolve what is under each
                // avatar's root and ankles — the terrain land height combined
                // with the simulator's collision (foot) plane, as the reference
                // viewer's `getGround` does — for the foot IK and the landing
                // recovery. It reads the ankle joint globals the pose pass wrote
                // *last* frame.
                crate::ground::probe_avatar_ground,
                // Animesh (P29): request each animated object's animation
                // motions, drive its control-avatar skeleton from them (after its
                // rigged meshes bind in `apply_rigged_attachments`), and drop
                // control avatars whose object is gone (after the object update
                // has processed removals).
                crate::animesh::ingest_object_animations,
                crate::animesh::drive_control_avatars
                    .after(crate::rigged_attachments::apply_rigged_attachments),
                // Spawn a control avatar as soon as an animesh has an animation
                // playing (after `drive_control_avatars` folds the
                // `ObjectAnimation` into the playback clock), so a late mesh bind
                // does not lose an early animation.
                crate::rigged_attachments::spawn_animesh_control_avatars
                    .after(crate::animesh::drive_control_avatars),
                crate::rigged_attachments::prune_control_avatars.after(WorldPhase::ObjectsUpdated),
            ),
        );
    }
}

/// The avatar pose pass's own scheduling (P18.3).
///
/// Write the posed avatars' animated joint world matrices straight into their
/// [`GlobalTransform`]s, after transform propagation has produced the rest
/// globals this frame — so the animated pose is what skinning / render
/// extraction reads, without the limb-shear a rotation overlaid on the
/// baked-scale local transform would cause.
#[derive(Debug, Default)]
pub struct AvatarPosePlugin;

impl Plugin for AvatarPosePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            pose_avatar_skeletons.after(TransformSystems::Propagate),
        );
    }
}

/// The animation resolve/decode/cache pipeline: an [`AssetStore`] over the
/// `ViewerAsset` capability (for downloadable `.anim` assets), the optional
/// `--viewer-assets` directory (for pre-provisioned built-in `.anim` files), the
/// in-flight resolve tasks, the decoded motions already in hand, and the set of
/// ids known to have no fetchable asset (procedural built-ins / failed fetches).
///
/// Mirrors [`MeshManager`](crate::meshes::MeshManager) /
/// [`WearableAssetManager`](crate::bake_inputs::WearableAssetManager).
#[derive(Debug, Resource)]
pub struct AnimationManager {
    /// The generic-asset store doing the `ViewerAsset` fetch, dedupe, off-thread
    /// work, and on-disk caching of `.anim` bytes.
    store: AssetStore,
    /// The store's HTTP fetcher, kept so its `ViewerAsset` capability URL can be
    /// refreshed on a region change.
    fetcher: Arc<BevyAssetFetcher>,
    /// The background resolve+decode task per animation id, polled to completion
    /// by [`poll_animations`]; presence means "already being resolved".
    inflight: HashMap<AssetKey, Task<Option<Motion>>>,
    /// Successfully decoded motions by id, shared across every avatar playing the
    /// animation so it is fetched and decoded once.
    motions: HashMap<AssetKey, Arc<Motion>>,
    /// Ids with no fetchable/decodable asset — a procedural built-in, or a fetch
    /// that failed — so [`request`](Self::request) does not retry them forever.
    unavailable: HashSet<AssetKey>,
    /// Ids requested before the region's `ViewerAsset` capability was known (and
    /// not shipped as a static asset either), held here so the fetch is not run —
    /// and the id not marked permanently [`unavailable`](Self::unavailable) — until
    /// the cap arrives. Drained by `retry_pending`.
    pending: HashSet<AssetKey>,
}

impl Default for AnimationManager {
    /// [`AnimationManager::new`]: the manager takes no configuration of its
    /// own — where an animation's bytes come from is the asset store's
    /// business.
    fn default() -> Self {
        Self::new()
    }
}

impl AnimationManager {
    /// Build the manager over a fresh [`BevyAssetFetcher`], backed by the on-disk
    /// asset cache when a cache directory is available (falling back to an
    /// in-memory-only store).
    ///
    /// A built-in animation the viewer *ships* is answered by the store's
    /// static-asset library (`sl_asset::static_assets`) rather than by anything
    /// here, so this manager has no local-file path of its own.
    #[must_use]
    pub fn new() -> Self {
        let fetcher = Arc::new(BevyAssetFetcher::new());
        let store = build_asset_store(&fetcher, animation_cache_dir());
        Self {
            store,
            fetcher,
            inflight: HashMap::new(),
            motions: HashMap::new(),
            unavailable: HashSet::new(),
            pending: HashSet::new(),
        }
    }

    /// Ensure `id` is being resolved: a nil id, an already-decoded id, one in
    /// flight, or one known unavailable is ignored. A procedural built-in is
    /// recorded as unavailable without a fetch; everything else spawns a
    /// background fetch+decode. Idempotent.
    pub fn request(&mut self, id: AssetKey) {
        if id.uuid().is_nil()
            || self.motions.contains_key(&id)
            || self.inflight.contains_key(&id)
            || self.unavailable.contains(&id)
        {
            return;
        }
        // A procedural built-in (walk / stand / emote / …) has no downloadable
        // asset; skip the fetch that would 404 and never play it (synthesis is
        // out of this MVP's scope).
        if let Some(builtin) = builtin_animation(id.uuid())
            && !builtin.is_downloadable()
        {
            debug!(
                "animation {} is procedural built-in `{}`; no asset to fetch",
                id.uuid(),
                builtin.name
            );
            let _inserted = self.unavailable.insert(id);
            return;
        }
        // A downloadable `.anim` comes over the `ViewerAsset` cap unless the
        // store ships it as a static asset. If neither is available yet (the cap
        // is not set), hold the request rather than run a fetch that would fail
        // and mark the animation permanently unavailable; `retry_pending`
        // re-issues it once the cap arrives.
        if !self.store.holds_static(id) && !self.fetcher.has_cap_url() {
            let _inserted = self.pending.insert(id);
            return;
        }
        self.pending.remove(&id);
        let label = builtin_animation(id.uuid()).map_or("uploaded", |builtin| builtin.name);
        debug!("resolving animation {} (`{label}`)", id.uuid());
        let store = self.store.clone();
        let task = IoTaskPool::get().spawn(async move {
            // The store answers from its static library, its disk cache or the
            // `ViewerAsset` capability, in that order. Both the blocking reads
            // and the HTTP fetch run on this IoTaskPool thread, and the decode
            // with them, so the render thread never touches animation bytes.
            let bytes = match store.get(id, AssetType::Animation).await {
                Ok(entry) => match entry.data() {
                    Some(data) => data.to_vec(),
                    None => {
                        warn!("animation {} fetched but has no data", id.uuid());
                        return None;
                    }
                },
                Err(error) => {
                    warn!("fetching animation {}: {error}", id.uuid());
                    return None;
                }
            };
            match Motion::from_bytes(&bytes) {
                Ok(motion) => Some(motion),
                Err(error) => {
                    warn!("decoding animation {}: {error}", id.uuid());
                    None
                }
            }
        });
        let _prev = self.inflight.insert(id, task);
    }

    /// The decoded motion for `id`, once resolved, or `None` if it is still in
    /// flight, has no fetchable asset, or failed. Consumed by the skeleton-driver
    /// ([`drive_avatar_skeletons`]).
    pub(crate) fn motion(&self, id: AssetKey) -> Option<&Arc<Motion>> {
        self.motions.get(&id)
    }

    /// A point-in-time snapshot of the animation fetch/decode pipeline, for the
    /// F3 diagnostics overlay: entry counts bucketed by stage plus the cumulative
    /// disk-cache-hit / GC counters. Delegates to the wrapped [`AssetStore`].
    #[must_use]
    pub fn stats(&self) -> StoreStats {
        self.store.stats()
    }

    /// A point-in-time snapshot of the animation store's admission gate: its
    /// concurrency capacity, in-flight slots, and queued waiters.
    #[must_use]
    pub fn gate_stats(&self) -> GateStats {
        self.store.gate_stats()
    }

    /// How many fetches are parked outside the store's own accounting — held for
    /// the `ViewerAsset` capability that is not up yet (see
    /// `pending`) — so the pipeline overlay does not report
    /// "nothing left to load" while such work is still outstanding.
    #[must_use]
    pub fn deferred_count(&self) -> usize {
        self.pending.len()
    }

    /// Point the store's fetcher at the region's current `ViewerAsset` URL.
    fn set_cap_url(&self, url: Option<String>) {
        self.fetcher.set_cap_url(url);
    }

    /// Re-issue any animation resolves parked before the `ViewerAsset` capability
    /// was known (see `pending`), now that it is. A no-op while the
    /// cap is unset or nothing is pending. Call this whenever the cap is (re)set.
    pub(crate) fn retry_pending(&mut self) {
        if self.pending.is_empty() || !self.fetcher.has_cap_url() {
            return;
        }
        let pending: Vec<AssetKey> = self.pending.drain().collect();
        for id in pending {
            self.request(id);
        }
    }

    /// Re-park every id previously marked [`unavailable`](Self::unavailable) so the
    /// next `retry_pending` re-resolves it. Called on a
    /// capability refresh (a region cross / reconnect): an animation whose fetch
    /// failed transiently (a `ViewerAsset` 503, a region-cross URL swap) would
    /// otherwise never play for the rest of the session. A procedural built-in that
    /// shares the `unavailable` set is harmlessly re-marked (its re-request hits the
    /// same built-in branch and does no network work); re-arming on a cap refresh
    /// rather than every frame bounds that to region changes.
    fn rearm_unavailable(&mut self) {
        if self.unavailable.is_empty() {
            return;
        }
        let failed: Vec<AssetKey> = self.unavailable.drain().collect();
        for id in failed {
            let _inserted = self.pending.insert(id);
        }
    }
}

/// Build an [`AssetStore`] over `fetcher`, disk-backed when the cache opens and
/// in-memory only otherwise (a cache failure must never wedge the viewer).
/// Mirrors [`bake_inputs`](crate::bake_inputs)'s wearable-asset store builder.
fn build_asset_store(fetcher: &Arc<BevyAssetFetcher>, disk_dir: Option<PathBuf>) -> AssetStore {
    let concrete = Arc::clone(fetcher);
    let fetcher: Arc<dyn BlobFetcher> = concrete;
    if let Some(dir) = disk_dir {
        match AssetStore::new(
            Arc::clone(&fetcher),
            Some(dir),
            AssetCacheLimits {
                max_bytes: crate::paths::asset_cache_max_bytes(),
                ..AssetCacheLimits::default()
            },
        ) {
            Ok(store) => return store,
            Err(error) => warn!("animation disk cache unavailable ({error}); in-memory only"),
        }
    }
    // The disk-less store cannot fail to open; the loop extracts it without an
    // `unwrap`/`expect` and runs exactly once.
    loop {
        match AssetStore::new(
            Arc::clone(&fetcher),
            None,
            AssetCacheLimits {
                max_bytes: crate::paths::asset_cache_max_bytes(),
                ..AssetCacheLimits::default()
            },
        ) {
            Ok(store) => return store,
            Err(error) => warn!("in-memory animation store failed to open ({error}); retrying"),
        }
    }
}

/// The viewer's on-disk animation-asset cache directory
/// (`<cache>/sl-client-bevy-viewer/animcache`), from `XDG_CACHE_HOME` or
/// `~/.cache`, or `None` when neither is set (the store then runs in-memory only).
fn animation_cache_dir() -> Option<PathBuf> {
    crate::paths::asset_cache_dir("animcache")
}

/// Refresh the store fetcher's `ViewerAsset` capability URL each time the region's
/// capability map is (re)discovered.
pub(crate) fn update_animation_caps(
    mut capabilities: MessageReader<SlCapabilities>,
    mut manager: ResMut<AnimationManager>,
) {
    let mut caps_refreshed = false;
    for SlCapabilities(map) in capabilities.read() {
        manager.set_cap_url(map.get(CAP_VIEWER_ASSET).cloned());
        caps_refreshed = true;
    }
    // A capability refresh (region cross / reconnect) re-arms any animation a
    // post-cap transient failure had marked permanently unavailable.
    if caps_refreshed {
        manager.rearm_unavailable();
    }
    // Re-issue any animation resolves parked while the cap was still unknown.
    manager.retry_pending();
}

/// Ingest each `AvatarAnimation` update and request every signalled animation's
/// motion, so it is fetched and decoded ready for the skeleton-driver (P18.3).
/// The request is idempotent, so re-listing the same animation each update is
/// cheap.
pub(crate) fn ingest_avatar_animations(
    mut events: MessageReader<SlEvent>,
    mut manager: ResMut<AnimationManager>,
    derender: Res<crate::world_api::DerenderList>,
) {
    let log = std::env::var("SL_VIEWER_LOG_LOCOMOTION").as_deref() == Ok("1");
    for event in events.read() {
        if let SlSessionEvent::AvatarAnimation {
            avatar_id,
            animations,
            ..
        } = &event.0
        {
            for animation in animations {
                // A blacklisted animation is never run (`viewer-derender-blacklist`),
                // so there is nothing to fetch either.
                if derender.blacklists(animation.anim_id, DerenderKind::Animation) {
                    continue;
                }
                manager.request(AssetKey::from(animation.anim_id));
            }
            // Wire-truth diagnostic (env `SL_VIEWER_LOG_LOCOMOTION=1`): the exact
            // authoritative animation set the simulator broadcast for this avatar,
            // resolved to built-in names, so a live run can see whether the grid
            // drops `walk` on release (P31.6 investigation).
            if log {
                let names: Vec<String> = animations
                    .iter()
                    .map(|animation| {
                        let name = builtin_animation(animation.anim_id)
                            .map_or("uploaded", |builtin| builtin.name);
                        format!("{name}#{}", animation.sequence_id)
                    })
                    .collect();
                info!("P31.6 sim AvatarAnimation for {avatar_id}: {names:?}");
            }
        }
    }
}

/// Poll the in-flight resolve tasks; move each completed decode into the shared
/// motion cache (the skeleton-driver [`drive_avatar_skeletons`] reads it the next
/// frame), or record the id unavailable when the fetch / decode failed.
pub(crate) fn poll_animations(mut manager: ResMut<AnimationManager>) {
    // Collect the finished ids first — the borrow of the task map cannot overlap
    // the mutation of the motions / unavailable maps.
    let mut finished: Vec<(AssetKey, Option<Motion>)> = Vec::new();
    for (&id, task) in &mut manager.inflight {
        if let Some(result) = block_on(poll_once(task)) {
            finished.push((id, result));
        }
    }
    for (id, result) in finished {
        let _removed = manager.inflight.remove(&id);
        match result {
            Some(motion) => {
                debug!(
                    "animation {} decoded ({} joint track(s))",
                    id.uuid(),
                    motion.joints.len()
                );
                let _prev = manager.motions.insert(id, Arc::new(motion));
            }
            None => {
                let _inserted = manager.unavailable.insert(id);
            }
        }
    }
}

/// One animation an avatar (or an animesh control avatar, P29) is currently
/// playing, tracked for playback timing and priority-blend ordering (P18.4).
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlayState {
    /// The simulator's per-avatar animation sequence number; a change means the
    /// animation (re)started, so the playback clock resets and it re-activates.
    sequence_id: i32,
    /// The wall-clock time ([`Time::elapsed_secs`]) at which this animation
    /// started, so `now - start` gives the seconds elapsed into the motion.
    start: f32,
    /// The elapsed-since-start time at which the simulator dropped this animation
    /// from the avatar's set, so it eases out over its remaining tail rather than
    /// popping; `None` while it is still signalled.
    stopped_at: Option<f32>,
    /// The accumulated drift (seconds) between this animation's **playback** clock and
    /// wall time, for a motion whose playback is speed-scaled (P31.14): the reference
    /// viewer's `LLKeyframeWalkMotion` advances its own clock by `dt * "Walk Speed"`
    /// rather than by `dt`, so a walk cycle keeps pace with the ground. Zero — the
    /// default, and the value every non-walk motion keeps — means the playback clock
    /// *is* wall time, exactly as before.
    ///
    /// Kept as an offset rather than as an absolute clock so nothing that does not
    /// speed-scale (every gesture, and the whole animesh control-avatar path, P29) has
    /// to know this exists: `anim_elapsed = (now - start) + anim_offset`.
    ///
    /// Only the *sampling* clock is scaled. A motion's ease-in / ease-out weight and
    /// its expiry still run on wall time, as they do in the reference (the ease is
    /// driven by `LLMotionController` from the activation timestamp, never by the
    /// motion's own adjusted time).
    anim_offset: f32,
    /// This animation's activation recency (a per-avatar monotonic stamp): higher
    /// means more recently started, so it wins ties in priority — the reference
    /// viewer pushes each newly-started motion to the front of its active list.
    /// See `reconcile_playing` for how the stamp reproduces Second Life's
    /// present-observer vs. late-arriver ordering.
    order: u64,
}

impl PlayState {
    /// The wall-clock time this animation started at.
    pub(crate) const fn start(&self) -> f32 {
        self.start
    }

    /// The elapsed-since-start time the simulator dropped it at, or `None`
    /// while still signalled.
    pub(crate) const fn stopped_at(&self) -> Option<f32> {
        self.stopped_at
    }

    /// The accumulated walk-speed playback-clock skew (P31.14), seconds.
    pub(crate) const fn anim_offset(&self) -> f32 {
        self.anim_offset
    }

    /// The activation recency stamp (higher = more recent).
    pub(crate) const fn order(&self) -> u64 {
        self.order
    }
}

/// Per-avatar animation *playback* state (P18.3 / P18.4), distinct from the
/// [`AnimationManager`]'s asset resolve/cache: which animations each avatar is
/// playing, their timing / activation order, and the per-joint pose the driver
/// blended this frame for `pose_avatar_skeletons` to write into the skeleton's
/// world matrices.
#[derive(Debug, Resource, Default)]
pub struct AnimationPlayback {
    /// Each avatar's currently-playing animations, keyed by animation id — the
    /// authoritative simulator-driven set (from `AvatarAnimation`).
    playing: HashMap<AgentKey, HashMap<Uuid, PlayState>>,
    /// The own avatar's **client-driven** locomotion animation (P31.6), kept apart
    /// from the simulator-driven [`playing`](Self::playing) set so the two do not
    /// fight over one map: this is the built-in walk / run / stand / turn / fly /
    /// hover / fall the viewer plays for immediate feedback *when the simulator is
    /// not driving the avatar itself* (e.g. an OpenSim child presence that never
    /// broadcasts the agent's own animations). Reconciled by
    /// [`set_client_locomotion`](Self::set_client_locomotion); merged with the
    /// simulator set at pose time. Keyed by avatar for symmetry, though only the
    /// own avatar is ever present.
    client_locomotion: HashMap<AgentKey, HashMap<Uuid, PlayState>>,
    /// The own avatar's **client-driven** typing animation (P31.9): `ANIM_AGENT_TYPE`,
    /// the hands-on-keyboard gesture the viewer plays locally while the user is
    /// entering local chat, for immediate feedback in step with the `StartTyping` /
    /// `StopTyping` it broadcasts for others. Kept in its own slot rather than the
    /// [`client_locomotion`](Self::client_locomotion) one because typing is an
    /// *overlay* — it plays concurrently with stand / walk (the reference viewer
    /// requests it as an ordinary priority-blended animation), whereas the
    /// locomotion slot holds a single mutually-exclusive state. Reconciled by
    /// [`set_client_typing`](Self::set_client_typing); merged with the other two sets
    /// at pose time. Keyed by avatar for symmetry, though only the own avatar is ever
    /// present.
    client_typing: HashMap<AgentKey, HashMap<Uuid, PlayState>>,
    /// The next activation-recency stamp to hand out (monotonic across all
    /// avatars; only the relative order within an avatar is ever compared).
    next_order: u64,
    /// Each posed avatar's resolved per-joint pose this frame (only avatars with a
    /// drivable animation appear). An avatar absent here keeps its plain deformed
    /// rest pose, produced by ordinary transform propagation.
    poses: HashMap<AgentKey, AnimationPose>,
}

impl AnimationPlayback {
    /// Whether the simulator is currently driving at least one **active** (not
    /// easing-out) animation on `agent`. The client-side locomotion fallback
    /// (P31.6) defers to the simulator whenever this is true — a grid that
    /// broadcasts the agent's own locomotion / stand set (a root presence, or an AO
    /// on Second Life) already animates it, so the fallback only fills the gap when
    /// the simulator says nothing.
    #[must_use]
    pub(crate) fn has_active_sim_animation(&self, agent: AgentKey) -> bool {
        self.playing
            .get(&agent)
            .is_some_and(|anims| anims.values().any(|state| state.stopped_at.is_none()))
    }

    /// Whether the simulator-signalled animation set of `agent` contains
    /// `animation` as **active** (not easing out).
    ///
    /// This is a *state* query, not a visual one: the `AvatarAnimation`
    /// broadcast is the signalled set — e.g. `ANIM_AGENT_AWAY` stays in it
    /// while an agent is away even if an AO overrides what actually plays,
    /// which is exactly how the reference derives another avatar's Away
    /// status (`mSignaledAnimations`, and the protocol's only carrier of it).
    #[must_use]
    pub fn is_playing(&self, agent: AgentKey, animation: Uuid) -> bool {
        self.playing.get(&agent).is_some_and(|anims| {
            anims
                .get(&animation)
                .is_some_and(|state| state.stopped_at.is_none())
        })
    }

    /// Reconcile the own avatar's client-driven locomotion set (P31.6) to a single
    /// `desired` built-in animation, or `None` to ease out whatever is playing. An
    /// unchanged desire keeps its playback clock (so a continuous walk keeps
    /// looping); a change eases the old motion out and starts the new one, so
    /// transitions blend rather than pop. Kept separate from the simulator-driven
    /// [`playing`](Self::playing) set — the caller ([`crate::locomotion`]) gates on
    /// [`has_active_sim_animation`](Self::has_active_sim_animation) so the two never
    /// drive the same avatar at once.
    pub(crate) fn set_client_locomotion(
        &mut self,
        agent: AgentKey,
        desired: Option<Uuid>,
        now: f32,
    ) {
        let entry = self.client_locomotion.entry(agent).or_default();
        // A fixed sequence id: the animation *id* is what distinguishes one state
        // from the next, so `reconcile_playing` keeps an unchanged desire in place
        // and only (re)starts when the id itself changes.
        let pairs: Vec<(Uuid, i32)> = desired.map(|id| (id, 0)).into_iter().collect();
        reconcile_playing(entry, &mut self.next_order, &pairs, now);
    }

    /// Reconcile the own avatar's client-driven typing set (P31.9) to a single
    /// `desired` animation (`ANIM_AGENT_TYPE` while typing), or `None` to ease it
    /// out. Mirrors [`set_client_locomotion`](Self::set_client_locomotion) but on a
    /// separate slot so typing overlays — rather than replaces — the locomotion /
    /// simulator animation: an unchanged desire keeps its playback clock, a change
    /// (start ⟷ stop) eases the old motion out and starts the new one so the
    /// hands-on-keyboard gesture fades in and out rather than popping.
    pub(crate) fn set_client_typing(&mut self, agent: AgentKey, desired: Option<Uuid>, now: f32) {
        let entry = self.client_typing.entry(agent).or_default();
        let pairs: Vec<(Uuid, i32)> = desired.map(|id| (id, 0)).into_iter().collect();
        reconcile_playing(entry, &mut self.next_order, &pairs, now);
    }

    /// The [`HandPose`] the motions currently playing on `agent` request (P31.13),
    /// or [`None`] when none of them is decoded — the hand-pose morph driver then
    /// relaxes the hands, as the reference does with no `"Hand Pose"` animation data.
    ///
    /// Mirrors `LLKeyframeMotion::applyKeyframes`, which publishes its motion's hand
    /// pose only if the motion's [`max_priority`](Motion::max_priority) is **at
    /// least** the pose priority already published this frame. Every active motion
    /// takes part, including one easing out (the reference keeps updating a motion
    /// until its ease-out tail has passed and it is deactivated) — this set is
    /// pruned to exactly those by [`retain_active`].
    ///
    /// The `>=` in the reference means a *tie* on priority is won by whichever motion
    /// its active list visits **last**, and it pushes each newly-activated motion to
    /// the front — so among equal priorities the **oldest** activation wins, i.e. the
    /// lowest [`PlayState::order`] stamp. (Note this is the opposite of the per-joint
    /// pose blend, where the most recent activation wins a tie; both fall out of the
    /// one active-list order, and both are reproduced faithfully.)
    ///
    /// A *procedural* motion may request a hand pose too — the editing reach (P31.15) asks
    /// for `EDITING_HAND_POSE`(crate::reach::EDITING_HAND_POSE) while it reaches, as the
    /// reference's `LLEditingMotion` does. Such a request arrives as `procedural` and takes
    /// part in the same contest, at its own priority (the reference writes it *last*, after
    /// every keyframe motion, so it wins any tie — hence the `order` of 0 here).
    pub(crate) fn requested_hand_pose(
        &self,
        agent: AgentKey,
        manager: &AnimationManager,
        procedural: Option<(JointPriority, HandPose)>,
    ) -> Option<HandPose> {
        let merged = merge_playing(
            self.playing.get(&agent),
            self.client_locomotion.get(&agent),
            self.client_typing.get(&agent),
        );
        let mut winner: Option<(JointPriority, u64, HandPose)> =
            procedural.map(|(priority, pose)| (priority, 0, pose));
        for (anim_id, play) in &merged {
            let Some(motion) = manager.motion(AssetKey::from(*anim_id)) else {
                continue;
            };
            let priority = motion.max_priority();
            let beats = winner.is_none_or(|(best_priority, best_order, _pose)| {
                priority > best_priority || (priority == best_priority && play.order < best_order)
            });
            if beats {
                winner = Some((priority, play.order, motion.hand_pose));
            }
        }
        winner.map(|(_priority, _order, pose)| pose)
    }

    /// Which locomotion adjusters (P31.14) `agent`'s currently-playing animation set
    /// calls for, and — for the fall recovery — how far into its motion it is.
    ///
    /// The three questions the reference viewer answers from the same signalled set:
    /// is any of `AGENT_WALK_ANIMS` playing (so `LLWalkAdjustMotion` runs and publishes
    /// a walk speed), is an `LLKeyframeStandMotion` playing (so its lower body is
    /// foot-IK'd), and is the `LLKeyframeFallMotion` (`standup`) playing (so the pelvis
    /// blends up from the ground normal). Every set the avatar is playing takes part —
    /// the simulator's and both client-driven ones.
    #[must_use]
    pub(crate) fn adjuster_anims(
        &self,
        agent: AgentKey,
        now: f32,
        manager: &AnimationManager,
    ) -> AdjusterAnims {
        let merged = merge_playing(
            self.playing.get(&agent),
            self.client_locomotion.get(&agent),
            self.client_typing.get(&agent),
        );
        let mut anims = AdjusterAnims::default();
        for (&anim_id, play) in &merged {
            // A motion already easing out no longer drives an adjuster (the reference
            // stops the adjust motions on the state change, not on the fade).
            if play.stopped_at.is_some() {
                continue;
            }
            if sl_anim::is_walk_adjust_trigger(anim_id) {
                anims.walking = true;
            }
            match sl_anim::keyframe_motion_class(anim_id) {
                KeyframeMotionClass::Stand => anims.standing = true,
                KeyframeMotionClass::Fall => {
                    if let Some(motion) = manager.motion(AssetKey::from(anim_id)) {
                        anims.fall = Some((now - play.start, motion.duration));
                    }
                }
                KeyframeMotionClass::Walk | KeyframeMotionClass::Plain => {}
            }
        }
        anims
    }

    /// Whether `agent` is **aiming** — one of the reference's `AGENT_GUN_AIM_ANIMS` is
    /// signalled, which is what switches `LLTargetingMotion` on (P31.15) so the avatar's
    /// torso twists until its right hand points at its look-at target.
    ///
    /// Read from the same merged set as [`adjuster_anims`](Self::adjuster_anims), and with
    /// the same rule: a motion already easing out no longer drives an adjuster.
    #[must_use]
    pub(crate) fn is_aiming(&self, agent: AgentKey) -> bool {
        let merged = merge_playing(
            self.playing.get(&agent),
            self.client_locomotion.get(&agent),
            self.client_typing.get(&agent),
        );
        merged.iter().any(|(&anim_id, play)| {
            play.stopped_at.is_none() && sl_anim::is_gun_aim_trigger(anim_id)
        })
    }

    /// The avatar's merged playing set — simulator-driven plus the two
    /// client-driven sets — as owned `(animation id, play state)` pairs. The
    /// GPU-avatar scheduler (`crate::gpu_avatars`) builds its per-avatar
    /// playback rows and sample jobs from exactly this set, so the GPU blends
    /// the same motions the CPU pose resolver would.
    #[must_use]
    pub(crate) fn merged_active(&self, agent: AgentKey) -> Vec<(Uuid, PlayState)> {
        merge_playing(
            self.playing.get(&agent),
            self.client_locomotion.get(&agent),
            self.client_typing.get(&agent),
        )
        .into_iter()
        .collect()
    }

    /// Advance the speed-scaled playback clocks (P31.14): every
    /// [`Walk`](KeyframeMotionClass::Walk) motion an avatar is playing has its sampling
    /// clock advanced by `dt * walk_speed(agent)` rather than by `dt`, so the walk
    /// cycle's feet keep pace with the ground the walk-adjust servo measured.
    ///
    /// This is `LLKeyframeWalkMotion::onUpdate`: it is the *motion* that scales its own
    /// clock by the `"Walk Speed"` the always-on adjust motion publishes, which is why
    /// only the walk-class motions are touched and everything else keeps wall time. A
    /// clock driven negative (the avatar is walking backwards, so the cycle plays in
    /// reverse) wraps up into the motion's loop rather than clamping at zero, as the
    /// reference's `getDuration() + fmod(adjusted_time, getDuration())` does.
    pub(crate) fn advance_walk_speed(
        &mut self,
        now: f32,
        dt: f32,
        manager: &AnimationManager,
        walk_speed: impl Fn(AgentKey) -> f32,
    ) {
        for set in [
            &mut self.playing,
            &mut self.client_locomotion,
            &mut self.client_typing,
        ] {
            for (&agent, anims) in set.iter_mut() {
                let speed = walk_speed(agent);
                for (&anim_id, play) in anims.iter_mut() {
                    if sl_anim::keyframe_motion_class(anim_id) != KeyframeMotionClass::Walk {
                        continue;
                    }
                    play.anim_offset += dt * (speed - 1.0);
                    // Keep a reversed clock inside the loop rather than pinned at 0.
                    let Some(motion) = manager.motion(AssetKey::from(anim_id)) else {
                        continue;
                    };
                    if !motion.loops || motion.duration <= 0.0 {
                        continue;
                    }
                    let elapsed = now - play.start + play.anim_offset;
                    if elapsed < 0.0 {
                        play.anim_offset += motion.duration * (-elapsed / motion.duration).ceil();
                    }
                }
            }
        }
    }
}

/// Reconcile one avatar's playing-animation set with an authoritative
/// `AvatarAnimation` update, reproducing the reference viewer's activation
/// ordering (P18.4).
///
/// An animation that stays signalled with the same sequence id keeps its start
/// time and activation order (and is un-marked if it had begun easing out). One
/// that leaves the set begins easing out (its `stopped_at` is stamped with its
/// elapsed-since-start, `now - start`, the motion-elapsed timeline the ease-out
/// weight uses) but stays until it has faded, so its ease-out tail is not cut off.
/// A newly
/// signalled animation — or one whose sequence id changed, i.e. the simulator
/// re-triggered it — (re)activates: its clock resets and it takes a fresh, higher
/// activation-order stamp so it wins ties in priority.
///
/// The subtlety the ordering reproduces (a Second Life quirk, kept faithful on
/// purpose): the reference iterates its *sorted-by-UUID* signalled set and pushes
/// each newly-started motion to the front of the active list, so when several
/// animations start in one update — the case for an observer who arrives while
/// they are already playing — the highest-UUID one ends up first and wins equal
/// priorities. An observer present as each one starts instead activates them one
/// update at a time, so the last-*started* one wins. Assigning the monotonic
/// stamp in UUID order within each update yields both behaviours from the one
/// rule.
///
/// The signalled set is passed as `(anim_id, sequence_id)` pairs so both the
/// avatar path (from [`PlayingAnimation`](sl_client_bevy::PlayingAnimation)) and
/// the animesh control-avatar path (from
/// [`ObjectPlayingAnimation`](sl_client_bevy::ObjectPlayingAnimation), P29)
/// can drive it.
pub(crate) fn reconcile_playing(
    entry: &mut HashMap<Uuid, PlayState>,
    next_order: &mut u64,
    animations: &[(Uuid, i32)],
    now: f32,
) {
    let live: HashMap<Uuid, i32> = animations.iter().copied().collect();
    // Newly-activated (absent, or re-triggered with a changed sequence id); an
    // unchanged, still-signalled animation is left in place (and un-stopped).
    let mut newly: Vec<(Uuid, i32)> = Vec::new();
    for &(anim_id, sequence_id) in animations {
        match entry.get_mut(&anim_id) {
            Some(state) if state.sequence_id == sequence_id => state.stopped_at = None,
            _new_or_restarted => newly.push((anim_id, sequence_id)),
        }
    }
    // Begin easing out every animation that left the authoritative set. The stop
    // time is stored **relative to that animation's own start** — the same
    // motion-elapsed timeline [`Motion::pose_weight`] / [`Motion::is_finished`]
    // compare against `elapsed = now - start` — not the absolute wall clock. A
    // *non-looping* motion is saved by its natural ease-out (`min(stopped_at,
    // duration - ease_out)` picks the smaller), which is why gestures always faded
    // correctly; but a *looping* motion (walk / run / stand) has no natural
    // ease-out, so an absolute `now` here (a large, ever-growing number) would push
    // its ease-out start far into the future and leave the animation stuck at full
    // weight for seconds — effectively forever late into a session (P31.6).
    for (id, state) in entry.iter_mut() {
        if !live.contains_key(id) && state.stopped_at.is_none() {
            state.stopped_at = Some(now - state.start);
        }
    }
    // Activate the newcomers in UUID order, so the highest UUID takes the newest
    // stamp — the reference's sorted-set push-to-front order for a same-update batch.
    newly.sort_unstable_by_key(|&(id, _sequence_id)| id);
    for (id, sequence_id) in newly {
        let _prev = entry.insert(
            id,
            PlayState {
                sequence_id,
                start: now,
                stopped_at: None,
                order: *next_order,
                anim_offset: 0.0,
            },
        );
        *next_order = next_order.wrapping_add(1);
    }
}

/// Drop from one playing set every motion whose ease-out tail has fully passed
/// (its [`Motion::is_finished`]), and any stopped motion with no decodable asset
/// left to fade. Shared by the avatar driver and the animesh control-avatar
/// driver (P29).
pub(crate) fn retain_active(
    anims: &mut HashMap<Uuid, PlayState>,
    now: f32,
    manager: &AnimationManager,
) {
    anims.retain(|id, state| {
        let elapsed = now - state.start;
        match manager.motion(AssetKey::from(*id)) {
            Some(motion) => !motion.is_finished(elapsed, state.stopped_at),
            None => state.stopped_at.is_none(),
        }
    });
}

/// Merge an avatar's simulator-driven playing set with its client-driven
/// locomotion set (P31.6) and typing set (P31.9) into one map for
/// [`resolve_pose`]. Any side may be absent; the client sets are folded in on top
/// of the simulator set. The locomotion set never collides with the simulator set
/// (the P31.6 driver only fills genuine simulator silence); the typing set is a
/// deliberate overlay whose `ANIM_AGENT_TYPE` blends against whatever else is
/// playing by priority in [`resolve_pose`], so its only per-map collision is the
/// benign one where the simulator echoes the agent's own typing back under the
/// same id (the client entry then simply wins). Returns an owned map so the pose
/// resolver borrows one set regardless of how many contributed.
fn merge_playing(
    sim: Option<&HashMap<Uuid, PlayState>>,
    client_locomotion: Option<&HashMap<Uuid, PlayState>>,
    client_typing: Option<&HashMap<Uuid, PlayState>>,
) -> HashMap<Uuid, PlayState> {
    let mut merged = sim.cloned().unwrap_or_default();
    for client in [client_locomotion, client_typing].into_iter().flatten() {
        for (&id, &state) in client {
            let _prev = merged.insert(id, state);
        }
    }
    merged
}

/// Blend one playing set into a per-joint [`AnimationPose`], sampling each
/// decoded motion at its elapsed time, weighting it by its ease-in/out
/// [`pose_weight`](Motion::pose_weight), and resolving concurrent contributions
/// per joint by priority ([`blend_joint`], P18.4). `joint_index` maps a motion's
/// joint *name* to the skeleton index the pose is keyed by. Returns `None` when
/// no playing motion is decoded / contributes (the skeleton then keeps its rest
/// pose). Shared by the avatar driver and the animesh control-avatar driver (P29,
/// which resolves names against the same standard skeleton).
pub(crate) fn resolve_pose(
    anims: &HashMap<Uuid, PlayState>,
    now: f32,
    manager: &AnimationManager,
    joint_index: impl Fn(&str) -> Option<usize>,
) -> Option<AnimationPose> {
    // Gather every motion's weighted contribution per joint, then blend.
    let mut contributions: HashMap<usize, Vec<JointContribution>> = HashMap::new();
    for (anim_id, play) in anims {
        let elapsed = now - play.start;
        let Some(motion) = manager.motion(AssetKey::from(*anim_id)) else {
            continue;
        };
        // The ease-in/out weight runs on **wall** time; only the *sampling* clock is
        // speed-scaled (P31.14), and only for a walk-class motion (whose `anim_offset`
        // is the only non-zero one). See [`PlayState::anim_offset`].
        let weight = motion.pose_weight(elapsed, play.stopped_at);
        if weight <= 0.0 {
            continue;
        }
        let anim_elapsed = elapsed + play.anim_offset;
        for sampled in sample_motion(motion, anim_elapsed) {
            let Some(index) = joint_index(sampled.name) else {
                continue;
            };
            contributions
                .entry(index)
                .or_default()
                .push(JointContribution {
                    priority: sampled.priority,
                    order: play.order,
                    weight,
                    rotation: sampled.rotation.map(|rotation| rotation.to_array()),
                    position: sampled.position.map(|position| position.to_array()),
                });
        }
    }
    if contributions.is_empty() {
        return None;
    }
    let mut pose = AnimationPose::new();
    for (index, mut joint) in contributions {
        let blended = blend_joint(&mut joint);
        if let Some(rotation) = blended.rotation {
            pose.set_rotation(index, Quat::from_array(rotation));
        }
        if let Some(position) = blended.position {
            pose.set_position(index, Vec3::from_array(position));
        }
    }
    Some(pose)
}

/// The joints the CPU adjusters read geometry from or write channels to —
/// the static half of the §5.3 mini-pose subset: the look-at chain (neck /
/// head / eyes), the leg chains the locomotion IK solves, the left-arm reach
/// chain and aim wrists, the idle adjusters' chest / torso, and the physics
/// sample joints (`mChest` / `mPelvis` are already listed).
const ADJUSTER_JOINT_NAMES: &[&str] = &[
    "mPelvis",
    "mTorso",
    "mChest",
    "mNeck",
    "mHead",
    "mEyeLeft",
    "mEyeRight",
    "mFaceEyeAltLeft",
    "mFaceEyeAltRight",
    "mHipLeft",
    "mKneeLeft",
    "mAnkleLeft",
    "mHipRight",
    "mKneeRight",
    "mAnkleRight",
    "mCollarLeft",
    "mShoulderLeft",
    "mElbowLeft",
    "mWristLeft",
    "mWristRight",
];

/// One avatar's §5.3 **mini-pose subset** while the in-place GPU path owns
/// the skinning joints: the ancestor closure of every joint the CPU still
/// consumes — the [`ADJUSTER_JOINT_NAMES`] chains, every collision volume
/// (body-physics displacement targets), and the §5.4 sockets (every worn
/// attachment-point joint; the rigid eyeballs' joints are in the static list
/// already). The closure matters: the chain solve reads every ancestor's
/// animated channels, so an ancestor missing from the resolve would place a
/// socket or adjuster input at rest.
fn mini_pose_subset(
    skeleton: &BevySkeleton,
    body: &AvatarBody,
    state: &AvatarState,
    hooks: &GpuAvatarHooks<'_, '_>,
    agent: AgentKey,
) -> HashSet<usize> {
    let mut targets: Vec<usize> = ADJUSTER_JOINT_NAMES
        .iter()
        .filter_map(|name| body.joint_index(name))
        .collect();
    for index in 0..skeleton.len() {
        if skeleton.is_collision_volume(index) {
            targets.push(index);
        }
    }
    for (point_id, node) in state.attachment_nodes_of(agent) {
        // Worn = the node carries an attachment subtree (the same test the
        // socket writer applies). The node is a root child now (§5.4), so its
        // joint comes from the point's `avatar_lad.xml` binding, not its parent.
        let worn = hooks
            .children
            .get(node)
            .is_ok_and(|children| !children.is_empty());
        if !worn {
            continue;
        }
        if let Some((joint_index, _offset)) = body.attachment_point(point_id) {
            targets.push(joint_index);
        }
    }
    // The ancestor closure (parents precede children in canonical order; a
    // forward parent — the synthetic root — never chains further).
    let parents = skeleton.parents();
    let mut subset: HashSet<usize> = HashSet::new();
    for target in targets {
        let mut current = target;
        loop {
            if !subset.insert(current) {
                break;
            }
            match parents.get(current).copied().flatten() {
                Some(parent) if parent < current => current = parent,
                _root_or_forward => break,
            }
        }
    }
    subset
}

/// Resolve each rigged avatar's per-joint animation pose from the motions it is
/// playing, blending concurrent motions by priority with ease-in/out (P18.4), for
/// [`pose_avatar_skeletons`] to apply.
///
/// Each frame it folds the latest `AvatarAnimation` updates into the playback
/// clock (`reconcile_playing`), then for every avatar samples each playing,
/// decoded motion at its elapsed time, weights it by its ease-in/out
/// [`pose_weight`](Motion::pose_weight), and blends the per-joint contributions by
/// priority ([`blend_joint`]) — a higher-priority motion dominating a joint while a
/// lower-priority one shows through the weight it leaves unfilled. A motion that
/// has fully eased out is dropped. The resolved [`AnimationPose`]s are stored on
/// the [`AnimationPlayback`] resource; an avatar with no drivable animation is
/// simply omitted, so ordinary transform propagation leaves it at its deformed
/// rest pose. Procedural built-ins (walk / stand / …) have no cached motion, so an
/// idle avatar signalling only those keeps its rest pose.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources; the GPU hooks are \
              already bundled into one `GpuAvatarHooks` param, and the rest is the \
              animation pipeline, the adjuster feedback, and the avatar state / assets"
)]
pub(crate) fn drive_avatar_skeletons(
    time: Res<Time>,
    mut events: MessageReader<SlEvent>,
    manager: Res<AnimationManager>,
    mut playback: ResMut<AnimationPlayback>,
    adjust: Res<LocomotionAdjust>,
    state: Res<AvatarState>,
    derender: Res<crate::world_api::DerenderList>,
    body: Option<Res<AvatarBody>>,
    library: Option<Res<AvatarAssetLibrary>>,
    gpu: GpuAvatarHooks<'_, '_>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let playback = playback.as_mut();
    // Reconcile the playback clock with each authoritative animation set.
    for event in events.read() {
        if let SlSessionEvent::AvatarAnimation {
            avatar_id,
            animations,
            ..
        } = &event.0
        {
            // A blacklisted animation is dropped from the authoritative set, so
            // it never starts — the reference refuses it at the same point
            // (`isBlacklisted(animation_id, AT_ANIMATION)` while processing an
            // `AvatarAnimation`).
            let pairs: Vec<(Uuid, i32)> = animations
                .iter()
                .filter(|animation| {
                    !derender.blacklists(animation.anim_id, DerenderKind::Animation)
                })
                .map(|animation| (animation.anim_id, animation.sequence_id))
                .collect();
            let entry = playback.playing.entry(*avatar_id).or_default();
            reconcile_playing(entry, &mut playback.next_order, &pairs, now);
        }
    }
    // Drop fully-eased-out motions (their ease-out tail has passed), and any
    // stopped motion with no decodable asset to fade; forget emptied avatars. The
    // simulator-driven set and both client-driven sets — locomotion (P31.6) and
    // typing (P31.9) — are pruned the same way.
    for set in [
        &mut playback.playing,
        &mut playback.client_locomotion,
        &mut playback.client_typing,
    ] {
        set.retain(|_agent, anims| {
            retain_active(anims, now, &manager);
            !anims.is_empty()
        });
    }
    // Advance the walk-class motions' speed-scaled playback clocks by the walk speed
    // the previous frame's walk-adjust servo published (P31.14). Every other motion
    // keeps wall time.
    playback.advance_walk_speed(now, dt, &manager, |agent| adjust.walk_speed(agent));
    // Without the avatar asset library there are no skeleton instances to pose.
    let Some(body) = body else {
        playback.poses.clear();
        return;
    };
    // Resolve each avatar's blended per-joint pose from its playing motions — the
    // union of the simulator-driven set and the own avatar's client locomotion and
    // typing sets.
    //
    // While the in-place GPU path owns the skinning joints (Phase 2 of
    // `roadmap/context/gpu-avatars.md`, §5.3), the full fold is the GPU's job
    // (passes A+B): the CPU resolve is demoted to the **adjuster mini-pose**,
    // restricted to the joints the CPU still consumes — the adjuster chains,
    // the sockets, and their ancestors — so `pose_avatar_skeletons`' chain
    // solves and corrections see the same animated channels the full solve
    // would, at a fraction of the sampling cost.
    let gpu_real = gpu.real_active();
    let mut agents: HashSet<AgentKey> = playback.playing.keys().copied().collect();
    agents.extend(playback.client_locomotion.keys().copied());
    agents.extend(playback.client_typing.keys().copied());
    let mut poses: HashMap<AgentKey, AnimationPose> = HashMap::new();
    for agent in agents {
        // Only a rigged avatar (with a spawned body) can be posed.
        if !state.is_rigged(agent) {
            continue;
        }
        let merged = merge_playing(
            playback.playing.get(&agent),
            playback.client_locomotion.get(&agent),
            playback.client_typing.get(&agent),
        );
        let subset = if gpu_real {
            library
                .as_deref()
                .map(|library| mini_pose_subset(library.skeleton(), &body, &state, &gpu, agent))
        } else {
            None
        };
        let resolved = resolve_pose(&merged, now, &manager, |name| {
            let index = body.joint_index(name)?;
            match subset.as_ref() {
                Some(subset) if !subset.contains(&index) => None,
                _within => Some(index),
            }
        });
        if let Some(pose) = resolved {
            let _prev = poses.insert(agent, pose);
        }
    }
    // Edge-triggered logging (not every frame): an avatar starting / stopping being
    // posed is the live signal that a keyframe motion decoded and drove the skeleton.
    for &agent in poses.keys() {
        if !playback.poses.contains_key(&agent) {
            debug!("animation: posing avatar {agent} skeleton");
        }
    }
    for &agent in playback.poses.keys() {
        if !poses.contains_key(&agent) {
            debug!("animation: released avatar {agent} skeleton back to rest");
        }
    }
    playback.poses = poses;
}

/// The rate (Hz) the procedural idle clock ticks at: [`pose_avatar_skeletons`]
/// quantises the time it feeds `apply_idle_adjustments` to this grid, so the
/// breathe / body-noise output is **bit-identical between ticks** — which is
/// what makes an idle avatar's pose comparable frame-to-frame at all (the idle
/// motions are continuous functions of time). 15 Hz stepping of a 0.05 rad
/// breathing sine over a ~6 s period is imperceptible. `pub(crate)` because
/// the GPU-avatar scheduler quantises pass B's `idle_now` to the same grid.
pub(crate) const POSE_IDLE_HZ: f32 = 15.0;

/// The GPU-avatar pipeline's hooks into the pose driver (`crate::gpu_avatars`),
/// bundled into one system param:
///
/// - the **pose feed**, which receives each avatar's root matrix plus the
///   sparse adjuster corrections pass B folds in;
/// - the **mode**, whose capability-checked `active` flag says whether the
///   in-place GPU path is running this device;
/// - the attachment-node queries the **socket scan** needs to find which
///   attachment-point nodes carry a worn subtree.
#[derive(Debug, SystemParam)]
pub(crate) struct GpuAvatarHooks<'w, 's> {
    /// The correction feed pass B consumes.
    feed: Option<ResMut<'w, crate::gpu_avatars::GpuAvatarPoseFeed>>,
    /// The pipeline's capability-checked activity.
    mode: Option<Res<'w, crate::gpu_avatars::GpuAvatarsMode>>,
    /// Attachment-node children (a node with children carries a worn
    /// attachment).
    children: Query<'w, 's, &'static Children>,
}

impl GpuAvatarHooks<'_, '_> {
    /// Whether the in-place GPU path owns the skinning this run: the pipeline
    /// is registered and the device passed the startup capability check.
    fn real_active(&self) -> bool {
        self.mode.as_deref().is_some_and(|mode| mode.active)
    }

    /// Publish one avatar's root matrix + sparse adjuster corrections to the
    /// feed (the in-place real path, Phase 2: the GPU samples and blends the
    /// keyframes itself; the CPU contributes only what its adjusters
    /// changed). The corrections arrive sorted by joint.
    fn publish_real(
        &mut self,
        agent: AgentKey,
        root: Mat4,
        corrections: Vec<(u32, crate::gpu_avatars::types::GpuLocalPose)>,
    ) {
        if !self.real_active() {
            return;
        }
        if let Some(feed) = self.feed.as_mut() {
            feed.publish_real(
                crate::world_api::PoseSlotKey::Avatar(agent),
                root,
                corrections,
            );
        }
    }
}

/// Diff one avatar's mini pose across the adjuster folds (§5.3): every
/// channel of `posed` that differs from `baseline` — the mini pose as it
/// stood after the keyframe + idle folds, i.e. exactly what pass B computes
/// GPU-side — becomes a sparse correction replacing that channel. Sorted by
/// joint (the GPU binary-searches). Exact float comparison is deliberate: an
/// adjuster that wrote the identical value needs no correction, and an
/// active adjuster's output moves every frame.
fn pose_corrections(
    baseline: &AnimationPose,
    posed: &AnimationPose,
) -> Vec<(u32, crate::gpu_avatars::types::GpuLocalPose)> {
    use crate::gpu_avatars::types::{GpuLocalPose, POSE_FLAG_POS, POSE_FLAG_ROT};
    let mut by_joint: HashMap<usize, GpuLocalPose> = HashMap::new();
    for (index, rotation) in posed.rotations() {
        if baseline.rotation(index) == Some(rotation) {
            continue;
        }
        let entry = by_joint.entry(index).or_default();
        entry.rot = Vec4::new(rotation.x, rotation.y, rotation.z, rotation.w);
        entry.flags |= POSE_FLAG_ROT;
    }
    for (index, position) in posed.positions() {
        if baseline.position(index) == Some(position) {
            continue;
        }
        let entry = by_joint.entry(index).or_default();
        entry.pos = position;
        entry.flags |= POSE_FLAG_POS;
    }
    let mut out: Vec<(u32, GpuLocalPose)> = by_joint
        .into_iter()
        .filter_map(|(index, value)| Some((u32::try_from(index).ok()?, value)))
        .collect();
    out.sort_by_key(|&(joint, _value)| joint);
    out
}

/// The procedural adjusters' resources, bundled so [`pose_avatar_skeletons`] stays
/// inside Bevy's system-parameter limit — each fold (look-at, reach & aim, locomotion,
/// body physics) contributes its own target and state resource, and the runtime-morph
/// overrides are the channel two of them write their morph params through.
#[derive(Debug, SystemParam)]
pub(crate) struct AvatarAdjusters<'w> {
    /// Who each avatar is looking at (P31.12).
    look_targets: Res<'w, LookAtTargets>,
    /// The look-at / eye-blink motion state (P31.12, P31.12b).
    look_motion: ResMut<'w, LookAtMotion>,
    /// What each avatar has selected, which its left arm reaches for (P31.15).
    point_at_targets: Res<'w, PointAtTargets>,
    /// The reach & aim motion state (P31.15).
    reach: ResMut<'w, ReachMotion>,
    /// The locomotion adjusters' state — walk servo, foot IK, landing, fly bank (P31.14).
    locomotion: ResMut<'w, LocomotionAdjust>,
    /// The body-physics spring-damper state (P34.2).
    body_physics: ResMut<'w, BodyPhysicsMotion>,
    /// The per-frame runtime morph overrides the eye-blink (P31.12b) and body-physics
    /// (P34.2) folds write their morph params into (P31.12a).
    runtime_morphs: ResMut<'w, AvatarRuntimeMorphs>,
}

/// Drive each rigged avatar's **socket subset** and publish its GPU-pose feed
/// (Phase 4, `roadmap/context/gpu-avatars.md` §5.3–§5.4). The per-avatar
/// skinning joints are gone — the GPU samples, blends and FK-poses the palette
/// in place ([`crate::gpu_avatars`]) — so this system no longer writes ~200
/// joint globals. Instead, per rigged avatar it:
///
/// - folds the keyframe pose + idle + the procedural adjusters (look-at, reach,
///   locomotion IK, body physics) over the **mini pose** — a chain mini-FK
///   restricted to the adjuster / socket joints ([`BevySkeleton::deformed_world_chain`]);
/// - writes only the **socket** entities (worn attachment-point nodes, rigid
///   base parts, the `mHead` camera focus) by their local `Transform`, from the
///   same chain mini-FK (`write_socket_locals`);
/// - publishes the avatar's root matrix plus the **sparse adjuster corrections**
///   (the channels the folds changed vs. the keyframe+idle baseline) to the
///   GPU feed ([`GpuAvatarPoseFeed::publish_real`](crate::gpu_avatars::GpuAvatarPoseFeed)),
///   which passes A+B fold in GPU-side.
///
/// Runs in `PostUpdate` after transform propagation, so it seats the sockets on
/// the just-propagated avatar root. A downlevel device (no GPU path) has no
/// skinning at all, so the whole system is a no-op there.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries; the \
              procedural folds' own resources are already bundled into one \
              `AvatarAdjusters`, and what is left is the animation pipeline, the avatar \
              asset library and state, the ground, the part query, the root globals, \
              and the socket transforms"
)]
pub(crate) fn pose_avatar_skeletons(
    time: Res<Time>,
    manager: Res<AnimationManager>,
    playback: Res<AnimationPlayback>,
    library: Option<Res<AvatarAssetLibrary>>,
    body: Option<Res<AvatarBody>>,
    state: Res<AvatarState>,
    mut ground: ResMut<AvatarGround>,
    mut adjusters: AvatarAdjusters,
    motions: Query<&AvatarMotion>,
    parts: Query<(Entity, &AvatarBodyPart)>,
    // The avatar root's `GlobalTransform`, read to compose each socket and the
    // published feed root under. Read-only: Phase 4 writes no joint globals.
    globals: Query<&GlobalTransform>,
    // The socket entities (attachment-point nodes, rigid base parts, the head
    // socket) are avatar-root children the socket writer places by their **local**
    // `Transform` (§5.4), so ordinary propagation seats the worn/rigid subtrees.
    // A different component from `globals`.
    mut socket_transforms: Query<&mut Transform, Without<crate::world_api::AvatarAnchor>>,
    mut gpu: GpuAvatarHooks<'_, '_>,
) {
    let (Some(library), Some(body)) = (library, body) else {
        return;
    };
    // A downlevel device runs no GPU skinning and has no joint entities, so
    // nothing consumes this system's output — the startup capability check
    // already warned. Skip the whole pass.
    if !gpu.real_active() {
        return;
    }
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let look_debug = crate::look_at::LookAtDebug::from_env();
    let log_ik = crate::locomotion_ik::log_enabled();
    let log_reach = crate::reach::log_enabled();
    let log_physics = crate::body_physics::log_enabled();
    let rigged = state.rigged_agents();
    // Forget the adjuster state (and the runtime morph overrides) of avatars that
    // have despawned.
    adjusters
        .locomotion
        .retain(&|agent| rigged.contains(&agent));
    adjusters.reach.retain(&|agent| rigged.contains(&agent));
    adjusters
        .body_physics
        .retain(&|agent| rigged.contains(&agent));
    adjusters
        .runtime_morphs
        .retain(&|agent| rigged.contains(&agent));
    let leg_joints = LegJoints::resolve(|name| body.joint_index(name));
    let reach_joints = ReachJoints::resolve(|name| body.joint_index(name));
    // The fallback for an avatar whose shape displaces no collision volume (P34.3),
    // hoisted so the per-frame loop borrows rather than allocates.
    let no_volumes = VolumeDeformations::default();
    // The debug T-pose switch: freeze every avatar at its shaped rest pose, so two
    // runs of the viewer frame the same body from the same angle and can be compared
    // pixel for pixel (an avatar's AO would otherwise walk and turn it).
    let t_pose = t_pose_enabled();
    // The quantised procedural idle clock (see [`POSE_IDLE_HZ`]).
    let idle_now = (now * POSE_IDLE_HZ).floor() / POSE_IDLE_HZ;
    for agent in rigged {
        let Some(root) = state.body_root_of(agent) else {
            continue;
        };
        let Some(deform) = state.deformations(agent) else {
            continue;
        };
        // The camera's head-focus socket (§5.4), placed by the socket writer.
        let head_socket = state.head_socket_of(agent);

        // Advance the eye saccade / blink timers **every frame**: blinks drive the
        // (equality-guarded) runtime-morph path, not the skeleton, and stalling the
        // timers would freeze them. The returned event flag was a pose-gate wake
        // source; the gate is gone (Phase 4: every rigged avatar re-poses each
        // frame, the mini pose + socket writes being cheap and no joint globals
        // left to churn), so it is discarded. The T-pose freeze takes none of this.
        if !t_pose {
            let (blink, _event) =
                crate::look_at::advance_eyes(agent, &mut adjusters.look_motion, dt);
            adjusters
                .runtime_morphs
                .set(agent, BLINK_LEFT_PARAM, blink.left);
            adjusters
                .runtime_morphs
                .set(agent, BLINK_RIGHT_PARAM, blink.right);
        }
        let anims: AdjusterAnims = playback.adjuster_anims(agent, now, &manager);

        // Start from the resolved keyframe pose (or an empty rest pose), then fold
        // in the always-on procedural idle adjusters (P31.8) so every avatar
        // breathes and sways subtly even when no animation is playing. The T-pose
        // switch takes neither: the shaped rest skeleton *is* the T-pose.
        let mut pose = if t_pose {
            AnimationPose::default()
        } else {
            let mut pose = playback.poses.get(&agent).cloned().unwrap_or_default();
            crate::procedural::apply_idle_adjustments(&mut pose, idle_now, |name| {
                body.joint_index(name)
            });
            pose
        };
        // The shape's collision-volume displacements (P34.3) ride the same
        // recurrence; an avatar whose shape displaces none has no entry.
        let volumes = state.volume_deformations(agent).unwrap_or(&no_volumes);
        let overrides = state.effective_joint_overrides(agent).unwrap_or_default();
        // The avatar-root global carries the SL → Bevy axis change and the world
        // placement; each joint's Bevy global is that composed with its Second Life
        // world matrix. Copied out so it survives the mutable joint writes below.
        let Ok(root_global) = globals.get(root) else {
            continue;
        };
        let root_global = *root_global;
        let skeleton = library.skeleton();
        // The T-pose switch stops here: the shaped rest skeleton, with none of the
        // procedural adjusters below folded in (they would tilt the head, plant the
        // feet and bounce the body, all of which move between runs).
        if t_pose {
            // Only the socket subset is written, from the chain mini-FK. The
            // scheduler mirrors the freeze GPU-side (no playback staged, idle
            // disabled), so no corrections are needed.
            write_socket_locals(
                &mut socket_transforms,
                &parts,
                &gpu,
                &state,
                &body,
                skeleton,
                agent,
                head_socket,
                (deform, volumes, &overrides, &pose),
            );
            gpu.publish_real(agent, root_global.to_matrix(), Vec::new());
            continue;
        }
        // Fold in the head & eye look-at adjusters (P31.12) before the final world
        // matrices. When the avatar has a look-at target the head aim needs its head
        // and eye joint world positions, so resolve them from an initial deformed
        // pass; without one the eyes only jitter and the head relaxes to rest, so no
        // positions are needed and the single pass below suffices.
        let look_joints = LookAtJoints {
            neck: body.joint_index("mNeck"),
            head: body.joint_index("mHead"),
            eye_left: body.joint_index("mEyeLeft"),
            eye_right: body.joint_index("mEyeRight"),
            alt_eye_left: body.joint_index("mFaceEyeAltLeft"),
            alt_eye_right: body.joint_index("mFaceEyeAltRight"),
        };
        // An initial deformed pass (with the keyframe + idle pose already folded in),
        // which the procedural adjusters that need to know *where the avatar's joints
        // currently are* read from: the look-at's head / eye positions and the neck
        // parent's rotation, and the locomotion adjusters' whole leg geometry (P31.14).
        //
        // Under the in-place GPU path (Phase 2, §5.3) the full solve is
        // replaced by the chain mini-FK over just the adjuster joints: the
        // chain map (which includes every ancestor) is scattered into a dense
        // vector so the adjusters' `&[Mat4]` reads are unchanged; joints
        // outside the closure hold identity, and no adjuster reads one.
        let world0 = {
            let targets: Vec<usize> = ADJUSTER_JOINT_NAMES
                .iter()
                .filter_map(|name| body.joint_index(name))
                .collect();
            let chain = skeleton.deformed_world_chain(deform, volumes, &overrides, &pose, &targets);
            let mut dense = vec![Mat4::IDENTITY; skeleton.len()];
            for (index, matrix) in chain {
                if let Some(slot) = dense.get_mut(index) {
                    *slot = matrix;
                }
            }
            dense
        };
        // The adjuster-diff baseline (§5.3): the mini pose as it stands after
        // the keyframe + idle folds — exactly what pass B computes GPU-side —
        // so the corrections carry only what the adjusters below change.
        let baseline = pose.clone();
        let joint_pos = |index: Option<usize>| {
            index
                .and_then(|i| world0.get(i))
                .map(|matrix| matrix.w_axis.truncate())
        };
        // Only resolve the look-at inputs when the avatar actually has a target;
        // without one the eyes only jitter and the head relaxes to rest.
        let (head_pos, eye_positions, neck_parent_world) =
            if adjusters.look_targets.point(agent).is_some() {
                let eyes = joint_pos(look_joints.eye_left).zip(joint_pos(look_joints.eye_right));
                // The neck joint's parent world rotation (avatar-local Second Life frame),
                // so the head is aimed against where the animated spine actually is.
                let neck_parent = look_joints
                    .neck
                    .and_then(|neck| skeleton.parents().get(neck).copied().flatten())
                    .and_then(|parent| world0.get(parent))
                    .map_or(Quat::IDENTITY, |matrix| {
                        matrix.to_scale_rotation_translation().1
                    });
                (joint_pos(look_joints.head), eyes, neck_parent)
            } else {
                (None, None, Quat::IDENTITY)
            };
        // The head / eye look-at fold. (The blink timers were advanced — and the
        // eyelid morphs published — at the top of the loop, every frame.)
        crate::look_at::apply(
            &mut pose,
            agent,
            &adjusters.look_targets,
            &mut adjusters.look_motion,
            &root_global,
            head_pos,
            eye_positions,
            look_joints,
            neck_parent_world,
            dt,
            look_debug,
        );
        // Fold in the activity-driven reach & aim adjusters (P31.15): the left-arm IK reach
        // toward whatever the avatar has selected (its point-at target) and the torso twist
        // that aims its right hand at its look-at target while a gun-aim animation plays.
        // Like the locomotion fold below, they read the current geometry out of `world0`.
        let reach_report = crate::reach::apply(
            &mut pose,
            &mut adjusters.reach,
            &ReachInput {
                agent,
                world: &world0,
                root: &root_global,
                joints: reach_joints,
                point_at: adjusters.point_at_targets.point(agent),
                look_at: adjusters.look_targets.point(agent),
                aiming: playback.is_aiming(agent),
                dt,
            },
        );
        if log_reach {
            info!(
                "P31.15 reach agent={agent} edit_w={:.2} point_err={:.1}deg aim_w={:.2} \
                 twist={:.1}deg residual={:.1}deg aim_dir=({:+.2},{:+.2},{:+.2}) aiming={} \
                 target={}",
                reach_report.edit_weight,
                reach_report.point_error.to_degrees(),
                reach_report.aim_weight,
                reach_report.torso_twist.to_degrees(),
                reach_report.aim_residual.to_degrees(),
                reach_report.aim_dir.x,
                reach_report.aim_dir.y,
                reach_report.aim_dir.z,
                playback.is_aiming(agent),
                adjusters.point_at_targets.point(agent).is_some(),
            );
        }
        // Fold in the locomotion adjusters (P31.14): the walk-speed servo that keeps
        // the walk cycle's feet in step with the ground, the foot IK that plants a
        // standing avatar's ankles on it, the landing recovery's ground alignment, and
        // the fly bank. They read the leg geometry out of `world0` — the pose as it
        // stands after the keyframe, idle and look-at folds — and correct it.
        let avatar_motion = state
            .body_root_of(agent)
            .and_then(|anchor| motions.get(anchor).ok());
        // Publish this avatar's **pre-IK** ankle world positions for the next frame's
        // ground probe. `world0` is the pose *before* the locomotion fold, so the probe
        // stays a function of the animation alone and the foot IK cannot perturb its own
        // input — see `crate::ground::AvatarGround::targets`.
        if let (Some(left), Some(right)) = (
            leg_joints
                .left
                .and_then(|(_h, _k, ankle)| joint_pos(Some(ankle))),
            leg_joints
                .right
                .and_then(|(_h, _k, ankle)| joint_pos(Some(ankle))),
        ) {
            ground.set_probe_targets(
                agent,
                root_global.transform_point(left),
                root_global.transform_point(right),
            );
        }
        let report = crate::locomotion_ik::apply(
            &mut pose,
            &mut adjusters.locomotion,
            &AdjustInput {
                agent,
                world: &world0,
                root: &root_global,
                joints: leg_joints,
                motion: avatar_motion,
                ground: ground.get(agent),
                anims,
                seated: state.is_seated(agent),
                dt,
            },
        );
        if log_ik {
            // The knee bend angles *after* the fold, recomputed from the final pose: the
            // number that says whether a jitter is the ground under the feet moving or
            // the solve itself flipping between two solutions.
            let posed = skeleton.deformed_world_matrices(deform, volumes, &overrides, &pose);
            let bend = |leg: Option<(usize, usize, usize)>| -> f32 {
                leg.map_or(0.0, |(hip, knee, ankle)| {
                    crate::locomotion_ik::knee_bend_degrees(&posed, hip, knee, ankle)
                })
            };
            info!(
                "P31.14 locomotion-ik agent={agent} walking={} standing={} fall={} \
                 walk_speed={:.3} ik_w={:.2} roll={:.3} ground={:?} disp=({:+.3},{:+.3}) \
                 slope={:.1}deg knee=({:.1},{:.1})deg",
                anims.walking,
                anims.standing,
                anims.fall.is_some(),
                report.walk_speed,
                report.foot_ik_weight,
                report.roll,
                report.ground,
                report.displacement.0,
                report.displacement.1,
                report.slope_deg,
                bend(leg_joints.left),
                bend(leg_joints.right),
            );
        }
        // Fold in the body physics (P34.2): the breast / belly / butt spring-dampers,
        // stepped from where `world0` puts their joints, writing the system body's
        // `*_Driven` morph weights through the runtime-morph pipeline and the rigged
        // body's collision-volume displacements into the pose as position deltas —
        // which is why this runs before the final world matrices below.
        if let Some(physics) = state.body_physics(agent) {
            let report = crate::body_physics::apply(
                &mut pose,
                &mut adjusters.body_physics,
                &mut adjusters.runtime_morphs,
                &BodyPhysicsInput {
                    agent,
                    physics,
                    world: &world0,
                    root: &root_global,
                    dt,
                },
                |name| body.joint_index(name),
            );
            if log_physics {
                info!(
                    "P34.2 body-physics agent={agent} active={} breast_up_down={:.3} \
                     belly_up_down={:.3} butt_up_down={:.3}",
                    report.active, report.breast_up_down, report.belly_up_down, report.butt_up_down,
                );
            }
        }
        // The GPU samples, blends and re-runs FK itself (passes A+B+C). Only the
        // socket subset (worn attachment points, the rigid eyeballs, the camera's
        // head focus) is written, from the §5.4 chain mini-FK over the mini pose
        // — and the adjusters' channel changes are published as sparse
        // corrections pass B folds in.
        write_socket_locals(
            &mut socket_transforms,
            &parts,
            &gpu,
            &state,
            &body,
            skeleton,
            agent,
            head_socket,
            (deform, volumes, &overrides, &pose),
        );
        let corrections = pose_corrections(&baseline, &pose);
        gpu.publish_real(agent, root_global.to_matrix(), corrections);
    }
}

/// Place one avatar's **socket entities** — avatar-root children whose posed
/// world the CPU still owns (§5.4) — by writing their **local** `Transform`
/// (relative to the root) from the same final pose, so ordinary transform
/// propagation seats them and any worn/rigid subtree hanging off them:
///
/// - every **worn attachment-point node** (a node carrying an attachment
///   subtree): local = its joint's posed world × the point's fixed
///   `avatar_lad.xml` offset, so worn attachments ride normal propagation
///   (this is what let the old `pose_attachment_nodes` re-propagation go);
/// - the **rigid base parts** (the eyeballs): local = their bound joint's posed
///   world;
/// - the **head socket** (`mHead`): the camera's third-person focus / mouselook
///   eye (`sl_viewer_world_view::camera::own_avatar_head`).
///
/// The joint worlds are avatar-frame matrices from
/// [`BevySkeleton::deformed_world_chain`] over the same final pose — bit-equal
/// to the full recurrence on these joints (golden-tested), at the cost of only
/// their ancestor chains. Because each socket is a root child, its avatar-frame
/// world matrix *is* its local transform (the root global carries the SL → Bevy
/// change + placement), so no root composition is needed here.
///
/// `chain_inputs` bundles the recurrence inputs `(deform, volumes, overrides,
/// pose)` to stay inside the argument-count lint.
#[expect(
    clippy::too_many_arguments,
    reason = "the socket writer takes the pose driver's own borrowed context (queries, \
              state, skeleton, per-avatar identifiers); packing them into a struct would \
              only move the argument list into a struct literal at the call sites"
)]
fn write_socket_locals(
    socket_transforms: &mut Query<&mut Transform, Without<crate::world_api::AvatarAnchor>>,
    parts: &Query<(Entity, &AvatarBodyPart)>,
    hooks: &GpuAvatarHooks<'_, '_>,
    state: &AvatarState,
    body: &AvatarBody,
    skeleton: &BevySkeleton,
    agent: AgentKey,
    head_socket: Option<Entity>,
    chain_inputs: (
        &SkeletalDeformations,
        &VolumeDeformations,
        &JointOverrides,
        &AnimationPose,
    ),
) {
    let (deform, volumes, overrides, pose) = chain_inputs;
    // The socket subset and, per socket, the joint whose posed world places it.
    let mut targets: Vec<usize> = Vec::new();
    // Rigid base parts (the eyeballs), placed straight from their bound joint.
    let mut rigid_parts: Vec<(Entity, usize)> = Vec::new();
    for (entity, part) in parts {
        if part.agent() != agent {
            continue;
        }
        if let Some(index) = body.rigid_joint_index(part.part()) {
            targets.push(index);
            rigid_parts.push((entity, index));
        }
    }
    // Worn attachment-point nodes, placed from their joint × the fixed offset.
    let mut worn_nodes: Vec<(Entity, usize, Mat4)> = Vec::new();
    for (point_id, node) in state.attachment_nodes_of(agent) {
        // Worn = the node carries an attachment subtree.
        let worn = hooks
            .children
            .get(node)
            .is_ok_and(|children| !children.is_empty());
        if !worn {
            continue;
        }
        let Some((joint_index, offset)) = body.attachment_point(point_id) else {
            continue;
        };
        targets.push(joint_index);
        worn_nodes.push((node, joint_index, offset.to_matrix()));
    }
    // The head socket's joint (the camera's focus).
    let head_index = body.joint_index("mHead");
    if let Some(index) = head_index {
        targets.push(index);
    }
    let world = skeleton.deformed_world_chain(deform, volumes, overrides, pose, &targets);
    // Rigid parts: local (root-relative) = the joint's avatar-frame world.
    for (entity, index) in rigid_parts {
        if let Some(matrix) = world.get(&index)
            && let Ok(mut transform) = socket_transforms.get_mut(entity)
        {
            *transform = Transform::from_matrix(*matrix);
        }
    }
    // Worn nodes: local = the joint's world × the point's fixed offset
    // (`mul_mat4` is a method, not `*`, keeping clear of the workspace
    // `arithmetic_side_effects` lint).
    for (node, index, offset) in worn_nodes {
        if let Some(matrix) = world.get(&index)
            && let Ok(mut transform) = socket_transforms.get_mut(node)
        {
            *transform = Transform::from_matrix(matrix.mul_mat4(&offset));
        }
    }
    // The head socket: local = the `mHead` joint's world.
    if let (Some(socket), Some(index)) = (head_socket, head_index)
        && let Some(matrix) = world.get(&index)
        && let Ok(mut transform) = socket_transforms.get_mut(socket)
    {
        *transform = Transform::from_matrix(*matrix);
    }
}

/// The debug T-pose switch (env `SL_VIEWER_TPOSE=1`): whether to freeze every
/// avatar at its shaped **rest** skeleton — which in Second Life *is* the T-pose —
/// with no keyframe animation, no procedural idle, and none of the look-at /
/// locomotion / reach / body-physics adjusters folded in.
///
/// An avatar's AO walks, turns and fidgets it, so two runs of the viewer never
/// frame the same body the same way. Freezing the pose makes an A/B of anything
/// that shapes the body (a shape slider, a collision-volume displacement, a joint
/// override) comparable between runs. `pub(crate)` because the GPU-avatar
/// scheduler mirrors the freeze (no playback staged, idle disabled in pass B).
pub(crate) fn t_pose_enabled() -> bool {
    std::env::var("SL_VIEWER_TPOSE").as_deref() == Ok("1")
}

#[cfg(test)]
mod tests {
    use super::{PlayState, reconcile_playing};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::Uuid;
    use std::collections::HashMap;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect` when pulling a tracked entry out of the map.
    type TestError = Box<dyn core::error::Error>;

    /// Two distinct stand-in animation ids (the reconcile logic is id-agnostic).
    fn walk() -> Uuid {
        Uuid::from_u128(1)
    }
    fn stand() -> Uuid {
        Uuid::from_u128(2)
    }

    /// The stop time recorded for `id` (its `stopped_at`), or an error if `id` is
    /// no longer tracked in `entry`.
    fn stop_of(entry: &HashMap<Uuid, PlayState>, id: Uuid) -> Result<Option<f32>, TestError> {
        Ok(entry.get(&id).ok_or("animation still tracked")?.stopped_at)
    }

    /// A looping motion dropped from the authoritative set records its stop time
    /// **relative to its own start** (`now - start`), the motion-elapsed timeline
    /// the ease-out weight uses — not the absolute wall clock. Storing the absolute
    /// `now` is what left a looping walk stuck at full weight for seconds (P31.6).
    #[test]
    fn stopped_at_is_relative_to_start() -> Result<(), TestError> {
        let mut entry: HashMap<Uuid, PlayState> = HashMap::new();
        let mut next_order = 0u64;
        // Walk started 10 s into the session.
        reconcile_playing(&mut entry, &mut next_order, &[(walk(), 1)], 10.0);
        // 40 s in, the sim drops walk (empty locomotion set).
        reconcile_playing(&mut entry, &mut next_order, &[], 40.0);
        // Relative stop time is 40 - 10 = 30 s, not the absolute 40 s.
        assert_eq!(stop_of(&entry, walk())?, Some(30.0));
        Ok(())
    }

    /// A still-signalled animation keeps its start (and is un-stopped if it had
    /// begun easing out); a replacement animation starts fresh.
    #[test]
    fn resignal_keeps_start_and_new_starts_fresh() -> Result<(), TestError> {
        let mut entry: HashMap<Uuid, PlayState> = HashMap::new();
        let mut next_order = 0u64;
        reconcile_playing(&mut entry, &mut next_order, &[(walk(), 1)], 5.0);
        // Walk leaves, then is re-signalled with the same sequence id: un-stopped,
        // start preserved.
        reconcile_playing(&mut entry, &mut next_order, &[], 6.0);
        assert_eq!(stop_of(&entry, walk())?, Some(1.0));
        reconcile_playing(&mut entry, &mut next_order, &[(walk(), 1)], 7.0);
        assert_eq!(stop_of(&entry, walk())?, None);
        // Stand replaces walk: walk eases out (relative to its 5 s start), stand
        // starts active.
        reconcile_playing(&mut entry, &mut next_order, &[(stand(), 2)], 9.0);
        assert_eq!(stop_of(&entry, walk())?, Some(9.0 - 5.0));
        assert_eq!(stop_of(&entry, stand())?, None);
        Ok(())
    }
}
