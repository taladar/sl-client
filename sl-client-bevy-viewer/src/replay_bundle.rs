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
//! holds only the low-LOD prefix the viewer loaded — see [`run_texture_fetch`]).
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

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sl_client_bevy::{
    AssetCacheLimits, AssetDiskCache, AvatarAppearance, MAX_FACES, MeshCacheLimits, MeshDiskCache,
    Object, PlayingAnimation, RenderMaterialEntry, SculptOrMeshKey, Uuid, avatar_texture,
    decode_texture_entry, parse_material_asset, pcode,
};
use sl_texture::{CacheLimits as TextureCacheLimits, TextureDiskCache};

use crate::avatars::bake_service_slot_name;

/// The current manifest schema version. Bumped when the manifest's shape changes
/// incompatibly, so a loader can reject a bundle it cannot read.
pub(crate) const MANIFEST_VERSION: u32 = 2;

/// The bundle subdirectory holding the drop-in asset cache.
pub(crate) const CACHE_SUBDIR: &str = "cache";

/// The mesh-cache kind (a subdir of both the live cache and the bundle `cache/`).
pub(crate) const MESH_CACHE: &str = "meshcache";
/// The texture-cache kind.
pub(crate) const TEXTURE_CACHE: &str = "texturecache";
/// The animation-cache kind (a generic [`sl_asset`] store on disk).
pub(crate) const ANIM_CACHE: &str = "animcache";
/// The PBR render-material (`AT_MATERIAL`) cache kind (a generic [`sl_asset`]
/// store on disk).
pub(crate) const MATERIAL_CACHE: &str = "materialcache";

/// One captured avatar's full render inputs, serialised as `<agent>.json`.
///
/// Everything here is a verbatim, `serde`-round-tripping copy of the wire types
/// the session decoded, so replay can re-emit them as [`SlEvent`](sl_client_bevy::SlEvent)s
/// and let the normal render systems derive bakes / visibility / attachments —
/// exactly as a live login would.
#[derive(Serialize, Deserialize)]
pub(crate) struct ReplayManifest {
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
fn collect_object_assets(object: &Object, assets: &mut ReferencedAssets) {
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
fn insert_non_nil(set: &mut BTreeSet<Uuid>, id: Uuid) {
    if !id.is_nil() {
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
/// resolution separately ([`run_texture_fetch`]).
///
/// Best-effort: a missing source entry (evicted, or never fetched) is skipped,
/// not an error. `now_unix` stamps the bundle entries for their own LRU
/// bookkeeping.
pub(crate) fn copy_cache_assets(
    bundle_dir: &Path,
    manifest: &ReplayManifest,
    now_unix: u32,
) -> (BundleCounts, BTreeSet<Uuid>) {
    let mut assets = referenced_assets(manifest);
    // Copy the PBR material assets, expanding each into the texture maps it
    // references so they join the texture fetch set.
    let materials = copy_materials(
        bundle_dir,
        &assets.materials,
        &mut assets.textures,
        now_unix,
    );
    let counts = BundleCounts {
        meshes: copy_meshes(bundle_dir, &assets.meshes, now_unix),
        anims: copy_anims(bundle_dir, &assets.anims, now_unix),
        materials,
    };
    (counts, assets.textures)
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
    // The baked-body ids and the URL that serves each, from the appearance's
    // per-slot texture entry.
    let mut bake_urls: HashMap<Uuid, String> = HashMap::new();
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

/// Fetch each planned texture at **full resolution** and write the complete
/// codestream into the bundle's texture cache. Blocking HTTP — run it off the
/// main thread. Returns the count written.
///
/// This is what makes an offline replay show textures: a copy from the local
/// cache would carry only the LOD prefix the live viewer happened to load, which
/// the offline store then tries (and fails) to grow over the network.
pub(crate) fn run_texture_fetch(bundle_dir: &Path, plan: &[(Uuid, String)], now_unix: u32) -> u32 {
    if plan.is_empty() {
        return 0;
    }
    let Ok(dest) = TextureDiskCache::open(
        bundle_cache_dir(bundle_dir, TEXTURE_CACHE),
        TextureCacheLimits::default(),
    ) else {
        return 0;
    };
    let Ok(client) = sl_client_bevy::http_proxy::blocking_client_builder()
        .timeout(Duration::from_secs(30))
        .build()
    else {
        return 0;
    };
    let mut written = 0_u32;
    for (id, url) in plan {
        if let Some(bytes) = fetch_full_codestream(&client, url)
            && dest.write(*id, &bytes, now_unix).is_ok()
        {
            written = written.saturating_add(1);
        }
    }
    written
}

/// GET the whole J2C codestream at `url`, retrying the `GetTexture` service's
/// `503 "still baking / queued"` a few times. `None` on a hard failure (the
/// texture is then simply absent from the bundle).
fn fetch_full_codestream(client: &reqwest::blocking::Client, url: &str) -> Option<Vec<u8>> {
    for _attempt in 0..6_u8 {
        match client.get(url).header("Accept", "image/x-j2c").send() {
            Ok(response) if response.status().is_success() => {
                return response.bytes().ok().map(|bytes| bytes.to_vec());
            }
            Ok(response) if response.status().as_u16() == 503 => {
                std::thread::sleep(Duration::from_millis(300));
            }
            _other => return None,
        }
    }
    None
}

/// Copy the PBR render-material (`AT_MATERIAL`) assets for `ids` from the live
/// material cache into the bundle, and — decoding each — add the texture maps it
/// references to `textures` so they are bundled too. Returns the count copied.
fn copy_materials(
    bundle_dir: &Path,
    ids: &BTreeSet<Uuid>,
    textures: &mut BTreeSet<Uuid>,
    now_unix: u32,
) -> u32 {
    if ids.is_empty() {
        return 0;
    }
    let Some(source_dir) = live_cache_dir(MATERIAL_CACHE) else {
        return 0;
    };
    let Ok(source) = AssetDiskCache::open(source_dir, AssetCacheLimits::default()) else {
        return 0;
    };
    let Ok(dest) = AssetDiskCache::open(
        bundle_cache_dir(bundle_dir, MATERIAL_CACHE),
        AssetCacheLimits::default(),
    ) else {
        return 0;
    };
    let mut copied = 0_u32;
    for &id in ids {
        let Some(bytes) = source.read(id) else {
            continue;
        };
        // Enumerate the material's own texture maps so they are bundled.
        if let Ok(material) = parse_material_asset(bytes.as_ref()) {
            for texture in [
                material.base_color_texture,
                material.metallic_roughness_texture,
                material.normal_texture,
                material.emissive_texture,
            ]
            .into_iter()
            .flatten()
            {
                insert_non_nil(textures, texture.id.uuid());
            }
        }
        if dest.write(id, bytes.as_ref(), now_unix).is_ok() {
            copied = copied.saturating_add(1);
        }
    }
    copied
}

/// The live cache directory for `kind`, if the platform has a cache root.
fn live_cache_dir(kind: &str) -> Option<PathBuf> {
    crate::paths::asset_cache_dir(kind)
}

/// The bundle's cache directory for `kind` (`<bundle>/cache/<kind>`).
pub(crate) fn bundle_cache_dir(bundle_dir: &Path, kind: &str) -> PathBuf {
    bundle_dir.join(CACHE_SUBDIR).join(kind)
}

/// Copy the mesh-cache entries for `ids` from the live cache into the bundle.
fn copy_meshes(bundle_dir: &Path, ids: &BTreeSet<Uuid>, now_unix: u32) -> u32 {
    if ids.is_empty() {
        return 0;
    }
    let Some(source_dir) = live_cache_dir(MESH_CACHE) else {
        return 0;
    };
    let Ok(source) = MeshDiskCache::open(source_dir, MeshCacheLimits::default()) else {
        return 0;
    };
    let Ok(dest) = MeshDiskCache::open(
        bundle_cache_dir(bundle_dir, MESH_CACHE),
        MeshCacheLimits::default(),
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
fn copy_anims(bundle_dir: &Path, ids: &BTreeSet<Uuid>, now_unix: u32) -> u32 {
    if ids.is_empty() {
        return 0;
    }
    let Some(source_dir) = live_cache_dir(ANIM_CACHE) else {
        return 0;
    };
    let Ok(source) = AssetDiskCache::open(source_dir, AssetCacheLimits::default()) else {
        return 0;
    };
    let Ok(dest) = AssetDiskCache::open(
        bundle_cache_dir(bundle_dir, ANIM_CACHE),
        AssetCacheLimits::default(),
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
pub(crate) fn load_bundle(bundle_dir: &Path) -> Result<Vec<ReplayManifest>, String> {
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
    use pretty_assertions::assert_eq;
    use sl_client_bevy::Uuid;

    use super::{MANIFEST_VERSION, ReplayManifest, load_bundle, load_manifest, referenced_assets};

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
        assert!(load_manifest(&path).is_err());
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
