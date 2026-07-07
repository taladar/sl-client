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
//! playing it. [`AnimationDecoded`] announces each finished resolution so the
//! (later) skeleton-driver can react; this phase only resolves and caches —
//! nothing is posed yet.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use sl_anim::{Motion, builtin_animation};
use sl_client_bevy::{
    AssetCacheLimits, AssetKey, AssetStore, AssetType, BevyAssetFetcher, BlobFetcher,
    CAP_VIEWER_ASSET, SlCapabilities, SlEvent, SlSessionEvent,
};

/// Announced (once per animation id) when a background resolve finishes — whether
/// it decoded or failed. A later skeleton-driver (P18.3) reads this to pick up the
/// now-cached [`Motion`]; emitted on failure too so a consumer can stop waiting.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct AnimationDecoded(
    #[expect(
        dead_code,
        reason = "the P18.3 skeleton-driver reads the id to pick up the cached motion"
    )]
    pub(crate) AssetKey,
);

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
        let label = builtin_animation(id.uuid()).map_or("uploaded", |builtin| builtin.name);
        debug!("resolving animation {} (`{label}`)", id.uuid());
        let store = self.store.clone();
        let local = self
            .viewer_assets
            .as_ref()
            .map(|dir| dir.join(format!("{}.anim", id.uuid())));
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
                        None => return None,
                    },
                    Err(_error) => return None,
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
    /// (P18.3).
    #[expect(
        dead_code,
        reason = "the P18.3 skeleton-driver reads the cached motion to pose the avatar"
    )]
    pub(crate) fn motion(&self, id: AssetKey) -> Option<&Arc<Motion>> {
        self.motions.get(&id)
    }

    /// Point the store's fetcher at the region's current `ViewerAsset` URL.
    fn set_cap_url(&self, url: Option<String>) {
        self.fetcher.set_cap_url(url);
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
    manager: Res<AnimationManager>,
) {
    for SlCapabilities(map) in capabilities.read() {
        manager.set_cap_url(map.get(CAP_VIEWER_ASSET).cloned());
    }
}

/// Ingest each `AvatarAnimation` update and request every signalled animation's
/// motion, so it is fetched and decoded ready for the skeleton-driver (P18.3).
/// The request is idempotent, so re-listing the same animation each update is
/// cheap.
pub(crate) fn ingest_avatar_animations(
    mut events: MessageReader<SlEvent>,
    mut manager: ResMut<AnimationManager>,
) {
    for event in events.read() {
        if let SlSessionEvent::AvatarAnimation { animations, .. } = &event.0 {
            for animation in animations {
                manager.request(AssetKey::from(animation.anim_id));
            }
        }
    }
}

/// Poll the in-flight resolve tasks; move each completed decode into the shared
/// cache (or record it unavailable) and announce it with [`AnimationDecoded`].
pub(crate) fn poll_animations(
    mut manager: ResMut<AnimationManager>,
    mut decoded: MessageWriter<AnimationDecoded>,
) {
    // Collect the finished ids first — the borrow of the task map cannot overlap
    // the mutation of the decoded / unavailable maps.
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
        decoded.write(AnimationDecoded(id));
    }
}
