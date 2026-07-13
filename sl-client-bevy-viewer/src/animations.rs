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
//! The module also owns the P18.3 skeleton driver: [`drive_avatar_skeletons`]
//! folds each avatar's `AvatarAnimation` set into a playback clock and resolves a
//! per-joint [`AnimationPose`] from the playing motions, and
//! [`pose_avatar_skeletons`] writes that pose into the skeleton-instance joints'
//! world matrices (in `PostUpdate`, after transform propagation) — recomputing the
//! Second Life skeletal recurrence so a shaped avatar's limbs keep their length
//! under animation rather than shearing.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use sl_anim::{JointContribution, Motion, blend_joint, builtin_animation};
use sl_client_bevy::{
    AgentKey, AnimationPose, AssetCacheLimits, AssetKey, AssetStore, AssetType, BevyAssetFetcher,
    BlobFetcher, CAP_VIEWER_ASSET, SlCapabilities, SlEvent, SlSessionEvent, Uuid, sample_motion,
};

use crate::avatar_assets::AvatarAssetLibrary;
use crate::avatars::{AvatarBody, AvatarBodyPart, AvatarState};

/// The animation resolve/decode/cache pipeline: an [`AssetStore`] over the
/// `ViewerAsset` capability (for downloadable `.anim` assets), the optional
/// `--viewer-assets` directory (for pre-provisioned built-in `.anim` files), the
/// in-flight resolve tasks, the decoded motions already in hand, and the set of
/// ids known to have no fetchable asset (procedural built-ins / failed fetches).
///
/// Mirrors [`MeshManager`](crate::meshes::MeshManager) /
/// [`WearableAssetManager`](crate::bake_inputs::WearableAssetManager).
#[derive(Resource)]
pub(crate) struct AnimationManager {
    /// The generic-asset store doing the `ViewerAsset` fetch, dedupe, off-thread
    /// work, and on-disk caching of `.anim` bytes.
    store: AssetStore,
    /// The store's HTTP fetcher, kept so its `ViewerAsset` capability URL can be
    /// refreshed on a region change.
    fetcher: Arc<BevyAssetFetcher>,
    /// The `--viewer-assets` directory, searched for a `<uuid>.anim` built-in
    /// file before falling back to the `ViewerAsset` fetch; `None` when the flag
    /// was not given.
    viewer_assets: Option<PathBuf>,
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
    /// with no local `.anim` to read instead), held here so the fetch is not run —
    /// and the id not marked permanently [`unavailable`](Self::unavailable) — until
    /// the cap arrives. Drained by [`retry_pending`](Self::retry_pending).
    pending: HashSet<AssetKey>,
}

impl AnimationManager {
    /// Build the manager over a fresh [`BevyAssetFetcher`], backed by the on-disk
    /// asset cache when a cache directory is available (falling back to an
    /// in-memory-only store), and searching `viewer_assets` for local built-in
    /// `.anim` files.
    pub(crate) fn new(viewer_assets: Option<PathBuf>) -> Self {
        let fetcher = Arc::new(BevyAssetFetcher::new());
        let store = build_asset_store(&fetcher, animation_cache_dir());
        Self {
            store,
            fetcher,
            viewer_assets,
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
    pub(crate) fn request(&mut self, id: AssetKey) {
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
        let local = self
            .viewer_assets
            .as_ref()
            .map(|dir| dir.join(format!("{}.anim", id.uuid())));
        // A downloadable `.anim` comes over the `ViewerAsset` cap unless a local
        // built-in file can satisfy it. If neither is available yet (the cap is not
        // set), hold the request rather than run a fetch that would fail and mark
        // the animation permanently unavailable; `retry_pending` re-issues it once
        // the cap arrives.
        let local_exists = local.as_ref().is_some_and(|path| path.exists());
        if !local_exists && !self.fetcher.has_cap_url() {
            let _inserted = self.pending.insert(id);
            return;
        }
        self.pending.remove(&id);
        let label = builtin_animation(id.uuid()).map_or("uploaded", |builtin| builtin.name);
        debug!("resolving animation {} (`{label}`)", id.uuid());
        let store = self.store.clone();
        let task = IoTaskPool::get().spawn(async move {
            // A pre-provisioned built-in `.anim` on disk wins; otherwise fetch the
            // asset over `ViewerAsset`. Both the blocking file read and HTTP fetch
            // run on this IoTaskPool thread, and the decode with them, so the
            // render thread never touches animation bytes.
            let bytes = match local {
                Some(path) if path.exists() => match fs_err::read(&path) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        warn!("reading local animation {}: {error}", path.display());
                        return None;
                    }
                },
                _absent => match store.get(id, AssetType::Animation).await {
                    Ok(entry) => match entry.data() {
                        Some(data) => data.to_vec(),
                        None => {
                            warn!("animation {} fetched but has no data", id.uuid());
                            return None;
                        }
                    },
                    Err(error) => {
                        warn!("fetching animation {} over ViewerAsset: {error}", id.uuid());
                        return None;
                    }
                },
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

    /// Point the store's fetcher at the region's current `ViewerAsset` URL.
    fn set_cap_url(&self, url: Option<String>) {
        self.fetcher.set_cap_url(url);
    }

    /// Re-issue any animation resolves parked before the `ViewerAsset` capability
    /// was known (see [`pending`](Self::pending)), now that it is. A no-op while the
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
}

/// Build an [`AssetStore`] over `fetcher`, disk-backed when the cache opens and
/// in-memory only otherwise (a cache failure must never wedge the viewer).
/// Mirrors [`bake_inputs`](crate::bake_inputs)'s wearable-asset store builder.
fn build_asset_store(fetcher: &Arc<BevyAssetFetcher>, disk_dir: Option<PathBuf>) -> AssetStore {
    let concrete = Arc::clone(fetcher);
    let fetcher: Arc<dyn BlobFetcher> = concrete;
    if let Some(dir) = disk_dir {
        match AssetStore::new(Arc::clone(&fetcher), Some(dir), AssetCacheLimits::default()) {
            Ok(store) => return store,
            Err(error) => warn!("animation disk cache unavailable ({error}); in-memory only"),
        }
    }
    // The disk-less store cannot fail to open; the loop extracts it without an
    // `unwrap`/`expect` and runs exactly once.
    loop {
        match AssetStore::new(Arc::clone(&fetcher), None, AssetCacheLimits::default()) {
            Ok(store) => return store,
            Err(error) => warn!("in-memory animation store failed to open ({error}); retrying"),
        }
    }
}

/// The viewer's on-disk animation-asset cache directory
/// (`<cache>/sl-client-bevy-viewer/animcache`), from `XDG_CACHE_HOME` or
/// `~/.cache`, or `None` when neither is set (the store then runs in-memory only).
fn animation_cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(base.join("sl-client-bevy-viewer").join("animcache"))
}

/// Refresh the store fetcher's `ViewerAsset` capability URL each time the region's
/// capability map is (re)discovered.
pub(crate) fn update_animation_caps(
    mut capabilities: MessageReader<SlCapabilities>,
    mut manager: ResMut<AnimationManager>,
) {
    for SlCapabilities(map) in capabilities.read() {
        manager.set_cap_url(map.get(CAP_VIEWER_ASSET).cloned());
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
    /// This animation's activation recency (a per-avatar monotonic stamp): higher
    /// means more recently started, so it wins ties in priority — the reference
    /// viewer pushes each newly-started motion to the front of its active list.
    /// See [`reconcile_playing`] for how the stamp reproduces Second Life's
    /// present-observer vs. late-arriver ordering.
    order: u64,
}

/// Per-avatar animation *playback* state (P18.3 / P18.4), distinct from the
/// [`AnimationManager`]'s asset resolve/cache: which animations each avatar is
/// playing, their timing / activation order, and the per-joint pose the driver
/// blended this frame for [`pose_avatar_skeletons`] to write into the skeleton's
/// world matrices.
#[derive(Resource, Default)]
pub(crate) struct AnimationPlayback {
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
        let weight = motion.pose_weight(elapsed, play.stopped_at);
        if weight <= 0.0 {
            continue;
        }
        for sampled in sample_motion(motion, elapsed) {
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

/// Resolve each rigged avatar's per-joint animation pose from the motions it is
/// playing, blending concurrent motions by priority with ease-in/out (P18.4), for
/// [`pose_avatar_skeletons`] to apply.
///
/// Each frame it folds the latest `AvatarAnimation` updates into the playback
/// clock ([`reconcile_playing`]), then for every avatar samples each playing,
/// decoded motion at its elapsed time, weights it by its ease-in/out
/// [`pose_weight`](Motion::pose_weight), and blends the per-joint contributions by
/// priority ([`blend_joint`]) — a higher-priority motion dominating a joint while a
/// lower-priority one shows through the weight it leaves unfilled. A motion that
/// has fully eased out is dropped. The resolved [`AnimationPose`]s are stored on
/// the [`AnimationPlayback`] resource; an avatar with no drivable animation is
/// simply omitted, so ordinary transform propagation leaves it at its deformed
/// rest pose. Procedural built-ins (walk / stand / …) have no cached motion, so an
/// idle avatar signalling only those keeps its rest pose.
pub(crate) fn drive_avatar_skeletons(
    time: Res<Time>,
    mut events: MessageReader<SlEvent>,
    manager: Res<AnimationManager>,
    mut playback: ResMut<AnimationPlayback>,
    state: Res<AvatarState>,
    body: Option<Res<AvatarBody>>,
) {
    let now = time.elapsed_secs();
    let playback = playback.as_mut();
    // Reconcile the playback clock with each authoritative animation set.
    for event in events.read() {
        if let SlSessionEvent::AvatarAnimation {
            avatar_id,
            animations,
            ..
        } = &event.0
        {
            let pairs: Vec<(Uuid, i32)> = animations
                .iter()
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
    // Without the avatar asset library there are no skeleton instances to pose.
    let Some(body) = body else {
        playback.poses.clear();
        return;
    };
    // Resolve each avatar's blended per-joint pose from its playing motions — the
    // union of the simulator-driven set and the own avatar's client locomotion and
    // typing sets.
    let mut agents: HashSet<AgentKey> = playback.playing.keys().copied().collect();
    agents.extend(playback.client_locomotion.keys().copied());
    agents.extend(playback.client_typing.keys().copied());
    let mut poses: HashMap<AgentKey, AnimationPose> = HashMap::new();
    for agent in agents {
        // Only a rigged avatar (with skeleton-instance joints) can be posed.
        if state.joint_entities_of(agent).is_none() {
            continue;
        }
        let merged = merge_playing(
            playback.playing.get(&agent),
            playback.client_locomotion.get(&agent),
            playback.client_typing.get(&agent),
        );
        if let Some(pose) = resolve_pose(&merged, now, &manager, |name| body.joint_index(name)) {
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

/// Write each posed avatar's animated joint world matrices straight into the
/// skeleton-instance joints' `GlobalTransform`s (P18.3, the reference viewer's
/// matrix-palette skinning), so a shaped avatar's limbs keep their length under
/// animation instead of shearing.
///
/// Runs in `PostUpdate` **after** transform propagation, so it overwrites the
/// just-propagated rest globals with the animated ones for the frame's skinning /
/// render extraction. For each posed avatar it re-runs the Second Life skeletal
/// recurrence with the resolved [`AnimationPose`] folded in
/// ([`BevySkeleton::deformed_world_matrices`](sl_client_bevy::BevySkeleton::deformed_world_matrices)),
/// composes each joint's Second Life world matrix with the avatar-root global (the
/// SL → Bevy axis change + world placement), and writes it to the joint entity. A
/// rigid base part (the eyeballs, parented to an eye joint) is re-placed from its
/// joint's posed global too, since propagation ran before this.
///
/// Every rigged avatar is written **each frame** — its animated pose when a motion
/// is playing, or its plain deformed rest pose (an empty pose) when none is — so an
/// avatar returns to rest when its animations stop and several overlapping
/// animations with different runtimes compose without any per-animation reset.
/// Bevy's dirty-bit transform propagation cannot recompute a static joint whose
/// `GlobalTransform` the driver overwrote, so the driver owns every rigged avatar's
/// joint globals outright.
pub(crate) fn pose_avatar_skeletons(
    time: Res<Time>,
    playback: Res<AnimationPlayback>,
    library: Option<Res<AvatarAssetLibrary>>,
    body: Option<Res<AvatarBody>>,
    state: Res<AvatarState>,
    parts: Query<(Entity, &AvatarBodyPart)>,
    mut globals: Query<&mut GlobalTransform>,
) {
    let (Some(library), Some(body)) = (library, body) else {
        return;
    };
    let now = time.elapsed_secs();
    for agent in state.rigged_agents() {
        // Start from the resolved keyframe pose (or an empty rest pose), then fold
        // in the always-on procedural idle adjusters (P31.8) so every avatar
        // breathes and sways subtly even when no animation is playing.
        let mut pose = playback.poses.get(&agent).cloned().unwrap_or_default();
        crate::procedural::apply_idle_adjustments(&mut pose, now, |name| body.joint_index(name));
        let Some(root) = state.body_root_of(agent) else {
            continue;
        };
        let Some(joints) = state.joint_entities_of(agent) else {
            continue;
        };
        let Some(deform) = state.deformations(agent) else {
            continue;
        };
        let overrides = state.effective_joint_overrides(agent).unwrap_or_default();
        let world = library
            .skeleton()
            .deformed_world_matrices(deform, &overrides, &pose);
        // The avatar-root global carries the SL → Bevy axis change and the world
        // placement; each joint's Bevy global is that composed with its Second Life
        // world matrix. Copied out so the mutable joint writes below do not overlap
        // the read.
        let Ok(root_global) = globals.get(root) else {
            continue;
        };
        // `mul_mat4` (a method, not the `*` operator) keeps clear of the workspace
        // `arithmetic_side_effects` lint the glam operators trip.
        let root_matrix = root_global.to_matrix();
        for (entity, matrix) in joints.iter().zip(world.iter()) {
            if let Ok(mut global) = globals.get_mut(*entity) {
                *global = GlobalTransform::from(Affine3A::from_mat4(root_matrix.mul_mat4(matrix)));
            }
        }
        // Re-place each rigid base part (eyeballs) from its eye joint's posed
        // global, since transform propagation used the pre-overwrite joint global.
        for (entity, part) in &parts {
            if part.agent() != agent {
                continue;
            }
            if let Some(index) = body.rigid_joint_index(part.part())
                && let Some(matrix) = world.get(index)
                && let Ok(mut global) = globals.get_mut(entity)
            {
                *global = GlobalTransform::from(Affine3A::from_mat4(root_matrix.mul_mat4(matrix)));
            }
        }
    }
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
