//! Assemble our **own** avatar's client-side bake inputs (P15.2).
//!
//! On a grid without server-side baking (OpenSim), our own avatar is an
//! untextured cloud until the *client* composites its bake from the worn
//! wearable layers. This module gathers the inputs that compositing (P15.3)
//! needs:
//!
//! 1. ask the sim for our current outfit (`RequestWearables` →
//!    [`AgentWearables`](sl_client_bevy::SlSessionEvent::AgentWearables));
//! 2. fetch each worn wearable **asset** over the `ViewerAsset` capability and
//!    parse it (`sl-avatar`'s [`WearableAsset`]) into its layer texture ids +
//!    visual-param weights;
//! 3. request each layer texture through the shared [`TextureManager`];
//! 4. once the assets and their textures are in hand, walk each bake region's
//!    plan (`sl-bake`'s [`assemble_layers`], over its
//!    [`region_layers`](sl_client_bevy::region_layers)) — resolving each
//!    layer's texture and its tint from the wearable params — into the ordered
//!    [`Layer`] list the compositor wants, stored in [`OwnBakeInputs`] for
//!    P15.3.
//!
//! Only the worn-wearable *texture* layers (skin bodypaint, clothing, tattoos,
//! alpha masks) plus the solid skin-tone base are modelled; the reference
//! viewer's procedural cosmetic layers (skin shading, make-up, freckles, bump)
//! are out of the simplified compositor's scope. On a central-baking grid
//! (Second Life), where our own bake is server-published (P14) and no
//! `ViewerAsset` wearable fetch is needed to texture ourselves, this still
//! assembles the inputs but they are simply unused by the render path.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::prelude::*;

use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use sl_client_bevy::{
    AssetCacheLimits, AssetKey, AssetStore, AssetType, BakeRegion, BevyAssetFetcher, BlobFetcher,
    CAP_UPDATE_AVATAR_APPEARANCE, CAP_VIEWER_ASSET, Command, FolderType, GateStats,
    InventoryFolderKey, InventoryItem, InventoryKey, Layer, SlCapabilities, SlCommand, SlEvent,
    SlSessionEvent, StoreStats, TextureKey, Uuid, VisualParams, Wearable, WearableAsset,
    WearableType, assemble_layers, avatar_texture,
};
use sl_viewer_world_api::DecodedTextures;

use sl_viewer_kit::avatar_assets::AvatarAssetLibrary;
use sl_viewer_world_objects::textures::{TextureDecoded, TextureManager};

/// How long, in seconds, to wait for the wearable assets / their textures before
/// assembling the bake inputs from whatever has arrived, so a stuck or missing
/// fetch cannot wedge the pipeline forever.
const FETCH_GRACE_SECS: f32 = 20.0;

/// The stage of the one-shot own-avatar bake-input assembly.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum BakeInputStage {
    /// Nothing requested yet.
    #[default]
    Idle,
    /// `RequestWearables` sent; awaiting the `AgentWearables` reply.
    AwaitingWearables,
    /// Fetching the worn wearable assets over `ViewerAsset`.
    FetchingAssets,
    /// All assets parsed; awaiting their layer textures to decode.
    FetchingTextures,
    /// The per-region layer lists are assembled — nothing more to do.
    Ready,
}

/// Our own avatar's assembled client-side bake inputs: the per-region ordered
/// [`Layer`] lists the compositor (P15.3) drapes over the body, plus the
/// bookkeeping that produced them.
#[derive(Debug, Resource, Default)]
pub struct OwnBakeInputs {
    /// The assembly stage.
    stage: BakeInputStage,
    /// Whether the grid offers central (server-side) baking for our own avatar —
    /// the `UpdateAvatarAppearance` capability is present (Second Life). When it is,
    /// the server bake supersedes the client-side composite region-for-region, so
    /// assembling it is pure waste (and strictly *more* asset traffic + JPEG2000
    /// decode than the one already-composited server bake per region): the layer
    /// textures are not fetched and the composite is not built (the perf gate). The
    /// wearable *assets* are still parsed for their shape params, and the appearance
    /// editor still composites on demand for its live preview.
    server_bake_grid: bool,
    /// Bumped each time the per-region layer lists are (re)assembled, so the
    /// client-side composite ([`OwnLocalBake`](crate::avatars::OwnLocalBake)) knows
    /// to re-composite when a runtime outfit change re-assembles the inputs.
    generation: u64,
    /// A runtime outfit change that arrived while the pipeline was still
    /// mid-fetch, deferred until it settles ([`BakeInputStage::Ready`]) rather than
    /// resetting the in-flight fetch — which would let a stale asset-fetch event
    /// decrement the new round's pending count without parsing and assemble an
    /// empty bake. The latest deferred wearables win; applied by
    /// [`drive_wearable_requests`] once ready.
    queued_refetch: Option<Vec<Wearable>>,
    /// The outfit read from the Current Outfit Folder, where the grid's
    /// `AgentWearablesUpdate` is Second Life's placeholder answer.
    cof: CofOutfit,
    /// The worn wearables (those carrying an asset id), from `AgentWearables`.
    wearables: Vec<Wearable>,
    /// The parsed worn wearable assets, in worn order.
    assets: Vec<WearableAsset>,
    /// The wearable asset ids still being fetched.
    pending_assets: HashSet<AssetKey>,
    /// The layer texture ids still being fetched / decoded.
    pending_textures: HashSet<TextureKey>,
    /// The wall-clock deadline (`Time::elapsed_secs`) at which the inputs are
    /// assembled from whatever has arrived; `None` until requesting starts.
    deadline: Option<f32>,
    /// The assembled per-region layer lists — the P15.2 output consumed by P15.3.
    layers: HashMap<BakeRegion, Vec<Layer>>,
}

impl OwnBakeInputs {
    /// The assembled layer list for a bake region, once [`is_ready`](Self::is_ready).
    /// Empty (or absent) until assembly completes. Consumed by the P15.3
    /// composite/render pass
    /// ([`apply_own_local_bake`](crate::avatars::apply_own_local_bake)).
    #[must_use]
    pub(crate) fn region_layers(&self, region: BakeRegion) -> &[Layer] {
        self.layers.get(&region).map_or(&[], Vec::as_slice)
    }

    /// Whether the bake inputs have been assembled.
    #[must_use]
    pub(crate) fn is_ready(&self) -> bool {
        self.stage == BakeInputStage::Ready
    }

    /// The worn wearables' `(type, asset id)` in worn order — what the
    /// reference hashes each bake's cache id from
    /// (`LLWearableData::computeBakedTextureHash`, here
    /// [`avatar_texture::wearables_hash`]).
    /// Unlike [`generation`](Self::generation) it does not move on an
    /// appearance-editor preview, only when the outfit itself does.
    #[must_use]
    pub(crate) fn worn_assets(&self) -> Vec<(WearableType, Uuid)> {
        self.wearables
            .iter()
            .filter_map(|wearable| Some((wearable.wearable_type, wearable.asset_id?)))
            .collect()
    }

    /// The assembly generation, bumped on every (re)assembly. The client-side
    /// composite compares it to detect a runtime outfit change and re-composite.
    #[must_use]
    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    /// Build the transmitted `AvatarAppearance.visual_params` vector from the worn
    /// wearables' params (R12): the avatar's real shape (Shape body proportions,
    /// skin, ...), for publishing *and* rendering the own avatar instead of a
    /// neutral placeholder. A param no worn wearable sets falls back to its table
    /// default, so the result is always a complete, correctly-neutral vector (the
    /// default Ruth shape) even before every wearable is in hand — never the
    /// all-`128` midpoint set that half-applies every asymmetric body morph.
    #[must_use]
    pub fn visual_params(&self, params: &VisualParams) -> Vec<u8> {
        params.encode_appearance(|id| {
            self.assets
                .iter()
                .find_map(|wearable| wearable.params.get(&id).copied())
        })
    }

    /// The worn asset of `wearable_type`, for the appearance editor to seed its
    /// controls from the currently-worn wearable. The first of that type — the
    /// passive viewer keys worn assets by type only, so a layered same-type stack
    /// resolves to its lead layer (a known limitation the editor inherits).
    #[must_use]
    pub fn worn_asset(&self, wearable_type: WearableType) -> Option<&WearableAsset> {
        self.assets
            .iter()
            .find(|asset| asset.wearable_type == wearable_type)
    }

    /// Live-edit preview: substitute the worn asset of `edited`'s wearable type
    /// with `edited` (append it if none is worn) so the shape morph
    /// (`apply_own_shape_from_wearables`)
    /// and the bake composite re-derive from the edited wearable. The caller
    /// follows this with [`request_asset_textures`](Self::request_asset_textures)
    /// and [`reassemble`](Self::reassemble), then clears the local bake's built
    /// flag to re-composite.
    pub fn set_preview_asset(&mut self, edited: WearableAsset) {
        if let Some(slot) = self
            .assets
            .iter_mut()
            .find(|asset| asset.wearable_type == edited.wearable_type)
        {
            *slot = edited;
        } else {
            self.assets.push(edited);
        }
    }

    /// Request (boosted) every non-nil layer texture an edited asset references,
    /// so a just-picked fabric / bodypaint texture decodes for the live bake
    /// preview. New requests are tracked as pending so the editor can tell when
    /// the preview is fully textured.
    pub fn request_asset_textures(
        &mut self,
        asset: &WearableAsset,
        texture_manager: &mut TextureManager,
        store: &DecodedTextures,
    ) {
        for id in asset.textures.values().copied().filter(|id| !id.is_nil()) {
            let key = TextureKey::from(id);
            if store.get(key).is_none() {
                let _new = self.pending_textures.insert(key);
                texture_manager.request_boosted(key, sl_viewer_world_api::AVATAR_BOOST_PRIORITY);
            }
        }
    }

    /// Re-run the per-region layer assembly after a preview edit, so the bake
    /// composite picks up the substituted asset's textures / tints.
    pub fn reassemble(&mut self, store: &DecodedTextures, library: Option<&AvatarAssetLibrary>) {
        assemble(self, store, library);
    }
}

/// Announced (once per asset id) when a background wearable-asset fetch finishes,
/// whether it downloaded or failed. [`assemble_own_bake`] reads it and parses the
/// now-fetched bytes.
#[derive(Message, Debug, Clone, Copy)]
pub struct WearableAssetFetched(AssetKey);

/// The wearable-asset fetch/parse pipeline: an [`AssetStore`] over the
/// `ViewerAsset` capability plus the in-flight fetch tasks and the raw bytes
/// already downloaded. Mirrors the texture / mesh managers.
#[derive(Debug, Resource)]
pub struct WearableAssetManager {
    /// The generic-asset store doing the `ViewerAsset` fetch, dedupe, and on-disk
    /// caching.
    store: AssetStore,
    /// The store's HTTP fetcher, kept so its `ViewerAsset` capability URL can be
    /// refreshed on a region change.
    fetcher: Arc<BevyAssetFetcher>,
    /// The background fetch task per asset id, polled to completion by
    /// [`poll_wearable_assets`].
    inflight: HashMap<AssetKey, Task<Option<Vec<u8>>>>,
    /// Successfully downloaded asset bytes by id.
    fetched: HashMap<AssetKey, Vec<u8>>,
    /// Requests (id → asset class) made before the region's `ViewerAsset`
    /// capability was known, held here instead of failed, and issued for real by
    /// `retry_pending` once the cap is set. Wearable fetches
    /// normally follow the handshake's wearables reply — after the caps — so this
    /// is a latent-race guard rather than a routinely-hit path.
    pending: HashMap<AssetKey, AssetType>,
}

impl FromWorld for WearableAssetManager {
    fn from_world(world: &mut World) -> Self {
        let fetcher = Arc::new(BevyAssetFetcher::new());
        let cache = sl_viewer_platform::paths::ViewerPaths::of(world).asset_cache("assetcache");
        let store = build_asset_store(&fetcher, cache);
        Self {
            store,
            fetcher,
            inflight: HashMap::new(),
            fetched: HashMap::new(),
            pending: HashMap::new(),
        }
    }
}

impl WearableAssetManager {
    /// Ensure `id` (of class `asset_type`) is being fetched. Idempotent.
    pub fn request(&mut self, id: AssetKey, asset_type: AssetType) {
        if id.uuid().is_nil() || self.fetched.contains_key(&id) || self.inflight.contains_key(&id) {
            return;
        }
        // The fetch needs the region's `ViewerAsset` capability. If it is not set
        // yet, hold the request rather than spawn a fetch that would fail for good;
        // `retry_pending` issues it once the cap is up.
        if !self.fetcher.has_cap_url() {
            let _previous = self.pending.insert(id, asset_type);
            return;
        }
        self.pending.remove(&id);
        let store = self.store.clone();
        let task = IoTaskPool::get().spawn(async move {
            match store.get(id, asset_type).await {
                Ok(entry) => entry.data().map(|bytes| bytes.to_vec()),
                Err(_error) => None,
            }
        });
        let _prev = self.inflight.insert(id, task);
    }

    /// A point-in-time snapshot of the wearable-asset fetch/parse pipeline, for
    /// the F3 diagnostics overlay: entry counts bucketed by stage plus the
    /// cumulative disk-cache-hit / GC counters. Delegates to the wrapped
    /// [`AssetStore`].
    #[must_use]
    pub fn stats(&self) -> StoreStats {
        self.store.stats()
    }

    /// A point-in-time snapshot of the wearable store's admission gate: its
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

    /// Drop the in-flight fetches and downloaded-bytes cache so a re-fetch (a
    /// runtime outfit change) re-issues each worn asset request and re-announces its
    /// arrival via [`WearableAssetFetched`]: [`request`](Self::request)
    /// short-circuits on an already-downloaded id, so without this a re-fetch of an
    /// unchanged worn asset would never fire the event that drives its parse. The
    /// pending (cap-not-yet-set) queue is left as is.
    fn reset_for_refetch(&mut self) {
        self.inflight.clear();
        self.fetched.clear();
    }

    /// Issue any requests parked before the `ViewerAsset` capability was known (see
    /// `pending`), now that it is. A no-op while the cap is unset
    /// or nothing is pending. Call this whenever the cap is (re)set.
    fn retry_pending(&mut self) {
        if self.pending.is_empty() || !self.fetcher.has_cap_url() {
            return;
        }
        let pending: Vec<(AssetKey, AssetType)> = self.pending.drain().collect();
        for (id, asset_type) in pending {
            self.request(id, asset_type);
        }
    }
}

/// Build an [`AssetStore`] over `fetcher`, disk-backed when the cache opens and
/// in-memory only otherwise (a cache failure must never wedge the viewer).
fn build_asset_store(
    fetcher: &Arc<BevyAssetFetcher>,
    cache: sl_viewer_platform::paths::DiskCache,
) -> AssetStore {
    let concrete = Arc::clone(fetcher);
    let fetcher: Arc<dyn BlobFetcher> = concrete;
    if let Some(dir) = cache.dir {
        match AssetStore::new(
            Arc::clone(&fetcher),
            Some(dir),
            AssetCacheLimits {
                max_bytes: cache.max_bytes,
                ..AssetCacheLimits::default()
            },
        ) {
            Ok(store) => return store,
            Err(error) => warn!("asset disk cache unavailable ({error}); running in-memory only"),
        }
    }
    // The disk-less store cannot fail to open; the loop extracts it without an
    // `unwrap`/`expect` and runs exactly once.
    loop {
        match AssetStore::new(
            Arc::clone(&fetcher),
            None,
            AssetCacheLimits {
                max_bytes: cache.max_bytes,
                ..AssetCacheLimits::default()
            },
        ) {
            Ok(store) => return store,
            Err(error) => warn!("in-memory asset store failed to open ({error}); retrying"),
        }
    }
}

/// The `ViewerAsset` asset class for a wearable of `wearable_type`: body parts
/// (shape / skin / hair / eyes) are `Bodypart`, everything else is `Clothing`.
const fn wearable_asset_type(wearable_type: WearableType) -> AssetType {
    if wearable_type.is_body_part() {
        AssetType::Bodypart
    } else {
        AssetType::Clothing
    }
}

/// Refresh the wearable-asset store's `ViewerAsset` capability URL when the
/// region's capability map is (re)discovered.
pub fn update_asset_caps(
    mut capabilities: MessageReader<SlCapabilities>,
    mut manager: ResMut<WearableAssetManager>,
    mut state: ResMut<OwnBakeInputs>,
) {
    for SlCapabilities(map) in capabilities.read() {
        manager.set_cap_url(map.get(CAP_VIEWER_ASSET).cloned());
        // The central-baking capability marks a grid that server-bakes our own
        // avatar, so the client-side composite is superseded — gate it off (perf).
        // Latch on: the capability is not withdrawn once offered.
        if map.contains_key(CAP_UPDATE_AVATAR_APPEARANCE) {
            state.server_bake_grid = true;
        }
    }
    // Issue any wearable-asset requests parked while the cap was still unknown.
    manager.retry_pending();
}

/// What to do with a non-initial `AgentWearables` update (a runtime outfit
/// change), decided by the bake pipeline's current [`stage`](BakeInputStage).
enum OutfitAction {
    /// Ignore it — before the handshake requested the first outfit ([`Idle`]);
    /// an `AgentWearables` here is stray.
    ///
    /// [`Idle`]: BakeInputStage::Idle
    Ignore,
    /// The pipeline is settled ([`Ready`](BakeInputStage::Ready)): re-fetch now.
    Refetch,
    /// A fetch is still in flight ([`FetchingAssets`]/[`FetchingTextures`]):
    /// **defer** it. Resetting now would corrupt the in-flight bake — a stale
    /// asset-fetch event from the aborted round would decrement the new round's
    /// pending count without parsing, assembling an empty bake (the login
    /// "assembled from 0 wearables" symptom). Apply it once ready.
    ///
    /// [`FetchingAssets`]: BakeInputStage::FetchingAssets
    /// [`FetchingTextures`]: BakeInputStage::FetchingTextures
    Defer,
}

/// Decide how to handle a runtime `AgentWearables` update at `stage`. No
/// update-`serial` dedup on purpose (mirroring the server-bake path,
/// [`drive_server_bake`](crate::appearance::drive_server_bake)): live on aditi the
/// sim re-broadcasts our wearables on an outfit change *without* advancing the
/// serial, so a serial guard would silently drop real changes; a redundant
/// re-fetch of an unchanged outfit is merely a cache-hit re-composite, whereas a
/// missed change leaves a stale bake. [`AwaitingWearables`](BakeInputStage::AwaitingWearables)
/// is handled by the initial-reply arm, not here.
const fn outfit_action(stage: BakeInputStage) -> OutfitAction {
    match stage {
        BakeInputStage::Idle | BakeInputStage::AwaitingWearables => OutfitAction::Ignore,
        BakeInputStage::Ready => OutfitAction::Refetch,
        BakeInputStage::FetchingAssets | BakeInputStage::FetchingTextures => OutfitAction::Defer,
    }
}

/// The `AT_LINK` asset type of an item link.
const AT_LINK: i8 = 24;

/// The outfit as the Current Outfit Folder states it.
///
/// Second Life answers every `AgentWearablesRequest` with four fixed
/// placeholder items ([`Wearable::is_placeholder_outfit`]) and never states a
/// shape there: the outfit is the folder's links alone
/// (`book/src/gridspec/appearance.md` § Outfits). On such a grid the worn
/// wearables are read from the folder — its links, and each link's target
/// item for the wearable's asset and slot — as the reference's
/// `LLAppearanceMgr::updateAppearanceFromCOF` does.
#[derive(Debug, Default)]
struct CofOutfit {
    /// Whether the outfit is read from the folder: the grid answered the
    /// placeholders.
    active: bool,
    /// The folder, once known.
    folder: Option<InventoryFolderKey>,
    /// The folder's item links as last listed: each target and the link's
    /// description, in listed order.
    links: Vec<(InventoryKey, String)>,
    /// What each target resolved to: a wearable, or `None` for anything
    /// else (an attachment, a gesture).
    targets: HashMap<InventoryKey, Option<Wearable>>,
    /// When the current links were listed (`Time::elapsed_secs`), so a target
    /// that never resolves cannot hold the outfit back for ever.
    listed_at: Option<f32>,
    /// Whether the outfit of the current links was handed on.
    delivered: bool,
}

impl CofOutfit {
    /// Takes a listing of the folder: its item links. Returns the targets not
    /// yet resolved, to fetch; nothing when the links are unchanged.
    fn listed(&mut self, items: &[InventoryItem], now: f32) -> Vec<InventoryKey> {
        let links: Vec<(InventoryKey, String)> = items
            .iter()
            .filter(|item| item.item_type == AT_LINK)
            .map(|item| (InventoryKey::from(item.asset_id), item.description.clone()))
            .collect();
        self.note_items(items);
        if links == self.links {
            return Vec::new();
        }
        self.links = links;
        self.listed_at = Some(now);
        self.delivered = false;
        self.links
            .iter()
            .map(|(target, _description)| *target)
            .filter(|target| !self.targets.contains_key(target))
            .collect()
    }

    /// Notes every item of `items` that is a link's target.
    fn note_items(&mut self, items: &[InventoryItem]) {
        for item in items {
            if self.links.iter().any(|(target, _)| *target == item.item_id) {
                let _previous = self.targets.insert(item.item_id, wearable_of(item));
            }
        }
    }

    /// The outfit, once every target resolved or `grace` seconds passed since
    /// the listing — `None` before, and after it was handed on.
    fn ready(&mut self, now: f32, grace: f32) -> Option<Vec<Wearable>> {
        if !self.active || self.delivered || self.links.is_empty() {
            return None;
        }
        let resolved = self
            .links
            .iter()
            .all(|(target, _)| self.targets.contains_key(target));
        let waited = self.listed_at.is_some_and(|at| now >= at + grace);
        if !resolved && !waited {
            return None;
        }
        self.delivered = true;
        Some(cof_wearables(&self.links, &self.targets))
    }
}

/// The wearable `item` is, where it is one: a body part or clothing, its slot
/// the low byte of its flags.
fn wearable_of(item: &InventoryItem) -> Option<Wearable> {
    let asset_type = AssetType::from_code(i32::from(item.item_type));
    matches!(asset_type, AssetType::Bodypart | AssetType::Clothing).then(|| Wearable {
        item_id: item.item_id,
        asset_id: Some(item.asset_id),
        wearable_type: WearableType::from_code(u8::try_from(item.flags & 0xFF).unwrap_or(0)),
    })
}

/// The order a link's description gives its layer: the number of the
/// reference's `@<type * 100 + index>` token, or `None` where it is not one.
fn order_token(description: &str) -> Option<u32> {
    description.strip_prefix('@')?.parse().ok()
}

/// The worn wearables `links` resolve to, by slot and, within one, in the
/// order of the links' tokens — the order Second Life bakes the layers in —
/// and then as listed.
fn cof_wearables(
    links: &[(InventoryKey, String)],
    targets: &HashMap<InventoryKey, Option<Wearable>>,
) -> Vec<Wearable> {
    let mut worn: Vec<(u8, u32, Wearable)> = links
        .iter()
        .filter_map(|(target, description)| {
            let wearable = (*targets.get(target)?)?;
            Some((
                wearable.wearable_type.to_code(),
                order_token(description).unwrap_or(u32::MAX),
                wearable,
            ))
        })
        .collect();
    worn.sort_by_key(|(slot, order, _wearable)| (*slot, *order));
    let mut seen: HashSet<InventoryKey> = HashSet::new();
    worn.into_iter()
        .map(|(_slot, _order, wearable)| wearable)
        .filter(|wearable| seen.insert(wearable.item_id))
        .collect()
}

/// Hands `wearables` — the outfit — to the pipeline: the first bake while it
/// awaits the outfit, a runtime change once it has one.
fn take_outfit(
    state: &mut OwnBakeInputs,
    manager: &mut WearableAssetManager,
    wearables: &[Wearable],
    now: f32,
) {
    if state.stage == BakeInputStage::AwaitingWearables {
        start_asset_fetch(state, manager, wearables, now);
        return;
    }
    match outfit_action(state.stage) {
        OutfitAction::Refetch => {
            reset_for_refetch(state, manager);
            start_asset_fetch(state, manager, wearables, now);
            debug!("own outfit changed at runtime; re-fetching worn assets for a re-bake");
        }
        OutfitAction::Defer => state.queued_refetch = Some(wearables.to_vec()),
        OutfitAction::Ignore => {}
    }
}

/// Drive the request half: once the region handshake completes, ask the sim for
/// our current outfit; when it replies, record the worn wearables and start
/// fetching their assets.
pub fn drive_wearable_requests(
    time: Res<Time>,
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<OwnBakeInputs>,
    mut manager: ResMut<WearableAssetManager>,
    mut writer: MessageWriter<SlCommand>,
) {
    // Apply a deferred runtime outfit change once the pipeline settles (it arrived
    // mid-fetch and was queued to avoid corrupting the in-flight bake). The latest
    // deferred wearables win.
    if state.stage == BakeInputStage::Ready
        && let Some(wearables) = state.queued_refetch.take()
    {
        reset_for_refetch(&mut state, &mut manager);
        start_asset_fetch(&mut state, &mut manager, &wearables, time.elapsed_secs());
        debug!("applying deferred outfit change; re-fetching worn assets for a re-bake");
    }
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::RegionHandshakeComplete if state.stage == BakeInputStage::Idle => {
                writer.write(SlCommand(Command::RequestWearables));
                state.stage = BakeInputStage::AwaitingWearables;
                state.deadline = Some(time.elapsed_secs() + FETCH_GRACE_SECS);
                debug!("requested own wearables for client-side bake inputs");
            }
            // Second Life's placeholder answer: the outfit is the Current
            // Outfit Folder's, so read the folder (again, on every answer, as
            // a request after a change is how a change is noticed).
            SlSessionEvent::AgentWearables { wearables, .. }
                if Wearable::is_placeholder_outfit(wearables) =>
            {
                state.cof.active = true;
                match state.cof.folder {
                    Some(folder) => {
                        writer.write(SlCommand(Command::RequestFolderContents(folder)));
                    }
                    None => {
                        writer.write(SlCommand(Command::QueryInventoryFolders));
                    }
                }
                debug!("the grid keeps the outfit in the Current Outfit Folder; reading it");
            }
            // The initial outfit reply: start the first bake.
            SlSessionEvent::AgentWearables { wearables, .. }
                if state.stage == BakeInputStage::AwaitingWearables =>
            {
                start_asset_fetch(&mut state, &mut manager, wearables, time.elapsed_secs());
            }
            // A runtime outfit change (took off / put on / swapped a worn layer):
            // the sim re-broadcasts our wearables. Re-fetch + re-assemble so the
            // change shows — the passive-viewer assumption that the worn outfit
            // never changes no longer holds. Only when the pipeline is settled; a
            // change mid-fetch is deferred (applied above once ready) so it cannot
            // corrupt the in-flight bake.
            SlSessionEvent::AgentWearables { wearables, .. } => {
                take_outfit(&mut state, &mut manager, wearables, time.elapsed_secs());
            }
            SlSessionEvent::InventoryFolders(folders)
                if state.cof.active && state.cof.folder.is_none() =>
            {
                state.cof.folder = folders
                    .iter()
                    .find(|folder| folder.folder_type == FolderType::CurrentOutfit)
                    .map(|folder| folder.folder_id);
                if let Some(folder) = state.cof.folder {
                    writer.write(SlCommand(Command::RequestFolderContents(folder)));
                }
            }
            // A listing of the folder: its links, and the targets still to
            // fetch. A listing whose links changed is an outfit change.
            SlSessionEvent::InventoryDescendents {
                folder_id, items, ..
            } if state.cof.active && state.cof.folder == Some(*folder_id) => {
                for target in state.cof.listed(items, time.elapsed_secs()) {
                    writer.write(SlCommand(Command::Ais3FetchItem(target)));
                }
            }
            SlSessionEvent::InventoryDescendents { items, .. }
            | SlSessionEvent::InventoryBulkUpdate { items, .. }
                if state.cof.active =>
            {
                state.cof.note_items(items);
            }
            _other => {}
        }
    }
    let now = time.elapsed_secs();
    if let Some(wearables) = state.cof.ready(now, FETCH_GRACE_SECS) {
        info!(
            "read {} worn wearable(s) from the Current Outfit Folder",
            wearables.len()
        );
        take_outfit(&mut state, &mut manager, &wearables, now);
    }
}

/// Record the worn wearables (those with an asset id) and kick off their asset
/// fetches, moving to [`BakeInputStage::FetchingAssets`] (or straight to
/// [`BakeInputStage::Ready`] with an empty bake when nothing is worn).
fn start_asset_fetch(
    state: &mut OwnBakeInputs,
    manager: &mut WearableAssetManager,
    wearables: &[Wearable],
    now: f32,
) {
    state.wearables = wearables
        .iter()
        .filter(|w| w.asset_id.is_some_and(|id| !id.is_nil()))
        .copied()
        .collect();
    for wearable in &state.wearables {
        if let Some(asset_id) = wearable.asset_id {
            let key = AssetKey::from(asset_id);
            state.pending_assets.insert(key);
            manager.request(key, wearable_asset_type(wearable.wearable_type));
        }
    }
    state.deadline = Some(now + FETCH_GRACE_SECS);
    if state.pending_assets.is_empty() {
        info!("no worn wearable assets; own bake inputs are empty");
        state.stage = BakeInputStage::Ready;
    } else {
        info!(
            "fetching {} worn wearable asset(s) for client-side bake",
            state.pending_assets.len()
        );
        state.stage = BakeInputStage::FetchingAssets;
    }
}

/// Clear the assembled bake inputs and the asset manager's fetch bookkeeping so a
/// runtime outfit change re-fetches every worn asset from scratch: the parsed
/// assets, pending sets and per-region layers are stale, and the manager's
/// downloaded-bytes cache would otherwise short-circuit the re-request without
/// re-announcing it. The on-disk / in-memory [`AssetStore`] cache is untouched, so
/// an unchanged worn asset re-fetches straight from cache; a removed layer's asset
/// is simply no longer requested, so it drops out of the re-assembled bake.
fn reset_for_refetch(state: &mut OwnBakeInputs, manager: &mut WearableAssetManager) {
    state.assets.clear();
    state.pending_assets.clear();
    state.pending_textures.clear();
    state.layers.clear();
    manager.reset_for_refetch();
}

/// Poll the in-flight wearable-asset fetches; move each completed download into
/// the manager's `fetched` map and announce it with [`WearableAssetFetched`]
/// (emitted on failure too, so the assembler stops waiting on it).
pub fn poll_wearable_assets(
    mut manager: ResMut<WearableAssetManager>,
    mut fetched: MessageWriter<WearableAssetFetched>,
) {
    let mut finished: Vec<(AssetKey, Option<Vec<u8>>)> = Vec::new();
    for (&id, task) in &mut manager.inflight {
        if let Some(result) = block_on(poll_once(task)) {
            finished.push((id, result));
        }
    }
    for (id, result) in finished {
        let _removed = manager.inflight.remove(&id);
        if let Some(bytes) = result {
            let _prev = manager.fetched.insert(id, bytes);
        }
        fetched.write(WearableAssetFetched(id));
    }
}

/// The visual-param id of the Shape's `male` switch, used when the avatar asset
/// library is not loaded and its name table cannot be consulted.
const MALE_PARAM_ID: i32 = 80;

/// Whether a worn `shape` says the avatar is male — the reference viewer's
/// `getVisualParamWeight("male") > 0.5` rule (`LLVOAvatar::getSex`).
///
/// `None` when no shape is worn yet, so a caller can tell "not known" from
/// "female" — the appearance editor defaults such an avatar to female (the SL
/// default body), while RLV's `@getdebug_avatarsex` answers nothing at all.
#[must_use]
pub fn shape_is_male(
    library: Option<&AvatarAssetLibrary>,
    shape: Option<&WearableAsset>,
) -> Option<bool> {
    let male_id = library
        .and_then(|library| library.params().by_name("male"))
        .map_or(MALE_PARAM_ID, |param| param.id);
    shape.map(|shape| shape.params.get(&male_id).copied().unwrap_or(0.0) > 0.5)
}

/// Publish the avatar's sex for RLV's `@getdebug_avatarsex`, which is one of
/// the two facts the debug-setting allowlist cannot read out of a settings
/// store (`RlvExtGetSet::onGetPseudoDebug`).
///
/// The avatar layer publishes it because the avatar layer is what knows it: the
/// answer is the worn Shape's `male` param, which lives in the bake inputs.
pub fn publish_rlv_avatar_sex(
    inputs: Res<OwnBakeInputs>,
    library: Option<Res<AvatarAssetLibrary>>,
    mut facts: ResMut<sl_viewer_world_api::rlv::RlvExtFacts>,
) {
    let is_male = shape_is_male(library.as_deref(), inputs.worn_asset(WearableType::Shape));
    if facts.avatar_is_male != is_male {
        facts.avatar_is_male = is_male;
    }
}

/// The two arrivals that advance an own-bake assembly, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): a fetched wearable asset and
/// a decoded texture.
#[derive(Debug, bevy::ecs::system::SystemParam)]
pub struct BakeArrivals<'w, 's> {
    /// Wearable assets whose parameters and textures just landed.
    assets: MessageReader<'w, 's, WearableAssetFetched>,
    /// Textures whose decode just completed.
    textures: MessageReader<'w, 's, TextureDecoded>,
}

/// Drive the assembly half: parse each fetched wearable asset and request its
/// layer textures; as those decode, once every asset and texture is resolved (or
/// the grace period lapses) assemble the per-region layer lists.
pub fn assemble_own_bake(
    time: Res<Time>,
    mut arrivals: BakeArrivals,
    manager: Res<WearableAssetManager>,
    library: Option<Res<AvatarAssetLibrary>>,
    mut texture_manager: ResMut<TextureManager>,
    store: Res<DecodedTextures>,
    mut state: ResMut<OwnBakeInputs>,
) {
    // Parse newly fetched assets and request their layer textures.
    for &WearableAssetFetched(id) in arrivals.assets.read() {
        if !state.pending_assets.remove(&id) {
            continue;
        }
        if let Some(bytes) = manager.fetched.get(&id) {
            parse_and_request_textures(&mut state, &mut texture_manager, &store, bytes);
        }
    }
    // As layer textures decode, clear them from the pending set.
    for &TextureDecoded(id) in arrivals.textures.read() {
        let _removed = state.pending_textures.remove(&id);
    }

    if state.stage == BakeInputStage::FetchingAssets && state.pending_assets.is_empty() {
        state.stage = BakeInputStage::FetchingTextures;
    }

    let ready = matches!(
        state.stage,
        BakeInputStage::FetchingAssets | BakeInputStage::FetchingTextures
    ) && state.pending_assets.is_empty()
        && state.pending_textures.is_empty();
    let timed_out = matches!(
        state.stage,
        BakeInputStage::FetchingAssets | BakeInputStage::FetchingTextures
    ) && state
        .deadline
        .is_some_and(|deadline| time.elapsed_secs() >= deadline);

    if ready || timed_out {
        if timed_out && !ready {
            // Name the wearable types still in flight so a recurring stall is
            // diagnosable: which layer never arrived within the grace window, and
            // whether it is the same one across runs. On a server-bake grid these
            // feed only shape params (the body is textured by the server bake), so
            // a pending clothing asset is cosmetically harmless there; a pending
            // shape / skin skews the own-avatar proportions toward the defaults.
            let pending_types: Vec<WearableType> = state
                .wearables
                .iter()
                .filter(|wearable| {
                    wearable
                        .asset_id
                        .is_some_and(|id| state.pending_assets.contains(&AssetKey::from(id)))
                })
                .map(|wearable| wearable.wearable_type)
                .collect();
            warn!(
                "assembling own bake inputs after grace period ({} asset(s) {pending_types:?}, \
                 {} texture(s) still pending; server_bake_grid={})",
                state.pending_assets.len(),
                state.pending_textures.len(),
                state.server_bake_grid,
            );
        }
        // On a server-bake grid the composite is superseded, so skip building it
        // (the ~55 ms `build_local_bake` spike is pure waste there — the perf gate);
        // the parsed assets are still available for shape resolution, and the
        // appearance editor composites on demand via `reassemble`. Elsewhere
        // (OpenSim) assemble the per-region layer lists the composite drapes.
        if !state.server_bake_grid {
            assemble(&mut state, &store, library.as_deref());
        }
        state.stage = BakeInputStage::Ready;
    }
}

/// Parse one fetched wearable asset's bytes and request each of its layer
/// textures through the shared texture pipeline (skipping ones already decoded).
fn parse_and_request_textures(
    state: &mut OwnBakeInputs,
    texture_manager: &mut TextureManager,
    store: &DecodedTextures,
    bytes: &[u8],
) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        warn!("worn wearable asset is not UTF-8; skipping");
        return;
    };
    let asset = match WearableAsset::parse(text) {
        Ok(asset) => asset,
        Err(error) => {
            warn!("failed to parse worn wearable asset: {error}");
            return;
        }
    };
    // Request every layer texture this wearable supplies that a bake region uses —
    // unless the grid server-bakes our own avatar, where the client composite is
    // superseded, so its (JPEG2000-heavy) layer textures are never needed and are
    // not fetched (the perf gate). The wearable asset itself is still parsed and
    // kept for its shape params.
    if !state.server_bake_grid {
        for &(slot, _name, _wearable) in &avatar_texture::LAYER_TEXTURES {
            if !asset.supplies_layer(slot) {
                continue;
            }
            if let Some(id) = asset.layer_texture(slot) {
                let key = TextureKey::from(id);
                if store.get(key).is_none() {
                    state.pending_textures.insert(key);
                    // Our own avatar's client-side bake layer textures are boosted
                    // (P20.2): they clothe the own avatar and are not ranked by the
                    // on-screen prim-face pass, so a fixed boost loads them promptly.
                    texture_manager
                        .request_boosted(key, sl_viewer_world_api::AVATAR_BOOST_PRIORITY);
                }
            }
        }
    }
    state.assets.push(asset);
}

/// Assemble the per-region layer lists from the parsed wearable assets and their
/// decoded textures, resolving each layer's tint from the wearable's visual
/// params (when the `avatar_lad.xml` table is loaded). Logs a one-line summary.
fn assemble(
    state: &mut OwnBakeInputs,
    store: &DecodedTextures,
    library: Option<&AvatarAssetLibrary>,
) {
    let layers = assemble_layers(
        &state.assets,
        library.map(AvatarAssetLibrary::params),
        |id| {
            store
                .get(TextureKey::from(id))
                .map(|decoded| (**decoded).clone())
        },
        |file| library.and_then(|lib| lib.static_texture(file).cloned()),
    );
    let summary: Vec<String> = BakeRegion::ALL
        .iter()
        .map(|region| {
            format!(
                "{}={}",
                region.name(),
                layers.get(region).map_or(0, Vec::len)
            )
        })
        .collect();
    info!(
        "assembled own client-side bake inputs from {} wearable(s): {}",
        state.assets.len(),
        summary.join(" ")
    );
    state.layers = layers;
    // Signal the client-side composite that the inputs changed so it re-composites
    // (a runtime outfit change, or an appearance-editor live edit).
    state.generation = state.generation.wrapping_add(1);
}

#[cfg(test)]
mod tests {
    use super::{BakeInputStage, CofOutfit, OutfitAction, outfit_action};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, InventoryFolderKey, InventoryItem, InventoryKey, OwnerKey, Permissions5, Uuid,
        WearableType,
    };

    /// A runtime `AgentWearables` is ignored before the handshake requested the
    /// first outfit; the initial reply itself is handled by its own arm, not here.
    #[test]
    fn outfit_change_ignored_before_the_handshake() {
        assert!(matches!(
            outfit_action(BakeInputStage::Idle),
            OutfitAction::Ignore
        ));
        assert!(matches!(
            outfit_action(BakeInputStage::AwaitingWearables),
            OutfitAction::Ignore
        ));
    }

    /// A settled pipeline re-fetches immediately; a change that lands mid-fetch is
    /// deferred so it cannot corrupt the in-flight bake (the "assembled from 0"
    /// login race).
    #[test]
    fn outfit_change_refetches_when_settled_defers_mid_fetch() {
        assert!(matches!(
            outfit_action(BakeInputStage::Ready),
            OutfitAction::Refetch
        ));
        assert!(matches!(
            outfit_action(BakeInputStage::FetchingAssets),
            OutfitAction::Defer
        ));
        assert!(matches!(
            outfit_action(BakeInputStage::FetchingTextures),
            OutfitAction::Defer
        ));
    }

    /// An item `id` of `item_type` holding `asset`, its flags `flags` and its
    /// description `description`.
    fn record(
        id: u128,
        item_type: i8,
        flags: u32,
        asset: u128,
        description: &str,
    ) -> InventoryItem {
        InventoryItem {
            item_id: InventoryKey::from(Uuid::from_u128(id)),
            folder_id: InventoryFolderKey::from(Uuid::nil()),
            name: String::new(),
            description: description.to_owned(),
            asset_id: Uuid::from_u128(asset),
            item_type,
            inv_type: 18,
            flags,
            sale_type: 0,
            sale_price: None,
            creation_date: 0,
            owner: OwnerKey::Agent(AgentKey::from(Uuid::nil())),
            last_owner_id: Uuid::nil(),
            creator_id: AgentKey::from(Uuid::nil()),
            group: None,
            permissions: Permissions5::default(),
        }
    }

    /// An item of `item_type` and wearable `slot`, holding `asset`.
    fn item(id: u128, item_type: i8, slot: u8, asset: u128) -> InventoryItem {
        record(id, item_type, u32::from(slot), asset, "")
    }

    /// A link to `target` described `description`.
    fn link(id: u128, target: u128, description: &str) -> InventoryItem {
        record(id, 24, 0, target, description)
    }

    /// The folder's links resolve to the worn wearables: body parts and
    /// clothing only, by slot, each slot's layers in the order of their
    /// tokens rather than as listed.
    #[test]
    fn the_folder_states_the_outfit_in_token_order() {
        let mut cof = CofOutfit {
            active: true,
            ..CofOutfit::default()
        };
        let fetch = cof.listed(
            &[
                link(1, 0x10, "@402"),
                link(2, 0x11, ""),
                link(3, 0x12, "@401"),
                link(4, 0x13, ""),
            ],
            0.0,
        );
        assert_eq!(fetch.len(), 4);
        assert_eq!(cof.ready(1.0, 20.0), None, "nothing resolved yet");
        cof.note_items(&[
            item(0x10, 5, 4, 0xa0),
            item(0x11, 13, 0, 0xa1),
            item(0x12, 5, 4, 0xa2),
            // An attachment is no wearable.
            item(0x13, 6, 0, 0xa3),
        ]);
        let worn = cof.ready(1.0, 20.0).unwrap_or_default();
        let order: Vec<(WearableType, Option<Uuid>)> = worn
            .iter()
            .map(|worn| (worn.wearable_type, worn.asset_id))
            .collect();
        assert_eq!(
            order,
            vec![
                (WearableType::Shape, Some(Uuid::from_u128(0xa1))),
                (WearableType::Shirt, Some(Uuid::from_u128(0xa2))),
                (WearableType::Shirt, Some(Uuid::from_u128(0xa0))),
            ]
        );
        assert_eq!(cof.ready(2.0, 20.0), None, "handed on once");
    }

    /// The same links listed again are no change; different ones are, and
    /// only their new targets are fetched.
    #[test]
    fn a_relisted_folder_is_a_change_only_when_its_links_are() {
        let mut cof = CofOutfit {
            active: true,
            ..CofOutfit::default()
        };
        let _fetch = cof.listed(&[link(1, 0x10, "")], 0.0);
        cof.note_items(&[item(0x10, 13, 0, 0xa0)]);
        assert!(cof.ready(0.0, 20.0).is_some());
        assert!(cof.listed(&[link(1, 0x10, "")], 1.0).is_empty());
        assert_eq!(cof.ready(1.0, 20.0), None);
        let fetch = cof.listed(&[link(1, 0x10, ""), link(2, 0x11, "")], 2.0);
        assert_eq!(fetch, vec![InventoryKey::from(Uuid::from_u128(0x11))]);
        assert_eq!(cof.ready(3.0, 20.0), None, "the new target is unresolved");
        assert_eq!(
            cof.ready(22.0, 20.0).map(|worn| worn.len()),
            Some(1),
            "after the grace, what resolved"
        );
    }
}
