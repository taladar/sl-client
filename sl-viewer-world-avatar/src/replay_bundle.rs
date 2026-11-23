//! The avatar-state **replay bundle** format (viewer-avatar-state-dump-replay),
//! shared by the capture side ([`crate::avatar_dump`]) and the replay loader
//! ([`crate::avatar_replay`]).
//!
//! A bundle is a directory holding, per captured avatar, a `<agent>.json`
//! manifest of the *raw session events* needed to rebuild it — the avatar object
//! and its whole attachment tree (verbatim wire [`Object`]s), the decoded
//! [`AvatarAppearance`] (visual params + baked-texture entry), and the list of
//! currently-playing animations — plus a shared `cache/` subtree laid out as a
//! **drop-in asset cache** (`cache/<kind>/<first-char>/<uuid>.<ext>`) holding
//! every asset those events reference: meshes / animations / material assets
//! copied verbatim out of the viewer's live on-disk caches, and textures
//! **fetched at full resolution** from the live capabilities (the local cache
//! holds only the low-LOD prefix the viewer loaded — see `run_bundle_fetch`, which
//! also fetches meshes whole and whatever else the caches lack, and whose caller
//! reports anything a capture could not obtain).
//!
//! Replay re-emits the manifest's events as synthetic [`SlEvent`](sl_client_bevy::SlEvent)s
//! and points the asset stores at the bundle's `cache/`, so the viewer's *live*
//! render pipeline redraws the avatar with no grid — which is the point: a
//! render-only defect (a mesh-head brow spike, a blown-out facelight, missing
//! hair) can be reproduced offline after the avatar has logged out, and a
//! rendering **fix** can be tested against the same captured inputs.
//!
//! **Bundles are strictly local, ephemeral and NEVER committed or shared** — they
//! carry other residents' actual mesh/texture assets (creator permissions) and
//! appearance (privacy). The output directory has no in-repo default and a
//! `.gitignore` guard (`/avatar-dumps/`, `*.avatardump/`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sl_client_bevy::{
    AssetBytes, AssetCacheLimits, AssetDiskCache, AssetType, AvatarAppearance, CAP_GET_MESH,
    CAP_GET_MESH2, CAP_GET_TEXTURE, CAP_RENDER_MATERIALS, CAP_VIEWER_ASSET, MAX_FACES,
    MeshCacheLimits, MeshDiskCache, Object, PlayingAnimation, RenderMaterialEntry, SculptOrMeshKey,
    Uuid, avatar_texture, decode_texture_entry, parse_material_asset, pcode,
};
use sl_texture::{CacheLimits as TextureCacheLimits, TextureDiskCache};
use sl_viewer_platform::paths::ViewerPaths;

use crate::avatars::bake_service_slot_name;

/// The current manifest schema version. Bumped when the manifest's shape changes
/// incompatibly, so a loader can reject a bundle it cannot read.
pub(crate) const MANIFEST_VERSION: u32 = 2;

/// The bundle subdirectory holding the drop-in asset cache.
pub const CACHE_SUBDIR: &str = "cache";

/// The mesh-cache kind (a subdir of both the live cache and the bundle `cache/`).
pub(crate) const MESH_CACHE: &str = "meshcache";
/// The texture-cache kind.
pub(crate) const TEXTURE_CACHE: &str = "texturecache";
/// The animation-cache kind (a generic [`sl_asset`] store on disk).
pub(crate) const ANIM_CACHE: &str = "animcache";
/// The PBR render-material (`AT_MATERIAL`) cache kind (a generic [`sl_asset`]
/// store on disk).
pub(crate) const MATERIAL_CACHE: &str = "materialcache";

/// The most legacy material ids one capture request carries: the limit Second
/// Life and stock OpenSim both advertise as `MaxMaterialsPerTransaction`.
const MATERIALS_PER_FETCH: usize = 50;
/// How many times a [`RetryPolicy::Brief`] capture asks again after a failure.
const BRIEF_RETRIES: u8 = 5;
/// The pause between a [`RetryPolicy::Brief`] capture's attempts, in milliseconds.
const BRIEF_DELAY_MS: u64 = 300;
/// The pauses of a [`RetryPolicy::Patient`] capture, in milliseconds: about a
/// minute in all, which outlasts a service that is only busy.
const PATIENT_DELAYS_MS: [u64; 7] = [500, 1_000, 2_000, 4_000, 8_000, 16_000, 30_000];
/// The longest a service's own `Retry-After` is honoured for.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);
/// How often a retry's pause looks for a cancellation.
const CANCEL_POLL: Duration = Duration::from_millis(50);
/// The error of a download given up because the capture was cancelled. Those
/// assets are reported as missing like any other, without a warning each.
const CANCELLED: &str = "cancelled";
/// How many downloads a capture runs at once. A whole region references
/// thousands of assets, which one connection would take hours over.
const FETCH_WORKERS: usize = 8;

/// One captured avatar's full render inputs, serialised as `<agent>.json`.
///
/// Everything here is a verbatim, `serde`-round-tripping copy of the wire types
/// the session decoded, so replay can re-emit them as [`SlEvent`](sl_client_bevy::SlEvent)s
/// and let the normal render systems derive bakes / visibility / attachments —
/// exactly as a live login would.
#[derive(Debug, Serialize, Deserialize)]
pub struct ReplayManifest {
    /// The manifest schema version ([`MANIFEST_VERSION`]).
    pub(crate) version: u32,
    /// The captured avatar's agent id (also the file stem). No display name is
    /// stored — privacy, and the render does not need one.
    pub(crate) agent: Uuid,
    /// The avatar object followed by its whole attachment tree, as verbatim wire
    /// [`Object`]s (the avatar itself is the one with [`pcode::AVATAR`]).
    pub(crate) objects: Vec<Object>,
    /// The decoded [`AvatarAppearance`] (visual params + baked-texture entry),
    /// or `None` if none had arrived when the capture fired.
    pub(crate) appearance: Option<AvatarAppearance>,
    /// The animations the avatar was playing (asset ids + sequence numbers).
    pub(crate) animations: Vec<PlayingAnimation>,
    /// The legacy (`RenderMaterials`) `LLMaterial`s referenced by the avatar's
    /// faces, captured resolved from the live session so replay can re-emit them
    /// (the offline session cannot fetch the `RenderMaterials` cap). Defaults
    /// empty for a bundle written before this field existed.
    #[serde(default)]
    pub(crate) render_materials: Vec<RenderMaterialEntry>,
}

impl ReplayManifest {
    /// The avatar object among [`objects`](Self::objects) — the one whose class
    /// is [`pcode::AVATAR`] — if present.
    pub(crate) fn avatar_object(&self) -> Option<&Object> {
        self.objects
            .iter()
            .find(|object| object.pcode == pcode::AVATAR)
    }
}

/// The set of asset ids a manifest references, grouped by which cache they live
/// in, so the capture can bundle exactly those (meshes / animations / materials
/// copied from the live caches, textures fetched at full resolution).
#[derive(Default)]
pub(crate) struct ReferencedAssets {
    /// Rigged / sculpt-mesh asset ids (the mesh cache).
    pub(crate) meshes: BTreeSet<Uuid>,
    /// Diffuse / baked / sculpt-map / projector / material-map texture ids (the
    /// texture cache).
    pub(crate) textures: BTreeSet<Uuid>,
    /// Animation asset ids (the animation cache).
    pub(crate) anims: BTreeSet<Uuid>,
    /// PBR render-material (`AT_MATERIAL`) asset ids (the material cache).
    pub(crate) materials: BTreeSet<Uuid>,
}

/// Collect every asset id a manifest references, so the capture knows which
/// cache entries to bundle. Over-collection is harmless (a copy that finds no
/// source entry is skipped); under-collection leaves a hole that renders as a
/// cache miss on replay.
pub(crate) fn referenced_assets(manifest: &ReplayManifest) -> ReferencedAssets {
    let mut assets = ReferencedAssets::default();
    for animation in &manifest.animations {
        insert_non_nil(&mut assets.anims, animation.anim_id);
    }
    if let Some(appearance) = &manifest.appearance {
        for face in &appearance.texture_entry.faces {
            insert_non_nil(&mut assets.textures, face.texture_id.uuid());
        }
    }
    for object in &manifest.objects {
        collect_object_assets(object, &mut assets);
    }
    // Legacy `LLMaterial` normal / specular maps (the material params themselves
    // travel in the manifest, not a cache).
    for entry in &manifest.render_materials {
        insert_non_nil(&mut assets.textures, entry.material.normal_map.uuid());
        insert_non_nil(&mut assets.textures, entry.material.specular_map.uuid());
    }
    assets
}

/// Fold one object's referenced asset ids into `assets`.
pub(crate) fn collect_object_assets(object: &Object, assets: &mut ReferencedAssets) {
    // The object's sculpt/mesh: a mesh id lands in the mesh cache, a sculpt map
    // in the texture cache.
    if let Some(sculpt) = &object.extra.sculpt {
        match &sculpt.texture {
            SculptOrMeshKey::Mesh(mesh) => insert_non_nil(&mut assets.meshes, mesh.uuid()),
            SculptOrMeshKey::Sculpt(texture) => {
                insert_non_nil(&mut assets.textures, texture.uuid());
            }
        }
    }
    // A projected-light image is an ordinary texture.
    if let Some(light_image) = &object.extra.light_image {
        insert_non_nil(&mut assets.textures, light_image.texture.uuid());
    }
    // PBR (GLTF) render-material assets referenced per face.
    for material in &object.extra.render_material {
        insert_non_nil(&mut assets.materials, material.material_id);
    }
    // Every per-face diffuse texture. Decode at `MAX_FACES` so the default face
    // plus any per-face overrides are all captured (a few extra ids at worst).
    let entry = decode_texture_entry(&object.texture_entry, MAX_FACES);
    for face in &entry.faces {
        insert_non_nil(&mut assets.textures, face.texture_id.uuid());
    }
}

/// Insert `id` into `set` unless it is the nil UUID (no asset).
pub(crate) fn insert_non_nil(set: &mut BTreeSet<Uuid>, id: Uuid) {
    // The all-ones id is a sentinel too (`GLTF_OVERRIDE_NULL_UUID`), never an asset.
    if !id.is_nil() && !id.is_max() {
        let _inserted = set.insert(id);
    }
}

/// The tally of what the cache-copy step wrote, for the capture's log line.
/// Textures are fetched (not copied) and counted separately.
#[derive(Default)]
pub(crate) struct BundleCounts {
    /// Mesh cache entries copied.
    pub(crate) meshes: u32,
    /// Animation cache entries copied.
    pub(crate) anims: u32,
    /// PBR render-material cache entries copied.
    pub(crate) materials: u32,
}

/// Copy `manifest`'s referenced **mesh / animation / material** assets out of the
/// live caches into the bundle's `cache/` (a drop-in cache), and return, alongside
/// the copy tally, the full set of texture ids the avatar needs — including the
/// maps decoded out of its PBR materials. Textures are **not** copied here: the
/// local cache often holds only a low-LOD prefix, so they are fetched at full
/// resolution separately (`run_bundle_fetch`).
///
/// Best-effort: a missing source entry (evicted, or never fetched) is skipped,
/// not an error, and a viewer with no cache (`live` absent or storing nothing)
/// copies nothing. `now_unix` stamps the bundle entries for their own LRU
/// bookkeeping.
pub(crate) fn copy_cache_assets(
    live: Option<&ViewerPaths>,
    bundle_dir: &Path,
    manifest: &ReplayManifest,
    now_unix: u32,
) -> (BundleCounts, BTreeSet<Uuid>) {
    let mut assets = referenced_assets(manifest);
    let counts = copy_referenced(live, bundle_dir, &mut assets, now_unix);
    (counts, assets.textures)
}

/// Copy the meshes, animations and PBR materials of `assets` from the live caches
/// into the bundle, adding the texture maps those materials name to
/// `assets.textures`.
pub(crate) fn copy_referenced(
    live: Option<&ViewerPaths>,
    bundle_dir: &Path,
    assets: &mut ReferencedAssets,
    now_unix: u32,
) -> BundleCounts {
    // Copy the PBR material assets, expanding each into the texture maps it
    // references so they join the texture fetch set.
    let materials = copy_materials(
        live,
        bundle_dir,
        &assets.materials,
        &mut assets.textures,
        now_unix,
    );
    BundleCounts {
        meshes: copy_meshes(live, bundle_dir, &assets.meshes, now_unix),
        anims: copy_anims(live, bundle_dir, &assets.anims, now_unix),
        materials,
    }
}

/// Build the (texture id → full-resolution source URL) fetch list for a
/// manifest's `textures`: baked body textures route to the appearance service
/// (`<svc>texture/<agent>/<slot>/<id>`, which is where a central-baking grid
/// serves them — the `GetTexture` CDN rejects a baked id), everything else to the
/// `GetTexture` cap. A texture with no resolvable source is skipped.
pub(crate) fn texture_fetch_urls(
    manifest: &ReplayManifest,
    textures: &BTreeSet<Uuid>,
    get_texture_cap: Option<&str>,
    appearance_service: Option<&str>,
) -> Vec<(Uuid, String)> {
    let bake_urls = bake_fetch_urls(manifest, appearance_service);
    let mut plan = Vec::new();
    for &id in textures {
        if let Some(url) = bake_urls.get(&id) {
            plan.push((id, url.clone()));
        } else if let Some(cap) = get_texture_cap {
            plan.push((id, format!("{cap}/?texture_id={id}")));
        }
    }
    plan
}

/// The baked-body texture ids of `manifest`'s appearance and the appearance-service
/// URL that serves each.
pub(crate) fn bake_fetch_urls(
    manifest: &ReplayManifest,
    appearance_service: Option<&str>,
) -> HashMap<Uuid, String> {
    let mut bake_urls = HashMap::new();
    if let (Some(service), Some(appearance)) = (appearance_service, manifest.appearance.as_ref()) {
        for slot in 0..MAX_FACES {
            let Some(name) = bake_service_slot_name(slot) else {
                continue;
            };
            let Some(face) = appearance.texture_entry.faces.get(slot) else {
                continue;
            };
            if avatar_texture::is_bake_visible(face.texture_id) {
                let id = face.texture_id.uuid();
                let _previous = bake_urls.insert(
                    id,
                    format!("{service}texture/{}/{name}/{id}", manifest.agent),
                );
            }
        }
    }
    bake_urls
}

/// How hard a capture tries to get an asset from a struggling service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RetryPolicy {
    /// A few quick attempts, for a capture that holds up the frame.
    Brief,
    /// Exponential backoff, for one running in the background.
    Patient,
}

impl RetryPolicy {
    /// The pause before retry number `attempt` (from `0`), or `None` when the
    /// policy has given up.
    fn delay(self, attempt: u8) -> Option<Duration> {
        match self {
            Self::Brief => (attempt < BRIEF_RETRIES).then(|| Duration::from_millis(BRIEF_DELAY_MS)),
            Self::Patient => PATIENT_DELAYS_MS
                .get(usize::from(attempt))
                .map(|delay| Duration::from_millis(*delay)),
        }
    }
}

/// Whether an HTTP status says the service may answer differently later.
const fn is_transient(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504)
}

/// An HTTP client and how persistently it retries.
struct Downloader<'cancel> {
    /// The client.
    client: reqwest::blocking::Client,
    /// The retry policy.
    retry: RetryPolicy,
    /// Set to give up on everything not yet downloaded.
    cancel: &'cancel AtomicBool,
}

impl Downloader<'_> {
    /// Whether the capture has been cancelled.
    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Wait for `delay`, or until the capture is cancelled.
    fn pause(&self, delay: Duration) {
        let started = Instant::now();
        while started.elapsed() < delay && !self.is_cancelled() {
            std::thread::sleep(CANCEL_POLL.min(delay.saturating_sub(started.elapsed())));
        }
    }
}

/// The capability URLs a capture fetches from.
#[derive(Clone, Debug, Default)]
pub(crate) struct CaptureCaps {
    /// `GetTexture`.
    pub(crate) texture: Option<String>,
    /// `GetMesh2`, or `GetMesh` on a grid without it.
    pub(crate) mesh: Option<String>,
    /// `ViewerAsset`, which serves animations and PBR materials.
    pub(crate) viewer_asset: Option<String>,
    /// `RenderMaterials`, which serves legacy materials.
    pub(crate) render_materials: Option<String>,
}

impl CaptureCaps {
    /// Take the URLs a capture uses from a region's capability map.
    pub(crate) fn update(&mut self, map: &HashMap<String, String>) {
        if let Some(url) = map.get(CAP_GET_TEXTURE) {
            self.texture = Some(url.clone());
        }
        if let Some(url) = map.get(CAP_GET_MESH2).or_else(|| map.get(CAP_GET_MESH)) {
            self.mesh = Some(url.clone());
        }
        if let Some(url) = map.get(CAP_VIEWER_ASSET) {
            self.viewer_asset = Some(url.clone());
        }
        if let Some(url) = map.get(CAP_RENDER_MATERIALS) {
            self.render_materials = Some(url.clone());
        }
    }
}

/// What a capture still has to download, over every avatar in the bundle.
#[derive(Debug, Default)]
pub(crate) struct FetchPlan {
    /// Meshes, always fetched whole: a cached one may hold only the detail levels
    /// the live viewer happened to load.
    pub(crate) meshes: BTreeSet<Uuid>,
    /// Animations, fetched when the live cache did not have them.
    pub(crate) anims: BTreeSet<Uuid>,
    /// PBR materials, fetched when the live cache did not have them.
    pub(crate) materials: BTreeSet<Uuid>,
    /// Textures and the URL serving each at full resolution.
    pub(crate) textures: BTreeMap<Uuid, String>,
}

impl FetchPlan {
    /// Add the meshes, animations and PBR materials `manifest` references.
    pub(crate) fn add_assets(&mut self, manifest: &ReplayManifest) {
        let assets = referenced_assets(manifest);
        self.meshes.extend(assets.meshes);
        self.anims.extend(assets.anims);
        self.materials.extend(assets.materials);
    }
}

/// What a finished capture could not obtain, by kind. Replay renders each of
/// these wrongly or not at all, so a capture has to say so.
#[derive(Debug, Default)]
pub(crate) struct BundleGaps {
    /// Legacy materials no `RenderMaterials` reply carried.
    pub(crate) render_materials: BTreeSet<Uuid>,
    /// Meshes that could not be fetched whole (a partial cached copy may remain).
    pub(crate) meshes: BTreeSet<Uuid>,
    /// Animations neither cached nor fetchable.
    pub(crate) anims: BTreeSet<Uuid>,
    /// PBR materials neither cached nor fetchable.
    pub(crate) materials: BTreeSet<Uuid>,
    /// Textures that could not be fetched at full resolution.
    pub(crate) textures: BTreeSet<Uuid>,
}

impl BundleGaps {
    /// Each kind with something missing, as `(name, ids)`.
    pub(crate) fn by_kind(&self) -> Vec<(&'static str, &BTreeSet<Uuid>)> {
        [
            ("legacy material", &self.render_materials),
            ("mesh", &self.meshes),
            ("animation", &self.anims),
            ("PBR material", &self.materials),
            ("texture", &self.textures),
        ]
        .into_iter()
        .filter(|(_kind, ids)| !ids.is_empty())
        .collect()
    }
}

/// The ids of `referenced` that are not in `known`.
fn absent_ids(
    referenced: &BTreeSet<Uuid>,
    known: impl IntoIterator<Item = Uuid>,
) -> BTreeSet<Uuid> {
    let known: BTreeSet<Uuid> = known.into_iter().collect();
    referenced.difference(&known).copied().collect()
}

/// The legacy materials `manifest`'s faces reference and it does not carry.
pub(crate) fn absent_render_materials(manifest: &ReplayManifest) -> BTreeSet<Uuid> {
    let mut referenced = BTreeSet::new();
    for object in &manifest.objects {
        for face in &decode_texture_entry(&object.texture_entry, MAX_FACES).faces {
            if let Some(id) = face.material_id {
                insert_non_nil(&mut referenced, id);
            }
        }
    }
    absent_ids(
        &referenced,
        manifest
            .render_materials
            .iter()
            .map(|entry| entry.material_id),
    )
}

/// Add to `manifest` the entries of `fetched` its faces reference and it lacks.
pub(crate) fn add_render_materials(
    manifest: &mut ReplayManifest,
    fetched: &HashMap<Uuid, RenderMaterialEntry>,
) {
    for id in absent_render_materials(manifest) {
        if let Some(entry) = fetched.get(&id) {
            manifest.render_materials.push(entry.clone());
        }
    }
}

/// Fetch the legacy materials `ids` in batches the capability accepts. Blocking
/// HTTP, so off the main thread. A batch that fails is logged, and its materials
/// are reported as gaps by the caller.
pub(crate) fn run_render_material_fetch(
    cap: &str,
    ids: &BTreeSet<Uuid>,
) -> HashMap<Uuid, RenderMaterialEntry> {
    let ids: Vec<Uuid> = ids.iter().copied().collect();
    let mut fetched = HashMap::new();
    for batch in ids.chunks(MATERIALS_PER_FETCH) {
        match sl_client_bevy::fetch_render_materials(cap, batch) {
            Ok(entries) => fetched.extend(entries.into_iter().map(|e| (e.material_id, e))),
            Err(error) => {
                tracing::warn!(
                    "avatar dump: fetching {} legacy material(s) failed: {error}",
                    batch.len()
                );
            }
        }
    }
    fetched
}

/// Download everything in `plan` into the bundle's caches and return what could
/// not be obtained. Blocking HTTP, so off the main thread.
///
/// Textures go last, because a PBR material fetched here can name more of them.
/// This is what makes an offline replay faithful: a copy from the local cache
/// carries only what the live viewer happened to load.
pub(crate) fn run_bundle_fetch(
    bundle_dir: &Path,
    mut plan: FetchPlan,
    caps: &CaptureCaps,
    now_unix: u32,
    retry: RetryPolicy,
    cancel: &AtomicBool,
) -> BundleGaps {
    let client = match sl_client_bevy::http_proxy::blocking_client_builder()
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(client) => Downloader {
            client,
            retry,
            cancel,
        },
        Err(error) => {
            tracing::warn!("avatar dump: no HTTP client: {error}");
            return BundleGaps {
                render_materials: BTreeSet::new(),
                meshes: plan.meshes,
                anims: plan.anims,
                materials: plan.materials,
                textures: plan.textures.into_keys().collect(),
            };
        }
    };
    let meshes = fetch_meshes(
        &client,
        bundle_dir,
        &plan.meshes,
        caps.mesh.as_deref(),
        now_unix,
    );
    let (anims, _fresh) = fetch_absent_assets(
        &client,
        bundle_dir,
        (ANIM_CACHE, AssetType::Animation),
        &plan.anims,
        caps.viewer_asset.as_deref(),
        now_unix,
    );
    let (materials, fresh) = fetch_absent_assets(
        &client,
        bundle_dir,
        (MATERIAL_CACHE, AssetType::Material),
        &plan.materials,
        caps.viewer_asset.as_deref(),
        now_unix,
    );
    if let Some(cap) = caps.texture.as_deref() {
        for bytes in &fresh {
            for id in material_textures(bytes) {
                let _url = plan
                    .textures
                    .entry(id)
                    .or_insert_with(|| format!("{cap}/?texture_id={id}"));
            }
        }
    }
    BundleGaps {
        render_materials: BTreeSet::new(),
        meshes,
        anims,
        materials,
        textures: fetch_textures(&client, bundle_dir, &plan.textures, now_unix),
    }
}

/// Fetch each texture's complete codestream into the bundle, returning the ids
/// that failed.
fn fetch_textures(
    client: &Downloader<'_>,
    bundle_dir: &Path,
    plan: &BTreeMap<Uuid, String>,
    now_unix: u32,
) -> BTreeSet<Uuid> {
    let dest = match TextureDiskCache::open(
        bundle_cache_dir(bundle_dir, TEXTURE_CACHE),
        TextureCacheLimits {
            max_entries: usize::MAX,
            max_bytes: u64::MAX,
        },
    ) {
        Ok(dest) => dest,
        Err(error) => {
            tracing::warn!("avatar dump: cannot open the bundle's texture cache: {error}");
            return plan.keys().copied().collect();
        }
    };
    let items: Vec<(&Uuid, &String)> = plan.iter().collect();
    fetch_concurrently(&items, |(id, url)| {
        let stored = fetch_whole(client, url, "image/x-j2c").and_then(|bytes| {
            dest.write(**id, &bytes, now_unix)
                .map_err(|error| error.to_string())
        });
        stored.err().map(|error| {
            if error != CANCELLED {
                tracing::warn!("avatar dump: texture {id}: {error}");
            }
            **id
        })
    })
    .into_iter()
    .collect()
}

/// Run `fetch` over `items` on [`FETCH_WORKERS`] threads and collect what it
/// returns. A panic in a worker propagates to the caller, which then reports the
/// whole fetch as failed rather than silently losing an item.
fn fetch_concurrently<T: Sync, R: Send>(
    items: &[T],
    fetch: impl Fn(&T) -> Option<R> + Sync,
) -> Vec<R> {
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let workers: Vec<_> = std::iter::repeat_with(|| {
            scope.spawn(|| {
                let mut results = Vec::new();
                while let Some(item) = items.get(next.fetch_add(1, Ordering::Relaxed)) {
                    results.extend(fetch(item));
                }
                results
            })
        })
        .take(FETCH_WORKERS)
        .collect();
        workers
            .into_iter()
            .flat_map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    })
}

/// Fetch each mesh whole into the bundle, replacing any partial cached copy, and
/// return the ids that failed.
fn fetch_meshes(
    client: &Downloader<'_>,
    bundle_dir: &Path,
    ids: &BTreeSet<Uuid>,
    cap: Option<&str>,
    now_unix: u32,
) -> BTreeSet<Uuid> {
    if ids.is_empty() {
        return BTreeSet::new();
    }
    let Some(cap) = cap else {
        tracing::warn!("avatar dump: the region offers no mesh capability");
        return ids.clone();
    };
    let dest = match MeshDiskCache::open(
        bundle_cache_dir(bundle_dir, MESH_CACHE),
        unbounded_mesh_limits(),
    ) {
        Ok(dest) => dest,
        Err(error) => {
            tracing::warn!("avatar dump: cannot open the bundle's mesh cache: {error}");
            return ids.clone();
        }
    };
    let items: Vec<Uuid> = ids.iter().copied().collect();
    fetch_concurrently(&items, |&id| {
        let url = format!("{cap}/?mesh_id={id}");
        let stored = fetch_whole(client, &url, "application/vnd.ll.mesh").and_then(|bytes| {
            let asset = AssetBytes::from_whole(bytes).ok_or("no valid mesh header")?;
            dest.write(id, &asset, now_unix)
                .map_err(|error| error.to_string())
        });
        stored.err().map(|error| {
            if error != CANCELLED {
                tracing::warn!("avatar dump: mesh {id}: {error}");
            }
            id
        })
    })
    .into_iter()
    .collect()
}

/// Those of `ids` the bundle's texture cache does not hold. If the cache cannot be
/// opened, that is all of them.
pub(crate) fn textures_not_bundled(bundle_dir: &Path, ids: &BTreeSet<Uuid>) -> BTreeSet<Uuid> {
    let Ok(cache) = TextureDiskCache::open(
        bundle_cache_dir(bundle_dir, TEXTURE_CACHE),
        TextureCacheLimits {
            max_entries: usize::MAX,
            max_bytes: u64::MAX,
        },
    ) else {
        return ids.clone();
    };
    ids.iter()
        .copied()
        .filter(|id| cache.read(*id).is_none())
        .collect()
}

/// A bundle's mesh cache keeps everything: eviction would lose part of the capture.
const fn unbounded_mesh_limits() -> MeshCacheLimits {
    MeshCacheLimits {
        max_entries: usize::MAX,
        max_bytes: u64::MAX,
    }
}

/// A bundle's asset caches keep everything, as [`unbounded_mesh_limits`].
const fn unbounded_asset_limits() -> AssetCacheLimits {
    AssetCacheLimits {
        max_entries: usize::MAX,
        max_bytes: u64::MAX,
    }
}

/// Fetch the `ViewerAsset` assets of `ids` the bundle's `kind` cache lacks.
/// Returns the ids that failed and the bytes of those fetched.
fn fetch_absent_assets(
    client: &Downloader<'_>,
    bundle_dir: &Path,
    (kind, asset_type): (&str, AssetType),
    ids: &BTreeSet<Uuid>,
    cap: Option<&str>,
    now_unix: u32,
) -> (BTreeSet<Uuid>, Vec<Bytes>) {
    if ids.is_empty() {
        return (BTreeSet::new(), Vec::new());
    }
    let dest =
        match AssetDiskCache::open(bundle_cache_dir(bundle_dir, kind), unbounded_asset_limits()) {
            Ok(dest) => dest,
            Err(error) => {
                tracing::warn!("avatar dump: cannot open the bundle's {kind}: {error}");
                return (ids.clone(), Vec::new());
            }
        };
    let absent: Vec<Uuid> = ids
        .iter()
        .copied()
        .filter(|id| dest.read(*id).is_none())
        .collect();
    let (Some(cap), Some(key)) = (cap, asset_type.get_asset_query_key()) else {
        if !absent.is_empty() {
            tracing::warn!("avatar dump: no capability serves the {kind} assets still missing");
        }
        return (absent.into_iter().collect(), Vec::new());
    };
    let outcomes = fetch_concurrently(&absent, |&id| {
        let url = format!("{cap}/?{key}={id}");
        let stored = fetch_whole(client, &url, "application/octet-stream").and_then(|bytes| {
            dest.write(id, &bytes, now_unix)
                .map_err(|error| error.to_string())?;
            Ok(bytes)
        });
        Some(stored.map_err(|error| {
            if error != CANCELLED {
                tracing::warn!("avatar dump: {kind} {id}: {error}");
            }
            id
        }))
    });
    let mut failed = BTreeSet::new();
    let mut fresh = Vec::new();
    for outcome in outcomes {
        match outcome {
            Ok(bytes) => fresh.push(bytes),
            Err(id) => {
                let _inserted = failed.insert(id);
            }
        }
    }
    (failed, fresh)
}

/// GET the whole asset at `url`, retrying a transient failure as `downloader`'s
/// policy says and honouring a `Retry-After`. The error never quotes the URL,
/// which is a capability.
fn fetch_whole(downloader: &Downloader<'_>, url: &str, accept: &str) -> Result<Bytes, String> {
    let mut attempt = 0_u8;
    loop {
        if downloader.is_cancelled() {
            return Err(CANCELLED.to_owned());
        }
        let (failure, retry_after) =
            match downloader.client.get(url).header("Accept", accept).send() {
                Ok(response) if response.status().is_success() => match response.bytes() {
                    Ok(bytes) => return Ok(bytes),
                    Err(error) => (error.without_url().to_string(), None),
                },
                Ok(response) => {
                    let status = response.status();
                    if !is_transient(status.as_u16()) {
                        return Err(format!("status {status}"));
                    }
                    (format!("status {status}"), retry_after(&response))
                }
                Err(error) => (error.without_url().to_string(), None),
            };
        let Some(delay) = downloader.retry.delay(attempt) else {
            return Err(format!("{failure}, after {attempt} retries"));
        };
        downloader.pause(retry_after.map_or(delay, |asked| asked.max(delay)));
        attempt = attempt.saturating_add(1);
    }
}

/// The pause a response's `Retry-After` asks for in seconds, within
/// [`MAX_RETRY_AFTER`]. The HTTP-date form is not used by these services.
fn retry_after(response: &reqwest::blocking::Response) -> Option<Duration> {
    let seconds: u64 = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(seconds).min(MAX_RETRY_AFTER))
}

/// The texture maps a PBR material asset names.
fn material_textures(bytes: &[u8]) -> Vec<Uuid> {
    let Ok(material) = parse_material_asset(bytes) else {
        return Vec::new();
    };
    [
        material.base_color_texture,
        material.metallic_roughness_texture,
        material.normal_texture,
        material.emissive_texture,
    ]
    .into_iter()
    .flatten()
    .map(|texture| texture.id.uuid())
    .filter(|id| !id.is_nil())
    .collect()
}

/// Copy the PBR render-material (`AT_MATERIAL`) assets for `ids` from the live
/// material cache into the bundle, and — decoding each — add the texture maps it
/// references to `textures` so they are bundled too. Returns the count copied.
fn copy_materials(
    live: Option<&ViewerPaths>,
    bundle_dir: &Path,
    ids: &BTreeSet<Uuid>,
    textures: &mut BTreeSet<Uuid>,
    now_unix: u32,
) -> u32 {
    if ids.is_empty() {
        return 0;
    }
    let Some(source_dir) = live_cache_dir(live, MATERIAL_CACHE) else {
        return 0;
    };
    let Ok(source) = AssetDiskCache::open(source_dir, AssetCacheLimits::default()) else {
        return 0;
    };
    let Ok(dest) = AssetDiskCache::open(
        bundle_cache_dir(bundle_dir, MATERIAL_CACHE),
        unbounded_asset_limits(),
    ) else {
        return 0;
    };
    let mut copied = 0_u32;
    for &id in ids {
        let Some(bytes) = source.read(id) else {
            continue;
        };
        // Enumerate the material's own texture maps so they are bundled.
        textures.extend(material_textures(bytes.as_ref()));
        if dest.write(id, bytes.as_ref(), now_unix).is_ok() {
            copied = copied.saturating_add(1);
        }
    }
    copied
}

/// The live cache directory for `kind`, if the viewer has a cache root.
fn live_cache_dir(live: Option<&ViewerPaths>, kind: &str) -> Option<PathBuf> {
    live?.live_asset_cache_dir(kind)
}

/// The bundle's cache directory for `kind` (`<bundle>/cache/<kind>`).
pub(crate) fn bundle_cache_dir(bundle_dir: &Path, kind: &str) -> PathBuf {
    bundle_dir.join(CACHE_SUBDIR).join(kind)
}

/// Copy the mesh-cache entries for `ids` from the live cache into the bundle.
fn copy_meshes(
    live: Option<&ViewerPaths>,
    bundle_dir: &Path,
    ids: &BTreeSet<Uuid>,
    now_unix: u32,
) -> u32 {
    if ids.is_empty() {
        return 0;
    }
    let Some(source_dir) = live_cache_dir(live, MESH_CACHE) else {
        return 0;
    };
    let Ok(source) = MeshDiskCache::open(source_dir, MeshCacheLimits::default()) else {
        return 0;
    };
    let Ok(dest) = MeshDiskCache::open(
        bundle_cache_dir(bundle_dir, MESH_CACHE),
        unbounded_mesh_limits(),
    ) else {
        return 0;
    };
    let mut copied = 0_u32;
    for &id in ids {
        let Some(bytes) = source.read(id) else {
            continue;
        };
        if dest.write(id, &bytes, now_unix).is_ok() {
            copied = copied.saturating_add(1);
        }
    }
    copied
}

/// Copy the animation-cache entries for `ids` from the live cache into the
/// bundle (a generic [`sl_asset`] store on disk).
fn copy_anims(
    live: Option<&ViewerPaths>,
    bundle_dir: &Path,
    ids: &BTreeSet<Uuid>,
    now_unix: u32,
) -> u32 {
    if ids.is_empty() {
        return 0;
    }
    let Some(source_dir) = live_cache_dir(live, ANIM_CACHE) else {
        return 0;
    };
    let Ok(source) = AssetDiskCache::open(source_dir, AssetCacheLimits::default()) else {
        return 0;
    };
    let Ok(dest) = AssetDiskCache::open(
        bundle_cache_dir(bundle_dir, ANIM_CACHE),
        unbounded_asset_limits(),
    ) else {
        return 0;
    };
    let mut copied = 0_u32;
    for &id in ids {
        let Some(bytes) = source.read(id) else {
            continue;
        };
        if dest.write(id, bytes.as_ref(), now_unix).is_ok() {
            copied = copied.saturating_add(1);
        }
    }
    copied
}

/// Load and validate a `<agent>.json` manifest from `path`.
///
/// # Errors
///
/// Returns an error string if the file cannot be read, is not valid JSON, or
/// carries an unsupported [`version`](ReplayManifest::version).
pub(crate) fn load_manifest(path: &Path) -> Result<ReplayManifest, String> {
    let bytes = fs_err::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let manifest: ReplayManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse {}: {error}", path.display()))?;
    if manifest.version != MANIFEST_VERSION {
        return Err(format!(
            "{}: manifest version {} is not supported (expected {MANIFEST_VERSION})",
            path.display(),
            manifest.version
        ));
    }
    Ok(manifest)
}

/// Load every `*.json` avatar manifest in `bundle_dir`, sorted by file name for
/// a stable order. Skips (and logs) any file that fails to parse.
///
/// # Errors
///
/// Returns the directory read failure when `bundle_dir` cannot be listed. A
/// manifest that fails to parse is skipped rather than failing the load, so a
/// half-written bundle still yields the avatars that are readable.
pub fn load_bundle(bundle_dir: &Path) -> Result<Vec<ReplayManifest>, String> {
    let entries = fs_err::read_dir(bundle_dir)
        .map_err(|error| format!("read dir {}: {error}", bundle_dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();
    let mut manifests = Vec::new();
    for path in paths {
        match load_manifest(&path) {
            Ok(manifest) => manifests.push(manifest),
            Err(error) => tracing::warn!("avatar replay: skipping {error}"),
        }
    }
    Ok(manifests)
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use pretty_assertions::assert_eq;
    use sl_client_bevy::Uuid;

    use super::{
        BRIEF_RETRIES, BundleGaps, CANCELLED, Downloader, MANIFEST_VERSION, PATIENT_DELAYS_MS,
        ReplayManifest, RetryPolicy, absent_ids, fetch_whole, is_transient, load_bundle,
        load_manifest, referenced_assets,
    };

    /// A patient capture backs off exponentially and eventually gives up.
    #[test]
    fn patient_retries_back_off_and_end() {
        let delays: Vec<u64> = (0..=u8::MAX)
            .map_while(|attempt| RetryPolicy::Patient.delay(attempt))
            .map(|delay| u64::try_from(delay.as_millis()).unwrap_or(u64::MAX))
            .collect();
        assert_eq!(delays, PATIENT_DELAYS_MS);
        assert!(delays.windows(2).all(|pair| pair.first() < pair.get(1)));
        assert_eq!(RetryPolicy::Brief.delay(BRIEF_RETRIES), None);
    }

    /// Only a status that can change is worth asking again for.
    #[test]
    fn only_server_side_failures_are_retried() {
        for status in [429, 500, 502, 503, 504] {
            assert!(is_transient(status), "{status}");
        }
        for status in [400, 403, 404, 410] {
            assert!(!is_transient(status), "{status}");
        }
    }

    /// A service that is busy at first still delivers the asset.
    #[test]
    fn a_busy_service_is_retried_until_it_answers() -> Result<(), String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let server = std::thread::spawn(move || {
            for reply in [
                "HTTP/1.1 503 Busy\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nasset",
            ] {
                let Ok((mut stream, _peer)) = listener.accept() else {
                    return;
                };
                let mut request = [0_u8; 1024];
                let _read = stream.read(&mut request);
                let _written = stream.write_all(reply.as_bytes());
            }
        });
        let cancel = AtomicBool::new(false);
        let downloader = Downloader {
            client: reqwest::blocking::Client::builder()
                .no_proxy()
                .build()
                .map_err(|error| error.to_string())?,
            retry: RetryPolicy::Brief,
            cancel: &cancel,
        };
        let bytes = fetch_whole(&downloader, &format!("http://127.0.0.1:{port}/"), "*/*")?;
        assert_eq!(bytes.as_ref(), b"asset");
        server.join().map_err(|_panic| "server panicked")?;
        Ok(())
    }

    /// A cancelled capture leaves a long backoff at once.
    #[test]
    fn cancelling_ends_a_retry_pause() -> Result<(), String> {
        let cancel = AtomicBool::new(false);
        let downloader = Downloader {
            client: reqwest::blocking::Client::builder()
                .no_proxy()
                .build()
                .map_err(|error| error.to_string())?,
            retry: RetryPolicy::Patient,
            cancel: &cancel,
        };
        let started = Instant::now();
        std::thread::scope(|scope| {
            let _canceller = scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(100));
                cancel.store(true, Ordering::Relaxed);
            });
            downloader.pause(Duration::from_secs(30));
        });
        assert!(started.elapsed() < Duration::from_secs(5));
        let refusal = fetch_whole(&downloader, "http://127.0.0.1:1/", "*/*").err();
        assert_eq!(refusal.as_deref(), Some(CANCELLED));
        Ok(())
    }

    /// What a capture still has to fetch is what is referenced and not held.
    #[test]
    fn only_the_ids_not_held_are_absent() {
        let (held, missing) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let referenced = std::collections::BTreeSet::from([held, missing]);
        assert_eq!(
            absent_ids(&referenced, [held, Uuid::from_u128(3)]),
            std::collections::BTreeSet::from([missing])
        );
    }

    /// A capture reports only the kinds it actually failed to complete.
    #[test]
    fn a_complete_bundle_reports_no_gaps() {
        let mut gaps = BundleGaps::default();
        assert!(gaps.by_kind().is_empty());
        let _inserted = gaps.meshes.insert(Uuid::from_u128(1));
        let kinds: Vec<&str> = gaps.by_kind().into_iter().map(|(kind, _)| kind).collect();
        assert_eq!(kinds, vec!["mesh"]);
    }

    /// A minimal (empty) manifest — no heavy wire objects — for exercising the
    /// serialise / load / version-check plumbing.
    fn empty_manifest(agent: Uuid) -> ReplayManifest {
        ReplayManifest {
            version: MANIFEST_VERSION,
            agent,
            objects: Vec::new(),
            appearance: None,
            animations: Vec::new(),
            render_materials: Vec::new(),
        }
    }

    /// A throwaway per-test directory under the system temp dir.
    fn temp_dir(tag: &str) -> Result<std::path::PathBuf, String> {
        let dir =
            std::env::temp_dir().join(format!("sl-replay-bundle-{tag}-{}", std::process::id()));
        let _removed = fs_err::remove_dir_all(&dir);
        fs_err::create_dir_all(&dir).map_err(|error| error.to_string())?;
        Ok(dir)
    }

    /// Serialise `manifest` to `path`, mapping any error to a string.
    fn write_manifest(path: &std::path::Path, manifest: &ReplayManifest) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(manifest).map_err(|error| error.to_string())?;
        fs_err::write(path, json).map_err(|error| error.to_string())
    }

    #[test]
    fn manifest_round_trips_through_json() -> Result<(), String> {
        let dir = temp_dir("roundtrip")?;
        let agent = Uuid::from_u128(0x1234_5678);
        let path = dir.join(format!("{agent}.json"));
        write_manifest(&path, &empty_manifest(agent))?;

        let loaded = load_manifest(&path)?;
        assert_eq!(loaded.version, MANIFEST_VERSION);
        assert_eq!(loaded.agent, agent);
        assert!(loaded.objects.is_empty());
        assert!(loaded.appearance.is_none());
        assert!(loaded.animations.is_empty());

        let _removed = fs_err::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn load_rejects_an_unsupported_version() -> Result<(), String> {
        let dir = temp_dir("version")?;
        let path = dir.join("bad.json");
        // A hand-rolled manifest carrying a future version.
        fs_err::write(
            &path,
            br#"{"version":999,"agent":"00000000-0000-0000-0000-000000000000","objects":[],"appearance":null,"animations":[]}"#,
        )
        .map_err(|error| error.to_string())?;
        let Err(_) = load_manifest(&path) else {
            return Err("an unsupported manifest version must be rejected".to_owned());
        };
        let _removed = fs_err::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn load_bundle_collects_every_manifest_and_skips_junk() -> Result<(), String> {
        let dir = temp_dir("bundle")?;
        for agent in [Uuid::from_u128(1), Uuid::from_u128(2)] {
            let path = dir.join(format!("{agent}.json"));
            write_manifest(&path, &empty_manifest(agent))?;
        }
        // A non-JSON file and a bad-version JSON are both skipped, not fatal.
        fs_err::write(dir.join("notes.txt"), b"ignore me").map_err(|error| error.to_string())?;
        fs_err::write(dir.join("stale.json"), br#"{"version":1}"#)
            .map_err(|error| error.to_string())?;

        let manifests = load_bundle(&dir)?;
        assert_eq!(manifests.len(), 2);
        let _removed = fs_err::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn referenced_assets_of_an_empty_manifest_is_empty() {
        let assets = referenced_assets(&empty_manifest(Uuid::nil()));
        assert!(assets.meshes.is_empty());
        assert!(assets.textures.is_empty());
        assert!(assets.anims.is_empty());
        assert!(assets.materials.is_empty());
    }
}
