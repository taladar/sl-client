//! The shared texture pipeline: fetch, off-thread decode, and disk-cache every
//! texture the scene needs through the LOD-aware
//! [`TextureStore`], then hand the decoded RGBA8
//! to whichever consumer asked for it (prim faces here; terrain detail slots in
//! [`terrain`](crate::terrain)).
//!
//! Rather than decode JPEG-2000 on the render thread, the viewer drives the same
//! store the headless client uses: a [`BevyTextureFetcher`] pulls `GetTexture`
//! codestream bytes over blocking HTTP on Bevy's [`IoTaskPool`], the store
//! decodes them on its own `rayon` pool, keeps a Firestorm-compatible on-disk
//! cache (so a texture survives across runs), and dedupes concurrent requests for
//! the same texture. [`TextureManager`] owns that store; each texture is fetched
//! through a background [`Task`], and [`poll_textures`] folds a completed decode
//! into the shared cache and announces it with a [`TextureDecoded`] message that
//! every consumer (prims, terrain) reacts to independently.
//!
//! This is the Phase 6 slice — diffuse only. When [`objects`](crate::objects)
//! tessellates a prim it asks [`face_material`] for each face's material: the
//! face's decoded [`TextureFace`] gives the tint (`base_color`), the per-face
//! texture placement (repeat / offset / rotation, packed into the material's
//! `uv_transform` via [`texture_face_uv_transform`]), and the texture id; the
//! material is parked in [`PrimTextures`] until [`apply_prim_textures`] fills in
//! its `base_color_texture` once the texture decodes. A face with no texture (or
//! one that fails to fetch) keeps its flat tint. No normal / specular / PBR /
//! glow / bump — those are deferred (see the roadmap non-goals).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use sl_client_bevy::{
    BevyTextureFetcher, CAP_GET_TEXTURE, CacheLimits, DecodedTexture, DiscardLevel, GateStats,
    Priority, RemoteTextureSource, SlCapabilities, StoreStats, TextureFace, TextureFetcher,
    TextureKey, TextureRequest, TextureStore, Uuid, texture_face_uv_transform, to_bevy_image,
};

/// The GLTF material-override "no texture" sentinel (all-`f`, the reference
/// viewer's `LLGLTFMaterial::GLTF_OVERRIDE_NULL_UUID`): a face carrying it has no
/// diffuse texture to fetch, so it is treated exactly like the nil id rather than
/// endlessly re-requested (it is not a fetchable asset and 503s).
const GLTF_OVERRIDE_NULL_UUID: Uuid = Uuid::from_u128(u128::MAX);

/// Whether a face texture id denotes "no diffuse texture" — the nil id or the
/// GLTF override-null sentinel — so it should neither be fetched nor treated as a
/// textured face.
fn is_absent_texture(id: TextureKey) -> bool {
    let uuid = id.uuid();
    uuid.is_nil() || uuid == GLTF_OVERRIDE_NULL_UUID
}

/// The outcome of one background texture fetch: the decoded RGBA8 image, or
/// `None` if the texture could not be fetched or decoded.
type FetchResult = Option<Arc<DecodedTexture>>;

/// The discard level a pixel-area-managed texture (P21.1) is first requested at,
/// before its on-screen size is known: a coarse-but-quick placeholder (¼ linear
/// resolution, 1/16 the data) that loads fast, which the render-priority driver
/// then refines up or down once the first decode reveals the texture's native
/// size. This mirrors the reference viewer's progressive (coarse-first) texture
/// load — a distant texture stays coarse and is never upgraded, so only the
/// fidelity the view warrants is fetched.
const INITIAL_MANAGED_DISCARD: DiscardLevel = DiscardLevel::from_clamped(2);

/// The per-texture level-of-detail state of a pixel-area-managed texture (P21.1).
/// Its presence in [`TextureManager::managed`] marks the texture as LOD-managed
/// (an ordinary prim / mesh / sculpt diffuse face); boosted textures (terrain,
/// avatar bakes, worn attachments) are absent and stay at full resolution.
struct ManagedLod {
    /// The texture's full (discard-0) pixel dimensions, learned from the first
    /// decode; `None` until then (so no LOD can be selected yet).
    native: Option<(u32, u32)>,
    /// The discard level of the currently decoded image, `None` until the first
    /// decode. The render-priority driver compares its target against this to
    /// decide whether to upgrade, downgrade, or leave the texture unchanged.
    current: Option<DiscardLevel>,
}

/// Announced (once per texture id) when a background fetch finishes — whether it
/// decoded or failed. Every consumer that parked work on that texture reads this
/// and either applies the now-cached image or drops back to its fallback.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct TextureDecoded(pub(crate) TextureKey);

/// The shared texture fetch/decode/cache pipeline: one
/// [`TextureStore`] plus the in-flight background
/// fetch tasks and the decoded images already in hand.
///
/// A consumer calls [`request_boosted`](Self::request_boosted) to ensure a
/// texture is being fetched, then — once a [`TextureDecoded`] names it — reads
/// [`decoded`](Self::decoded) for the RGBA8 image to upload.
#[derive(Resource)]
pub(crate) struct TextureManager {
    /// The LOD-aware store doing the fetch, off-thread decode, dedupe, and
    /// on-disk caching.
    store: TextureStore,
    /// The store's HTTP fetcher, kept here so its `GetTexture` capability URL can
    /// be refreshed as the agent changes region.
    fetcher: Arc<BevyTextureFetcher>,
    /// The background fetch task per texture id, polled to completion by
    /// [`poll_textures`]; presence means "already being fetched".
    inflight: HashMap<TextureKey, Task<FetchResult>>,
    /// The re-prioritizable request handle per in-flight texture id (P20.2),
    /// paired with the request-time (base) priority it was issued at, so the
    /// render-priority driver can raise a texture the camera looks at (via
    /// [`set_priority`](Self::set_priority)) while it is still queued behind the
    /// store's admission gate — but never *demote* a boosted request (terrain, an
    /// avatar bake) below its base. Cleared alongside [`inflight`](Self::inflight)
    /// once the fetch resolves.
    requests: HashMap<TextureKey, (TextureRequest, Priority)>,
    /// Successfully decoded images by texture id, shared across all consumers so
    /// a texture is fetched and decoded once no matter how many faces use it.
    decoded: HashMap<TextureKey, Arc<DecodedTexture>>,
    /// Per-texture level-of-detail state for pixel-area-managed textures (P21.1),
    /// keyed by texture id. Presence marks a texture as LOD-managed; the
    /// render-priority driver upgrades / downgrades it toward the discard level
    /// its on-screen size warrants. The texture's initial [`TextureRequest`]
    /// handle is retained in [`requests`](Self::requests) (rather than dropped on
    /// resolve) for exactly these, so its store entry stays live for
    /// [`TextureStore::set_lod`].
    managed: HashMap<TextureKey, ManagedLod>,
    /// In-flight level-of-detail changes (P21.1), one per texture, kept separate
    /// from the initial [`inflight`](Self::inflight) fetch so a re-decode never
    /// blocks (or is mistaken for) the first fetch and at most one LOD change
    /// runs per texture at a time.
    lod_inflight: HashMap<TextureKey, Task<FetchResult>>,
    /// Default-source (`GetTexture`, by-UUID) requests made before the region's
    /// `GetTexture` capability was known, held here instead of failed. The terrain
    /// detail textures are requested the moment the composition is learned — during
    /// the region handshake, before the seed capabilities arrive — so a fetch
    /// issued then would fail for good ("GetTexture capability not available") and
    /// the ground would stay flat (R15). These are drained and issued for real by
    /// [`retry_pending_default`](Self::retry_pending_default) once the cap is set.
    pending_default: HashMap<TextureKey, PendingDefaultRequest>,
}

/// A default-source texture request deferred until the `GetTexture` capability is
/// available (see [`TextureManager::pending_default`]).
#[derive(Clone, Copy)]
struct PendingDefaultRequest {
    /// The request-time (base) priority the fetch will be admitted at.
    priority: Priority,
    /// The discard level (resolution) to fetch first.
    initial_lod: DiscardLevel,
    /// Whether the texture is pixel-area LOD managed (an ordinary face) rather than
    /// fetched at full resolution (a boosted consumer such as terrain).
    managed: bool,
}

impl FromWorld for TextureManager {
    /// Build the store over a fresh [`BevyTextureFetcher`], backed by the
    /// on-disk texture cache when a cache directory is available (falling back to
    /// an in-memory-only store if the cache cannot be opened).
    fn from_world(_world: &mut World) -> Self {
        let fetcher = Arc::new(BevyTextureFetcher::new());
        let disk_dir = texture_cache_dir();
        let store = build_store(&fetcher, disk_dir);
        Self {
            store,
            fetcher,
            inflight: HashMap::new(),
            requests: HashMap::new(),
            decoded: HashMap::new(),
            managed: HashMap::new(),
            lod_inflight: HashMap::new(),
            pending_default: HashMap::new(),
        }
    }
}

impl TextureManager {
    /// Ensure `id` is being fetched from the default `GetTexture` service at
    /// request-time (base) priority `priority`. A nil id (no texture) is ignored.
    ///
    /// An ordinary prim face passes [`Priority::IDLE`] and the render-priority
    /// driver ([`drive_render_priority`]) raises the texture each throttled frame
    /// from the on-screen pixel area of the faces using it (P20.2), so it starts
    /// idle and rises to what the camera warrants. A texture the driver does not
    /// (or cannot) rank from a scene object's pixel area — a terrain detail
    /// texture, an avatar texture, a worn attachment's face texture — passes a
    /// fixed boost instead (mirroring `LLGLTexture::BOOST_TERRAIN` /
    /// `BOOST_AVATAR`), which the driver never demotes below, so it is not starved
    /// behind nearer prims.
    ///
    /// Idempotent — many faces requesting the same texture trigger a single
    /// fetch, on top of the store's own single-flight dedupe.
    ///
    /// [`drive_render_priority`]: crate::render_priority::drive_render_priority
    pub(crate) fn request_boosted(&mut self, id: TextureKey, priority: Priority) {
        // Boosted textures (terrain, avatar layers, sculpt maps that drive
        // geometry) are fetched at full resolution and are *not* pixel-area LOD
        // managed (P21.1).
        self.request_from(
            id,
            RemoteTextureSource::Default,
            priority,
            DiscardLevel::FULL,
            false,
        );
    }

    /// Ensure an ordinary scene face's diffuse texture `id` is being fetched at
    /// request-time priority `priority`.
    ///
    /// An unboosted face is **pixel-area LOD managed** (P21.1): it is first
    /// requested at a coarse [placeholder level](INITIAL_MANAGED_DISCARD) and the
    /// render-priority driver then upgrades / downgrades it via
    /// [`set_lod_for_area`](Self::set_lod_for_area) to the discard level its
    /// on-screen size warrants, so a small / distant face fetches only a coarse
    /// image. A boosted face (an avatar's baked-on-mesh face, a worn attachment —
    /// whose skinned transform the face pass cannot rank) is instead fetched at
    /// full resolution, exactly like [`request_boosted`](Self::request_boosted).
    pub(crate) fn request_face(&mut self, id: TextureKey, priority: Priority) {
        if crate::render_priority::is_boost_priority(priority) {
            self.request_from(
                id,
                RemoteTextureSource::Default,
                priority,
                DiscardLevel::FULL,
                false,
            );
        } else {
            self.request_from(
                id,
                RemoteTextureSource::Default,
                priority,
                INITIAL_MANAGED_DISCARD,
                true,
            );
        }
    }

    /// Ensure a server-side ("Sunshine") avatar bake `id` is being fetched from the
    /// appearance service at `url` (`FTT_SERVER_BAKE`) — a baked id is not fetchable
    /// by UUID from the `GetTexture` CDN. The decoded bake is stored in the same
    /// [`TextureStore`] keyed by `id`, so every consumer reads it exactly like any
    /// other texture (P17.3 / P14). Boosted like any avatar texture (P20.2) so the
    /// bake loads promptly rather than queued behind nearer prims.
    pub(crate) fn request_server_bake(&mut self, id: TextureKey, url: String) {
        self.request_from(
            id,
            RemoteTextureSource::ServerBake { url },
            crate::render_priority::AVATAR_BOOST_PRIORITY,
            DiscardLevel::FULL,
            false,
        );
    }

    /// Spawn a background fetch of `id` from `source` at `priority` if it is not
    /// already decoded or in flight; the decode runs off-thread on the store's own
    /// pool. The fetch is admitted through the store's priority gate — the request
    /// handle is retained so [`set_priority`](Self::set_priority) can re-rank it
    /// while it waits (P20.2).
    fn request_from(
        &mut self,
        id: TextureKey,
        source: RemoteTextureSource,
        priority: Priority,
        initial_lod: DiscardLevel,
        managed: bool,
    ) {
        if is_absent_texture(id) {
            return;
        }
        // A boosted (full-resolution) consumer — an avatar body part, an
        // attachment, a HUD attachment, or a terrain detail texture — must never
        // leave this texture below full resolution. If an ordinary prim face had
        // already registered the *same* texture id for pixel-area LOD (a builder
        // reusing, say, a terrain texture on a prim), stop managing it and upgrade
        // it back to full resolution.
        if !managed && self.managed.remove(&id).is_some() {
            self.upgrade_to_full(id);
        }
        if self.decoded.contains_key(&id) || self.inflight.contains_key(&id) {
            return;
        }
        // A default (by-UUID `GetTexture`) fetch needs the region's `GetTexture`
        // capability. If it is not set yet — the terrain detail textures are
        // requested during the region handshake, before the seed caps arrive —
        // hold the request rather than spawn a fetch that would fail for good;
        // `retry_pending_default` issues it once the cap is up (R15). A server-bake
        // source carries its own URL and needs no such deferral.
        if matches!(source, RemoteTextureSource::Default) && !self.fetcher.has_default_cap() {
            self.pending_default.insert(
                id,
                PendingDefaultRequest {
                    priority,
                    initial_lod,
                    managed,
                },
            );
            return;
        }
        self.pending_default.remove(&id);
        let request = self.store.request(id, initial_lod, priority, source);
        let task_request = request.clone();
        let task = IoTaskPool::get().spawn(async move {
            // The blocking fetch runs on this IoTaskPool thread once the request is
            // admitted through the gate (in priority order); the decode is
            // dispatched onto the store's own CPU pool, so the render thread never
            // decodes.
            match task_request.resolved().await {
                Ok(entry) => entry.image(),
                Err(error) => {
                    warn!("texture {id} fetch/decode failed: {error}");
                    None
                }
            }
        });
        let _previous = self.requests.insert(id, (request, priority));
        if managed {
            // Register for pixel-area LOD management (P21.1); the retained
            // request handle keeps its store entry live for later `set_lod`.
            let _existing = self.managed.entry(id).or_insert(ManagedLod {
                native: None,
                current: None,
            });
        }
        self.inflight.insert(id, task);
    }

    /// Upgrade or downgrade a pixel-area-managed texture (P21.1) toward the
    /// discard level its on-screen `pixel_area` warrants, via
    /// [`TextureStore::set_lod`]. Called by the render-priority driver each
    /// throttled frame with the largest area any visible face using the texture
    /// covers.
    ///
    /// A no-op unless the texture is LOD-managed, has decoded at least once (so
    /// its native size — and hence the level a given area maps to — is known),
    /// the chosen level differs from the current one, and no LOD change for it is
    /// already running. The store's `set_lod` fetches + decodes on an upgrade and
    /// downsamples in place on a downgrade (waiting for any GPU read-lease to
    /// release before it frees the finer buffer). The completed image is folded
    /// in by [`poll_textures`], which re-uploads it in place.
    pub(crate) fn set_lod_for_area(&mut self, id: TextureKey, pixel_area: f32) {
        let Some(state) = self.managed.get(&id) else {
            return;
        };
        let (Some((width, height)), Some(current)) = (state.native, state.current) else {
            // Not decoded yet — the native size the level depends on is unknown.
            return;
        };
        let desired = DiscardLevel::for_pixel_area(pixel_area, width, height);
        if desired == current || self.lod_inflight.contains_key(&id) {
            return;
        }
        let Some((request, _base)) = self.requests.get(&id) else {
            // The retained handle is what keeps the entry live; without it we
            // cannot drive a LOD change.
            return;
        };
        let entry = request.entry();
        let store = self.store.clone();
        debug!(
            "texture {id} pixel-area LOD: discard {} -> {} (area {pixel_area:.0} px, native {width}x{height})",
            current.get(),
            desired.get(),
        );
        let task = IoTaskPool::get().spawn(async move {
            if let Err(error) = store.set_lod(&entry, desired).await {
                warn!(
                    "texture {id} LOD change to discard {} failed: {error}",
                    desired.get()
                );
            }
            entry.image()
        });
        let _previous = self.lod_inflight.insert(id, task);
    }

    /// Upgrade a (now un-managed) texture back to full resolution and keep it
    /// there — used when a boosted consumer claims a texture a prim face had been
    /// pixel-area LOD managing (see [`request_from`](Self::request_from)). A
    /// no-op if the texture is already at full resolution or its retained request
    /// handle is gone.
    fn upgrade_to_full(&mut self, id: TextureKey) {
        let Some((request, _base)) = self.requests.get(&id) else {
            return;
        };
        let entry = request.entry();
        if entry.current_discard() == Some(DiscardLevel::FULL) {
            return;
        }
        let store = self.store.clone();
        let task = IoTaskPool::get().spawn(async move {
            if let Err(error) = store.set_lod(&entry, DiscardLevel::FULL).await {
                warn!("texture {id} upgrade to full resolution failed: {error}");
            }
            entry.image()
        });
        // Supersedes any coarser LOD change still queued for this texture.
        let _previous = self.lod_inflight.insert(id, task);
    }

    /// Record a freshly decoded image as the current one for `id`: update the
    /// shared decoded cache and, for a pixel-area-managed texture, its learned
    /// native (discard-0) size and current level (P21.1). The native size is the
    /// decoded size scaled back up by the discard level (each level halves both
    /// dimensions).
    fn record_decoded(&mut self, id: TextureKey, image: Arc<DecodedTexture>) {
        if let Some(state) = self.managed.get_mut(&id) {
            let scale = u32::from(image.discard_level.get());
            state.native = Some((
                image.width.checked_shl(scale).unwrap_or(image.width),
                image.height.checked_shl(scale).unwrap_or(image.height),
            ));
            state.current = Some(image.discard_level);
        }
        let _previous = self.decoded.insert(id, image);
    }

    /// Re-rank an in-flight texture request from the on-screen pixel area the
    /// driver computed (P20.2), clamped to never fall below the request-time base
    /// priority — so the per-frame face pass can raise an unboosted prim texture
    /// the camera turns toward, but cannot demote a boosted terrain / avatar
    /// request that the face pass does not (and should not) rank. A no-op for a
    /// texture already decoded, never requested, or whose fetch already finished
    /// (its handle is dropped once it resolves).
    pub(crate) fn set_priority(&self, id: TextureKey, priority: Priority) {
        if let Some((request, base)) = self.requests.get(&id) {
            request.set_priority(Priority::combine(priority, *base));
        }
    }

    /// The decoded image for `id`, once it has been fetched, or `None` if it is
    /// still in flight or the fetch failed.
    pub(crate) fn decoded(&self, id: TextureKey) -> Option<&Arc<DecodedTexture>> {
        self.decoded.get(&id)
    }

    /// Point the store's fetcher at the region's current `GetTexture` capability
    /// URL (or clear it when absent).
    fn set_cap_url(&self, url: Option<String>) {
        self.fetcher.set_cap_url(url);
    }

    /// Issue any default-source requests that were made before the `GetTexture`
    /// capability was known (see [`pending_default`](Self::pending_default)), now
    /// that it is. A no-op while the cap is still unset (nothing to fetch against)
    /// or when nothing is pending. Call this whenever the cap is (re)set.
    pub(crate) fn retry_pending_default(&mut self) {
        if self.pending_default.is_empty() || !self.fetcher.has_default_cap() {
            return;
        }
        // Drain first, then re-issue: `request_from` removes each id from
        // `pending_default` and spawns its fetch now the cap resolves.
        let pending: Vec<(TextureKey, PendingDefaultRequest)> =
            self.pending_default.drain().collect();
        for (id, request) in pending {
            self.request_from(
                id,
                RemoteTextureSource::Default,
                request.priority,
                request.initial_lod,
                request.managed,
            );
        }
    }

    /// A point-in-time snapshot of the texture fetch/decode pipeline (P19.2),
    /// for the diagnostics overlay: entry counts bucketed by stage plus the
    /// cumulative disk-cache-hit / GC counters.
    pub(crate) fn stats(&self) -> StoreStats {
        self.store.stats()
    }

    /// A point-in-time snapshot of the texture store's admission gate (P19.2):
    /// its concurrency capacity, in-flight slots, and queued waiters.
    pub(crate) fn gate_stats(&self) -> GateStats {
        self.store.gate_stats()
    }
}

/// Build a [`TextureStore`] over `fetcher`, backed
/// by the on-disk cache at `disk_dir` when it can be opened, and otherwise
/// in-memory only (a disk-cache failure must never keep the viewer from
/// rendering).
fn build_store(fetcher: &Arc<BevyTextureFetcher>, disk_dir: Option<PathBuf>) -> TextureStore {
    // Coerce the concrete fetcher to the trait object the store stores (a move
    // through a typed binding, since `Arc::clone`'s inferred `T` would otherwise
    // demand the argument already be the trait object). The concrete `Arc` is
    // kept in the manager for `set_cap_url`.
    let concrete = Arc::clone(fetcher);
    let fetcher: Arc<dyn TextureFetcher> = concrete;
    if let Some(dir) = disk_dir {
        match TextureStore::new(Arc::clone(&fetcher), Some(dir), CacheLimits::default()) {
            Ok(store) => return store,
            Err(error) => warn!("texture disk cache unavailable ({error}); running in-memory only"),
        }
    }
    // The disk-less store opens no files and so cannot fail; the loop extracts it
    // without an `unwrap`/`expect` (which the lints forbid) and runs exactly once.
    loop {
        match TextureStore::new(Arc::clone(&fetcher), None, CacheLimits::default()) {
            Ok(store) => return store,
            Err(error) => warn!("in-memory texture store failed to open ({error}); retrying"),
        }
    }
}

/// The viewer's on-disk texture cache directory (`<cache>/sl-client-bevy-viewer/
/// texturecache`), from `XDG_CACHE_HOME` or `~/.cache`, or `None` when neither is
/// set (the store then runs in-memory only).
fn texture_cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(base.join("sl-client-bevy-viewer").join("texturecache"))
}

/// Refresh the store fetcher's `GetTexture` capability URL each time the region's
/// capability map is (re)discovered, then issue any default-source requests that
/// were parked while the cap was still unknown (the terrain detail textures,
/// requested during the handshake before the seed caps arrived — R15).
pub(crate) fn update_texture_caps(
    mut capabilities: MessageReader<SlCapabilities>,
    mut manager: ResMut<TextureManager>,
) {
    for SlCapabilities(map) in capabilities.read() {
        manager.set_cap_url(map.get(CAP_GET_TEXTURE).cloned());
    }
    manager.retry_pending_default();
}

/// Poll the in-flight fetch tasks; move each completed decode into the shared
/// cache and announce it with a [`TextureDecoded`] message (emitted on failure
/// too, so parked consumers can release their fallback state).
pub(crate) fn poll_textures(
    mut manager: ResMut<TextureManager>,
    mut decoded: MessageWriter<TextureDecoded>,
) {
    // Collect the ids whose task has finished, then apply — the borrow of the
    // task map cannot overlap the mutation of the decoded map.
    let mut finished: Vec<(TextureKey, FetchResult)> = Vec::new();
    for (&id, task) in &mut manager.inflight {
        if let Some(result) = block_on(poll_once(task)) {
            finished.push((id, result));
        }
    }
    for (id, result) in finished {
        let _removed = manager.inflight.remove(&id);
        // Drop the schedulable request handle now the initial fetch is done — the
        // decoded pixels live in `decoded`, independent of the store entry (P20.2)
        // — *unless* the texture is pixel-area LOD managed (P21.1), where the
        // retained handle keeps its store entry live for later `set_lod`.
        if !manager.managed.contains_key(&id) {
            let _request = manager.requests.remove(&id);
        }
        if let Some(image) = result {
            manager.record_decoded(id, image);
        }
        decoded.write(TextureDecoded(id));
    }

    // Fold in completed level-of-detail changes (P21.1): the store entry now
    // holds the finer / coarser image, so refresh the shared decoded cache and
    // re-announce the texture so `apply_prim_textures` re-uploads it in place.
    let mut lod_finished: Vec<(TextureKey, FetchResult)> = Vec::new();
    for (&id, task) in &mut manager.lod_inflight {
        if let Some(result) = block_on(poll_once(task)) {
            lod_finished.push((id, result));
        }
    }
    for (id, result) in lod_finished {
        let _removed = manager.lod_inflight.remove(&id);
        if let Some(image) = result {
            manager.record_decoded(id, image);
            decoded.write(TextureDecoded(id));
        }
    }
}

/// Prim-face texturing bookkeeping: the Bevy images already uploaded for prim
/// faces (deduped by texture id, sampled with a repeating address mode so tiled
/// faces wrap) and the face materials waiting on a texture that has not decoded
/// yet.
#[derive(Resource, Default)]
pub(crate) struct PrimTextures {
    /// Uploaded diffuse images by texture id, so a texture shared by many faces
    /// is turned into a Bevy [`Image`] once.
    images: HashMap<TextureKey, Handle<Image>>,
    /// Face materials parked on a texture id, patched with the diffuse image (or
    /// released to their flat tint) once the fetch resolves.
    pending: HashMap<TextureKey, Vec<Handle<StandardMaterial>>>,
}

/// Build the diffuse [`StandardMaterial`] for one prim face: `base_color` is the
/// face tint (opaque white = untinted), and `base_color_texture` is filled in
/// immediately when the face's texture is already uploaded, otherwise the
/// material is parked in `prim_textures` and its texture requested through
/// `manager` (which dedupes) so [`apply_prim_textures`] can fill it in later.
///
/// A face with no texture (nil id) keeps just its flat tint.
pub(crate) fn face_material(
    face: &TextureFace,
    materials: &mut Assets<StandardMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
) -> Handle<StandardMaterial> {
    let texture_id = face.texture_id;
    let has_texture = !is_absent_texture(texture_id);
    let mut material = StandardMaterial {
        base_color: tint_color(face.color),
        perceptual_roughness: 0.9,
        // The per-face `TextureEntry` placement: texture repeats (`scale_s` /
        // `scale_t`), offset, and rotation, packed into the material's UV
        // transform exactly as the reference viewer's `xform` maps the face's
        // texture coordinates (about the face centre). Identity faces get the
        // identity transform, so an un-repeated texture is unaffected.
        uv_transform: texture_face_uv_transform(face),
        // Transparency (R5): a face whose tint colour is non-opaque blends now; a
        // face whose *texture* carries an alpha channel is upgraded to blending
        // once the texture decodes (in [`apply_prim_textures`]). Without this a
        // transparent surface — an invisible prim, a glass pane, a sky-platform
        // floor — renders as a solid wall (the Second Life world is full of them,
        // so the viewer otherwise fills with opaque region-sized panels).
        alpha_mode: face_alpha_mode(face.color),
        // Single-sided (default back-face culling): Second Life renders a face
        // only from its front, so a one-sided surface (a flat mesh quad, a prim
        // cut face) is invisible from behind rather than doubled. The tessellated
        // prim faces and decoded mesh submeshes carry outward-facing windings.
        ..default()
    };
    if has_texture && let Some(image) = prim_textures.images.get(&texture_id) {
        material.base_color_texture = Some(image.clone());
    }
    let handle = materials.add(material);
    // A textured face whose image is not uploaded yet: park the material and ask
    // the pipeline for the texture (idempotent across faces).
    if has_texture && !prim_textures.images.contains_key(&texture_id) {
        prim_textures
            .pending
            .entry(texture_id)
            .or_default()
            .push(handle.clone());
        manager.request_face(texture_id, priority);
    }
    handle
}

/// Fill each newly decoded prim texture into the faces parked on it: upload (and
/// cache) its diffuse [`Image`], then drop it into every parked material's
/// `base_color_texture`. A decode that failed drops the parked materials so they
/// keep their flat tint.
pub(crate) fn apply_prim_textures(
    mut decoded: MessageReader<TextureDecoded>,
    manager: Res<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for &TextureDecoded(id) in decoded.read() {
        // Level-of-detail re-decode (P21.1): a texture already uploaded to the GPU
        // whose store entry the driver upgraded / downgraded. Refresh the Bevy
        // image *behind its existing handle*, so every material sampling it shows
        // the new resolution without any material re-patching.
        if let Some(handle) = prim_textures.images.get(&id).cloned() {
            if let Some(image) = manager.decoded(id) {
                let refreshed = build_prim_image(image);
                let _replaced = images.insert(&handle, refreshed);
            }
            continue;
        }
        let Some(parked) = prim_textures.pending.remove(&id) else {
            // Not a texture any prim face is waiting on (e.g. a terrain texture).
            continue;
        };
        // Whether the decoded texture carries alpha, so a face showing it must
        // blend (R5) — read before `prim_image` borrows `prim_textures` mutably.
        let has_alpha = manager
            .decoded(id)
            .is_some_and(|decoded| texture_has_alpha(decoded));
        let Some(image_handle) = prim_image(&manager, &mut prim_textures, &mut images, id) else {
            // The fetch failed: the parked faces keep their flat tint.
            continue;
        };
        for material_handle in parked {
            if let Some(mut material) = materials.get_mut(&material_handle) {
                material.base_color_texture = Some(image_handle.clone());
                // Upgrade an opaque face to blending when its texture has alpha; a
                // face already blending (a non-opaque tint) stays blending.
                if has_alpha && material.alpha_mode == AlphaMode::Opaque {
                    material.alpha_mode = AlphaMode::Blend;
                }
            }
        }
    }
}

/// The uploaded diffuse [`Image`] for `id`, uploading and caching it from the
/// manager's decoded pixels on first use, or `None` if the texture is not
/// decoded (the fetch failed).
fn prim_image(
    manager: &TextureManager,
    prim_textures: &mut PrimTextures,
    images: &mut Assets<Image>,
    id: TextureKey,
) -> Option<Handle<Image>> {
    if let Some(handle) = prim_textures.images.get(&id) {
        return Some(handle.clone());
    }
    let decoded = manager.decoded(id)?;
    let handle = images.add(build_prim_image(decoded));
    let _inserted = prim_textures.images.insert(id, handle.clone());
    Some(handle)
}

/// Build the Bevy [`Image`] for a prim/mesh/sculpt face's decoded diffuse
/// texture, with the repeating address mode Second Life object faces need.
///
/// Second Life object faces tile their texture (the per-face `scale_s` /
/// `scale_t` repeats push the UVs outside `[0, 1]`), and the reference viewer
/// samples them with a wrapping address mode. Bevy's default sampler is
/// clamp-to-edge, which — on a face with repeats above one — smears the edge
/// texel across every out-of-range tile instead of repeating it (a texture
/// "coherent in the centre, streaked toward the edges"). Sample prim/mesh face
/// textures with a repeating sampler so tiled faces render as the reference
/// viewer does. Shared by the first upload and a level-of-detail re-upload
/// (P21.1).
fn build_prim_image(decoded: &Arc<DecodedTexture>) -> Image {
    let mut image = to_bevy_image(decoded);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// The alpha mode a face's tint colour alone implies: [`AlphaMode::Blend`] when
/// the tint is non-opaque (its alpha byte below `255`), else [`AlphaMode::Opaque`].
///
/// This is the colour-only half of a face's transparency; the texture half — a
/// diffuse texture with its own alpha channel — is folded in by
/// [`apply_prim_textures`] once the texture decodes (it can only *upgrade* an
/// opaque face to blending, never the reverse). It mirrors the reference viewer's
/// legacy default (a face is alpha-blended when its colour or texture carries
/// alpha), short of the per-face `DiffuseAlphaMode` mask/emissive variants, which
/// are deferred.
const fn face_alpha_mode(color: [u8; 4]) -> AlphaMode {
    if color[3] < 255 {
        AlphaMode::Blend
    } else {
        AlphaMode::Opaque
    }
}

/// Whether a decoded texture carries an alpha channel (a grey+alpha, RGBA, or
/// Second Life 5-component bake codestream — `2` or `4`+ source components), so a
/// face showing it must blend.
const fn texture_has_alpha(decoded: &DecodedTexture) -> bool {
    decoded.components == 2 || decoded.components >= 4
}

/// Convert a face tint (RGBA bytes, `[255; 4]` = opaque white = no tint) into a
/// Bevy sRGB [`Color`] to multiply the diffuse texture by.
fn tint_color(color: [u8; 4]) -> Color {
    Color::srgba(
        f32::from(color[0]) / 255.0,
        f32::from(color[1]) / 255.0,
        f32::from(color[2]) / 255.0,
        f32::from(color[3]) / 255.0,
    )
}

#[cfg(test)]
mod tests {
    use super::{face_alpha_mode, texture_has_alpha};
    use bevy::prelude::AlphaMode;
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{DecodedTexture, DiscardLevel};

    /// A decoded texture with the given source component count (pixels unused by
    /// the alpha test, so a single RGBA8 texel stands in).
    fn decoded(components: u16) -> DecodedTexture {
        DecodedTexture {
            width: 1,
            height: 1,
            components,
            discard_level: DiscardLevel::FULL,
            pixels: Bytes::from(vec![0xFF_u8; 4]),
            aux: None,
        }
    }

    #[test]
    fn opaque_tint_stays_opaque_transparent_tint_blends() {
        assert_eq!(face_alpha_mode([255; 4]), AlphaMode::Opaque);
        // Any sub-255 alpha byte forces blending.
        assert_eq!(face_alpha_mode([255, 255, 255, 254]), AlphaMode::Blend);
        assert_eq!(face_alpha_mode([10, 20, 30, 0]), AlphaMode::Blend);
    }

    #[test]
    fn only_alpha_bearing_component_counts_have_alpha() {
        // Grey (1) and RGB (3) have no alpha; grey+alpha (2) and RGBA (4) do.
        assert!(!texture_has_alpha(&decoded(1)));
        assert!(texture_has_alpha(&decoded(2)));
        assert!(!texture_has_alpha(&decoded(3)));
        assert!(texture_has_alpha(&decoded(4)));
    }
}
