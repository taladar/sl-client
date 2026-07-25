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
//! Per-face GLTF material **overrides** (P27.2) — the sparse deltas the simulator
//! pushes in a GLTF material-override `GenericStreamingMessage` — are layered on
//! top of this base material by [`apply_material_overrides`]. Each registered face
//! is tracked by its scoped-object + face-index key so an override addressed to it
//! can be found and the face recomposed ([`recompose_face`]): the decoded base
//! material with the override folded on, re-applied to the face's
//! [`StandardMaterial`].
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
    GltfTextureTransform, MaterialOverride, Priority, ScopedObjectId, SlCapabilities, SlEvent,
    SlSessionEvent, TextureFace, TextureKey, Uuid, parse_material_asset, parse_material_override,
};

use crate::edit_selection::SelectionSet;
use crate::edit_texture::MatModeState;
use crate::edit_tool::EditToolState;
use crate::face_material::FaceMaterial;
use crate::objects::{FaceTextureDebug, ObjectState, PrimFaceEntity};
use crate::render_priority::TERRAIN_BOOST_PRIORITY;
use crate::textures::{PrimTextures, TextureAlpha, TextureManager, compose_face_material};

/// A face-material identity: the scoped object id and its Linden face index — the
/// key both a registered face material and an incoming per-face GLTF override
/// (P27.2) are addressed by.
type FaceKey = (ScopedObjectId, u8);

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
#[derive(Component, Debug, Clone)]
pub(crate) struct ObjectRenderMaterials {
    /// The scoped id of the object owning these faces — the key a per-face GLTF
    /// override (P27.2) is addressed by, so a registered face can be found again
    /// when its override arrives.
    pub(crate) scoped_id: ScopedObjectId,
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
    /// The four slots, in the order [`GltfMaterial`] carries their textures.
    const ALL: [Self; 4] = [
        Self::BaseColor,
        Self::MetallicRoughness,
        Self::Normal,
        Self::Emissive,
    ];

    /// Whether this slot's texture is sRGB-encoded (base colour / emissive) as
    /// opposed to linear (normal / metallic-roughness).
    const fn is_srgb(self) -> bool {
        matches!(self, Self::BaseColor | Self::Emissive)
    }

    /// This slot's texture reference on a decoded [`GltfMaterial`].
    const fn texture(self, material: &GltfMaterial) -> Option<GltfTexture> {
        match self {
            Self::BaseColor => material.base_color_texture,
            Self::MetallicRoughness => material.metallic_roughness_texture,
            Self::Normal => material.normal_texture,
            Self::Emissive => material.emissive_texture,
        }
    }

    /// The **fetchable** texture this slot names on `material`, or `None` when it
    /// names none (or a non-fetchable sentinel / nil id). `None` means "clear the
    /// slot": a PBR render material supersedes the face's Blinn-Phong layer, so a
    /// slot the material does not define shows the PBR factor alone, never a
    /// leftover legacy texture.
    fn fetchable_texture(self, material: &GltfMaterial) -> Option<TextureKey> {
        match self.texture(material) {
            Some(GltfTexture { id, .. }) if is_fetchable_texture(id) => Some(id),
            _ => None,
        }
    }

    /// Clear this slot's texture on a face [`StandardMaterial`] (the
    /// metallic-roughness slot also drives occlusion, so both are cleared).
    fn clear(self, standard: &mut StandardMaterial) {
        match self {
            Self::BaseColor => standard.base_color_texture = None,
            Self::MetallicRoughness => {
                standard.metallic_roughness_texture = None;
                standard.occlusion_texture = None;
            }
            Self::Normal => standard.normal_map_texture = None,
            Self::Emissive => standard.emissive_texture = None,
        }
    }
}

/// A pending patch of one PBR texture slot on one face material, waiting for the
/// texture to decode.
struct PbrTexturePatch {
    /// The face material to write the uploaded image into.
    material: Handle<FaceMaterial>,
    /// The slot the image fills.
    slot: PbrSlot,
}

/// A registered PBR face material: which base material asset feeds it, the
/// material handle to patch, and the face's own (texture-entry) UV placement to
/// recompose each material's `KHR_texture_transform` onto.
struct FaceSlot {
    /// The base GLTF material asset id this face renders (before any override).
    material_id: AssetKey,
    /// The face's [`StandardMaterial`] handle, re-patched on each recomposition.
    handle: Handle<FaceMaterial>,
    /// The face's diffuse (texture-entry) `uv_transform`, captured at registration
    /// before any material composition, so recomposition never double-applies the
    /// base-colour `KHR_texture_transform`.
    base_uv: Affine2,
}

/// The PBR material fetch/decode/apply pipeline: an [`AssetStore`] over the
/// `ViewerAsset` capability for `AT_MATERIAL` assets, the in-flight fetch tasks,
/// the decoded materials, and the bookkeeping tying face materials to the assets,
/// per-face overrides, and texture maps they wait on.
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
    /// Each registered PBR face by its scoped-object + face-index key, recomposed
    /// whenever its base material decodes or its override changes.
    face_slots: HashMap<FaceKey, FaceSlot>,
    /// Per-face GLTF material overrides (P27.2), layered on the base material at
    /// recomposition; absent for a face with no override.
    overrides: HashMap<FaceKey, MaterialOverride>,
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
    /// Face keys whose override was set locally by the build tool
    /// ([`apply_local_override`](Self::apply_local_override)) and still need a
    /// recompose — so an edit shows immediately, before the simulator echoes it.
    local_recompose: Vec<FaceKey>,
    /// Faces whose PBR material is currently **hidden** so they render their
    /// Blinn-Phong layer instead — the FIRE-35138 build-tool behaviour: a face in
    /// a selected linkset while the build tool's Texture tab is on the Blinn-Phong
    /// (Material) mode. A hidden face's PBR (re)composition is suppressed
    /// ([`recompose_face`] is a no-op for it) so a late-arriving material / override
    /// / texture map does not overwrite the Blinn-Phong preview; it is restored to
    /// PBR when it leaves the set. Maintained by [`apply_blinn_phong_hide`].
    hidden: HashSet<FaceKey>,
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
            face_slots: HashMap::new(),
            overrides: HashMap::new(),
            unavailable: HashSet::new(),
            pending_cap: HashSet::new(),
            images: HashMap::new(),
            texture_pending: HashMap::new(),
            local_recompose: Vec::new(),
            hidden: HashSet::new(),
        }
    }

    /// Apply a build-tool edit to a face's override locally — set (or clear) the
    /// override and queue the face for recompose — so the edit is visible at once
    /// (the swatch, and the prim once its base material has decoded) without
    /// waiting for the simulator's echo. The echo later re-applies the same
    /// override idempotently.
    pub(crate) fn apply_local_override(
        &mut self,
        scoped: ScopedObjectId,
        face: u8,
        over: &MaterialOverride,
    ) {
        let key = (scoped, face);
        if over.is_empty() {
            let _removed = self.overrides.remove(&key);
        } else {
            let _prev = self.overrides.insert(key, *over);
        }
        self.local_recompose.push(key);
    }

    /// Register a PBR face material (its base material id, handle, and the face's
    /// diffuse UV placement) and ensure the base asset is being fetched. Replaces
    /// any prior registration for the same face (an object re-tessellation).
    fn register(
        &mut self,
        key: FaceKey,
        id: AssetKey,
        handle: Handle<FaceMaterial>,
        base_uv: Affine2,
    ) {
        if id.uuid().is_nil() {
            return;
        }
        let _prev = self.face_slots.insert(
            key,
            FaceSlot {
                material_id: id,
                handle,
                base_uv,
            },
        );
        self.request(id);
    }

    /// Register or refresh a face's PBR base material on a **material (re)assignment
    /// to an existing prim** — its object's [`ObjectRenderMaterials`] changed
    /// without a re-tessellation (the build tool / an in-world retexture), so the
    /// face entity is not freshly spawned and [`register_pbr_materials`] never sees
    /// it. Updates the face's base material id, capturing its diffuse UV placement
    /// **only on first registration** so a later change never re-reads the
    /// already-composed handle and double-applies a `KHR_texture_transform`; ensures
    /// the asset is fetched. Returns whether the material actually changed, so the
    /// caller recomposes only then — a moving prim's per-update
    /// `ObjectRenderMaterials` refresh then costs a lookup, not a recomposition.
    fn refresh_face_material(
        &mut self,
        key: FaceKey,
        id: AssetKey,
        handle: &Handle<FaceMaterial>,
        base_uv: Affine2,
    ) -> bool {
        if id.uuid().is_nil() {
            return false;
        }
        if let Some(slot) = self.face_slots.get_mut(&key) {
            if slot.material_id == id {
                return false;
            }
            slot.material_id = id;
        } else {
            let _prev = self.face_slots.insert(
                key,
                FaceSlot {
                    material_id: id,
                    handle: handle.clone(),
                    base_uv,
                },
            );
        }
        self.request(id);
        true
    }

    /// **Live-preview** a render material on a face while the user browses the
    /// *Pick: Material* picker (the reference's `setGLTFRenderMaterial` preview,
    /// applied on each picker selection before OK): show `id` on the face now, with
    /// no wire send. A non-nil `id` composes it as the face's base PBR material
    /// (registering the face if it had none); the **nil** id — the picker's revert
    /// of a face that had no material — drops the registration and recomposes the
    /// face's Blinn-Phong layer, so cancelling the picker restores the prim.
    ///
    /// The captured diffuse `base_uv` is used only when the face is newly
    /// registered (a face already carrying a material keeps its stored placement),
    /// so re-previewing never double-applies a `KHR_texture_transform`. The eventual
    /// OK sends the assignment for real; the simulator's echo then reconciles
    /// idempotently ([`register_changed_render_materials`]).
    #[expect(
        clippy::too_many_arguments,
        reason = "the preview needs the face key + previewed id, the face's handle / diffuse UV / \
                  TextureEntry for either composition, and the three material resources it \
                  composes through"
    )]
    pub(crate) fn preview_face_material(
        &mut self,
        key: FaceKey,
        id: AssetKey,
        handle: &Handle<FaceMaterial>,
        base_uv: Affine2,
        texture_face: &TextureFace,
        textures: &mut TextureManager,
        prim_textures: &mut PrimTextures,
        materials: &mut Assets<FaceMaterial>,
    ) {
        if id.uuid().is_nil() {
            // Revert a face that had no material back to its Blinn-Phong layer.
            let _hidden = self.hidden.remove(&key);
            let _over = self.overrides.remove(&key);
            if self.face_slots.remove(&key).is_some() {
                for slot in PbrSlot::ALL {
                    drop_texture_patches(self, handle, slot);
                }
                compose_face_material(
                    handle,
                    texture_face,
                    materials,
                    textures,
                    prim_textures,
                    MATERIAL_TEXTURE_PRIORITY,
                    TextureAlpha::Mask,
                );
            }
            return;
        }
        match self.face_slots.get_mut(&key) {
            Some(slot) => {
                if slot.material_id == id {
                    return;
                }
                slot.material_id = id;
            }
            None => {
                let _prev = self.face_slots.insert(
                    key,
                    FaceSlot {
                        material_id: id,
                        handle: handle.clone(),
                        base_uv,
                    },
                );
            }
        }
        self.request(id);
        recompose_face(self, textures, materials, key);
    }

    /// The decoded GLTF material for `id`, if its `ViewerAsset` fetch/decode has
    /// succeeded — a read-only lookup for the Texture tab's PBR channel display
    /// ([`crate::edit_material`]).
    pub(crate) fn decoded_material(&self, id: AssetKey) -> Option<&GltfMaterial> {
        self.decoded.get(&id)
    }

    /// Ensure material asset `id` is being fetched + decoded so
    /// [`decoded_material`](Self::decoded_material) can later return it — the
    /// material-swatch **sphere preview** ([`crate::material_preview`]) uses this to
    /// resolve a material chosen by asset id (in the picker) into a
    /// [`GltfMaterial`]. Idempotent, and parked until the `ViewerAsset` cap is known.
    pub(crate) fn request_material(&mut self, id: AssetKey) {
        self.request(id);
    }

    /// Shade an **arbitrary** [`StandardMaterial`] (a material-preview sphere, not a
    /// prim face) with an effective [`GltfMaterial`]: write its scalar / factor
    /// fields and (re)request its texture maps into the shared texture-patch queue,
    /// which [`apply_pbr_textures`] fills as each map decodes. The preview sphere
    /// has no per-face UV placement, so the identity affine is used ([`base` and
    /// `effective` are the same material — the sphere shows the material itself, not
    /// a face's base + override composition, which the caller has already folded).
    pub(crate) fn apply_preview(
        &mut self,
        textures: &mut TextureManager,
        materials: &mut Assets<FaceMaterial>,
        handle: &Handle<FaceMaterial>,
        material: &GltfMaterial,
    ) {
        // Clear every slot first, dropping any parked patches still targeting this
        // handle from the previous material: `request_material_textures` clears a
        // slot the material does not name, but for a slot it *does* name it parks a
        // fresh patch without first dropping a stale one, so a reused preview sphere
        // would otherwise keep the prior material's map on a re-used slot.
        for slot in PbrSlot::ALL {
            drop_texture_patches(self, handle, slot);
            if let Some(mut standard) = materials.get_mut(handle) {
                slot.clear(&mut standard.base);
            }
        }
        apply_material_scalars(materials, handle, material, Affine2::IDENTITY);
        request_material_textures(self, textures, materials, handle, material);
    }

    /// The per-face GLTF override layered on the base material, if the face has
    /// one — the starting point the Texture tab's PBR transform editor amends
    /// before re-sending ([`crate::edit_material`]).
    pub(crate) fn face_override(
        &self,
        scoped: ScopedObjectId,
        face: u8,
    ) -> Option<MaterialOverride> {
        self.overrides.get(&(scoped, face)).copied()
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
    crate::paths::asset_cache_dir("materialcache")
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
/// material, register the face with the [`MaterialManager`] (keyed by its scoped
/// object id + face index) and recompose it — the base material (P27.1) plus any
/// override already received for the face (P27.2). A face with no PBR material
/// keeps its diffuse material.
pub(crate) fn register_pbr_materials(
    mut manager: ResMut<MaterialManager>,
    mut textures: ResMut<TextureManager>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    new_faces: Query<
        (&MeshMaterial3d<FaceMaterial>, &PrimFaceEntity, &ChildOf),
        Added<PrimFaceEntity>,
    >,
    holders: Query<&ObjectRenderMaterials>,
) {
    for (material, face, child_of) in &new_faces {
        let Ok(holder) = holders.get(child_of.parent()) else {
            continue;
        };
        let face_index = face.face_id.as_usize();
        let Some(&(face_id, material_id)) = holder
            .faces
            .iter()
            .find(|(index, _id)| usize::from(*index) == face_index)
        else {
            continue;
        };
        // The face's diffuse UV placement, captured before any material
        // composition so recomposition never double-applies a `KHR_texture_transform`.
        let base_uv = materials
            .get(&material.0)
            .map_or(Affine2::IDENTITY, |standard| standard.base.uv_transform);
        let key = (holder.scoped_id, face_id);
        manager.register(
            key,
            AssetKey::from(material_id),
            material.0.clone(),
            base_uv,
        );
        recompose_face(&mut manager, &mut textures, &mut materials, key);
    }
}

/// (Re)register a face's PBR material when its object's [`ObjectRenderMaterials`]
/// **changes** — a render material assigned to (or changed on) an *existing* prim
/// that does not re-tessellate its faces (the build tool's material assignment, an
/// in-world retexture). [`register_pbr_materials`] only sees freshly-**spawned**
/// faces (`Added<PrimFaceEntity>`), so without this a material dropped onto a prim
/// already in the scene would refresh the holder but never actually render — the
/// face stayed Blinn-Phong. Recomposes only the faces whose material genuinely
/// changed ([`refresh_face_material`](MaterialManager::refresh_face_material)
/// returns `false` for an unchanged echo), so a moving prim's per-update holder
/// refresh costs a lookup, not a recomposition.
pub(crate) fn register_changed_render_materials(
    mut manager: ResMut<MaterialManager>,
    mut textures: ResMut<TextureManager>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    changed: Query<(&ObjectRenderMaterials, &Children), Changed<ObjectRenderMaterials>>,
    faces: Query<(&MeshMaterial3d<FaceMaterial>, &PrimFaceEntity)>,
) {
    for (holder, children) in &changed {
        for child in children.iter() {
            let Ok((material, face)) = faces.get(child) else {
                continue;
            };
            let face_index = face.face_id.as_usize();
            let Some(&(face_id, material_id)) = holder
                .faces
                .iter()
                .find(|(index, _id)| usize::from(*index) == face_index)
            else {
                continue;
            };
            let key = (holder.scoped_id, face_id);
            let base_uv = materials
                .get(&material.0)
                .map_or(Affine2::IDENTITY, |standard| standard.base.uv_transform);
            if manager.refresh_face_material(key, AssetKey::from(material_id), &material.0, base_uv)
            {
                recompose_face(&mut manager, &mut textures, &mut materials, key);
            }
        }
    }
}

/// Poll the in-flight material fetches; fold each result into the decoded cache
/// (or mark it unavailable), then recompose every registered face whose base
/// material just decoded — applying its base scalars, any override, and its
/// texture maps.
pub(crate) fn poll_materials(
    mut manager: ResMut<MaterialManager>,
    mut textures: ResMut<TextureManager>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    // Collect finished ids first — the borrow of the task map cannot overlap the
    // mutation of the decoded / unavailable maps.
    let mut finished: Vec<(AssetKey, Option<GltfMaterial>)> = Vec::new();
    for (&id, task) in &mut manager.inflight {
        if let Some(result) = block_on(poll_once(task)) {
            finished.push((id, result));
        }
    }
    let mut newly_decoded: Vec<AssetKey> = Vec::new();
    for (id, result) in finished {
        let _removed = manager.inflight.remove(&id);
        match result {
            Some(material) => {
                let _prev = manager.decoded.insert(id, material);
                newly_decoded.push(id);
            }
            None => {
                let _inserted = manager.unavailable.insert(id);
            }
        }
    }

    // Recompose every face whose base material just became available.
    for id in newly_decoded {
        let keys: Vec<FaceKey> = manager
            .face_slots
            .iter()
            .filter(|(_key, slot)| slot.material_id == id)
            .map(|(key, _slot)| *key)
            .collect();
        for key in keys {
            recompose_face(&mut manager, &mut textures, &mut materials, key);
        }
    }
}

/// Apply per-face GLTF material overrides (P27.2) pushed by the simulator in a
/// GLTF material-override `GenericStreamingMessage`. Each affected face's override
/// document is decoded and stored (or cleared, when it reverts to base), and the
/// face recomposed so the delta layers onto its base material. Faces of the same
/// object omitted from the message have their override cleared, mirroring the
/// reference (`LLGLTFMaterialList::applyOverrideMessage`).
pub(crate) fn apply_material_overrides(
    mut manager: ResMut<MaterialManager>,
    mut textures: ResMut<TextureManager>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut events: MessageReader<SlEvent>,
) {
    for SlEvent(event) in events.read() {
        let SlSessionEvent::GltfMaterialOverride {
            local_id,
            faces,
            overrides,
            ..
        } = event
        else {
            continue;
        };
        let scoped = *local_id;
        debug!(
            "GLTF material override for object {scoped} on {} face(s)",
            faces.len()
        );
        let mut present: HashSet<u8> = HashSet::new();
        for (face, raw) in faces.iter().zip(overrides.iter()) {
            present.insert(*face);
            let key = (scoped, *face);
            let decoded = parse_material_override(raw).unwrap_or_default();
            if decoded.is_empty() {
                let _removed = manager.overrides.remove(&key);
            } else {
                let _prev = manager.overrides.insert(key, decoded);
            }
            recompose_face(&mut manager, &mut textures, &mut materials, key);
        }
        // Clear overrides on this object's faces the message no longer lists (a
        // revert to base for a face whose override was dropped).
        let stale: Vec<FaceKey> = manager
            .overrides
            .keys()
            .filter(|(object, face)| *object == scoped && !present.contains(face))
            .copied()
            .collect();
        for key in stale {
            let _removed = manager.overrides.remove(&key);
            recompose_face(&mut manager, &mut textures, &mut materials, key);
        }
    }
}

/// Recompose the faces the build tool edited locally
/// ([`MaterialManager::apply_local_override`]) so an override edit shows
/// immediately (the prim, once its base material has decoded), independent of the
/// simulator's echo.
pub(crate) fn drive_local_overrides(
    mut manager: ResMut<MaterialManager>,
    mut textures: ResMut<TextureManager>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    if manager.local_recompose.is_empty() {
        return;
    }
    let keys = core::mem::take(&mut manager.local_recompose);
    for key in keys {
        recompose_face(&mut manager, &mut textures, &mut materials, key);
    }
}

/// Hide or restore each PBR face's render material for the build tool's
/// Blinn-Phong editing mode — the Firestorm FIRE-35138 behaviour ("show the
/// selection in Blinn-Phong"): while the Build Tools floater is open **and** its
/// Texture tab is on the Material (Blinn-Phong) mode, every PBR face of a
/// **selected linkset** renders its Blinn-Phong layer instead of its PBR material,
/// so its diffuse / tint / surface flags can be judged as they are edited; leaving
/// build mode, switching to the PBR tab, or deselecting restores the PBR material.
///
/// This diverges from the reference in one deliberate way ([[sl-client-prefer-maximal-scope]]):
/// the reference hides only the selected **prims**, but we hide the whole
/// **linkset** so a multi-prim build (a house's walls + floor + roof) is judged as
/// a coherent whole rather than one wall in Blinn-Phong beside PBR everything else.
///
/// The face keeps its one stable [`StandardMaterial`] handle throughout (so every
/// other system that reads it is unaffected); only its composition is swapped
/// between the PBR material ([`recompose_face`]) and the Blinn-Phong material
/// ([`compose_face_material`]). While a face is in [`hidden`](MaterialManager::hidden)
/// its PBR (re)composition is suppressed, so a material / override / map that
/// arrives mid-edit cannot overwrite the preview.
///
/// Only recomputed when its inputs change (the tool, the mode, the selection) or a
/// new face registered, so the linkset walk is not paid every frame.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool / mode / \
              selection state driving the hide, the three material resources it recomposes \
              through, and the hierarchy / holder / face queries the linkset walk reads"
)]
pub(crate) fn apply_blinn_phong_hide(
    tool: Res<EditToolState>,
    mode: Res<MatModeState>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    new_faces: Query<(), Added<PrimFaceEntity>>,
    mut manager: ResMut<MaterialManager>,
    mut textures: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    children: Query<&Children>,
    holders: Query<&ObjectRenderMaterials>,
    faces: Query<(&PrimFaceEntity, &FaceTextureDebug)>,
) {
    // Recompute only on a real change; a face already hidden stays hidden without
    // work (its PBR recomposition is suppressed, so nothing un-hides it silently).
    if !(tool.is_changed() || mode.is_changed() || selection.is_changed() || !new_faces.is_empty())
    {
        return;
    }

    // The PBR faces that should render Blinn-Phong now: every PBR face of a
    // selected linkset, but only while the build tool is open on the Blinn-Phong
    // (Material) mode. Keyed like `face_slots`, carrying the face entity so its
    // `TextureEntry` is read when it is (re)composed as Blinn-Phong.
    let mut want: HashMap<FaceKey, Entity> = HashMap::new();
    if tool.active && mode.is_material() {
        for node in selection.iter() {
            // Hide the **whole linkset**, not just the selected prim: a Select-Face
            // (or edit-linked) selection carries the clicked *part*, whose subtree
            // holds only its own faces — resolve to the linkset root so a sibling
            // prim of a multi-prim build is previewed in Blinn-Phong too (the whole
            // house, not one wall). Falls back to the node's own entity when the
            // root is not resolvable.
            let root_scoped = objects.linkset_root_of(&node.scoped).unwrap_or(node.scoped);
            let root_entity = objects
                .entity_by_scoped(&root_scoped)
                .unwrap_or(node.entity);
            collect_linkset_pbr_faces(root_entity, &children, &holders, &faces, &mut want);
        }
    }
    let want_keys: HashSet<FaceKey> = want.keys().copied().collect();

    // Restore PBR on faces that left the set (deselected, mode switched, build
    // mode closed): drop any Blinn-Phong diffuse still parked on the handle so it
    // cannot land over the restored PBR composition, then recompose the material.
    let to_show: Vec<FaceKey> = manager.hidden.difference(&want_keys).copied().collect();
    for key in to_show {
        let _present = manager.hidden.remove(&key);
        if let Some(slot) = manager.face_slots.get(&key) {
            let handle = slot.handle.clone();
            prim_textures.drop_pending_material(&handle);
        }
        recompose_face(&mut manager, &mut textures, &mut materials, key);
    }

    // Hide PBR on faces that entered the set: drop any parked PBR map so a late one
    // cannot land over the Blinn-Phong preview, mark the face hidden (so its PBR
    // recomposition is suppressed from now on), then compose its Blinn-Phong look.
    for (key, entity) in want {
        if manager.hidden.contains(&key) {
            continue;
        }
        let Some(slot) = manager.face_slots.get(&key) else {
            continue;
        };
        let handle = slot.handle.clone();
        for pbr_slot in PbrSlot::ALL {
            drop_texture_patches(&mut manager, &handle, pbr_slot);
        }
        let _absent = manager.hidden.insert(key);
        let Ok((_face, FaceTextureDebug(texture_face))) = faces.get(entity) else {
            continue;
        };
        let texture_face = *texture_face;
        compose_face_material(
            &handle,
            &texture_face,
            &mut materials,
            &mut textures,
            &mut prim_textures,
            MATERIAL_TEXTURE_PRIORITY,
            TextureAlpha::Mask,
        );
    }
}

/// Walk the linkset rooted at `root`, collecting every PBR face (a face whose
/// index appears in its geometry holder's [`ObjectRenderMaterials`]) into `out`,
/// keyed by its scoped-object + face index and carrying the face entity. The whole
/// subtree is descended (through nested linkset-child objects) so a multi-prim
/// build's every PBR face is included.
fn collect_linkset_pbr_faces(
    root: Entity,
    children: &Query<&Children>,
    holders: &Query<&ObjectRenderMaterials>,
    faces: &Query<(&PrimFaceEntity, &FaceTextureDebug)>,
    out: &mut HashMap<FaceKey, Entity>,
) {
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if let Ok(holder) = holders.get(entity)
            && let Ok(holder_children) = children.get(entity)
        {
            for child in holder_children.iter() {
                let Ok((face, _debug)) = faces.get(child) else {
                    continue;
                };
                let face_index = face.face_id.as_usize();
                if let Some(&(face_id, _material_id)) = holder
                    .faces
                    .iter()
                    .find(|(index, _id)| usize::from(*index) == face_index)
                {
                    let _prev = out.insert((holder.scoped_id, face_id), child);
                }
            }
        }
        if let Ok(list) = children.get(entity) {
            for child in list.iter() {
                stack.push(child);
            }
        }
    }
}

/// Recompose one registered face's [`StandardMaterial`]: layer its override (if
/// any) onto its base material, write the effective scalars / UV placement, and
/// (re)request its texture maps.
///
/// A GLTF render material **supersedes** the face's Blinn-Phong layer (the
/// reference viewer's `getGLTFRenderMaterial`), so a PBR face never falls back to
/// its legacy diffuse/normal appearance: while the base asset is still fetching
/// (or turned out unavailable) the glTF **default** material stands in — neutral
/// white PBR, exactly as the reference renders a face whose `LLFetchedGLTFMaterial`
/// has not yet loaded — rather than leaving the Blinn-Phong texture showing
/// through. The face is recomposed again with the real scalars/maps once the
/// asset decodes.
fn recompose_face(
    manager: &mut MaterialManager,
    textures: &mut TextureManager,
    materials: &mut Assets<FaceMaterial>,
    key: FaceKey,
) {
    // A face whose PBR material is hidden for the Blinn-Phong build-tool preview
    // (FIRE-35138) renders its Blinn-Phong composition; suppress the PBR
    // (re)composition so a late material / override / map does not overwrite it.
    if manager.hidden.contains(&key) {
        return;
    }
    let Some(slot) = manager.face_slots.get(&key) else {
        return;
    };
    let material_id = slot.material_id;
    let handle = slot.handle.clone();
    let base_uv = slot.base_uv;
    let base = manager
        .decoded
        .get(&material_id)
        .copied()
        .unwrap_or_default();
    let mut effective = base;
    if let Some(over) = manager.overrides.get(&key) {
        over.apply_to(&mut effective);
    }
    apply_material_scalars(materials, &handle, &effective, base_uv);
    request_material_textures(manager, textures, materials, &handle, &effective);
}

/// Write a decoded [`GltfMaterial`]'s scalar / factor fields onto a face's
/// [`StandardMaterial`]: base colour (linear factor), metallic / roughness,
/// emissive, alpha mode + cutoff, and the double-sided / cull-mode pair. The
/// texture maps are filled in later by [`apply_pbr_textures`].
fn apply_material_scalars(
    materials: &mut Assets<FaceMaterial>,
    handle: &Handle<FaceMaterial>,
    material: &GltfMaterial,
    base_uv: Affine2,
) {
    let Some(mut material_asset) = materials.get_mut(handle) else {
        return;
    };
    let standard = &mut material_asset.base;
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
    // diffuse (texture-entry) UV placement `base_uv`. Recomposing from the captured
    // `base_uv` (rather than the live `uv_transform`) keeps a re-application (a
    // later override) from stacking the transform. Bevy carries a single
    // `uv_transform` for all maps, so the base-colour transform stands in for every
    // slot (a documented approximation of the reference's per-slot transforms).
    // `Mul::mul` (a method, not the `*` operator) keeps clear of the workspace
    // `arithmetic_side_effects` lint the glam operators trip.
    standard.uv_transform = match material.base_color_texture {
        Some(texture) => base_uv.mul(gltf_uv_transform(&texture.transform)),
        None => base_uv,
    };
}

/// Reconcile a face's PBR texture slots to `effective` (the base material with any
/// override applied): request each slot the effective material names, and clear
/// any slot it does **not** name.
///
/// A GLTF render material supersedes the face's Blinn-Phong layer entirely (the
/// reference viewer's `getGLTFRenderMaterial`), so a slot the material does not
/// define is *cleared* rather than left showing the face's legacy diffuse/normal
/// texture: a factor-only material renders its base-colour factor, not the
/// leftover Blinn-Phong texture that was on the shared material before the PBR
/// composition. (A PBR face that is being shown as Blinn-Phong for editing — the
/// FIRE-35138 hide — has its PBR recomposition suppressed entirely, so this path
/// never runs for it; its handle is composed as Blinn-Phong instead.)
fn request_material_textures(
    manager: &mut MaterialManager,
    textures: &mut TextureManager,
    materials: &mut Assets<FaceMaterial>,
    handle: &Handle<FaceMaterial>,
    effective: &GltfMaterial,
) {
    for slot in PbrSlot::ALL {
        match slot.fetchable_texture(effective) {
            Some(id) => {
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
            // The effective material does not name a (fetchable) texture for this
            // slot: clear it so the PBR factor stands alone, dropping any parked
            // patch first so a stale one cannot later re-fill the cleared slot.
            None => {
                drop_texture_patches(manager, handle, slot);
                if let Some(mut standard) = materials.get_mut(handle) {
                    slot.clear(&mut standard.base);
                }
            }
        }
    }
}

/// Drop any parked (not-yet-applied) texture patches targeting `handle`'s `slot`,
/// so an override that clears a slot is not later re-filled by a stale patch left
/// from an earlier composition of the same face.
fn drop_texture_patches(
    manager: &mut MaterialManager,
    handle: &Handle<FaceMaterial>,
    slot: PbrSlot,
) {
    for patches in manager.texture_pending.values_mut() {
        patches.retain(|patch| !(patch.material == *handle && patch.slot == slot));
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
    mut materials: ResMut<Assets<FaceMaterial>>,
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
            let Some(mut material_asset) = materials.get_mut(&patch.material) else {
                continue;
            };
            let standard = &mut material_asset.base;
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A [`GltfTexture`] naming `id` with the identity `KHR_texture_transform`.
    fn texture(id: TextureKey) -> GltfTexture {
        GltfTexture {
            id,
            transform: GltfTextureTransform::default(),
        }
    }

    #[test]
    fn factor_only_material_clears_every_slot() {
        // A PBR material with only factors (no texture maps) names no fetchable
        // texture in any slot, so every slot is *cleared* rather than left showing
        // the face's leftover Blinn-Phong diffuse — PBR fully supersedes the
        // legacy layer (the bug the fix addresses: a factor-only material used to
        // keep the diffuse texture bleeding through).
        let material = GltfMaterial::default();
        for slot in PbrSlot::ALL {
            assert_eq!(
                slot.fetchable_texture(&material),
                None,
                "{slot:?} of a factor-only material must clear, not keep the diffuse",
            );
        }
    }

    #[test]
    fn only_the_defined_slot_is_requested() {
        // A material that names only a base-colour texture requests exactly that
        // slot; the others clear (so their PBR factors stand, no Blinn-Phong
        // bleed-through).
        let id = TextureKey::from(Uuid::from_u128(0x1234));
        let material = GltfMaterial {
            base_color_texture: Some(texture(id)),
            ..GltfMaterial::default()
        };
        assert_eq!(PbrSlot::BaseColor.fetchable_texture(&material), Some(id));
        assert_eq!(
            PbrSlot::MetallicRoughness.fetchable_texture(&material),
            None
        );
        assert_eq!(PbrSlot::Normal.fetchable_texture(&material), None);
        assert_eq!(PbrSlot::Emissive.fetchable_texture(&material), None);
    }

    #[test]
    fn non_fetchable_slot_ids_clear() {
        // The nil id and the GLTF override-null sentinel are "no texture" — a slot
        // an override cleared reverts to its factor, it does not try to fetch the
        // sentinel or keep a stale map.
        for cleared in [Uuid::nil(), GLTF_OVERRIDE_NULL_UUID] {
            let material = GltfMaterial {
                normal_texture: Some(texture(TextureKey::from(cleared))),
                emissive_texture: Some(texture(TextureKey::from(cleared))),
                ..GltfMaterial::default()
            };
            assert_eq!(PbrSlot::Normal.fetchable_texture(&material), None);
            assert_eq!(PbrSlot::Emissive.fetchable_texture(&material), None);
        }
    }
}
