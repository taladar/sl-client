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
//!    plan (`sl-bake`'s [`region_layers`]) — resolving each layer's texture and
//!    its tint from the wearable params — into the ordered [`Layer`] list the
//!    compositor wants, stored in [`OwnBakeInputs`] for P15.3.
//!
//! Only the worn-wearable *texture* layers (skin bodypaint, clothing, tattoos,
//! alpha masks) plus the solid skin-tone base are modelled; the reference
//! viewer's procedural cosmetic layers (skin shading, make-up, freckles, bump)
//! are out of the simplified compositor's scope. On a central-baking grid
//! (Second Life), where our own bake is server-published (P14) and no
//! `ViewerAsset` wearable fetch is needed to texture ourselves, this still
//! assembles the inputs but they are simply unused by the render path.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use sl_client_bevy::{
    AssetCacheLimits, AssetKey, AssetStore, AssetType, BakeRegion, BevyAssetFetcher, BlobFetcher,
    CAP_VIEWER_ASSET, Command, DecodedTexture, Layer, LayerTint, SlCapabilities, SlCommand,
    SlEvent, SlSessionEvent, TextureKey, VisualParams, Wearable, WearableAsset, WearableType,
    avatar_texture, combine_layer_color, global_color, region_layers,
};

use crate::avatar_assets::AvatarAssetLibrary;
use crate::textures::{TextureDecoded, TextureManager};

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
#[derive(Resource, Default)]
pub(crate) struct OwnBakeInputs {
    /// The assembly stage.
    stage: BakeInputStage,
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
    /// Empty (or absent) until assembly completes.
    #[must_use]
    #[expect(
        dead_code,
        reason = "the P15.2 output surface, consumed by the P15.3 composite/render pass"
    )]
    pub(crate) fn region_layers(&self, region: BakeRegion) -> &[Layer] {
        self.layers.get(&region).map_or(&[], Vec::as_slice)
    }

    /// Whether the bake inputs have been assembled.
    #[must_use]
    #[expect(
        dead_code,
        reason = "the P15.2 output surface, consumed by the P15.3 composite/render pass"
    )]
    pub(crate) fn is_ready(&self) -> bool {
        self.stage == BakeInputStage::Ready
    }
}

/// Announced (once per asset id) when a background wearable-asset fetch finishes,
/// whether it downloaded or failed. [`assemble_own_bake`] reads it and parses the
/// now-fetched bytes.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct WearableAssetFetched(AssetKey);

/// The wearable-asset fetch/parse pipeline: an [`AssetStore`] over the
/// `ViewerAsset` capability plus the in-flight fetch tasks and the raw bytes
/// already downloaded. Mirrors the texture / mesh managers.
#[derive(Resource)]
pub(crate) struct WearableAssetManager {
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
}

impl FromWorld for WearableAssetManager {
    fn from_world(_world: &mut World) -> Self {
        let fetcher = Arc::new(BevyAssetFetcher::new());
        let store = build_asset_store(&fetcher, asset_cache_dir());
        Self {
            store,
            fetcher,
            inflight: HashMap::new(),
            fetched: HashMap::new(),
        }
    }
}

impl WearableAssetManager {
    /// Ensure `id` (of class `asset_type`) is being fetched. Idempotent.
    fn request(&mut self, id: AssetKey, asset_type: AssetType) {
        if id.uuid().is_nil() || self.fetched.contains_key(&id) || self.inflight.contains_key(&id) {
            return;
        }
        let store = self.store.clone();
        let task = IoTaskPool::get().spawn(async move {
            match store.get(id, asset_type).await {
                Ok(entry) => entry.data().map(|bytes| bytes.to_vec()),
                Err(_error) => None,
            }
        });
        let _prev = self.inflight.insert(id, task);
    }

    /// Point the store's fetcher at the region's current `ViewerAsset` URL.
    fn set_cap_url(&self, url: Option<String>) {
        self.fetcher.set_cap_url(url);
    }
}

/// Build an [`AssetStore`] over `fetcher`, disk-backed when the cache opens and
/// in-memory only otherwise (a cache failure must never wedge the viewer).
fn build_asset_store(fetcher: &Arc<BevyAssetFetcher>, disk_dir: Option<PathBuf>) -> AssetStore {
    let concrete = Arc::clone(fetcher);
    let fetcher: Arc<dyn BlobFetcher> = concrete;
    if let Some(dir) = disk_dir {
        match AssetStore::new(Arc::clone(&fetcher), Some(dir), AssetCacheLimits::default()) {
            Ok(store) => return store,
            Err(error) => warn!("asset disk cache unavailable ({error}); running in-memory only"),
        }
    }
    // The disk-less store cannot fail to open; the loop extracts it without an
    // `unwrap`/`expect` and runs exactly once.
    loop {
        match AssetStore::new(Arc::clone(&fetcher), None, AssetCacheLimits::default()) {
            Ok(store) => return store,
            Err(error) => warn!("in-memory asset store failed to open ({error}); retrying"),
        }
    }
}

/// The viewer's on-disk generic-asset cache directory, or `None` when neither
/// `XDG_CACHE_HOME` nor `HOME` is set (the store then runs in-memory only).
fn asset_cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(base.join("sl-client-bevy-viewer").join("assetcache"))
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
pub(crate) fn update_asset_caps(
    mut capabilities: MessageReader<SlCapabilities>,
    manager: Res<WearableAssetManager>,
) {
    for SlCapabilities(map) in capabilities.read() {
        manager.set_cap_url(map.get(CAP_VIEWER_ASSET).cloned());
    }
}

/// Drive the request half: once the region handshake completes, ask the sim for
/// our current outfit; when it replies, record the worn wearables and start
/// fetching their assets.
pub(crate) fn drive_wearable_requests(
    time: Res<Time>,
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<OwnBakeInputs>,
    mut manager: ResMut<WearableAssetManager>,
    mut writer: MessageWriter<SlCommand>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::RegionHandshakeComplete if state.stage == BakeInputStage::Idle => {
                writer.write(SlCommand(Command::RequestWearables));
                state.stage = BakeInputStage::AwaitingWearables;
                state.deadline = Some(time.elapsed_secs() + FETCH_GRACE_SECS);
                debug!("requested own wearables for client-side bake inputs");
            }
            SlSessionEvent::AgentWearables { wearables, .. }
                if state.stage == BakeInputStage::AwaitingWearables =>
            {
                start_asset_fetch(&mut state, &mut manager, wearables, time.elapsed_secs());
            }
            _other => {}
        }
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

/// Poll the in-flight wearable-asset fetches; move each completed download into
/// the manager's `fetched` map and announce it with [`WearableAssetFetched`]
/// (emitted on failure too, so the assembler stops waiting on it).
pub(crate) fn poll_wearable_assets(
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

/// Drive the assembly half: parse each fetched wearable asset and request its
/// layer textures; as those decode, once every asset and texture is resolved (or
/// the grace period lapses) assemble the per-region layer lists.
pub(crate) fn assemble_own_bake(
    time: Res<Time>,
    mut asset_events: MessageReader<WearableAssetFetched>,
    mut texture_events: MessageReader<TextureDecoded>,
    manager: Res<WearableAssetManager>,
    library: Option<Res<AvatarAssetLibrary>>,
    mut texture_manager: ResMut<TextureManager>,
    mut state: ResMut<OwnBakeInputs>,
) {
    // Parse newly fetched assets and request their layer textures.
    for &WearableAssetFetched(id) in asset_events.read() {
        if !state.pending_assets.remove(&id) {
            continue;
        }
        if let Some(bytes) = manager.fetched.get(&id) {
            parse_and_request_textures(&mut state, &mut texture_manager, bytes);
        }
    }
    // As layer textures decode, clear them from the pending set.
    for &TextureDecoded(id) in texture_events.read() {
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
            warn!(
                "assembling own bake inputs after grace period ({} asset(s), {} texture(s) still pending)",
                state.pending_assets.len(),
                state.pending_textures.len()
            );
        }
        assemble(&mut state, &texture_manager, library.as_deref());
        state.stage = BakeInputStage::Ready;
    }
}

/// Parse one fetched wearable asset's bytes and request each of its layer
/// textures through the shared texture pipeline (skipping ones already decoded).
fn parse_and_request_textures(
    state: &mut OwnBakeInputs,
    texture_manager: &mut TextureManager,
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
    // Request every layer texture this wearable supplies that a bake region uses.
    for &(slot, _name, _wearable) in &avatar_texture::LAYER_TEXTURES {
        if !asset.supplies_layer(slot) {
            continue;
        }
        if let Some(id) = asset.layer_texture(slot) {
            let key = TextureKey::from(id);
            if texture_manager.decoded(key).is_none() {
                state.pending_textures.insert(key);
                texture_manager.request(key);
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
    texture_manager: &TextureManager,
    library: Option<&AvatarAssetLibrary>,
) {
    let assets = state.assets.clone();
    let params = library.map(AvatarAssetLibrary::params);
    let mut layers = HashMap::new();
    let mut summary: Vec<String> = Vec::new();
    for region in BakeRegion::ALL {
        let region_layers = region_layers(
            region,
            |wearable| assets.iter().any(|asset| asset.wearable_type == wearable),
            |slot| layer_image(&assets, texture_manager, slot),
            |tint, wearable| layer_tint(&assets, params, tint, wearable),
        );
        summary.push(format!("{}={}", region.name(), region_layers.len()));
        let _prev = layers.insert(region, region_layers);
    }
    info!(
        "assembled own client-side bake inputs from {} wearable(s): {}",
        assets.len(),
        summary.join(" ")
    );
    state.layers = layers;
}

/// The decoded texture for a bake-region layer `slot`: the first worn wearable of
/// the slot's wearable type that supplies it, if its texture has decoded.
fn layer_image(
    assets: &[WearableAsset],
    texture_manager: &TextureManager,
    slot: usize,
) -> Option<DecodedTexture> {
    let wearable_type = avatar_texture::layer_wearable_type(slot)?;
    for asset in assets {
        if asset.wearable_type != wearable_type {
            continue;
        }
        if let Some(id) = asset.layer_texture(slot)
            && let Some(decoded) = texture_manager.decoded(TextureKey::from(id))
        {
            return Some((**decoded).clone());
        }
    }
    None
}

/// The linear-RGBA tint for a layer, resolved from the worn wearable's visual
/// params against the `avatar_lad.xml` table (opaque white when the table is
/// absent or the tint is [`LayerTint::White`]).
fn layer_tint(
    assets: &[WearableAsset],
    params: Option<&VisualParams>,
    tint: LayerTint,
    wearable: WearableType,
) -> [f32; 4] {
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let (LayerTint::Global(_) | LayerTint::Params(_)) = tint else {
        return WHITE;
    };
    let Some(params) = params else {
        return WHITE;
    };
    // The worn wearable of this layer's type supplies the colour param weights.
    let asset = assets.iter().find(|asset| asset.wearable_type == wearable);
    let weight_of = |id: i32| asset.and_then(|asset| asset.params.get(&id).copied());
    match tint {
        LayerTint::Global(name) => global_color(params, name, weight_of).unwrap_or(WHITE),
        LayerTint::Params(ids) => combine_layer_color(params, ids, weight_of),
        LayerTint::White => WHITE,
    }
}
