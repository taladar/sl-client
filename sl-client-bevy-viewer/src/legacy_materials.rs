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
//! Mapping onto `StandardMaterial` is necessarily approximate (the reference's
//! `LLMaterial` is a separate specular-workflow shader): the **normal map** is the
//! faithful part; the specular map texture and the per-map UV transforms are not
//! expressible on a `StandardMaterial` and are dropped, with the specular colour /
//! glossiness / environment intensity folded into the scalar `reflectance` /
//! `perceptual_roughness` (`metallic`-workflow) fields "where possible" (the
//! roadmap's wording).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sl_client_bevy::{
    Command, DecodedTexture, LegacyMaterial, Priority, SlCommand, SlEvent, SlSessionEvent,
    TextureKey, Uuid,
};

use crate::materials::ObjectRenderMaterials;
use crate::objects::{FaceTextureDebug, PrimFaceEntity};
use crate::render_priority::TERRAIN_BOOST_PRIORITY;
use crate::textures::TextureManager;

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
#[derive(Resource, Default)]
pub(crate) struct LegacyMaterialManager {
    /// Successfully fetched materials by their 16-byte id, shared across every
    /// face using the material so it is fetched once.
    decoded: HashMap<Uuid, LegacyMaterial>,
    /// Face [`StandardMaterial`] handles waiting for a material to arrive, keyed
    /// by the material id they requested; drained by [`apply_legacy_materials`]
    /// once the material decodes.
    pending_faces: HashMap<Uuid, Vec<Handle<StandardMaterial>>>,
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
    texture_pending: HashMap<TextureKey, Vec<Handle<StandardMaterial>>>,
    /// Face materials whose `alpha_mode` a legacy material has **overridden**
    /// (an opaque-tint face whose `LLMaterial` diffuse alpha mode applied): the
    /// material's mode is authoritative from then on, so the R22d texture-alpha
    /// resolution must not "upgrade" it when the diffuse texture decodes later
    /// (`NONE` means opaque in the reference even over an alpha texture — the
    /// outcome used to depend on which of the two applied last, R25a). Entries
    /// for despawned faces go stale harmlessly (asset ids are not reused) and
    /// are dropped with the manager at session end.
    alpha_overridden: HashSet<AssetId<StandardMaterial>>,
}

impl LegacyMaterialManager {
    /// The decoded legacy material for `id`, if its `RenderMaterials` fetch has
    /// succeeded — a read-only lookup for the pick diagnostic (R25), which uses
    /// it to tell a fetched-but-opaque material from a missing fetch.
    pub(crate) fn decoded_material(&self, id: &Uuid) -> Option<&LegacyMaterial> {
        self.decoded.get(id)
    }

    /// Whether a legacy material has overridden this face material's alpha
    /// mode, making that mode authoritative over the R22d texture-alpha
    /// resolution (see [`Self::alpha_overridden`]).
    pub(crate) fn is_alpha_overridden(&self, id: AssetId<StandardMaterial>) -> bool {
        self.alpha_overridden.contains(&id)
    }

    /// Register a face material handle against its legacy material id: park the
    /// handle until the material arrives and queue the id for fetch if it is not
    /// already known / requested.
    fn register(&mut self, handle: Handle<StandardMaterial>, material_id: Uuid) {
        self.pending_faces
            .entry(material_id)
            .or_default()
            .push(handle);
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
}

/// Build a Bevy [`Image`] for a legacy normal map from decoded RGBA8 pixels, in
/// the linear colour space a normal map needs (`Rgba8Unorm`) and with the
/// repeating sampler object faces tile their textures with.
pub(crate) fn build_linear_image(decoded: &Arc<DecodedTexture>) -> Image {
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

/// The `perceptual_roughness` a legacy material's specular exponent (glossiness,
/// `0..=255`) maps to: a glossier surface (higher exponent) is smoother (lower
/// roughness). Clamped to a small minimum so a maximally-glossy face keeps a
/// pinpoint highlight rather than a singular mirror.
fn roughness_from_glossiness(glossiness: u8) -> f32 {
    let roughness = 1.0 - f32::from(glossiness) / 255.0;
    roughness.clamp(0.05, 1.0)
}

/// The `reflectance` a legacy material's environment-reflection intensity
/// (`0..=255`) maps to: linearly, so a zero-environment material is matte (its
/// surface detail still shows through the normal map) and a full-environment one
/// is fully reflective.
fn reflectance_from_environment(environment_intensity: u8) -> f32 {
    f32::from(environment_intensity) / 255.0
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
pub(crate) fn register_legacy_materials(
    mut manager: ResMut<LegacyMaterialManager>,
    new_faces: Query<
        (
            &MeshMaterial3d<StandardMaterial>,
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
        // A PBR GLTF material on the same face supersedes the legacy material.
        if let Ok(holder) = pbr_holders.get(child_of.parent())
            && holder
                .faces
                .iter()
                .any(|(index, _id)| usize::from(*index) == face_index)
        {
            if legacy_log_enabled() {
                info!(
                    "legacy: face {face_index} material {material_id} superseded by PBR \
                     (handle {:?})",
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
pub(crate) fn drive_legacy_material_requests(
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
pub(crate) fn receive_legacy_materials(
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
pub(crate) fn apply_legacy_materials(
    mut manager: ResMut<LegacyMaterialManager>,
    mut textures: ResMut<TextureManager>,
    mut materials: ResMut<Assets<StandardMaterial>>,
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

/// Write one legacy material's scalar fields onto a face [`StandardMaterial`],
/// returning whether the material's diffuse alpha mode **overrode** the face's
/// `alpha_mode` — the caller records that ([`LegacyMaterialManager::
/// alpha_overridden`]) so the R22d texture-alpha resolution leaves the mode
/// alone from then on, whichever of the two applies last (R25a).
///
/// The pure half of [`apply_legacy_to_face`], split out so it is reachable without
/// a fetch behind it: everything here is a function of the decoded material, while
/// the normal map is a grid asset the caller has to go and get. That is what lets
/// [`crate::render_scene`]'s legacy-material scene exercise the real mapping with
/// no capability, no `TextureManager` and no grid — the registry's rule that
/// construction is separable from transport, applied to this module.
pub(crate) fn apply_legacy_scalars(
    standard: &mut StandardMaterial,
    material: &LegacyMaterial,
) -> bool {
    standard.reflectance = reflectance_from_environment(material.environment_intensity);
    standard.perceptual_roughness = roughness_from_glossiness(material.specular_exponent);
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
    let overrides_alpha = standard.base_color.alpha() >= OPAQUE_TINT_ALPHA;
    if overrides_alpha {
        standard.alpha_mode =
            legacy_alpha_override(material.diffuse_alpha_mode, material.alpha_mask_cutoff);
    }
    // A material that carries no normal map clears any it had previously.
    if material.normal_map.uuid().is_nil() {
        standard.normal_map_texture = None;
    }
    overrides_alpha
}

/// Write one legacy material's scalar fields onto a face [`StandardMaterial`] and
/// queue its normal map for fetch. The normal-map texture is dropped into the
/// material's normal slot later by [`apply_legacy_normal_maps`].
fn apply_legacy_to_face(
    manager: &mut LegacyMaterialManager,
    textures: &mut TextureManager,
    materials: &mut Assets<StandardMaterial>,
    handle: &Handle<StandardMaterial>,
    material: &LegacyMaterial,
) {
    if let Some(mut standard) = materials.get_mut(handle) {
        if apply_legacy_scalars(&mut standard, material) {
            // The material's alpha mode is authoritative for this face now: the
            // R22d texture-alpha resolution must not upgrade it later (R25a).
            let _new = manager.alpha_overridden.insert(handle.id());
        }
        if legacy_log_enabled() {
            info!(
                "legacy: apply to handle {:?}: base_a={:.3} mode_in={} -> alpha_mode={:?}",
                handle.id(),
                standard.base_color.alpha(),
                material.diffuse_alpha_mode,
                standard.alpha_mode,
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
}

/// Drop each decoded normal-map texture into the face materials parked on it:
/// upload the map (linear) and set it as their normal texture. Drains parked
/// faces for any texture that has decoded (freshly or already cached), so it needs
/// no decode message.
pub(crate) fn apply_legacy_normal_maps(
    mut manager: ResMut<LegacyMaterialManager>,
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
        let handles = manager.texture_pending.remove(&id).unwrap_or_default();
        let image = manager.normal_image(&mut images, id, &decoded);
        for handle in handles {
            if let Some(mut standard) = materials.get_mut(&handle) {
                standard.normal_map_texture = Some(image.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glossiness_maps_to_inverse_roughness() {
        // A fully-glossy surface is nearly smooth (clamped off zero); a
        // zero-glossiness one is fully rough.
        assert!((roughness_from_glossiness(255) - 0.05).abs() < 1e-6);
        assert!((roughness_from_glossiness(0) - 1.0).abs() < 1e-6);
        // The reference default exponent (51 = 0.2 * 255) sits in between.
        let mid = roughness_from_glossiness(51);
        assert!(mid > 0.05 && mid < 1.0);
    }

    #[test]
    fn environment_maps_to_reflectance() {
        assert!((reflectance_from_environment(0) - 0.0).abs() < 1e-6);
        assert!((reflectance_from_environment(255) - 1.0).abs() < 1e-6);
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
        let mut standard = StandardMaterial {
            base_color: bevy::color::Color::srgba(1.0, 1.0, 1.0, 0.5),
            alpha_mode: AlphaMode::Blend,
            ..StandardMaterial::default()
        };
        // The apply reports NO override (the tint stays authoritative), so the
        // R22d texture-alpha resolution stays free to act on this face.
        assert!(!apply_legacy_scalars(
            &mut standard,
            &material_with_alpha(0, 0)
        ));
        assert!(matches!(standard.alpha_mode, AlphaMode::Blend));
        // EMISSIVE would also have forced opaque; the tint still wins.
        assert!(!apply_legacy_scalars(
            &mut standard,
            &material_with_alpha(3, 0)
        ));
        assert!(matches!(standard.alpha_mode, AlphaMode::Blend));
    }

    #[test]
    fn opaque_tint_takes_the_material_alpha_mode() {
        // With an opaque tint the material's mode is authoritative, in both
        // directions: NONE forces an alpha-textured face opaque, BLEND forces a
        // previously-opaque face into the transparent path.
        let mut standard = StandardMaterial {
            base_color: bevy::color::Color::srgba(1.0, 1.0, 1.0, 1.0),
            alpha_mode: AlphaMode::Blend,
            ..StandardMaterial::default()
        };
        // The apply reports the override, which is what marks the face so the
        // R22d texture-alpha resolution leaves its mode alone thereafter
        // (R25a): a `NONE` material over an alpha texture renders opaque in
        // the reference, whichever of the two applied last.
        assert!(apply_legacy_scalars(
            &mut standard,
            &material_with_alpha(0, 0)
        ));
        assert!(matches!(standard.alpha_mode, AlphaMode::Opaque));
        assert!(apply_legacy_scalars(
            &mut standard,
            &material_with_alpha(DIFFUSE_ALPHA_MODE_BLEND, 0),
        ));
        assert!(matches!(standard.alpha_mode, AlphaMode::Blend));
    }
}
