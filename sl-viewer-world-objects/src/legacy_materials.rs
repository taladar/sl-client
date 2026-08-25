//! The legacy (pre-PBR) render-material pipeline (P27.3): fetch each face's
//! `LLMaterial` over the `RenderMaterials` capability and map it onto the face's
//! Bevy [`StandardMaterial`] — the normal map, plus scalar approximations of the
//! specular / environment / glossiness stack and the diffuse alpha mode.
//!
//! A prim face references a legacy material by the 16-byte `material_id` in its
//! `TextureEntry` face (`sl_proto::TextureFace::material_id`, carried on each face
//! entity as [`FaceTextureDebug`]).
//! [`register_legacy_materials`] picks up each newly-spawned face carrying such an
//! id — skipping any face that already has a PBR GLTF material
//! ([`ObjectRenderMaterials`], which supersedes the legacy material like the
//! reference viewer) — and queues the material to be fetched.
//!
//! Unlike the PBR pipeline's per-asset `ViewerAsset` fetch, legacy materials come
//! from a **batch** capability POST: [`drive_legacy_material_requests`] sends the
//! outstanding ids as a `RequestRenderMaterials` command (chunked to the
//! per-transaction limit), the runtime POSTs the `RenderMaterials` cap, and the
//! decoded `RenderMaterialEntry` list returns as an
//! [`SlSessionEvent::RenderMaterials`] that [`receive_legacy_materials`] caches.
//! [`apply_legacy_materials`] then writes each material's scalars onto the waiting
//! faces and requests its normal map through the shared
//! [`TextureManager`];
//! [`apply_legacy_normal_maps`] uploads that map (linear) into the face material's
//! normal slot once it decodes.
//!
//! Since Phase 2 of the custom face material ([`crate::face_material`]) the mapping
//! is **faithful**, not the earlier scalar approximation: the material's normal and
//! specular maps (each with its own offset / repeat / rotation UV transform), its
//! specular colour, glossiness (exponent) and environment intensity are written
//! onto the face material's [`SlFaceExt`](crate::face_material::SlFaceExt) extension,
//! which renders them as an analytic normalized Blinn-Phong specular lobe over the
//! matte base (see `face_material.wgsl`). The face's base [`StandardMaterial`] is
//! set matte (metallic 0, roughness 1, reflectance 0) so the added lobe — not the
//! `StandardMaterial` GGX lobe — is the whole specular story. The remaining
//! approximation is the environment reflection (a spec-tinted ambient term with no
//! reflection probe on the headless path), tracked by
//! [[viewer-legacy-material-exact-port]].

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sl_client_bevy::{
    Command, DecodedTexture, LegacyMaterial, Priority, SlCommand, SlEvent, SlSessionEvent,
    TextureKey, Uuid, texture_uv_transform,
};

use crate::face_material::{FaceMaterial, MAP_FLAG_NORMAL, MAP_FLAG_SPEC};
use crate::materials::ObjectRenderMaterials;
use crate::objects::{FaceTextureDebug, PrimFaceEntity};
use crate::textures::{TextureApplyBudget, TextureManager};
use crate::world_api::TERRAIN_BOOST_PRIORITY;

/// The fetch priority a legacy material's normal map is requested at — the same
/// modest boost the PBR pipeline uses for its maps, so the map loads at full
/// resolution rather than starved behind the pixel-area-ranked diffuse faces.
const MATERIAL_TEXTURE_PRIORITY: Priority = TERRAIN_BOOST_PRIORITY;

/// The most material ids to fetch in one `RenderMaterials` POST — the reference's
/// `MaxMaterialsPerTransaction` (advertised in `SimulatorFeatures`), which stock
/// OpenSim also enforces. Requests are chunked to this size.
const MAX_MATERIALS_PER_REQUEST: usize = 50;

/// The diffuse alpha-blend mode (`DIFFUSE_ALPHA_MODE_BLEND`): the z-sorted
/// transparent path.
const DIFFUSE_ALPHA_MODE_BLEND: u8 = 1;
/// The diffuse alpha-mask mode (`DIFFUSE_ALPHA_MODE_MASK`): alpha-test at the
/// material's cutoff.
const DIFFUSE_ALPHA_MODE_MASK: u8 = 2;

/// The legacy render-material fetch/apply pipeline: the decoded materials, the
/// faces waiting on each, the ids queued for (and already issued to) the
/// `RenderMaterials` capability, and the normal-map upload bookkeeping.
#[derive(Debug, Resource, Default)]
pub struct LegacyMaterialManager {
    /// Successfully fetched materials by their 16-byte id, shared across every
    /// face using the material so it is fetched once.
    decoded: HashMap<Uuid, LegacyMaterial>,
    /// Face [`StandardMaterial`] handles waiting for a material to arrive, keyed
    /// by the material id they requested; drained by [`apply_legacy_materials`]
    /// once the material decodes.
    pending_faces: HashMap<Uuid, Vec<Handle<FaceMaterial>>>,
    /// Material ids already queued or issued to the capability, so each is
    /// requested only once (the pipeline is eventually consistent — a face that
    /// registers after the material decoded is served straight from `decoded`).
    requested: HashSet<Uuid>,
    /// Material ids queued for the next `RenderMaterials` POST, drained (in
    /// chunks) by [`drive_legacy_material_requests`].
    to_request: Vec<Uuid>,
    /// Uploaded (linear) normal-map images by texture id, so a map shared by
    /// several materials is uploaded once.
    images: HashMap<TextureKey, Handle<Image>>,
    /// Face materials parked on a normal-map texture id, applied once it decodes.
    texture_pending: HashMap<TextureKey, Vec<Handle<FaceMaterial>>>,
    /// Uploaded (sRGB) specular-map images by texture id — the legacy specular map
    /// is a colour texture (its RGB tints the highlight, its alpha weights the
    /// environment), uploaded once per id.
    spec_images: HashMap<TextureKey, Handle<Image>>,
    /// Face materials parked on a specular-map texture id, applied once it decodes.
    spec_pending: HashMap<TextureKey, Vec<Handle<FaceMaterial>>>,
    /// Face materials whose `alpha_mode` a legacy material has **overridden**
    /// (an opaque-tint face whose `LLMaterial` diffuse alpha mode applied): the
    /// material's mode is authoritative from then on, so the R22d texture-alpha
    /// resolution must not "upgrade" it when the diffuse texture decodes later
    /// (`NONE` means opaque in the reference even over an alpha texture — the
    /// outcome used to depend on which of the two applied last, R25a). Entries
    /// for despawned faces go stale harmlessly (asset ids are not reused) and
    /// are dropped with the manager at session end.
    alpha_overridden: HashSet<AssetId<FaceMaterial>>,
}

impl LegacyMaterialManager {
    /// The decoded legacy material for `id`, if its `RenderMaterials` fetch has
    /// succeeded — a read-only lookup for the pick diagnostic (R25), which uses
    /// it to tell a fetched-but-opaque material from a missing fetch.
    #[must_use]
    pub fn decoded_material(&self, id: &Uuid) -> Option<&LegacyMaterial> {
        self.decoded.get(id)
    }

    /// Whether a legacy material has overridden this face material's alpha
    /// mode, making that mode authoritative over the R22d texture-alpha
    /// resolution (see [`Self::alpha_overridden`]).
    pub(crate) fn is_alpha_overridden(&self, id: AssetId<FaceMaterial>) -> bool {
        self.alpha_overridden.contains(&id)
    }

    /// Drop everything a FIRE-35138 Blinn-Phong preview
    /// ([`preview_legacy_material`]) parked on `handle` — the material fetch itself
    /// (`pending_faces`) and any normal / specular map still in flight
    /// (`texture_pending` / `spec_pending`) — so nothing lands on the extension of a
    /// face that has since been restored to its PBR material
    /// ([`crate::materials::apply_blinn_phong_hide`]). Called only for a face leaving
    /// the preview (always a PBR face, which never needs its legacy material once
    /// restored); a non-PBR face is never in the preview set.
    pub(crate) fn drop_pending_preview(&mut self, handle: &Handle<FaceMaterial>) {
        for parked in self.pending_faces.values_mut() {
            parked.retain(|parked_handle| parked_handle != handle);
        }
        for parked in self.texture_pending.values_mut() {
            parked.retain(|parked_handle| parked_handle != handle);
        }
        for parked in self.spec_pending.values_mut() {
            parked.retain(|parked_handle| parked_handle != handle);
        }
    }

    /// Register a face material handle against its legacy material id: park the
    /// handle until the material arrives and queue the id for fetch if it is not
    /// already known / requested.
    fn register(&mut self, handle: Handle<FaceMaterial>, material_id: Uuid) {
        self.pending_faces
            .entry(material_id)
            .or_default()
            .push(handle);
        self.queue_fetch(material_id);
    }

    /// Queue a legacy material id for fetch (once) without parking any face on it —
    /// so a **PBR** face's legacy material is still fetched into [`decoded`](Self::decoded)
    /// (a PBR render material supersedes the legacy one, but the FIRE-35138
    /// Blinn-Phong build-preview needs the real specular / normal when that face is
    /// shown as legacy), yet is never applied to the PBR face's live material.
    fn queue_fetch(&mut self, material_id: Uuid) {
        if self.decoded.contains_key(&material_id) || !self.requested.insert(material_id) {
            return;
        }
        self.to_request.push(material_id);
    }

    /// The uploaded (linear) normal-map [`Image`] for `id`, uploading it from the
    /// decoded texture on first use and caching it.
    fn normal_image(
        &mut self,
        images: &mut Assets<Image>,
        id: TextureKey,
        decoded: &Arc<DecodedTexture>,
    ) -> Handle<Image> {
        if let Some(handle) = self.images.get(&id) {
            return handle.clone();
        }
        let handle = images.add(build_linear_image(decoded));
        let _inserted = self.images.insert(id, handle.clone());
        handle
    }

    /// The uploaded (sRGB) specular-map [`Image`] for `id`, uploading it from the
    /// decoded texture on first use and caching it.
    fn spec_image(
        &mut self,
        images: &mut Assets<Image>,
        id: TextureKey,
        decoded: &Arc<DecodedTexture>,
    ) -> Handle<Image> {
        if let Some(handle) = self.spec_images.get(&id) {
            return handle.clone();
        }
        let handle = images.add(build_srgb_image(decoded));
        let _inserted = self.spec_images.insert(id, handle.clone());
        handle
    }
}

/// Build a Bevy [`Image`] for a legacy normal map from decoded RGBA8 pixels, in
/// the linear colour space a normal map needs (`Rgba8Unorm`) and with the
/// repeating sampler object faces tile their textures with.
pub fn build_linear_image(decoded: &Arc<DecodedTexture>) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: decoded.width,
            height: decoded.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        decoded.pixels.to_vec(),
        TextureFormat::Rgba8Unorm,
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

/// Build a Bevy [`Image`] for a legacy specular map from decoded RGBA8 pixels, in
/// the **sRGB** colour space its RGB tint is authored in (`Rgba8UnormSrgb`, so the
/// GPU sample is already linear like the reference's `srgb_to_linear(spec.rgb)`),
/// with the repeating sampler object faces tile their textures with. The alpha
/// channel (the per-texel environment weight) is unaffected by the sRGB transfer.
pub fn build_srgb_image(decoded: &Arc<DecodedTexture>) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: decoded.width,
            height: decoded.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        decoded.pixels.to_vec(),
        TextureFormat::Rgba8UnormSrgb,
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

/// The linear specular highlight tint a legacy material's sRGB-encoded specular
/// colour (`0..=255` RGB) maps to — the reference `srgb_to_linear(specular_color)`,
/// which its RGB and the (linear-sampled sRGB) specular map are multiplied together
/// in.
fn linear_specular_color(specular_color: [u8; 4]) -> [f32; 3] {
    let [r, g, b, _a] = specular_color;
    let linear = Color::srgb_u8(r, g, b).to_linear();
    [linear.red, linear.green, linear.blue]
}

/// Build the per-map UV [`Affine2`] for a legacy material's normal / specular map
/// from its own offset / repeat / rotation — applied to the face's raw texture
/// coordinates independently of the diffuse placement, exactly as the reference
/// viewer's per-channel `xform` (`llface.cpp`).
fn map_uv_transform(offset: (f32, f32), repeat: (f32, f32), rotation: f32) -> Affine2 {
    texture_uv_transform(rotation, offset.0, offset.1, repeat.0, repeat.1)
}

/// The tint alpha at or above which a face counts as opaque for the legacy
/// alpha-mode override — the reference viewer's `blinn_phong_transparent`
/// threshold (`te->getColor().mV[3] < 0.999f`, `llvovolume.cpp`).
const OPAQUE_TINT_ALPHA: f32 = 0.999;

/// The [`AlphaMode`] a face's `LLMaterial` diffuse alpha mode forces — the
/// authoritative per-face alpha property (the "alpha mode" control in the reference
/// viewer's build/texture tab: none / alpha-blend / alpha-mask / emissive). All
/// four modes are honoured: `NONE` and `EMISSIVE` force opaque (emissive glow is a
/// separate channel), `MASK` an alpha test at the material cutoff, and `BLEND` the
/// z-sorted transparent path. This must cover every mode because the diffuse-pipeline
/// default no longer blends off the texture's alpha (R22d) — so a `BLEND` face has to
/// be forced back into the transparent path here rather than inheriting it.
fn legacy_alpha_override(diffuse_alpha_mode: u8, alpha_mask_cutoff: u8) -> AlphaMode {
    match diffuse_alpha_mode {
        DIFFUSE_ALPHA_MODE_MASK => AlphaMode::Mask(f32::from(alpha_mask_cutoff) / 255.0),
        DIFFUSE_ALPHA_MODE_BLEND => AlphaMode::Blend,
        // `NONE`, `EMISSIVE`, and any unknown value render opaque.
        _other => AlphaMode::Opaque,
    }
}

/// Whether the legacy-material diagnostic log is enabled
/// (`SL_VIEWER_LOG_LEGACY_MATERIALS`, R25a): every face registration and
/// every scalar apply — with the guard's inputs and outcome — is logged at
/// `info`, so a transparency divergence across a LoD / derender cycle can be
/// read off a live session's log.
fn legacy_log_enabled() -> bool {
    std::env::var_os("SL_VIEWER_LOG_LEGACY_MATERIALS").is_some()
}

/// Register each newly-spawned face carrying a legacy `TextureEntry` material id
/// with the [`LegacyMaterialManager`], skipping any face that already has a PBR
/// GLTF material (which supersedes the legacy material, as in the reference
/// viewer) and any face whose id is nil.
///
/// A PBR face's legacy material is **not** fetched here: most objects are never
/// edited, so eagerly requesting the `RenderMaterials` cap for every PBR face just
/// to have its Blinn-Phong appearance ready would waste bandwidth on a viewer that
/// mostly looks rather than builds. The FIRE-35138 build-preview fetches it
/// **on-demand** instead, only when a face actually enters the preview
/// ([`crate::materials::apply_blinn_phong_hide`] → [`preview_legacy_material`]).
pub fn register_legacy_materials(
    mut manager: ResMut<LegacyMaterialManager>,
    new_faces: Query<
        (
            &MeshMaterial3d<FaceMaterial>,
            &PrimFaceEntity,
            &FaceTextureDebug,
            &ChildOf,
        ),
        Added<PrimFaceEntity>,
    >,
    pbr_holders: Query<&ObjectRenderMaterials>,
) {
    for (material, face, FaceTextureDebug(texture_face), child_of) in &new_faces {
        let Some(material_id) = texture_face.material_id else {
            continue;
        };
        if material_id.is_nil() {
            continue;
        }
        let face_index = face.face_id.as_usize();
        // A PBR GLTF material on the same face supersedes the legacy material, so the
        // legacy material is neither applied nor fetched here — the build-preview
        // fetches it on-demand if this face is ever edited (see the fn docs).
        if let Ok(holder) = pbr_holders.get(child_of.parent())
            && holder
                .faces
                .iter()
                .any(|(index, _id)| usize::from(*index) == face_index)
        {
            if legacy_log_enabled() {
                info!(
                    "legacy: face {face_index} material {material_id} superseded by PBR, \
                     not fetched (handle {:?})",
                    material.0.id()
                );
            }
            continue;
        }
        if legacy_log_enabled() {
            info!(
                "legacy: register face {face_index} material {material_id} tint_a={} \
                 (handle {:?})",
                texture_face.color[3],
                material.0.id()
            );
        }
        manager.register(material.0.clone(), material_id);
    }
}

/// Issue the outstanding legacy material ids to the `RenderMaterials` capability
/// (via the runtime `RequestRenderMaterials` command), chunked to the
/// per-transaction limit. A no-op while nothing is queued.
pub fn drive_legacy_material_requests(
    mut manager: ResMut<LegacyMaterialManager>,
    mut commands: MessageWriter<SlCommand>,
) {
    if manager.to_request.is_empty() {
        return;
    }
    let queued = std::mem::take(&mut manager.to_request);
    debug!("requesting {} legacy render-material(s)", queued.len());
    for chunk in queued.chunks(MAX_MATERIALS_PER_REQUEST) {
        commands.write(SlCommand(Command::RequestRenderMaterials {
            material_ids: chunk.to_vec(),
        }));
    }
}

/// Fold each `RenderMaterials` capability reply (the runtime
/// [`SlSessionEvent::RenderMaterials`]) into the decoded-material cache;
/// [`apply_legacy_materials`] then applies each to the faces waiting on it.
pub fn receive_legacy_materials(
    mut manager: ResMut<LegacyMaterialManager>,
    mut events: MessageReader<SlEvent>,
) {
    for SlEvent(event) in events.read() {
        let SlSessionEvent::RenderMaterials(entries) = event else {
            continue;
        };
        debug!("received {} legacy render-material(s)", entries.len());
        for entry in entries {
            let _prev = manager
                .decoded
                .insert(entry.material_id, entry.material.clone());
        }
    }
}

/// Apply every decoded legacy material to the faces waiting on it: write the
/// scalar fields onto each face's [`StandardMaterial`] and request its normal map.
/// Serves both faces registered before the material arrived and faces registered
/// after (both wait in `pending_faces`).
pub fn apply_legacy_materials(
    mut manager: ResMut<LegacyMaterialManager>,
    mut textures: ResMut<TextureManager>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    let ready: Vec<Uuid> = manager
        .pending_faces
        .keys()
        .filter(|id| manager.decoded.contains_key(id))
        .copied()
        .collect();
    for id in ready {
        let Some(material) = manager.decoded.get(&id).cloned() else {
            continue;
        };
        let handles = manager.pending_faces.remove(&id).unwrap_or_default();
        for handle in handles {
            apply_legacy_to_face(
                &mut manager,
                &mut textures,
                &mut materials,
                &handle,
                &material,
            );
        }
    }
}

/// Write one legacy material onto a face [`FaceMaterial`] — the matte base, the
/// diffuse alpha-mode override, and the legacy Blinn-Phong specular workflow onto
/// the [`SlFaceExt`](crate::face_material::SlFaceExt) extension (specular colour /
/// glossiness / environment intensity, and the normal- and specular-map UV
/// transforms) — returning whether the material's diffuse alpha mode **overrode**
/// the face's `alpha_mode` (the caller records that in
/// `LegacyMaterialManager::alpha_overridden` so the R22d texture-alpha resolution
/// leaves the mode alone from then on, whichever of the two applies last, R25a).
///
/// The pure half of `apply_legacy_to_face`, split out so it is reachable without
/// a fetch behind it: everything here is a function of the decoded material, while
/// the normal / specular maps are grid assets the caller has to go and get. That is
/// what lets `sl_viewer_world_scene::render_scene`'s legacy-material scene exercise the real
/// mapping with no capability, no `TextureManager` and no grid — the registry's
/// rule that construction is separable from transport, applied to this module.
pub fn apply_legacy_scalars(material_asset: &mut FaceMaterial, material: &LegacyMaterial) -> bool {
    {
        let standard = &mut material_asset.base;
        // A legacy face's base is matte: the SL specular is the analytic Blinn-Phong
        // lobe the extension adds (face_material.wgsl), not the `StandardMaterial`
        // GGX lobe, so metallic / roughness / reflectance are zeroed to avoid a
        // doubled highlight.
        standard.metallic = 0.0;
        standard.perceptual_roughness = 1.0;
        standard.reflectance = 0.0;
    }
    // A translucent TE tint wins over the material's diffuse alpha mode (R25): the
    // reference viewer ORs `te->getColor().mV[3] < 0.999f` into `is_alpha` and
    // registers the alpha pass *before* the material-pass dispatch
    // (`llvovolume.cpp` `getDrawInfo`), so a tinted-transparent face stays in the
    // blend pass for *every* material mode — the material's mode only decides the
    // pass when the tint is opaque. On this legacy path `base_color` still holds
    // the TE tint (only the PBR path replaces it), so its alpha is that tint's.
    // Without this guard the common "transparent prim that also carries a
    // shiny/bump material" content (whose `LLMaterial` defaults to alpha mode
    // `NONE`) was forced opaque the moment its material arrived.
    let overrides_alpha = material_asset.base.base_color.alpha() >= OPAQUE_TINT_ALPHA;
    if overrides_alpha {
        material_asset.base.alpha_mode =
            legacy_alpha_override(material.diffuse_alpha_mode, material.alpha_mask_cutoff);
    }
    // The legacy Blinn-Phong specular workflow onto the extension.
    material_asset.extension.params.set_legacy(
        linear_specular_color(material.specular_color),
        f32::from(material.specular_exponent) / 255.0,
        f32::from(material.environment_intensity) / 255.0,
        map_uv_transform(
            material.normal_offset,
            material.normal_repeat,
            material.normal_rotation,
        ),
        map_uv_transform(
            material.specular_offset,
            material.specular_repeat,
            material.specular_rotation,
        ),
    );
    // A material that carries no normal / specular map clears any it had previously
    // and its re-sample bit (the map is not sampled without the bit).
    if material.normal_map.uuid().is_nil() {
        material_asset.extension.normal_map = Handle::default();
        material_asset.extension.params.map_flags &= !MAP_FLAG_NORMAL;
    }
    if material.specular_map.uuid().is_nil() {
        material_asset.extension.specular_map = Handle::default();
        material_asset.extension.params.map_flags &= !MAP_FLAG_SPEC;
    }
    overrides_alpha
}

/// Write one legacy material onto a face [`FaceMaterial`] and queue its normal and
/// specular maps for fetch. The maps are dropped into the extension's slots later by
/// [`apply_legacy_normal_maps`] / [`apply_legacy_specular_maps`].
fn apply_legacy_to_face(
    manager: &mut LegacyMaterialManager,
    textures: &mut TextureManager,
    materials: &mut Assets<FaceMaterial>,
    handle: &Handle<FaceMaterial>,
    material: &LegacyMaterial,
) {
    if let Some(mut material_asset) = materials.get_mut(handle) {
        if apply_legacy_scalars(&mut material_asset, material) {
            // The material's alpha mode is authoritative for this face now: the
            // R22d texture-alpha resolution must not upgrade it later (R25a).
            let _new = manager.alpha_overridden.insert(handle.id());
        }
        if legacy_log_enabled() {
            info!(
                "legacy: apply to handle {:?}: base_a={:.3} mode_in={} -> alpha_mode={:?}",
                handle.id(),
                material_asset.base.base_color.alpha(),
                material.diffuse_alpha_mode,
                material_asset.base.alpha_mode,
            );
        }
    } else if legacy_log_enabled() {
        info!(
            "legacy: apply to handle {:?}: material asset GONE (face despawned?)",
            handle.id()
        );
    }
    let normal = material.normal_map;
    if !normal.uuid().is_nil() {
        textures.request_boosted(normal, MATERIAL_TEXTURE_PRIORITY);
        manager
            .texture_pending
            .entry(normal)
            .or_default()
            .push(handle.clone());
    }
    let specular = material.specular_map;
    if !specular.uuid().is_nil() {
        textures.request_boosted(specular, MATERIAL_TEXTURE_PRIORITY);
        manager
            .spec_pending
            .entry(specular)
            .or_default()
            .push(handle.clone());
    }
}

/// Show a **PBR** face's real legacy specular / normal for the FIRE-35138
/// Blinn-Phong build-preview ([`crate::materials::apply_blinn_phong_hide`]): apply
/// the face's cached legacy material to `handle` now if it has been fetched, else
/// **request it on-demand** and park the handle so [`apply_legacy_materials`] applies
/// it when it arrives (the preview shows the plain diffuse until then). This is the
/// only place a PBR face's legacy material is fetched — a viewer that never edits an
/// object never pays for its Blinn-Phong appearance. When the face leaves the
/// preview the handle is dropped from the pending queues
/// (`LegacyMaterialManager::drop_pending_preview`) so a late arrival cannot land
/// on the restored PBR material.
pub fn preview_legacy_material(
    manager: &mut LegacyMaterialManager,
    textures: &mut TextureManager,
    materials: &mut Assets<FaceMaterial>,
    handle: &Handle<FaceMaterial>,
    material_id: Uuid,
) {
    if let Some(material) = manager.decoded.get(&material_id).cloned() {
        apply_legacy_to_face(manager, textures, materials, handle, &material);
    } else {
        manager.register(handle.clone(), material_id);
    }
}

/// Drop each decoded normal-map texture into the face materials parked on it:
/// upload the map (linear), set it into the extension's normal slot and turn on its
/// re-sample bit. Drains parked faces for any texture that has decoded (freshly or
/// already cached), so it needs no decode message.
pub fn apply_legacy_normal_maps(
    mut manager: ResMut<LegacyMaterialManager>,
    textures: Res<TextureManager>,
    mut budget: ResMut<TextureApplyBudget>,
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
        let handles = manager.texture_pending.remove(&id).unwrap_or_default();
        // One linear normal map is built per texture (then cached). When this frame's
        // shared image budget is spent, re-park the whole texture's faces for a later
        // frame so a cache-warm burst does not upload every map at once (the serial
        // `extract_render_asset<GpuImage>` spike). A cached map is free to reuse.
        if !manager.images.contains_key(&id) && !budget.take_image() {
            manager
                .texture_pending
                .entry(id)
                .or_default()
                .extend(handles);
            continue;
        }
        let image = manager.normal_image(&mut images, id, &decoded);
        for handle in handles {
            if let Some(mut material) = materials.get_mut(&handle) {
                material.extension.normal_map = image.clone();
                material.extension.params.map_flags |= MAP_FLAG_NORMAL;
            }
        }
    }
}

/// Drop each decoded specular-map texture into the face materials parked on it:
/// upload the map (sRGB), set it into the extension's specular slot and turn on its
/// re-sample bit. Mirrors [`apply_legacy_normal_maps`] for the specular channel.
pub fn apply_legacy_specular_maps(
    mut manager: ResMut<LegacyMaterialManager>,
    textures: Res<TextureManager>,
    mut budget: ResMut<TextureApplyBudget>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    let ready: Vec<TextureKey> = manager
        .spec_pending
        .keys()
        .filter(|id| textures.decoded(**id).is_some())
        .copied()
        .collect();
    for id in ready {
        let Some(decoded) = textures.decoded(id).map(Arc::clone) else {
            continue;
        };
        let handles = manager.spec_pending.remove(&id).unwrap_or_default();
        // One sRGB specular map is built per texture (then cached); mirror the
        // normal-map lane's budget gate so a specular burst cannot spike the serial
        // image upload either. A cached map is free to reuse.
        if !manager.spec_images.contains_key(&id) && !budget.take_image() {
            manager.spec_pending.entry(id).or_default().extend(handles);
            continue;
        }
        let image = manager.spec_image(&mut images, id, &decoded);
        for handle in handles {
            if let Some(mut material) = materials.get_mut(&handle) {
                material.extension.specular_map = image.clone();
                material.extension.params.map_flags |= MAP_FLAG_SPEC;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::face_material::{SL_FACE_MODE_LEGACY, SL_FACE_MODE_PBR, inert_face_material};

    use super::*;

    /// A [`FaceMaterial`] whose base tint is `(1, 1, 1, alpha)` and everything else
    /// inert — the starting point the legacy apply writes over.
    fn face_material(alpha: f32) -> FaceMaterial {
        inert_face_material(StandardMaterial {
            base_color: bevy::color::Color::srgba(1.0, 1.0, 1.0, alpha),
            alpha_mode: AlphaMode::Blend,
            ..StandardMaterial::default()
        })
    }

    #[test]
    fn legacy_specular_workflow_lands_on_the_extension() {
        // The legacy specular colour / glossiness / environment intensity and the
        // per-map transforms are written onto the extension, switching it to the
        // Blinn-Phong mode, and the base is made matte.
        let mut material = face_material(1.0);
        let mut legacy = material_with_alpha(0, 0);
        legacy.specular_color = [255, 128, 0, 255];
        legacy.specular_exponent = 204; // 0.8 of 255.
        legacy.environment_intensity = 51; // 0.2 of 255.
        let _override = apply_legacy_scalars(&mut material, &legacy);

        let params = material.extension.params;
        assert_eq!(params.mode, SL_FACE_MODE_LEGACY);
        assert!((params.glossiness - 204.0 / 255.0).abs() < 1e-6);
        assert!((params.env_intensity - 51.0 / 255.0).abs() < 1e-6);
        // The specular tint is the sRGB colour decoded to linear (so red, the
        // largest channel, stays largest and 0 stays 0).
        assert!(params.specular_color.x > params.specular_color.y);
        assert!(params.specular_color.z.abs() < 1e-6);
        // A matte base so the added lobe is the whole specular story.
        assert!((material.base.perceptual_roughness - 1.0).abs() < 1e-6);
        assert!(material.base.metallic.abs() < 1e-6);
        assert!(material.base.reflectance.abs() < 1e-6);
    }

    #[test]
    fn missing_maps_clear_their_resample_bits() {
        // A material with neither map applied leaves the normal / specular re-sample
        // bits off (the shader does not sample a map without its bit).
        let mut material = face_material(1.0);
        let _override = apply_legacy_scalars(&mut material, &material_with_alpha(0, 0));
        assert_eq!(material.extension.params.map_flags & MAP_FLAG_NORMAL, 0);
        assert_eq!(material.extension.params.map_flags & MAP_FLAG_SPEC, 0);
    }

    #[test]
    fn a_face_with_no_material_stays_pbr_inert() {
        // Sanity: the inert starting point is PBR mode (the legacy apply is the only
        // thing that flips a face to the Blinn-Phong lobe).
        assert_eq!(face_material(1.0).extension.params.mode, SL_FACE_MODE_PBR);
    }

    #[test]
    fn every_alpha_mode_is_authoritative() {
        // The face's alpha-mode property is honoured for all four modes: NONE (0)
        // and EMISSIVE (3) render opaque, MASK (2) alpha-tests at the cutoff, and
        // BLEND (1) takes the transparent path.
        assert!(matches!(legacy_alpha_override(0, 0), AlphaMode::Opaque));
        assert!(matches!(
            legacy_alpha_override(DIFFUSE_ALPHA_MODE_MASK, 128),
            AlphaMode::Mask(cutoff) if (cutoff - 128.0 / 255.0).abs() < 1e-6
        ));
        assert!(matches!(
            legacy_alpha_override(DIFFUSE_ALPHA_MODE_BLEND, 0),
            AlphaMode::Blend
        ));
        assert!(matches!(legacy_alpha_override(3, 0), AlphaMode::Opaque));
    }

    /// A [`LegacyMaterial`] whose alpha fields are `diffuse_alpha_mode` /
    /// `alpha_mask_cutoff` and everything else the wire default.
    fn material_with_alpha(diffuse_alpha_mode: u8, alpha_mask_cutoff: u8) -> LegacyMaterial {
        LegacyMaterial {
            normal_map: TextureKey::from(Uuid::nil()),
            normal_offset: (0.0, 0.0),
            normal_repeat: (1.0, 1.0),
            normal_rotation: 0.0,
            specular_map: TextureKey::from(Uuid::nil()),
            specular_offset: (0.0, 0.0),
            specular_repeat: (1.0, 1.0),
            specular_rotation: 0.0,
            specular_color: [255, 255, 255, 255],
            specular_exponent: 51,
            environment_intensity: 0,
            diffuse_alpha_mode,
            alpha_mask_cutoff,
        }
    }

    #[test]
    fn translucent_tint_wins_over_the_material_alpha_mode() {
        // R25: a translucent TE tint keeps the face in the blend pass whatever
        // the material's diffuse alpha mode says — the reference ORs
        // `color.a < 0.999` into `is_alpha` before the material-pass dispatch,
        // and the common tinted-transparent-plus-shiny content carries a
        // default (`NONE`) material that would otherwise force it opaque.
        let mut material = face_material(0.5);
        // The apply reports NO override (the tint stays authoritative), so the
        // R22d texture-alpha resolution stays free to act on this face.
        assert!(!apply_legacy_scalars(
            &mut material,
            &material_with_alpha(0, 0)
        ));
        assert!(matches!(material.base.alpha_mode, AlphaMode::Blend));
        // EMISSIVE would also have forced opaque; the tint still wins.
        assert!(!apply_legacy_scalars(
            &mut material,
            &material_with_alpha(3, 0)
        ));
        assert!(matches!(material.base.alpha_mode, AlphaMode::Blend));
    }

    #[test]
    fn opaque_tint_takes_the_material_alpha_mode() {
        // With an opaque tint the material's mode is authoritative, in both
        // directions: NONE forces an alpha-textured face opaque, BLEND forces a
        // previously-opaque face into the transparent path.
        let mut material = face_material(1.0);
        // The apply reports the override, which is what marks the face so the
        // R22d texture-alpha resolution leaves its mode alone thereafter
        // (R25a): a `NONE` material over an alpha texture renders opaque in
        // the reference, whichever of the two applied last.
        assert!(apply_legacy_scalars(
            &mut material,
            &material_with_alpha(0, 0)
        ));
        assert!(matches!(material.base.alpha_mode, AlphaMode::Opaque));
        assert!(apply_legacy_scalars(
            &mut material,
            &material_with_alpha(DIFFUSE_ALPHA_MODE_BLEND, 0),
        ));
        assert!(matches!(material.base.alpha_mode, AlphaMode::Blend));
    }
}
