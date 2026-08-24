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
//! tessellates a prim it asks `face_material` for each face's material: the
//! face's decoded [`TextureFace`] gives the tint (`base_color`), the per-face
//! texture placement (repeat / offset / rotation, packed into the material's
//! `uv_transform` via [`texture_face_uv_transform`]), and the texture id; the
//! material is parked in [`PrimTextures`] until [`apply_prim_textures`] fills in
//! its `base_color_texture` once the texture decodes. A face with no texture (or
//! one that fails to fetch) keeps its flat tint. No normal / specular / PBR /
//! glow / bump — those are deferred (see the roadmap non-goals).

use std::collections::{HashMap, HashSet, VecDeque};
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

use crate::asset_retry::RetryState;
use crate::face_material::{FaceMaterial, inert_face_material};
use crate::material_cache::{MaterialCache, MaterialKey};

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
#[derive(Debug)]
struct ManagedLod {
    /// The texture's full (discard-0) pixel dimensions, learned from the first
    /// decode; `None` until then (so no LOD can be selected yet).
    native: Option<(u32, u32)>,
    /// The discard level of the currently decoded image, `None` until the first
    /// decode. The render-priority driver compares its target against this to
    /// decide whether to upgrade, downgrade, or leave the texture unchanged.
    current: Option<DiscardLevel>,
}

/// A snapshot of a texture's level-of-detail state for the crosshair pick tool
/// (P21.1 diagnostics), returned by [`TextureManager::lod_debug`].
pub(crate) struct TextureLodDebug {
    /// The discard level of the currently decoded / uploaded image (`0` = full
    /// resolution). This is what should fall as the camera approaches.
    pub(crate) discard: DiscardLevel,
    /// The width of the currently decoded image, in texels.
    pub(crate) width: u32,
    /// The height of the currently decoded image, in texels.
    pub(crate) height: u32,
    /// The learned native (discard-0) size, if the texture is LOD-managed and has
    /// decoded at least once; `None` for an unmanaged (boosted) texture. This is
    /// the manager's `decoded_size << discard_level` back-calculation.
    pub(crate) native: Option<(u32, u32)>,
    /// The texture's *true* native size from the parsed J2C codestream header, the
    /// authoritative full-resolution dimensions. If this exceeds
    /// [`native`](Self::native), the back-calculation under-counted (a partial /
    /// non-power-of-two decode) and the texture is wrongly capped coarse.
    pub(crate) header_native: Option<(u32, u32)>,
    /// Whether the texture is pixel-area LOD managed (an ordinary face) rather
    /// than fetched at full resolution (a boosted avatar / terrain texture).
    pub(crate) managed: bool,
}

/// Announced (once per texture id) when a background fetch finishes — whether it
/// decoded or failed. Every consumer that parked work on that texture reads this
/// and either applies the now-cached image or drops back to its fallback.
#[derive(Message, Debug, Clone, Copy)]
pub struct TextureDecoded(pub TextureKey);

/// The shared texture fetch/decode/cache pipeline: one
/// [`TextureStore`] plus the in-flight background
/// fetch tasks and the decoded images already in hand.
///
/// A consumer calls [`request_boosted`](Self::request_boosted) to ensure a
/// texture is being fetched, then — once a [`TextureDecoded`] names it — reads
/// [`decoded`](Self::decoded) for the RGBA8 image to upload.
#[derive(Debug, Resource)]
pub struct TextureManager {
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
    /// How to re-issue each in-flight fetch, kept so a failed one can be retried
    /// with the same source / priority / LOD / managed flag. Removed once the fetch
    /// resolves (moved into [`retry`](Self::retry) on failure).
    in_flight_params: HashMap<TextureKey, DeferredRequest>,
    /// Fetches that failed and are waiting to be re-issued (bounded backoff —
    /// [`asset_retry`](crate::asset_retry)). Without this a transient `GetTexture`
    /// failure would strand a one-shot boosted consumer's texture (terrain, an
    /// avatar bake) for the whole session, invisible to the F3 overlay. Drained by
    /// [`poll_textures`] once each entry is due.
    retry: HashMap<TextureKey, (DeferredRequest, RetryState)>,
    /// The blacklisted **texture** asset ids
    /// (`viewer-derender-blacklist`), mirrored from the derender list by
    /// [`sync_texture_blacklist`] whenever it changes. A blacklisted id is never
    /// fetched, so a face using it stays untextured — the reference refuses at
    /// the same chokepoint (`LLTextureFetch::createRequest`). Mirrored rather
    /// than consulted directly because the gate is inside
    /// [`request_from`](Self::request_from), which is called from deep inside
    /// non-system code that has no access to a Bevy resource.
    blacklist: HashSet<Uuid>,
    /// The derender-list revision [`blacklist`](Self::blacklist) was mirrored at,
    /// so the copy happens only when the list actually moves.
    blacklist_revision: u64,
}

/// A default-source texture request deferred until the `GetTexture` capability is
/// available (see [`TextureManager::pending_default`]).
#[derive(Debug, Clone, Copy)]
struct PendingDefaultRequest {
    /// The request-time (base) priority the fetch will be admitted at.
    priority: Priority,
    /// The discard level (resolution) to fetch first.
    initial_lod: DiscardLevel,
    /// Whether the texture is pixel-area LOD managed (an ordinary face) rather than
    /// fetched at full resolution (a boosted consumer such as terrain).
    managed: bool,
}

/// Everything `TextureManager::request_from` needs to (re-)issue a fetch: enough
/// to retry one that failed transiently without the original caller re-asking.
#[derive(Debug, Clone)]
struct DeferredRequest {
    /// Where the texture is fetched from (the default CDN, or a bake's URL).
    source: RemoteTextureSource,
    /// The request-time (base) priority.
    priority: Priority,
    /// The discard level (resolution) to fetch first.
    initial_lod: DiscardLevel,
    /// Whether the texture is pixel-area LOD managed rather than boosted full-res.
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
            in_flight_params: HashMap::new(),
            retry: HashMap::new(),
            blacklist: HashSet::new(),
            blacklist_revision: 0,
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
    pub fn request_boosted(&mut self, id: TextureKey, priority: Priority) {
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
    /// `set_lod_for_area` to the discard level its
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
            crate::world_api::AVATAR_BOOST_PRIORITY,
            DiscardLevel::FULL,
            false,
        );
    }

    /// Evict `id` so the next request re-fetches it from scratch — the manual
    /// **Tex Refresh** path (avatar bakes). Drops the decoded image, the retained
    /// request handle (releasing the store's weak cache entry so the fetch is not
    /// short-circuited by the cache), and any in-flight / retry / pending
    /// bookkeeping. The material currently displaying the texture keeps its own
    /// Bevy `Handle`, so the avatar does not blank; the image is replaced when the
    /// fresh fetch re-decodes.
    pub(crate) fn forget(&mut self, id: TextureKey) {
        let _decoded = self.decoded.remove(&id);
        let _request = self.requests.remove(&id);
        let _inflight = self.inflight.remove(&id);
        let _lod = self.lod_inflight.remove(&id);
        let _managed = self.managed.remove(&id);
        let _pending = self.pending_default.remove(&id);
        let _params = self.in_flight_params.remove(&id);
        let _retry = self.retry.remove(&id);
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
        if is_absent_texture(id) || self.blacklist.contains(&id.uuid()) {
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
        // Record how to re-issue this fetch so a transient failure can be retried
        // (`poll_textures`) with the same source / priority / LOD; a fresh explicit
        // request supersedes any pending retry for the id.
        let _retried = self.retry.remove(&id);
        let _prev_params = self.in_flight_params.insert(
            id,
            DeferredRequest {
                source: source.clone(),
                priority,
                initial_lod,
                managed,
            },
        );
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
    /// native (discard-0) size and current level (P21.1).
    ///
    /// The native size is read from the store entry's parsed J2C header — the
    /// authoritative full-resolution dimensions — falling back to the
    /// decoded-size-scaled-up-by-discard-level back-calculation only when the
    /// header is not yet available. The back-calculation is unreliable if a decode
    /// was partial (a truncated resolution-progressive codestream decodes to a
    /// smaller image than its discard level implies), which would cap the texture
    /// coarse; the header dimensions never lie.
    fn record_decoded(&mut self, id: TextureKey, image: Arc<DecodedTexture>) {
        let header_native = self
            .requests
            .get(&id)
            .and_then(|(request, _base)| request.entry().native_dimensions());
        if let Some(state) = self.managed.get_mut(&id) {
            state.native = Some(header_native.unwrap_or_else(|| {
                let scale = u32::from(image.discard_level.get());
                (
                    image.width.checked_shl(scale).unwrap_or(image.width),
                    image.height.checked_shl(scale).unwrap_or(image.height),
                )
            }));
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
    #[must_use]
    pub fn decoded(&self, id: TextureKey) -> Option<&Arc<DecodedTexture>> {
        self.decoded.get(&id)
    }

    /// Classify a diffuse texture for a consumer that paints it onto a material
    /// directly (the build tool's live texture preview, `crate::edit_texture`),
    /// uploading a fresh Bevy image when ready. Distinguishes a genuinely
    /// **absent** texture (a nil / null-sentinel id — the material should clear
    /// its `base_color_texture` and show the flat tint) from one that simply has
    /// not **decoded** yet (keep the old image and wait), which a bare
    /// `Option<Handle>` would conflate. Reads the decode store the preview pane
    /// uses, so a freshly-picked texture no face yet carries still resolves.
    pub fn diffuse_image(&self, id: TextureKey, images: &mut Assets<Image>) -> DiffuseImage {
        if is_absent_texture(id) {
            DiffuseImage::Absent
        } else if let Some(decoded) = self.decoded.get(&id) {
            DiffuseImage::Ready(images.add(build_prim_image(decoded)))
        } else {
            DiffuseImage::Pending
        }
    }

    /// A snapshot of a texture's level-of-detail state for the crosshair pick tool
    /// (P21.1 diagnostics): the currently decoded discard level and pixel
    /// dimensions, its learned native (discard-0) size (only for a LOD-managed
    /// texture), and whether it is pixel-area LOD managed at all. `None` if the
    /// texture has not decoded yet. Aim at a face and press the pick key while
    /// walking toward it to watch the discard level fall (finer) as it should.
    pub(crate) fn lod_debug(&self, id: TextureKey) -> Option<TextureLodDebug> {
        let image = self.decoded.get(&id)?;
        // The true native size lives on the store entry's parsed J2C header; the
        // retained request handle (kept for managed textures) is the way to it.
        let header_native = self
            .requests
            .get(&id)
            .and_then(|(request, _base)| request.entry().native_dimensions());
        Some(TextureLodDebug {
            discard: image.discard_level,
            width: image.width,
            height: image.height,
            native: self.managed.get(&id).and_then(|state| state.native),
            header_native,
            managed: self.managed.contains_key(&id),
        })
    }

    /// A texture's **full-resolution** (discard-0) pixel dimensions once it has
    /// decoded, or `None` while it has not. The cost models want the asset's real
    /// size, not the level this viewer happens to be showing: the reference's
    /// render-complexity charge per texture is `256 + 16·(h/128 + w/128)` over
    /// `getFullHeight` / `getFullWidth` ([`crate::avatar_complexity`]).
    ///
    /// Read from the store entry's parsed J2C header where available (the
    /// authoritative size), falling back to the decoded-size-scaled-by-discard
    /// back-calculation — the same order [`record_decoded`](Self::record_decoded)
    /// uses, and the only route for a boosted texture, which retains no request
    /// handle to reach the header through.
    pub(crate) fn native_dimensions(&self, id: TextureKey) -> Option<(u32, u32)> {
        if let Some(native) = self
            .requests
            .get(&id)
            .and_then(|(request, _base)| request.entry().native_dimensions())
        {
            return Some(native);
        }
        let image = self.decoded.get(&id)?;
        let scale = u32::from(image.discard_level.get());
        Some((
            image.width.checked_shl(scale).unwrap_or(image.width),
            image.height.checked_shl(scale).unwrap_or(image.height),
        ))
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

    /// How many fetches are parked outside the store's own accounting — held for a
    /// capability that is not up yet, or waiting out a post-failure retry backoff.
    /// The store keeps only weak references, so a failed / not-yet-issued fetch is
    /// invisible to [`stats`](Self::stats); the pipeline overlay adds this so
    /// "nothing left to load" does not lie while such work is still outstanding.
    pub(crate) fn deferred_count(&self) -> usize {
        self.pending_default.len().saturating_add(self.retry.len())
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
        match TextureStore::new(
            Arc::clone(&fetcher),
            Some(dir),
            CacheLimits {
                max_bytes: crate::paths::texture_cache_max_bytes(),
                ..CacheLimits::default()
            },
        ) {
            Ok(store) => return store,
            Err(error) => warn!("texture disk cache unavailable ({error}); running in-memory only"),
        }
    }
    // The disk-less store opens no files and so cannot fail; the loop extracts it
    // without an `unwrap`/`expect` (which the lints forbid) and runs exactly once.
    loop {
        match TextureStore::new(
            Arc::clone(&fetcher),
            None,
            CacheLimits {
                max_bytes: crate::paths::texture_cache_max_bytes(),
                ..CacheLimits::default()
            },
        ) {
            Ok(store) => return store,
            Err(error) => warn!("in-memory texture store failed to open ({error}); retrying"),
        }
    }
}

/// The viewer's on-disk texture cache directory (`<cache>/sl-client-bevy-viewer/
/// texturecache`), from `XDG_CACHE_HOME` or `~/.cache`, or `None` when neither is
/// set (the store then runs in-memory only).
fn texture_cache_dir() -> Option<PathBuf> {
    crate::paths::asset_cache_dir("texturecache")
}

/// Mirror the blacklisted **texture** ids into the manager whenever the derender
/// list changes (`viewer-derender-blacklist`), so
/// `TextureManager::request_from` can refuse a fetch without reaching for a
/// Bevy resource. Cheap: a revision compare per frame, a rebuild only on a real
/// change.
pub fn sync_texture_blacklist(
    derender: Res<crate::world_api::DerenderList>,
    mut manager: ResMut<TextureManager>,
) {
    if manager.blacklist_revision == derender.revision() {
        return;
    }
    manager.blacklist_revision = derender.revision();
    manager.blacklist = derender.ids_of_kind(crate::world_api::DerenderKind::Texture);
}

/// Refresh the store fetcher's `GetTexture` capability URL each time the region's
/// capability map is (re)discovered, then issue any default-source requests that
/// were parked while the cap was still unknown (the terrain detail textures,
/// requested during the handshake before the seed caps arrived — R15).
pub fn update_texture_caps(
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
pub fn poll_textures(
    time: Res<Time>,
    mut manager: ResMut<TextureManager>,
    mut decoded: MessageWriter<TextureDecoded>,
) {
    let now = time.elapsed_secs_f64();
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
        let params = manager.in_flight_params.remove(&id);
        // Drop the schedulable request handle now the initial fetch is done — the
        // decoded pixels live in `decoded`, independent of the store entry (P20.2)
        // — *unless* the texture is pixel-area LOD managed (P21.1), where the
        // retained handle keeps its store entry live for later `set_lod`.
        if !manager.managed.contains_key(&id) {
            let _request = manager.requests.remove(&id);
        }
        match result {
            Some(image) => {
                let _cleared = manager.retry.remove(&id);
                manager.record_decoded(id, image);
                decoded.write(TextureDecoded(id));
            }
            None => {
                // The fetch or decode failed. Rather than give up — which strands a
                // one-shot consumer's texture (terrain, an avatar bake) for the whole
                // session while the F3 overlay shows nothing left to load — schedule a
                // bounded backoff retry with the same parameters. Only announce the
                // failure (releasing parked faces to their fallback tint) once the
                // retry budget is exhausted, so the faces stay parked meanwhile.
                let previous = manager.retry.get(&id).map(|(_params, state)| *state);
                match (params, RetryState::after_failure(previous, now)) {
                    (Some(params), Some(state)) => {
                        // Logged so a transient GetTexture 503 and its recovery are
                        // observable in any test run (there is no other way to
                        // live-verify the retry). Grep `scheduling retry`.
                        warn!(
                            "texture {id} fetch failed; scheduling retry {}/{} in {:.1}s",
                            state.attempts,
                            crate::asset_retry::MAX_RETRY_ATTEMPTS,
                            state.next_at - now
                        );
                        let _prev = manager.retry.insert(id, (params, state));
                    }
                    _exhausted_or_unknown => {
                        warn!(
                            "texture {id} fetch failed; gave up after {} attempts",
                            crate::asset_retry::MAX_RETRY_ATTEMPTS
                        );
                        let _cleared = manager.retry.remove(&id);
                        decoded.write(TextureDecoded(id));
                    }
                }
            }
        }
    }

    // Re-issue any failed fetches whose backoff has now elapsed.
    let due: Vec<(TextureKey, DeferredRequest)> = manager
        .retry
        .iter()
        .filter(|(_id, (_params, state))| state.due(now))
        .map(|(&id, (params, _state))| (id, params.clone()))
        .collect();
    for (id, params) in due {
        // Park the retry state (keeping its attempt count) rather than removing it:
        // the re-issued fetch's result reschedules or clears it. Removing it here
        // dropped the count, so the next failure reset to attempt 1 and the backoff
        // looped forever at "retry 1/N" without ever giving up.
        if let Some((_params, state)) = manager.retry.get_mut(&id) {
            *state = state.issued();
        }
        manager.request_from(
            id,
            params.source,
            params.priority,
            params.initial_lod,
            params.managed,
        );
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
#[derive(Debug, Resource, Default)]
pub struct PrimTextures {
    /// Uploaded diffuse images by texture id, so a texture shared by many faces
    /// is turned into a Bevy [`Image`] once.
    images: HashMap<TextureKey, Handle<Image>>,
    /// Face materials parked on a texture id, patched with the diffuse image (or
    /// released to their flat tint) once the fetch resolves.
    pending: HashMap<TextureKey, Vec<(Handle<FaceMaterial>, TextureAlpha)>>,
    /// Every material that samples each texture, tracked so a level-of-detail
    /// re-upload (P21.1) can mark them changed. A Bevy material's bind group caches
    /// the texture's GPU view and is **not** rebuilt when the [`Image`] behind its
    /// handle is replaced with a different-size one (nothing in `bevy_pbr` watches
    /// `AssetEvent<Image>` for materials), so the new resolution never appears until
    /// the material asset itself is touched. Stored as weak [`AssetId`]s so a
    /// despawned face's material is not kept alive; ids that no longer resolve are
    /// pruned when the texture is refreshed.
    materials: HashMap<TextureKey, Vec<AssetId<FaceMaterial>>>,
    /// Level-of-detail re-uploads deferred past a frame's image-build budget
    /// (`TextureApplyBudget`), drained FIFO by [`drain_lod_reuploads`]. A LOD
    /// re-decode rebuilds the texture's RGBA image (~1.5 ms each) and marks its
    /// materials changed; a camera move that upgrades many textures at once would
    /// otherwise rebuild them all in one frame (the residual ~40 ms
    /// `apply_prim_textures` spike). Only the id is held — the store keeps the
    /// latest decoded image, so a re-queued id still refreshes to the newest level.
    pending_lod: VecDeque<TextureKey>,
}

/// Default per-frame cap on how many face materials the texture-apply systems may
/// re-prep (mutate) in one frame — see [`TextureApplyBudget`]. Chosen so the common
/// frame (which drapes only a handful of freshly decoded faces) is never touched,
/// while a decode-burst frame that would otherwise re-prep hundreds of faces at
/// once is spread across a few frames instead. Tune with
/// `SL_VIEWER_FACE_REPREP_BUDGET`.
const DEFAULT_FACE_REPREP_BUDGET: usize = 48;

/// Default per-frame cap on how many decoded textures the texture-apply systems may
/// turn into GPU [`Image`]s in one frame — see [`TextureApplyBudget`]. When textures
/// are served from local cache a whole region's set can decode in a single frame;
/// building them all at once (`build_prim_image` → `to_bevy_image`, a full RGBA
/// upload each ~1.5 ms) was measured as a ~40–55 ms main-thread spike. Kept low
/// because each build is expensive. Tune with `SL_VIEWER_TEXTURE_IMAGE_BUDGET`.
const DEFAULT_TEXTURE_IMAGE_BUDGET: usize = 6;

/// The two per-frame texture-apply budgets, shared by [`apply_prim_textures`],
/// [`patch_parked_decoded_textures`] and the backlog drainer
/// [`drain_deferred_face_textures`], and refilled each frame by
/// [`reset_texture_apply_budget`].
///
/// **Re-preps** (`reprep_remaining`): every
/// `Assets<FaceMaterial>::get_mut` marks the material changed, and Bevy's render
/// world then rebuilds its whole bindless bind group in `prepare_erased_assets` —
/// cheap per material, but draping a burst of just-decoded textures onto hundreds
/// of parked faces at once produces a multi-millisecond prepare spike. Capping the
/// re-preps and deferring the overflow ([`DeferredFaceTextures`]) spreads it.
///
/// **Image builds** (`image_remaining`): turning a decoded
/// texture into a GPU image (`build_prim_image`) is a full RGBA upload; a
/// cache-warm frame that decodes a region's whole texture set builds them all at
/// once (~55 ms). Capping the builds leaves the excess textures' faces parked so
/// [`patch_parked_decoded_textures`] builds them over the next frames.
///
/// The two orderings this covers: cache-warm, geometry arrives last and its faces
/// meet already-decoded textures (re-prep spike); cache-cold, textures arrive last
/// and a burst decodes together (image-build + re-prep spike).
#[derive(Debug, Resource)]
pub struct TextureApplyBudget {
    /// The full per-frame material-reprep cap, refilled each frame.
    reprep_per_frame: usize,
    /// Material re-preps still allowed this frame; once zero, the rest defer.
    reprep_remaining: usize,
    /// The full per-frame image-build cap, refilled each frame.
    image_per_frame: usize,
    /// Image builds still allowed this frame; once zero, the texture's faces stay
    /// parked for a later frame.
    image_remaining: usize,
}

impl Default for TextureApplyBudget {
    fn default() -> Self {
        let reprep_per_frame =
            env_budget("SL_VIEWER_FACE_REPREP_BUDGET", DEFAULT_FACE_REPREP_BUDGET);
        let image_per_frame = env_budget(
            "SL_VIEWER_TEXTURE_IMAGE_BUDGET",
            DEFAULT_TEXTURE_IMAGE_BUDGET,
        );
        Self {
            reprep_per_frame,
            reprep_remaining: reprep_per_frame,
            image_per_frame,
            image_remaining: image_per_frame,
        }
    }
}

impl TextureApplyBudget {
    /// Try to spend one image-build from this frame's shared image lane. Returns
    /// `true` (and decrements) when there was budget, `false` when the lane is spent
    /// and the caller should leave its decoded image parked for a later frame. This
    /// lane is shared by **every** image-inserting apply system — prim diffuse here,
    /// plus the PBR-map / bump-normal / legacy-map / avatar-bake systems in other
    /// modules — so the combined new-`Image` count per frame is bounded and the
    /// serial `extract_render_asset<GpuImage>` upload cannot spike from stacked
    /// per-system budgets.
    pub(crate) const fn take_image(&mut self) -> bool {
        if self.image_remaining > 0 {
            self.image_remaining = self.image_remaining.saturating_sub(1);
            true
        } else {
            false
        }
    }
}

/// A positive per-frame budget from `var`, or `default` when it is unset /
/// unparsable / zero.
#[must_use]
pub fn env_budget(var: &str, default: usize) -> usize {
    std::env::var(var)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

/// A decoded texture's alpha classification, resolved once when it decodes and
/// carried alongside every parked / deferred drape so the alpha mode need not be
/// recomputed per face: whether the image carries an alpha channel (component-based)
/// and whether that alpha holds real transparency (value-based).
#[derive(Debug, Clone, Copy)]
struct DecodedAlpha {
    /// The decoded texture carries an alpha channel.
    has_alpha: bool,
    /// That alpha channel holds genuinely transparent texels.
    has_transparency: bool,
}

/// One face-texture drape deferred past a frame's [`TextureApplyBudget`]: the diffuse
/// [`Image`] is already built and its alpha classification already computed, so
/// [`drain_deferred_face_textures`] applies it in a later frame with a plain
/// `get_mut` and no recomputation.
#[derive(Debug)]
struct DeferredFaceTexture {
    /// The parked face's material to drape the diffuse onto.
    material: Handle<FaceMaterial>,
    /// The already-uploaded diffuse image for the texture.
    image: Handle<Image>,
    /// The parked face's own alpha hint (mask / blend / none).
    texture_alpha: TextureAlpha,
    /// The decoded texture's alpha classification.
    alpha: DecodedAlpha,
}

/// The backlog of face-texture drapes deferred past their frame's
/// [`TextureApplyBudget`], drained at up to the budget per frame by
/// [`drain_deferred_face_textures`]. FIFO, so the earliest-decoded (usually
/// nearest / first-visible) faces texture first.
#[derive(Debug, Resource, Default)]
pub struct DeferredFaceTextures {
    /// Drapes not yet applied, oldest at the front.
    queue: VecDeque<DeferredFaceTexture>,
}

/// Refill the per-frame [`TextureApplyBudget`] counters at the start of the apply
/// pass, before [`apply_prim_textures`] / [`patch_parked_decoded_textures`] / the
/// drainer spend from them.
pub fn reset_texture_apply_budget(mut budget: ResMut<TextureApplyBudget>) {
    budget.reprep_remaining = budget.reprep_per_frame;
    budget.image_remaining = budget.image_per_frame;
}

/// The state of a face's diffuse image — see [`TextureManager::diffuse_image`].
#[derive(Debug, Clone)]
pub enum DiffuseImage {
    /// The face carries no texture (a nil / null-sentinel id); a material should
    /// clear its `base_color_texture` and show the flat tint.
    Absent,
    /// The texture exists but has not decoded yet — keep the current image and
    /// try again once it lands.
    Pending,
    /// The decoded image, uploaded as a fresh Bevy image ready to paint onto a
    /// material.
    Ready(Handle<Image>),
}

/// How a face treats a diffuse texture that carries its **own** alpha channel,
/// matching the reference viewer's per-face alpha handling.
///
/// In the reference (`LLPipeline::getPoolTypeFromTE`), a face whose texture has an
/// alpha channel (`getComponents() == 4`, or `2`) goes to the alpha pool. Which
/// pass it draws in then depends on the face:
/// - [`Mask`](Self::Mask): an ordinary prim / static-mesh / tree / grass face can
///   alpha-*mask* (`LLFace::canRenderAsMask`) — opaque pass + alpha test — so a
///   wholly transparent texel is cut while a solid texel stays solid (an invisible
///   prim stays invisible).
/// - [`Blend`](Self::Blend): a **rigged** face is *never* auto-masked (`llface.cpp`:
///   "never auto alpha-mask rigged faces"), so a rigged face with a genuinely
///   transparent texture (hair, eyelashes) alpha-*blends* — soft edges, not a hard
///   cut or a solid card.
///
/// Note a rigged face sampling a 5-channel avatar **bake** (bake-on-mesh) is a
/// different path handled in `avatars.rs`: a 5-channel texture does not satisfy the
/// `== 4` test, so it renders in the avatar alpha-*mask* pass at `sMinimumAlpha`
/// (0.2) — see `apply_bom_face_materials`. This enum is only for a rigged face
/// sampling an ordinary fetched texture.
///
/// This is only the *default* for a face with no explicit alpha mode: a non-opaque
/// TE tint already forces blending when the material is built
/// (`face_alpha_mode`), and a face's `LLMaterial` diffuse alpha mode (fetched
/// later over `RenderMaterials`) overrides it authoritatively
/// ([`legacy_alpha_override`](crate::legacy_materials)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureAlpha {
    /// Alpha-mask a texture-alpha face (ordinary prim / mesh / tree / grass faces).
    Mask,
    /// Alpha-blend a rigged face whose texture actually carries transparency (hair,
    /// eyelashes) — rigged faces cannot mask, so they blend. A texture with no real
    /// transparency stays opaque (so solid rigged clothing is not needlessly moved
    /// into the transparent pass).
    Blend,
}

/// The alpha-test cutoff a [`TextureAlpha::Mask`] face discards below — a texel
/// whose alpha is under this fraction is cut. Half, matching the bake-mask cutoff
/// and the reference viewer's default auto-mask.
const FACE_ALPHA_MASK_CUTOFF: f32 = 0.5;

/// Build the diffuse [`StandardMaterial`] for one prim face: `base_color` is the
/// face tint (opaque white = untinted), and `base_color_texture` is filled in
/// immediately when the face's texture is already uploaded, otherwise the
/// material is parked in `prim_textures` and its texture requested through
/// `manager` (which dedupes) so [`apply_prim_textures`] can fill it in later.
///
/// `texture_alpha` selects how a diffuse texture's own alpha channel is treated
/// (the reference-faithful default before any `LLMaterial` alpha mode arrives):
/// [`Mask`](TextureAlpha::Mask) for an ordinary face, [`Blend`](TextureAlpha::Blend)
/// for a rigged one (which cannot mask).
///
/// A face with no texture (nil id) keeps just its flat tint.
pub(crate) fn face_material(
    face: &TextureFace,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
    texture_alpha: TextureAlpha,
) -> Handle<FaceMaterial> {
    let handle = materials.add(FaceMaterial::default());
    compose_face_material(
        &handle,
        face,
        materials,
        manager,
        prim_textures,
        priority,
        texture_alpha,
    );
    handle
}

/// Build the diffuse material for one **internable** prim face through the
/// cross-instance [`MaterialCache`] (roadmap `viewer-perf-material-intern`):
/// when `internable` and an identical face's material is still alive, revive
/// and share its handle (so Bevy batches the matching draws); otherwise
/// compose a fresh material via `face_material` and — when internable —
/// record it for the next identical face. Returns the handle plus whether it
/// is (or seeded) a cache entry, so the caller can mark the face
/// [`SharedFaceMaterial`](crate::material_cache::SharedFaceMaterial) for the
/// copy-on-write detach net.
///
/// A **hit** skips [`compose_face_material`] entirely — the shared material
/// already carries the composed state, including its diffuse texture if that
/// has decoded (a bonus: no re-prep budget spent, the face appears textured
/// immediately). While the texture is still undecoded the shared handle is
/// re-parked in `prim_textures` **deduplicated by handle** (the original
/// compose already parked it once; a duplicate would only waste a re-prep,
/// since [`drape_face_texture`] is idempotent) and the texture re-requested —
/// which both bumps the fetch priority for a boosted sharer and revives the
/// retry after a failed decode consumed the original parking.
pub(crate) fn intern_face_material(
    face: &TextureFace,
    internable: bool,
    cache: &mut MaterialCache,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
) -> (Handle<FaceMaterial>, bool) {
    if !internable {
        cache.note_excluded();
        let handle = face_material(
            face,
            materials,
            manager,
            prim_textures,
            priority,
            TextureAlpha::Mask,
        );
        return (handle, false);
    }
    let key = MaterialKey::new(face, TextureAlpha::Mask);
    if let Some(handle) = cache.revive(&key, materials) {
        cache.note_hit();
        let texture_id = face.texture_id;
        if !is_absent_texture(texture_id) && !prim_textures.images.contains_key(&texture_id) {
            let parked = prim_textures.pending.entry(texture_id).or_default();
            if !parked
                .iter()
                .any(|(parked_handle, _alpha)| *parked_handle == handle)
            {
                parked.push((handle.clone(), TextureAlpha::Mask));
            }
            manager.request_face(texture_id, priority);
        }
        return (handle, true);
    }
    cache.note_miss();
    let handle = face_material(
        face,
        materials,
        manager,
        prim_textures,
        priority,
        TextureAlpha::Mask,
    );
    cache.record(key, handle.id());
    (handle, true)
}

/// Compose the diffuse Blinn-Phong appearance of `face` **onto an existing**
/// material `handle` — the same mapping `face_material` builds into a fresh
/// handle (tint, UV placement, alpha resolution, the legacy surface flags, and
/// the diffuse texture, filled now if resident else parked + requested), but
/// written over whatever the handle already held.
///
/// This exists so a PBR face's material can be **reverted to its Blinn-Phong
/// look** while it is edited on the build tool's Blinn-Phong tab (the FIRE-35138
/// hide, [`crate::materials::apply_blinn_phong_hide`]): the face keeps its one
/// stable handle (every other system that reads it is unaffected) and only its
/// composition changes. `face_material` is the fresh-handle wrapper over it.
pub fn compose_face_material(
    handle: &Handle<FaceMaterial>,
    face: &TextureFace,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
    texture_alpha: TextureAlpha,
) {
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
        // Transparency: a face whose tint colour is non-opaque blends now (the TE
        // color alpha, the reference's `blinn_phong_transparent`). A face whose
        // *texture* carries an alpha channel is NOT blended — the reference viewer
        // never blends off the texture alpha alone (R22d); it is instead resolved
        // per `texture_alpha` once the texture decodes (in [`apply_prim_textures`]):
        // an ordinary face alpha-masks (an invisible prim stays invisible), a rigged
        // face stays opaque. A face's `LLMaterial` diffuse alpha mode overrides both
        // later ([`legacy_alpha_override`](crate::legacy_materials)).
        alpha_mode: face_alpha_mode(face.color),
        // Single-sided (default back-face culling): Second Life renders a face
        // only from its front, so a one-sided surface (a flat mesh quad, a prim
        // cut face) is invisible from behind rather than doubled. The tessellated
        // prim faces and decoded mesh submeshes carry outward-facing windings.
        ..default()
    };
    if has_texture && let Some(image) = prim_textures.images.get(&texture_id) {
        material.base_color_texture = Some(image.clone());
        // The texture is already uploaded, so resolve its alpha channel **now**
        // (R25a): this build path never parks the face, so without this the
        // R22d resolution below ([`apply_prim_textures`]) never runs for it —
        // a face re-built while its texture was already resident (the prim-LoD
        // re-tessellation on approach, a shape change, a derender/re-create)
        // popped back in opaque, losing the alpha-texture transparency its
        // first build had gained when the texture decoded.
        let has_alpha = manager
            .decoded(texture_id)
            .is_some_and(|decoded| texture_has_alpha(decoded));
        let has_transparency = has_alpha
            && manager
                .decoded(texture_id)
                .is_some_and(|decoded| texture_has_transparency(decoded));
        resolve_texture_alpha_mode(&mut material, texture_alpha, has_alpha, has_transparency);
    }
    // Legacy per-face surface flags (P27.4): fullbright / glow / shiny fold onto
    // this material as it is built (bump needs the decoded diffuse and is applied
    // later by the `bump` pipeline). A face with none of these flags is untouched.
    crate::bump::apply_surface_flags(&mut material, face);
    // Write the composed material over the existing handle (Bevy marks the asset
    // changed, so a re-composition — e.g. the FIRE-35138 Blinn-Phong revert of a
    // PBR face — rebuilds its bind group). A handle whose asset was dropped is a
    // no-op.
    let Some(mut slot) = materials.get_mut(handle) else {
        return;
    };
    // Write the composed diffuse into the face material's `base`, resetting the
    // extension to inert (a fresh Blinn-Phong composition carries no per-map
    // transforms / legacy specular until the legacy pipeline populates it).
    *slot = inert_face_material(material);
    // The faithful SL glow mask (`crate::glow`): carry the face's glow scalar into
    // the extension; `face_material.wgsl` writes it to alpha for an opaque / mask
    // face (0 for a non-glowing face) and leaves a blend face's coverage alone. A
    // face with no glow keeps the inert `0`. Inert until the glow pass is enabled.
    slot.extension.params.glow = face.glow;
    if has_texture {
        // Track this material so a later level-of-detail re-upload can mark it
        // changed and rebuild its bind group (P21.1) — see `PrimTextures::materials`.
        prim_textures
            .materials
            .entry(texture_id)
            .or_default()
            .push(handle.id());
        // A textured face whose image is not uploaded yet: park the material and
        // ask the pipeline for the texture (idempotent across faces).
        if !prim_textures.images.contains_key(&texture_id) {
            prim_textures
                .pending
                .entry(texture_id)
                .or_default()
                .push((handle.clone(), texture_alpha));
            manager.request_face(texture_id, priority);
        }
    }
}

impl PrimTextures {
    /// Drop `handle` from every parked-face list, so a diffuse texture that
    /// decodes later does not paint itself onto a material that has since been
    /// recomposed as something else — used when a PBR face reverts from its
    /// Blinn-Phong hide back to PBR ([`crate::materials::apply_blinn_phong_hide`]),
    /// which must not let a still-pending Blinn-Phong diffuse land over the
    /// restored PBR composition.
    pub(crate) fn drop_pending_material(&mut self, handle: &Handle<FaceMaterial>) {
        for parked in self.pending.values_mut() {
            parked.retain(|(parked_handle, _alpha)| parked_handle != handle);
        }
    }
}

/// Fill each newly decoded prim texture into the faces parked on it: upload (and
/// cache) its diffuse [`Image`], then drop it into every parked material's
/// `base_color_texture`. A decode that failed drops the parked materials so they
/// keep their flat tint.
/// Drape a decoded diffuse `image` onto one parked face `material`, resolving its
/// alpha mode (unless a legacy material already fixed it). Returns `true` if the
/// material still existed — a despawned face's handle no longer resolves and is
/// skipped, spending no re-prep budget. The single point every face-texture apply
/// path funnels through ([`apply_prim_textures`], [`patch_parked_decoded_textures`],
/// [`drain_deferred_face_textures`]).
fn drape_face_texture(
    materials: &mut Assets<FaceMaterial>,
    legacy: &crate::legacy_materials::LegacyMaterialManager,
    material: &Handle<FaceMaterial>,
    image: &Handle<Image>,
    texture_alpha: TextureAlpha,
    alpha: DecodedAlpha,
) -> bool {
    let Some(mut face) = materials.get_mut(material) else {
        return false;
    };
    face.base.base_color_texture = Some(image.clone());
    // A face whose alpha mode a legacy material has already overridden keeps it
    // (R25a): `NONE` means opaque in the reference even over an alpha texture, so
    // the material must win regardless of whether it or this decode applied last.
    if !legacy.is_alpha_overridden(material.id()) {
        resolve_texture_alpha_mode(
            &mut face.base,
            texture_alpha,
            alpha.has_alpha,
            alpha.has_transparency,
        );
    }
    true
}

/// Drape a texture's freshly decoded `parked` faces under the per-frame re-prep
/// `budget`, pushing the overflow onto `deferred` for a later frame. Each real
/// re-prep (a live face) spends one unit of budget; a despawned face is skipped for
/// free. Shared by [`apply_prim_textures`] and [`patch_parked_decoded_textures`].
fn drape_parked_faces(
    materials: &mut Assets<FaceMaterial>,
    legacy: &crate::legacy_materials::LegacyMaterialManager,
    budget: &mut usize,
    deferred: &mut VecDeque<DeferredFaceTexture>,
    image: &Handle<Image>,
    alpha: DecodedAlpha,
    parked: Vec<(Handle<FaceMaterial>, TextureAlpha)>,
) {
    for (material, texture_alpha) in parked {
        if *budget == 0 {
            deferred.push_back(DeferredFaceTexture {
                material,
                image: image.clone(),
                texture_alpha,
                alpha,
            });
            continue;
        }
        if drape_face_texture(materials, legacy, &material, image, texture_alpha, alpha) {
            *budget = budget.saturating_sub(1);
        }
    }
}

/// Whether a decoded texture carries alpha, and whether that alpha is really
/// transparent — the classification [`resolve_texture_alpha_mode`] needs, computed
/// once per decode.
fn decoded_alpha(manager: &TextureManager, id: TextureKey) -> DecodedAlpha {
    let has_alpha = manager
        .decoded(id)
        .is_some_and(|decoded| texture_has_alpha(decoded));
    let has_transparency = has_alpha
        && manager
            .decoded(id)
            .is_some_and(|decoded| texture_has_transparency(decoded));
    DecodedAlpha {
        has_alpha,
        has_transparency,
    }
}

/// Guard a texture's image build against the per-frame image-build budget. Returns
/// `Some(parked)` — the caller should build the image and drape — when the image is
/// already built (free) or the frame still has image-build budget. Returns `None`,
/// after re-parking `parked` back onto `pending`, when the budget is spent and the
/// image is not built yet: [`patch_parked_decoded_textures`] will build it in a later
/// frame. This is the only place a cache-warm decode burst is throttled.
fn reserve_image_build(
    prim_textures: &mut PrimTextures,
    budget: &TextureApplyBudget,
    id: TextureKey,
    parked: Vec<(Handle<FaceMaterial>, TextureAlpha)>,
) -> Option<Vec<(Handle<FaceMaterial>, TextureAlpha)>> {
    if prim_textures.images.contains_key(&id) || budget.image_remaining > 0 {
        return Some(parked);
    }
    prim_textures.pending.entry(id).or_default().extend(parked);
    None
}

/// Turn a decoded texture's parked faces into a rendered, textured batch: build (and
/// cache) its GPU image, then drape it onto the parked faces under the two per-frame
/// budgets. Returns `false` — leaving the faces parked for a later frame — when the
/// image is not yet built and the frame's image-build budget is spent (so a
/// cache-warm burst of decodes does not upload every texture in one frame);
/// [`patch_parked_decoded_textures`] retries the parked faces next frame. Shared by
/// [`apply_prim_textures`] and [`patch_parked_decoded_textures`].
#[expect(
    clippy::too_many_arguments,
    reason = "funnels every resource the two texture-apply systems share into one place"
)]
fn drape_decoded_texture(
    manager: &TextureManager,
    legacy: &crate::legacy_materials::LegacyMaterialManager,
    prim_textures: &mut PrimTextures,
    budget: &mut TextureApplyBudget,
    deferred: &mut VecDeque<DeferredFaceTexture>,
    images: &mut Assets<Image>,
    materials: &mut Assets<FaceMaterial>,
    id: TextureKey,
    parked: Vec<(Handle<FaceMaterial>, TextureAlpha)>,
) -> bool {
    let already_built = prim_textures.images.contains_key(&id);
    let Some(parked) = reserve_image_build(prim_textures, budget, id, parked) else {
        // Out of image-build budget this frame and the image is not built: the faces
        // were re-parked for `patch` to build (and drape) in a later frame.
        return false;
    };
    let alpha = decoded_alpha(manager, id);
    let Some(image_handle) = prim_image(manager, prim_textures, images, id) else {
        // The fetch failed: the parked faces keep their flat tint.
        return true;
    };
    if !already_built {
        budget.image_remaining = budget.image_remaining.saturating_sub(1);
    }
    drape_parked_faces(
        materials,
        legacy,
        &mut budget.reprep_remaining,
        deferred,
        &image_handle,
        alpha,
        parked,
    );
    true
}

/// Refresh the Bevy image behind a texture's existing handle after a level-of-detail
/// re-decode (P21.1) and mark every material sampling it changed, so Bevy rebuilds
/// their bind groups and the new resolution appears (a material's bind group caches
/// the texture's GPU view and is not rebuilt on an image change alone). Pruning any
/// material whose face has despawned. A no-op if the texture is not decoded or has no
/// built image.
fn refresh_lod_image(
    manager: &TextureManager,
    prim_textures: &mut PrimTextures,
    images: &mut Assets<Image>,
    materials: &mut Assets<FaceMaterial>,
    id: TextureKey,
) {
    let Some(handle) = prim_textures.images.get(&id).cloned() else {
        return;
    };
    let Some(image) = manager.decoded(id) else {
        return;
    };
    let refreshed = build_prim_image(image);
    let _replaced = images.insert(&handle, refreshed);
    if let Some(material_ids) = prim_textures.materials.get_mut(&id) {
        // Touch each live material (prune any whose face was despawned).
        material_ids.retain(|&material_id| materials.get_mut(material_id).is_some());
    }
}

/// Guard a level-of-detail re-upload against the per-frame image-build budget: with
/// the budget spent, queue `id` on `pending_lod` (deduplicated) for
/// [`drain_lod_reuploads`] and return `true`; otherwise return `false` so the caller
/// refreshes now. Rebuilding a texture's RGBA image is the dominant LOD cost, so this
/// alone bounds the LOD path (fewer textures processed also means fewer materials
/// re-marked). LOD shares the budget with the initial-decode path but drains last, so
/// a face's first appearance always wins the frame's budget over a mere refinement.
fn defer_lod_reupload(
    prim_textures: &mut PrimTextures,
    budget: &TextureApplyBudget,
    id: TextureKey,
) -> bool {
    if budget.image_remaining > 0 {
        return false;
    }
    if !prim_textures.pending_lod.contains(&id) {
        prim_textures.pending_lod.push_back(id);
    }
    true
}

/// Drop each freshly decoded diffuse texture onto the prim faces parked on it (or
/// refresh the image behind a level-of-detail re-decode), building images and draping
/// faces under the per-frame [`TextureApplyBudget`].
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system: the decode reader plus the resources building + draping textures need"
)]
pub fn apply_prim_textures(
    mut decoded: MessageReader<TextureDecoded>,
    manager: Res<TextureManager>,
    legacy: Res<crate::legacy_materials::LegacyMaterialManager>,
    mut prim_textures: ResMut<PrimTextures>,
    mut budget: ResMut<TextureApplyBudget>,
    mut deferred: ResMut<DeferredFaceTextures>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    for &TextureDecoded(id) in decoded.read() {
        // Level-of-detail re-decode (P21.1): a texture already uploaded to the GPU
        // whose store entry the driver upgraded / downgraded. Refresh it under the
        // per-frame image-build budget — a camera move that upgrades many textures at
        // once would otherwise rebuild them all in one frame; the overflow defers to
        // `drain_lod_reuploads`.
        if prim_textures.images.contains_key(&id) {
            if manager.decoded(id).is_some() && !defer_lod_reupload(&mut prim_textures, &budget, id)
            {
                refresh_lod_image(
                    &manager,
                    &mut prim_textures,
                    &mut images,
                    &mut materials,
                    id,
                );
                budget.image_remaining = budget.image_remaining.saturating_sub(1);
            }
            continue;
        }
        let Some(parked) = prim_textures.pending.remove(&id) else {
            // Not a texture any prim face is waiting on (e.g. a terrain texture).
            continue;
        };
        // Build the image (image-budgeted) and drape its faces (reprep-budgeted); the
        // overflow of either defers to a later frame so a cache-warm decode burst does
        // not upload every texture / re-prep hundreds of materials in one frame.
        let _draped = drape_decoded_texture(
            &manager,
            &legacy,
            &mut prim_textures,
            &mut budget,
            &mut deferred.queue,
            &mut images,
            &mut materials,
            id,
            parked,
        );
    }
}

/// Patch prim faces parked on a texture that is **already decoded** — the
/// [`apply_prim_textures`] event only fires when a texture *decodes*, so a face
/// that parks **after** its texture was decoded (e.g. the build tool pre-fetched
/// it for a live preview, then a commit re-tessellated the face) would never be
/// filled and would render as a flat solid tint. This runs after
/// [`apply_prim_textures`] and drains any such stranded parked faces.
pub fn patch_parked_decoded_textures(
    manager: Res<TextureManager>,
    legacy: Res<crate::legacy_materials::LegacyMaterialManager>,
    mut prim_textures: ResMut<PrimTextures>,
    mut budget: ResMut<TextureApplyBudget>,
    mut deferred: ResMut<DeferredFaceTextures>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    let ready: Vec<TextureKey> = prim_textures
        .pending
        .keys()
        .copied()
        .filter(|id| !prim_textures.images.contains_key(id) && manager.decoded(*id).is_some())
        .collect();
    for id in ready {
        // Stop once the image-build budget is spent — the rest stay parked for the
        // next frame (re-parking inside `drape_decoded_texture` is a no-op here, since
        // the faces were never removed).
        if budget.image_remaining == 0 {
            break;
        }
        let Some(parked) = prim_textures.pending.remove(&id) else {
            continue;
        };
        let _draped = drape_decoded_texture(
            &manager,
            &legacy,
            &mut prim_textures,
            &mut budget,
            &mut deferred.queue,
            &mut images,
            &mut materials,
            id,
            parked,
        );
    }
}

/// Drain up to the remaining per-frame reprep budget of deferred face-texture drapes
/// (overflow the apply systems pushed past the budget), spreading a decode burst's
/// re-preps across frames. Runs after the apply systems each frame, so a fresh decode
/// textures ahead of an older backlog item.
pub fn drain_deferred_face_textures(
    legacy: Res<crate::legacy_materials::LegacyMaterialManager>,
    mut budget: ResMut<TextureApplyBudget>,
    mut deferred: ResMut<DeferredFaceTextures>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    drain_deferred(
        &mut materials,
        &legacy,
        &mut budget.reprep_remaining,
        &mut deferred.queue,
    );
}

/// Drain the level-of-detail re-upload backlog (`PrimTextures::pending_lod`) up to
/// the remaining per-frame image-build budget, FIFO. Runs after the initial-decode
/// apply systems each frame so a face's first appearance always wins the frame's image
/// budget over a mere LOD refinement. A queued id whose store entry is gone (the
/// texture was evicted) is dropped for free.
pub fn drain_lod_reuploads(
    manager: Res<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
    mut budget: ResMut<TextureApplyBudget>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    while budget.image_remaining > 0 {
        let Some(id) = prim_textures.pending_lod.pop_front() else {
            break;
        };
        if manager.decoded(id).is_none() {
            continue;
        }
        refresh_lod_image(
            &manager,
            &mut prim_textures,
            &mut images,
            &mut materials,
            id,
        );
        budget.image_remaining = budget.image_remaining.saturating_sub(1);
    }
}

/// Apply up to `budget` deferred drapes from `queue` (FIFO). Each live face spends
/// one budget unit; a despawned face's item is dropped for free. Testable core of
/// [`drain_deferred_face_textures`].
fn drain_deferred(
    materials: &mut Assets<FaceMaterial>,
    legacy: &crate::legacy_materials::LegacyMaterialManager,
    budget: &mut usize,
    queue: &mut VecDeque<DeferredFaceTexture>,
) {
    while *budget > 0 {
        let Some(item) = queue.pop_front() else {
            break;
        };
        if drape_face_texture(
            materials,
            legacy,
            &item.material,
            &item.image,
            item.texture_alpha,
            item.alpha,
        ) {
            *budget = budget.saturating_sub(1);
        }
    }
}

/// Resolve a texture-alpha face's mode (reference-faithful, R22d): an ordinary
/// face alpha-*masks* off its texture alpha (an invisible prim stays cut, solid
/// texels stay solid); a rigged face cannot mask, so one with genuinely
/// transparent texels alpha-*blends* (hair / eyelashes render soft, not as a
/// solid card). A texture with no real transparency stays opaque. A face
/// already blending (a non-opaque tint) is left blending; a `LLMaterial` mode
/// later wins.
///
/// Shared by both moments a face meets its decoded texture: the parked path
/// ([`apply_prim_textures`], texture decoded after the face was built) and the
/// immediate path (`face_material`, face built while the texture was already
/// resident — the branch that used to skip this entirely, R25a).
fn resolve_texture_alpha_mode(
    material: &mut StandardMaterial,
    texture_alpha: TextureAlpha,
    has_alpha: bool,
    has_transparency: bool,
) {
    if material.alpha_mode != AlphaMode::Opaque {
        return;
    }
    match texture_alpha {
        TextureAlpha::Mask if has_alpha => {
            material.alpha_mode = AlphaMode::Mask(FACE_ALPHA_MASK_CUTOFF);
        }
        TextureAlpha::Blend if has_transparency => {
            material.alpha_mode = AlphaMode::Blend;
        }
        _keep_opaque => {}
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

/// The 8-bit alpha value below which a decoded texel counts as genuinely
/// transparent when deciding whether a rigged face must blend
/// ([`texture_has_transparency`]) — half, so the near-opaque noise a lossy J2C
/// leaves on a nominally opaque alpha channel does not force a solid texture into
/// the transparent pass.
const TRANSPARENCY_ALPHA_CUTOFF: u8 = 128;

/// Whether a decoded texture holds **real** transparency — an alpha-bearing source
/// with at least one texel below [`TRANSPARENCY_ALPHA_CUTOFF`]. Distinguishes a
/// hair / eyelash texture (soft alpha, blends) from a solid texture that merely
/// carries a fully-opaque alpha channel (stays opaque), so only the former moves a
/// rigged face into the transparent pass. O(1): the pixel scan happened once
/// in the decode task ([`DecodedTexture::min_alpha`]), never on the frame
/// thread.
const fn texture_has_transparency(decoded: &DecodedTexture) -> bool {
    texture_has_alpha(decoded) && decoded.min_alpha < TRANSPARENCY_ALPHA_CUTOFF
}

/// Convert a face tint (RGBA bytes, `[255; 4]` = opaque white = no tint) into a
/// Bevy sRGB [`Color`] to multiply the diffuse texture by.
pub(crate) fn tint_color(color: [u8; 4]) -> Color {
    Color::srgba(
        f32::from(color[0]) / 255.0,
        f32::from(color[1]) / 255.0,
        f32::from(color[2]) / 255.0,
        f32::from(color[3]) / 255.0,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        DecodedAlpha, DeferredFaceTexture, FACE_ALPHA_MASK_CUTOFF, FaceMaterial, PrimTextures,
        TextureAlpha, TextureApplyBudget, defer_lod_reupload, drain_deferred, drape_parked_faces,
        face_alpha_mode, reserve_image_build, resolve_texture_alpha_mode, texture_has_alpha,
    };
    use crate::face_material::inert_face_material;
    use crate::legacy_materials::LegacyMaterialManager;
    use bevy::asset::{Assets, Handle};
    use bevy::image::Image;
    use bevy::pbr::StandardMaterial;
    use bevy::prelude::AlphaMode;
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{DecodedTexture, DiscardLevel, TextureKey, Uuid};
    use std::collections::VecDeque;

    /// A decoded texture with the given source component count (pixels unused by
    /// the alpha test, so a single RGBA8 texel stands in).
    pub(crate) fn decoded(components: u16) -> DecodedTexture {
        DecodedTexture::new(
            1,
            1,
            components,
            DiscardLevel::FULL,
            Bytes::from(vec![0xFF_u8; 4]),
            None,
        )
    }

    /// The R22d texture-alpha resolution is identical whether the face meets
    /// its texture on the parked path or is built with it already resident
    /// (R25a): an opaque ordinary face masks off an alpha texture, an opaque
    /// rigged face blends off real transparency, an already-blending face is
    /// left alone, and an alpha-free texture keeps the face opaque.
    #[test]
    fn texture_alpha_resolution_upgrades_only_opaque_faces() {
        let mut face = StandardMaterial {
            alpha_mode: AlphaMode::Opaque,
            ..StandardMaterial::default()
        };
        resolve_texture_alpha_mode(&mut face, TextureAlpha::Mask, true, false);
        assert_eq!(face.alpha_mode, AlphaMode::Mask(FACE_ALPHA_MASK_CUTOFF));
        // A rigged face blends when the texture holds real transparency…
        face.alpha_mode = AlphaMode::Opaque;
        resolve_texture_alpha_mode(&mut face, TextureAlpha::Blend, true, true);
        assert_eq!(face.alpha_mode, AlphaMode::Blend);
        // …but not off a merely-present, fully-solid alpha channel.
        face.alpha_mode = AlphaMode::Opaque;
        resolve_texture_alpha_mode(&mut face, TextureAlpha::Blend, true, false);
        assert_eq!(face.alpha_mode, AlphaMode::Opaque);
        // An alpha-free texture never upgrades.
        face.alpha_mode = AlphaMode::Opaque;
        resolve_texture_alpha_mode(&mut face, TextureAlpha::Mask, false, false);
        assert_eq!(face.alpha_mode, AlphaMode::Opaque);
        // A tint-blending face is left blending.
        face.alpha_mode = AlphaMode::Blend;
        resolve_texture_alpha_mode(&mut face, TextureAlpha::Mask, true, false);
        assert_eq!(face.alpha_mode, AlphaMode::Blend);
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

    /// An opaque, alpha-free classification, so a drape only fills the diffuse and
    /// [`resolve_texture_alpha_mode`] is a no-op (its own resolution is covered by
    /// [`texture_alpha_resolution_upgrades_only_opaque_faces`]).
    const OPAQUE: DecodedAlpha = DecodedAlpha {
        has_alpha: false,
        has_transparency: false,
    };

    /// Add one untextured face material and return its handle.
    fn add_face(materials: &mut Assets<FaceMaterial>) -> Handle<FaceMaterial> {
        materials.add(inert_face_material(StandardMaterial::default()))
    }

    /// Add `n` untextured face materials and return their handles.
    fn add_faces(materials: &mut Assets<FaceMaterial>, n: usize) -> Vec<Handle<FaceMaterial>> {
        std::iter::repeat_with(|| add_face(materials))
            .take(n)
            .collect()
    }

    /// A deferred drape of `image` onto `material` with the opaque classification.
    fn deferred(material: Handle<FaceMaterial>, image: Handle<Image>) -> DeferredFaceTexture {
        DeferredFaceTexture {
            material,
            image,
            texture_alpha: TextureAlpha::Mask,
            alpha: OPAQUE,
        }
    }

    /// How many of `handles` now carry a diffuse texture.
    fn textured(materials: &Assets<FaceMaterial>, handles: &[Handle<FaceMaterial>]) -> usize {
        handles
            .iter()
            .filter(|handle| {
                materials
                    .get(*handle)
                    .is_some_and(|material| material.base.base_color_texture.is_some())
            })
            .count()
    }

    /// The overflow past a frame's re-prep budget is deferred, not applied: 5 parked
    /// faces with a budget of 2 texture 2 now and queue 3 for later; draining then
    /// finishes the backlog at 2 per frame.
    #[test]
    fn parked_faces_over_budget_defer_and_drain_across_frames() {
        let mut materials = Assets::<FaceMaterial>::default();
        let legacy = LegacyMaterialManager::default();
        let image = Handle::<Image>::default();
        let faces = add_faces(&mut materials, 5);
        let parked = faces
            .iter()
            .map(|handle| (handle.clone(), TextureAlpha::Mask))
            .collect();
        let mut queue = VecDeque::new();

        // Frame 1 apply: budget 2 → 2 textured, 3 deferred.
        let mut budget = 2;
        drape_parked_faces(
            &mut materials,
            &legacy,
            &mut budget,
            &mut queue,
            &image,
            OPAQUE,
            parked,
        );
        assert_eq!(budget, 0, "the whole budget is spent");
        assert_eq!(queue.len(), 3, "the overflow is deferred");
        assert_eq!(textured(&materials, &faces), 2);

        // Frame 1 drain: nothing left in the budget, so the backlog is untouched.
        drain_deferred(&mut materials, &legacy, &mut budget, &mut queue);
        assert_eq!(queue.len(), 3);
        assert_eq!(textured(&materials, &faces), 2);

        // Frame 2 drain: budget refilled to 2 → 2 more textured, 1 remains.
        let mut budget = 2;
        drain_deferred(&mut materials, &legacy, &mut budget, &mut queue);
        assert_eq!(queue.len(), 1);
        assert_eq!(textured(&materials, &faces), 4);

        // Frame 3 drain: the last one textures, budget to spare.
        let mut budget = 2;
        drain_deferred(&mut materials, &legacy, &mut budget, &mut queue);
        assert!(queue.is_empty());
        assert_eq!(textured(&materials, &faces), 5);
        assert_eq!(budget, 1, "the drain stops when the backlog is empty");
    }

    /// A texture-apply budget with the given image-build allowance (the reprep side
    /// left generous, since these tests exercise the image gate).
    fn budget_with_image(image_remaining: usize) -> TextureApplyBudget {
        TextureApplyBudget {
            reprep_per_frame: 64,
            reprep_remaining: 64,
            image_per_frame: 8,
            image_remaining,
        }
    }

    /// The image-build gate: with budget spent and the image not yet built, the
    /// texture's faces are re-parked (deferred to a later frame); with budget left, or
    /// once the image is built, the caller proceeds and nothing is re-parked.
    #[test]
    fn image_build_gate_defers_only_when_over_budget_and_unbuilt() {
        let id = TextureKey::from(Uuid::from_u128(0x51_ace));
        let faces = || vec![(Handle::<FaceMaterial>::default(), TextureAlpha::Mask)];

        // Over budget, image not built → deferred (None), faces re-parked.
        let mut prim = PrimTextures::default();
        let spent = budget_with_image(0);
        assert!(
            reserve_image_build(&mut prim, &spent, id, faces()).is_none(),
            "over budget + unbuilt defers"
        );
        assert_eq!(
            prim.pending.get(&id).map(Vec::len),
            Some(1),
            "the faces were re-parked for a later frame"
        );

        // Budget available → proceeds (Some), nothing re-parked.
        let mut prim = PrimTextures::default();
        assert!(
            reserve_image_build(&mut prim, &budget_with_image(1), id, faces()).is_some(),
            "budget available → proceeds"
        );
        assert!(
            prim.pending.is_empty(),
            "nothing deferred when budget remains"
        );

        // Image already built → proceeds even with budget spent (a built image is free).
        let mut prim = PrimTextures::default();
        let _prev = prim.images.insert(id, Handle::<Image>::default());
        assert!(
            reserve_image_build(&mut prim, &spent, id, faces()).is_some(),
            "an already-built image is not gated"
        );
    }

    /// The LOD re-upload gate: with the image budget spent, a re-decode is deferred
    /// onto `pending_lod` (deduplicated); with budget available it refreshes now.
    #[test]
    fn lod_reupload_gate_defers_and_dedups_when_over_budget() {
        let id = TextureKey::from(Uuid::from_u128(0x10d));
        let mut prim = PrimTextures::default();

        // Budget available → not deferred, nothing queued.
        assert!(!defer_lod_reupload(&mut prim, &budget_with_image(1), id));
        assert!(prim.pending_lod.is_empty());

        // Budget spent → deferred, queued once.
        let spent = budget_with_image(0);
        assert!(defer_lod_reupload(&mut prim, &spent, id));
        assert_eq!(prim.pending_lod.len(), 1);

        // A repeat while still queued does not double-enqueue (the store keeps the
        // latest level, so one refresh suffices).
        assert!(defer_lod_reupload(&mut prim, &spent, id));
        assert_eq!(prim.pending_lod.len(), 1);
    }

    /// A deferred drape whose face despawned is dropped for free — it neither panics
    /// nor spends re-prep budget that a live face could have used.
    #[test]
    fn a_despawned_deferred_face_is_dropped_without_spending_budget() {
        let mut materials = Assets::<FaceMaterial>::default();
        let legacy = LegacyMaterialManager::default();
        let image = Handle::<Image>::default();
        let dead = add_face(&mut materials);
        let live = add_face(&mut materials);
        let mut queue = VecDeque::new();
        // The first queued face despawns before the drain reaches it.
        let _removed = materials.remove(dead.id());
        queue.push_back(deferred(dead, image.clone()));
        queue.push_back(deferred(live.clone(), image));

        let mut budget = 1;
        drain_deferred(&mut materials, &legacy, &mut budget, &mut queue);

        // The dead item was skipped without cost, so the live face still textured.
        assert_eq!(budget, 0);
        assert!(queue.is_empty());
        assert!(
            materials
                .get(&live)
                .is_some_and(|material| material.base.base_color_texture.is_some()),
            "the live face textured despite the dead item ahead of it"
        );
    }
}
