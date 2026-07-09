//! The PBR (GLTF 2.0) render-material pipeline (P27.1): fetch each face's
//! `AT_MATERIAL` asset, decode it into a [`GltfMaterial`], and map it onto the
//! face's Bevy [`StandardMaterial`], sourcing each referenced texture through the
//! shared [`TextureManager`].
//!
//! A prim face references a base PBR material by asset id in its object's
//! `LLRenderMaterialParams` (`sl_proto::RenderMaterialRef`, decoded onto the
//! object's [`ObjectRenderMaterials`] holder). [`register_pbr_materials`] joins
//! each newly-spawned face to that holder to discover its material id and hand
//! the face's material handle to the [`MaterialManager`]. The manager fetches the
//! asset over the `ViewerAsset` capability (its own [`AssetStore`], like the
//! wearable / animation asset pipelines), decodes it with `sl_material`, and —
//! once parsed — patches the face material's PBR scalars (base colour, metallic /
//! roughness, emissive, alpha mode, double-sided) and requests its texture maps.
//! When a map decodes, [`apply_pbr_textures`] uploads it in the right colour
//! space (sRGB base colour / emissive, linear normal / metallic-roughness) and
//! drops it into the material's matching slot.
//!
//! Per-face GLTF material **overrides** (delivered over the material cap) are a
//! later phase (P27.2) layered on top of this base material.
//!
//! Mirrors the structure of [`AnimationManager`](crate::animations) /
//! [`WearableAssetManager`](crate::bake_inputs) for the fetch/decode/cache half.

use core::ops::Mul as _;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat};
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use sl_client_bevy::{
    AssetCacheLimits, AssetKey, AssetStore, AssetType, BevyAssetFetcher, BlobFetcher,
    CAP_VIEWER_ASSET, DecodedTexture, GltfAlphaMode, GltfMaterial, GltfTexture,
    GltfTextureTransform, Priority, SlCapabilities, TextureKey, Uuid, parse_material_asset,
};

use crate::objects::PrimFaceEntity;
use crate::render_priority::TERRAIN_BOOST_PRIORITY;
use crate::textures::TextureManager;

/// The fetch priority PBR material texture maps are requested at: a modest boost
/// (like a terrain detail texture), so a material's maps load at full resolution
/// rather than starved behind the pixel-area-ranked diffuse faces. They are not
/// pixel-area LOD managed — the render-priority driver ranks a face's *diffuse*
/// texture, not the material maps behind it.
const MATERIAL_TEXTURE_PRIORITY: Priority = TERRAIN_BOOST_PRIORITY;

/// The GLTF override-null texture sentinel (all-`f`), treated like the nil id as
/// "no texture" so it is neither fetched nor parked (mirrors the diffuse
/// pipeline's `GLTF_OVERRIDE_NULL_UUID`).
const GLTF_OVERRIDE_NULL_UUID: Uuid = Uuid::from_u128(u128::MAX);

/// Whether a texture id names an actual fetchable texture (not the nil id or the
/// override-null sentinel).
fn is_fetchable_texture(id: TextureKey) -> bool {
    let uuid = id.uuid();
    !uuid.is_nil() && uuid != GLTF_OVERRIDE_NULL_UUID
}

/// The per-face GLTF render-material asset references decoded from an object's
/// `LLRenderMaterialParams` (`sl_proto::RenderMaterialRef`), attached to the
/// object's **geometry holder** entity (the parent of its face entities) so
/// [`register_pbr_materials`] can look up a face's material id by its face index.
/// Present only on objects that carry at least one PBR material.
#[derive(Component, Debug, Clone, Default)]
pub(crate) struct ObjectRenderMaterials {
    /// `(face index, material asset id)` pairs, straight from the object's
    /// `render_material` extra-params block.
    pub(crate) faces: Vec<(u8, Uuid)>,
}

/// Which PBR texture slot of a [`StandardMaterial`] a fetched map fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PbrSlot {
    /// The base-colour (albedo) texture, sampled sRGB.
    BaseColor,
    /// The packed metallic-roughness (ORM) texture, sampled linear; also drives
    /// the occlusion slot (Bevy reads occlusion from its red channel).
    MetallicRoughness,
    /// The tangent-space normal map, sampled linear.
    Normal,
    /// The emissive texture, sampled sRGB.
    Emissive,
}

impl PbrSlot {
    /// Whether this slot's texture is sRGB-encoded (base colour / emissive) as
    /// opposed to linear (normal / metallic-roughness).
    const fn is_srgb(self) -> bool {
        matches!(self, Self::BaseColor | Self::Emissive)
    }
}

/// A pending patch of one PBR texture slot on one face material, waiting for the
/// texture to decode.
struct PbrTexturePatch {
    /// The face material to write the uploaded image into.
    material: Handle<StandardMaterial>,
    /// The slot the image fills.
    slot: PbrSlot,
}

/// The PBR material fetch/decode/apply pipeline: an [`AssetStore`] over the
/// `ViewerAsset` capability for `AT_MATERIAL` assets, the in-flight fetch tasks,
/// the decoded materials, and the bookkeeping tying face materials to the assets
/// and texture maps they wait on.
#[derive(Resource)]
pub(crate) struct MaterialManager {
    /// The generic-asset store doing the `ViewerAsset` fetch, dedupe, off-thread
    /// work, and on-disk caching of material asset bytes.
    store: AssetStore,
    /// The store's HTTP fetcher, kept so its `ViewerAsset` capability URL can be
    /// refreshed on a region change.
    fetcher: Arc<BevyAssetFetcher>,
    /// The background fetch+decode task per material id, polled by
    /// [`poll_materials`]; presence means "already being resolved".
    inflight: HashMap<AssetKey, Task<Option<GltfMaterial>>>,
    /// Successfully decoded materials by id, shared across every face using the
    /// material so it is fetched and decoded once.
    decoded: HashMap<AssetKey, GltfMaterial>,
    /// Face materials parked on a material id, patched once its asset decodes.
    pending: HashMap<AssetKey, Vec<Handle<StandardMaterial>>>,
    /// Material ids whose fetch / decode failed, so they are not retried forever
    /// (the parked faces keep their diffuse material).
    unavailable: HashSet<AssetKey>,
    /// Material ids requested before the region's `ViewerAsset` capability was
    /// known, held until [`retry_pending`](Self::retry_pending) re-issues them.
    pending_cap: HashSet<AssetKey>,
    /// Uploaded PBR-slot images by `(texture id, srgb)` — a texture used in two
    /// colour spaces (e.g. base colour on one material, a linear map on another)
    /// is uploaded once per space.
    images: HashMap<(TextureKey, bool), Handle<Image>>,
    /// Material-slot patches parked on a texture id, applied once it decodes.
    texture_pending: HashMap<TextureKey, Vec<PbrTexturePatch>>,
}

impl MaterialManager {
    /// Build the manager over a fresh [`BevyAssetFetcher`], backed by the on-disk
    /// asset cache when available (falling back to an in-memory-only store).
    pub(crate) fn new() -> Self {
        let fetcher = Arc::new(BevyAssetFetcher::new());
        let store = build_asset_store(&fetcher, material_cache_dir());
        Self {
            store,
            fetcher,
            inflight: HashMap::new(),
            decoded: HashMap::new(),
            pending: HashMap::new(),
            unavailable: HashSet::new(),
            pending_cap: HashSet::new(),
            images: HashMap::new(),
            texture_pending: HashMap::new(),
        }
    }

    /// Park face material `handle` on GLTF material `id` and ensure the asset is
    /// being fetched. Idempotent across the many faces that share a material.
    fn register(&mut self, id: AssetKey, handle: Handle<StandardMaterial>) {
        if id.uuid().is_nil() || self.unavailable.contains(&id) {
            return;
        }
        self.pending.entry(id).or_default().push(handle);
        self.request(id);
    }

    /// Spawn a background fetch+decode of material `id` if it is not already
    /// decoded, in flight, or known unavailable. Parked until the `ViewerAsset`
    /// cap is known if it is not (re-issued by [`retry_pending`](Self::retry_pending)).
    fn request(&mut self, id: AssetKey) {
        if self.decoded.contains_key(&id)
            || self.inflight.contains_key(&id)
            || self.unavailable.contains(&id)
        {
            return;
        }
        if !self.fetcher.has_cap_url() {
            let _inserted = self.pending_cap.insert(id);
            return;
        }
        let _removed = self.pending_cap.remove(&id);
        let store = self.store.clone();
        let task = IoTaskPool::get().spawn(async move {
            // Both the blocking HTTP fetch and the LLSD/glTF decode run on this
            // IoTaskPool thread, so the render thread never touches material bytes.
            match store.get(id, AssetType::Material).await {
                Ok(entry) => match entry.data() {
                    Some(bytes) => match parse_material_asset(&bytes) {
                        Ok(material) => Some(material),
                        Err(error) => {
                            warn!("decoding material {}: {error}", id.uuid());
                            None
                        }
                    },
                    None => None,
                },
                Err(_error) => None,
            }
        });
        let _prev = self.inflight.insert(id, task);
    }

    /// Point the store's fetcher at the region's current `ViewerAsset` URL.
    fn set_cap_url(&self, url: Option<String>) {
        self.fetcher.set_cap_url(url);
    }

    /// Re-issue any material fetches parked before the `ViewerAsset` capability
    /// was known. A no-op while the cap is unset or nothing is parked.
    fn retry_pending(&mut self) {
        if self.pending_cap.is_empty() || !self.fetcher.has_cap_url() {
            return;
        }
        let pending: Vec<AssetKey> = self.pending_cap.drain().collect();
        for id in pending {
            self.request(id);
        }
    }

    /// The uploaded PBR-slot [`Image`] for `id` in the requested colour space,
    /// uploading it from `decoded` on first use and caching it.
    fn slot_image(
        &mut self,
        images: &mut Assets<Image>,
        id: TextureKey,
        srgb: bool,
        decoded: &Arc<DecodedTexture>,
    ) -> Handle<Image> {
        if let Some(handle) = self.images.get(&(id, srgb)) {
            return handle.clone();
        }
        let handle = images.add(build_pbr_image(decoded, srgb));
        let _inserted = self.images.insert((id, srgb), handle.clone());
        handle
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
            Err(error) => warn!("material disk cache unavailable ({error}); in-memory only"),
        }
    }
    // The disk-less store cannot fail to open; the loop extracts it without an
    // `unwrap`/`expect` and runs exactly once.
    loop {
        match AssetStore::new(Arc::clone(&fetcher), None, AssetCacheLimits::default()) {
            Ok(store) => return store,
            Err(error) => warn!("in-memory material store failed to open ({error}); retrying"),
        }
    }
}

/// The viewer's on-disk material-asset cache directory
/// (`<cache>/sl-client-bevy-viewer/materialcache`), from `XDG_CACHE_HOME` or
/// `~/.cache`, or `None` when neither is set (the store then runs in-memory only).
fn material_cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(base.join("sl-client-bevy-viewer").join("materialcache"))
}

/// Build a Bevy [`Image`] for a PBR material texture map from decoded RGBA8
/// pixels, in the colour space its slot needs (`Rgba8UnormSrgb` for base colour /
/// emissive, `Rgba8Unorm` for the linear normal / metallic-roughness maps) and
/// with the repeating sampler object faces tile their textures with.
fn build_pbr_image(decoded: &Arc<DecodedTexture>, srgb: bool) -> Image {
    let format = if srgb {
        TextureFormat::Rgba8UnormSrgb
    } else {
        TextureFormat::Rgba8Unorm
    };
    let mut image = Image::new(
        Extent3d {
            width: decoded.width,
            height: decoded.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        decoded.pixels.to_vec(),
        format,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// Refresh the material store fetcher's `ViewerAsset` capability URL each time the
/// region's capability map is (re)discovered, then re-issue any parked fetches.
pub(crate) fn update_material_caps(
    mut capabilities: MessageReader<SlCapabilities>,
    mut manager: ResMut<MaterialManager>,
) {
    for SlCapabilities(map) in capabilities.read() {
        manager.set_cap_url(map.get(CAP_VIEWER_ASSET).cloned());
    }
    manager.retry_pending();
}

/// Join each newly-spawned face entity to its object's [`ObjectRenderMaterials`]
/// holder (its geometry-holder parent), and, when the face's index carries a PBR
/// material, hand the face's material handle to the [`MaterialManager`] to fetch
/// and apply (P27.1). A face with no PBR material keeps its diffuse material.
pub(crate) fn register_pbr_materials(
    mut manager: ResMut<MaterialManager>,
    new_faces: Query<
        (&MeshMaterial3d<StandardMaterial>, &PrimFaceEntity, &ChildOf),
        Added<PrimFaceEntity>,
    >,
    holders: Query<&ObjectRenderMaterials>,
) {
    for (material, face, child_of) in &new_faces {
        let Ok(holder) = holders.get(child_of.parent()) else {
            continue;
        };
        let face_index = face.face_id.as_usize();
        let Some(&(_face, material_id)) = holder
            .faces
            .iter()
            .find(|(index, _id)| usize::from(*index) == face_index)
        else {
            continue;
        };
        manager.register(AssetKey::from(material_id), material.0.clone());
    }
}

/// Poll the in-flight material fetches; fold each result into the decoded cache
/// (or mark it unavailable), then patch every face parked on a now-decoded
/// material: set its PBR scalars and request its texture maps.
pub(crate) fn poll_materials(
    mut manager: ResMut<MaterialManager>,
    mut textures: ResMut<TextureManager>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Collect finished ids first — the borrow of the task map cannot overlap the
    // mutation of the decoded / unavailable maps.
    let mut finished: Vec<(AssetKey, Option<GltfMaterial>)> = Vec::new();
    for (&id, task) in &mut manager.inflight {
        if let Some(result) = block_on(poll_once(task)) {
            finished.push((id, result));
        }
    }
    for (id, result) in finished {
        let _removed = manager.inflight.remove(&id);
        match result {
            Some(material) => {
                let _prev = manager.decoded.insert(id, material);
            }
            None => {
                let _inserted = manager.unavailable.insert(id);
                let _dropped = manager.pending.remove(&id);
            }
        }
    }

    // Patch every parked face whose material has decoded. `GltfMaterial` is `Copy`,
    // so it can be lifted out before the parked handles borrow the manager again.
    let ready: Vec<AssetKey> = manager
        .pending
        .keys()
        .filter(|id| manager.decoded.contains_key(id))
        .copied()
        .collect();
    for id in ready {
        let Some(material) = manager.decoded.get(&id).copied() else {
            continue;
        };
        let handles = manager.pending.remove(&id).unwrap_or_default();
        for handle in handles {
            apply_material_scalars(&mut materials, &handle, &material);
            request_material_textures(&mut manager, &mut textures, &handle, &material);
        }
    }
}

/// Write a decoded [`GltfMaterial`]'s scalar / factor fields onto a face's
/// [`StandardMaterial`]: base colour (linear factor), metallic / roughness,
/// emissive, alpha mode + cutoff, and the double-sided / cull-mode pair. The
/// texture maps are filled in later by [`apply_pbr_textures`].
fn apply_material_scalars(
    materials: &mut Assets<StandardMaterial>,
    handle: &Handle<StandardMaterial>,
    material: &GltfMaterial,
) {
    let Some(mut standard) = materials.get_mut(handle) else {
        return;
    };
    let [r, g, b, a] = material.base_color;
    standard.base_color = Color::linear_rgba(r, g, b, a);
    standard.metallic = material.metallic_factor;
    standard.perceptual_roughness = material.roughness_factor;
    let [er, eg, eb] = material.emissive_factor;
    standard.emissive = LinearRgba::rgb(er, eg, eb);
    standard.double_sided = material.double_sided;
    standard.cull_mode = if material.double_sided {
        None
    } else {
        Some(Face::Back)
    };
    standard.alpha_mode = match material.alpha_mode {
        GltfAlphaMode::Opaque => AlphaMode::Opaque,
        GltfAlphaMode::Mask => AlphaMode::Mask(material.alpha_cutoff),
        GltfAlphaMode::Blend => AlphaMode::Blend,
    };
    // Compose the base-colour texture's `KHR_texture_transform` onto the face's
    // existing (texture-entry) UV placement. Bevy carries a single `uv_transform`
    // for all maps, so the base-colour transform stands in for every slot (a
    // documented approximation of the reference's per-slot transforms). `Mul::mul`
    // (a method, not the `*` operator) keeps clear of the workspace
    // `arithmetic_side_effects` lint the glam operators trip.
    if let Some(texture) = material.base_color_texture {
        standard.uv_transform = standard
            .uv_transform
            .mul(gltf_uv_transform(&texture.transform));
    }
}

/// Request each of a decoded material's texture maps through the shared texture
/// pipeline and park the corresponding slot patch on it, so [`apply_pbr_textures`]
/// fills the slot once the map decodes.
fn request_material_textures(
    manager: &mut MaterialManager,
    textures: &mut TextureManager,
    handle: &Handle<StandardMaterial>,
    material: &GltfMaterial,
) {
    for (slot, texture) in [
        (PbrSlot::BaseColor, material.base_color_texture),
        (
            PbrSlot::MetallicRoughness,
            material.metallic_roughness_texture,
        ),
        (PbrSlot::Normal, material.normal_texture),
        (PbrSlot::Emissive, material.emissive_texture),
    ] {
        let Some(GltfTexture { id, .. }) = texture else {
            continue;
        };
        if !is_fetchable_texture(id) {
            continue;
        }
        textures.request_boosted(id, MATERIAL_TEXTURE_PRIORITY);
        manager
            .texture_pending
            .entry(id)
            .or_default()
            .push(PbrTexturePatch {
                material: handle.clone(),
                slot,
            });
    }
}

/// Fill each decoded PBR material texture into the face-material slots parked on
/// it: upload the map in its slot's colour space and drop it into the matching
/// [`StandardMaterial`] slot. Drains parked patches for any texture that has
/// decoded (whether freshly or already cached), so it needs no decode message.
pub(crate) fn apply_pbr_textures(
    mut manager: ResMut<MaterialManager>,
    textures: Res<TextureManager>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let ready: Vec<TextureKey> = manager
        .texture_pending
        .keys()
        .filter(|id| textures.decoded(**id).is_some())
        .copied()
        .collect();
    for id in ready {
        let Some(decoded) = textures.decoded(id).map(Arc::clone) else {
            continue;
        };
        let patches = manager.texture_pending.remove(&id).unwrap_or_default();
        for patch in patches {
            let image = manager.slot_image(&mut images, id, patch.slot.is_srgb(), &decoded);
            let Some(mut standard) = materials.get_mut(&patch.material) else {
                continue;
            };
            match patch.slot {
                PbrSlot::BaseColor => standard.base_color_texture = Some(image),
                PbrSlot::MetallicRoughness => {
                    // Second Life packs occlusion into the metallic-roughness
                    // map's red channel (the ORM convention); Bevy samples the
                    // occlusion slot's red and the metallic-roughness slot's
                    // green/blue, so the one image drives both.
                    standard.metallic_roughness_texture = Some(image.clone());
                    standard.occlusion_texture = Some(image);
                }
                PbrSlot::Normal => standard.normal_map_texture = Some(image),
                PbrSlot::Emissive => standard.emissive_texture = Some(image),
            }
        }
    }
}

/// Convert a GLTF `KHR_texture_transform` into a Bevy UV [`Affine2`]. The
/// identity transform (no extension) maps to the identity affine, so composing it
/// is a no-op.
fn gltf_uv_transform(transform: &GltfTextureTransform) -> Affine2 {
    Affine2::from_scale_angle_translation(
        Vec2::new(transform.scale[0], transform.scale[1]),
        transform.rotation,
        Vec2::new(transform.offset[0], transform.offset[1]),
    )
}
