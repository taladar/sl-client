//! Publish our **own** avatar's client-side bake to the grid (P15.4).
//!
//! On a legacy client-side-baking grid (OpenSim, and any grid without central
//! server baking) the simulator and *other* viewers only see our avatar textured
//! if the *client* uploads its composited bakes and advertises them. P15.3 draped
//! the composite over our own body locally; this module finishes the loop:
//!
//! 1. composite each bake region (`composite_own_region`, the same canonical
//!    bytes P15.3 draped);
//! 2. J2C-encode it as a five-component baked avatar texture
//!    ([`sl_texture::encode_baked_avatar_j2c`]) and upload it over the legacy
//!    `UploadBakedTexture` capability, one region at a time (the upload reply
//!    carries no correlation id, so the uploads are serialised);
//! 3. once every region is uploaded, advertise the baked-texture ids in an
//!    `AgentSetAppearance` ([`Command::SetAppearance`]) so the sim broadcasts our
//!    textured avatar to other observers.
//!
//! Before compositing anything, the region's cache is asked which bakes it
//! already holds (`AgentCachedTexture`, as the reference's
//! `LLAgentWearables::queryWearableCache` does) under each slot's wearables
//! hash ([`avatar_texture::wearables_hash`]); a slot it names takes the named
//! texture and is neither composited nor uploaded, and the publish names each
//! slot's hash as its cache id so the next query can hit. OpenSim keeps that
//! cache for the session only (`book/src/gridspec/appearance.md` § Baking), so
//! what it saves is the re-upload of every slot an outfit change leaves alone.
//!
//! A publish runs once per outfit — again whenever the worn wearables change —
//! gated on the region advertising `UploadBakedTexture`: Second Life bakes
//! centrally (P14) and does not advertise that capability, so this runs only
//! where the legacy path is the live one (OpenSim). The advertised appearance now carries the avatar's **real** visual
//! parameters resolved from its worn wearables ([`OwnBakeInputs::visual_params`],
//! R12) — publishing an all-`128` placeholder there deformed our own body,
//! because the sim stores and rebroadcasts it and we render our own shape from
//! that broadcast.

use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;
use sl_client_bevy::{
    BakeRegion, CAP_UPLOAD_BAKED_TEXTURE, Command, JointOverrides, ResolvedParams,
    SkeletalDeformations, SlCapabilities, SlCommand, SlEvent, SlSessionEvent, TextureEntry,
    TextureFace, TextureKey, Uuid, Vector, WearableType, avatar_texture, encode_texture_entry,
};
use sl_texture::encode_baked_avatar_j2c;

use crate::avatars::composite_own_region;
use crate::bake_inputs::OwnBakeInputs;
use sl_viewer_kit::avatar_assets::AvatarAssetLibrary;

/// How long, in seconds, to wait for one region's `UploadBakedTexture` reply
/// before giving up on it, so a lost reply cannot wedge the publish.
const UPLOAD_TIMEOUT_SECS: f32 = 30.0;

/// How long, in seconds, to wait for the region's answer to the cache query
/// before baking every slot, so a region that never answers cannot wedge the
/// publish.
const QUERY_TIMEOUT_SECS: f32 = 10.0;

/// The fallback advertised avatar bounding box (metres), used only in
/// placeholder-sphere mode (no character assets to compute a real height
/// from). The X/Y are the reference's `DEFAULT_AGENT_DEPTH` / `_WIDTH` —
/// `computeBodySize` never measures those ("TODO -- measure the real depth
/// and width") — and the real path keeps them while replacing Z.
///
/// The advertised Z is **load-bearing** (R23): OpenSim's `SetSize` takes the
/// client's word for the avatar height, sizes the physics capsule with it, and
/// reports the avatar's wire Z as the capsule *centre* — so a Z that disagrees
/// with the height the viewer renders (`body_size_metrics` of the same shape)
/// plants the avatar off the ground by half the difference. A fixed `1.9` here
/// floated the own avatar by a few centimetres.
const AVATAR_SIZE: Vector = Vector {
    x: 0.45,
    y: 0.6,
    z: 1.9,
};

/// The advertised avatar bounding box for the published shape: the reference's
/// `mBodySize` — depth / width fixed ([`AVATAR_SIZE`]), height the
/// `computeBodySize` body size of exactly the `visual_params` being published,
/// so the simulator's capsule height matches the height this viewer renders
/// the avatar at (R23). Firestorm on OpenSim advertises plain `mBodySize` too
/// (its Havok hover adjustment is Second Life-only). Worn-mesh joint overrides
/// are not folded in here — the publish happens at login, before any rigged
/// mesh resolves; the reference's later corrective re-sends are a follow-up.
fn advertised_size(library: &AvatarAssetLibrary, visual_params: &[u8]) -> Vector {
    let resolved = ResolvedParams::from_appearance(library.params(), visual_params);
    let deform = SkeletalDeformations::from_resolved(library.params(), &resolved);
    library
        .skeleton()
        .body_size_metrics(&deform, &JointOverrides::default())
        .map_or(AVATAR_SIZE, |metrics| Vector {
            z: metrics.body_size_z,
            ..AVATAR_SIZE
        })
}

/// The number of neutral visual parameters advertised (a full modern set); the
/// simulator accepts any length, and the exact count is not load-bearing.
const VISUAL_PARAM_COUNT: usize = 253;

/// A neutral (mid-range) visual-parameter value, so the published appearance is a
/// plausible avatar rather than an all-minimum deformation.
const NEUTRAL_VISUAL_PARAM: u8 = 128;

/// The stage of one own-avatar bake publish.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum PublishStage {
    /// Waiting for the bake inputs and the `UploadBakedTexture` capability.
    #[default]
    Idle,
    /// Waiting for the region's answer to the cache query of this serial.
    Querying(i32),
    /// Encoding and uploading each region's bake, one at a time.
    Uploading,
    /// Every slot baked or found in the cache, and the appearance published
    /// (or nothing to do).
    Done,
}

/// Our own avatar's client-side bake publish state (P15.4): the outfit it is
/// for, the cache query, the upload queue, the slot awaiting a reply, and the
/// baked-texture id of every slot so far.
#[derive(Debug, Resource, Default)]
pub struct OwnBakePublish {
    /// The publish stage.
    stage: PublishStage,
    /// Whether the current region advertises the legacy `UploadBakedTexture`
    /// capability (present on OpenSim, absent on server-baking Second Life).
    upload_cap: bool,
    /// The worn wearables the current (or last) publish is of; a different
    /// set starts the next one.
    outfit: Option<Vec<(WearableType, Uuid)>>,
    /// Each baked slot's wearables hash for [`outfit`](Self::outfit) — the
    /// cache id it is asked and published under — where a wearable feeds it.
    hashes: HashMap<usize, Uuid>,
    /// The serial of the last cache query sent.
    query_serial: i32,
    /// The serial of the last `AgentSetAppearance` sent.
    publish_serial: u32,
    /// The bake regions still to encode and upload.
    pending: VecDeque<BakeRegion>,
    /// The baked slot currently awaiting an `AssetUploaded` / `AssetUploadFailed`
    /// reply (`None` when free to start the next region).
    in_flight: Option<usize>,
    /// The wall-clock deadline (`Time::elapsed_secs`) for the in-flight upload
    /// or the cache query.
    deadline: Option<f32>,
    /// The baked-texture id per baked slot: uploaded, or named by the cache.
    uploaded: HashMap<usize, TextureKey>,
    /// How many bakes this session has uploaded.
    upload_count: usize,
}

impl OwnBakePublish {
    /// The baked textures the publish named in the own appearance, sorted —
    /// empty until it has published, and on a grid that bakes centrally.
    #[must_use]
    pub fn published(&self) -> Vec<Uuid> {
        if self.stage != PublishStage::Done {
            return Vec::new();
        }
        let mut ids: Vec<Uuid> = self.uploaded.values().map(|baked| baked.uuid()).collect();
        ids.sort_unstable();
        ids
    }

    /// How many bakes this session has uploaded — what a cache hit saves.
    #[must_use]
    pub const fn uploads(&self) -> usize {
        self.upload_count
    }

    /// Starts the publish of `outfit`: hashes each baked slot it feeds and
    /// asks the region's cache for them under `serial`, or — where no slot
    /// has a hash to ask by — goes straight to baking.
    fn start(
        &mut self,
        outfit: Vec<(WearableType, Uuid)>,
        now: f32,
        writer: &mut MessageWriter<SlCommand>,
    ) {
        self.hashes = BakeRegion::ALL
            .into_iter()
            .filter_map(|region| {
                let slot = region.slot();
                avatar_texture::wearables_hash(slot, &outfit).map(|hash| (slot, hash))
            })
            .collect();
        self.outfit = Some(outfit);
        self.uploaded.clear();
        self.in_flight = None;
        self.pending = BakeRegion::ALL.into_iter().collect();
        let mut slots: Vec<(Uuid, u8)> = self
            .hashes
            .iter()
            .filter_map(|(&slot, &hash)| Some((hash, u8::try_from(slot).ok()?)))
            .collect();
        if slots.is_empty() {
            self.deadline = None;
            self.stage = PublishStage::Uploading;
            return;
        }
        slots.sort_unstable_by_key(|&(_hash, slot)| slot);
        self.query_serial = self.query_serial.wrapping_add(1);
        writer.write(SlCommand(Command::RequestCachedTextures {
            serial: self.query_serial,
            slots,
        }));
        self.deadline = Some(now + QUERY_TIMEOUT_SECS);
        self.stage = PublishStage::Querying(self.query_serial);
        debug!(
            "asking the region's bake cache ({} slot(s))",
            self.hashes.len()
        );
    }

    /// Folds in the cache's answer to the query of `serial`: every slot it
    /// names takes that texture and is struck from the bake queue.
    fn answered(&mut self, serial: i32, textures: &[(u8, Uuid)]) {
        if self.stage != PublishStage::Querying(serial) {
            return;
        }
        for &(index, texture) in textures {
            let slot = usize::from(index);
            if texture.is_nil() || !self.hashes.contains_key(&slot) {
                continue;
            }
            let _previous = self.uploaded.insert(slot, TextureKey::from(texture));
            self.pending.retain(|region| region.slot() != slot);
        }
        info!(
            "the region's bake cache holds {} of {} slot(s)",
            self.uploaded.len(),
            self.hashes.len()
        );
        self.deadline = None;
        self.stage = PublishStage::Uploading;
    }
}

/// A full neutral visual-parameter set for the appearance publish.
fn neutral_visual_params() -> Vec<u8> {
    vec![NEUTRAL_VISUAL_PARAM; VISUAL_PARAM_COUNT]
}

/// The appearance a publish sends: a full 45-face avatar [`TextureEntry`]
/// whose baked slots name their texture (every other face nil), and the cache
/// id of each — its wearables hash, or nil for a slot no wearable feeds.
fn publish_payload(
    uploaded: &HashMap<usize, TextureKey>,
    hashes: &HashMap<usize, Uuid>,
) -> (TextureEntry, Vec<(Uuid, u8)>) {
    let nil = TextureFace::new(TextureKey::from(Uuid::nil()));
    let mut faces = vec![nil; avatar_texture::COUNT];
    let mut wearable_cache = Vec::new();
    for (&slot, &baked) in uploaded {
        if let Some(face) = faces.get_mut(slot) {
            *face = TextureFace::new(baked);
        }
        if let Ok(slot_u8) = u8::try_from(slot) {
            let cache_id = hashes.get(&slot).copied().unwrap_or_else(Uuid::nil);
            wearable_cache.push((cache_id, slot_u8));
        }
    }
    wearable_cache.sort_unstable_by_key(|&(_id, slot)| slot);
    (TextureEntry { faces }, wearable_cache)
}

/// Advertise the baked slots in an `AgentSetAppearance` of `serial`
/// ([`publish_payload`]), with the avatar's real `visual_params` (resolved
/// from its worn wearables, R12).
fn publish_appearance(
    publish: &OwnBakePublish,
    serial: u32,
    visual_params: Vec<u8>,
    size: Vector,
    writer: &mut MessageWriter<SlCommand>,
) {
    if publish.uploaded.is_empty() {
        debug!("client-side bake produced no baked regions; nothing to publish");
        return;
    }
    let (entry, wearable_cache) = publish_payload(&publish.uploaded, &publish.hashes);
    writer.write(SlCommand(Command::SetAppearance {
        serial,
        size,
        texture_entry: encode_texture_entry(&entry),
        visual_params,
        wearable_cache,
    }));
    info!(
        "published client-side bake appearance ({} baked slot(s))",
        publish.uploaded.len()
    );
}

/// Drive the client-side bake publish (P15.4): once the bake inputs of an
/// outfit not yet published are ready and the region advertises
/// `UploadBakedTexture`, ask the region's cache for its bakes, encode and
/// upload each region's bake the cache did not name one at a time, then
/// publish an `AgentSetAppearance` naming them all.
pub(crate) fn drive_bake_publish(
    time: Res<Time>,
    inputs: Res<OwnBakeInputs>,
    library: Option<Res<AvatarAssetLibrary>>,
    mut capabilities: MessageReader<SlCapabilities>,
    mut events: MessageReader<SlEvent>,
    mut publish: ResMut<OwnBakePublish>,
    mut writer: MessageWriter<SlCommand>,
) {
    // Track whether the legacy upload capability is advertised.
    for SlCapabilities(map) in capabilities.read() {
        publish.upload_cap = map.contains_key(CAP_UPLOAD_BAKED_TEXTURE);
    }

    // Fold in the reply to the in-flight upload, if any.
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::AssetUploaded { new_asset, .. } => {
                if let Some(slot) = publish.in_flight.take() {
                    let _prev = publish.uploaded.insert(slot, TextureKey::from(*new_asset));
                    publish.upload_count = publish.upload_count.saturating_add(1);
                    publish.deadline = None;
                    debug!("uploaded baked texture for slot {slot} -> {new_asset}");
                }
            }
            SlSessionEvent::AssetUploadFailed { reason } => {
                if let Some(slot) = publish.in_flight.take() {
                    warn!("baked-texture upload for slot {slot} failed: {reason}");
                    publish.deadline = None;
                }
            }
            SlSessionEvent::CachedTextureResponse { serial, textures } => {
                publish.answered(*serial, textures);
            }
            _other => {}
        }
    }

    match publish.stage {
        PublishStage::Idle | PublishStage::Done => {
            // Second Life advertises no `UploadBakedTexture` (it bakes centrally),
            // so gating on the capability keeps this to the legacy grids that need
            // it (OpenSim).
            if !inputs.is_ready() || !publish.upload_cap {
                return;
            }
            let outfit = inputs.worn_assets();
            if publish.outfit.as_ref() == Some(&outfit) {
                return;
            }
            info!("publishing client-side bake (UploadBakedTexture) for own avatar");
            publish.start(outfit, time.elapsed_secs(), &mut writer);
        }
        PublishStage::Querying(_serial) => {
            // A region that never answers costs a bake of every slot, not the
            // publish.
            if publish
                .deadline
                .is_some_and(|deadline| time.elapsed_secs() >= deadline)
            {
                warn!("the region never answered the bake cache query; baking every slot");
                publish.deadline = None;
                publish.stage = PublishStage::Uploading;
            }
        }
        PublishStage::Uploading => {
            // Abandon a stuck in-flight upload so the queue keeps moving.
            if let (Some(slot), Some(deadline)) = (publish.in_flight, publish.deadline)
                && time.elapsed_secs() >= deadline
            {
                warn!("baked-texture upload for slot {slot} timed out; skipping it");
                publish.in_flight = None;
                publish.deadline = None;
            }
            // One region in flight at a time (the reply carries no correlation id).
            if publish.in_flight.is_some() {
                return;
            }
            // Encode and start the next region with worn layers (one per frame, so
            // the encode cost is spread across frames).
            while let Some(region) = publish.pending.pop_front() {
                let Some(decoded) = composite_own_region(&inputs, region) else {
                    continue;
                };
                // A bake is a *five*-component (`R G B alpha mask`) codestream,
                // not an ordinary texture: the reference viewer reads the fifth
                // plane as the morph mask for the head, upper and lower bakes,
                // and when that read fails it discards the colour decode with
                // it and marks the texture missing — so a bake uploaded as an
                // ordinary opaque texture (three components) would make every
                // reference viewer see this avatar as an unbaked cloud. The
                // mask is all-`255` — what the reference's `gatherMorphMaskAlpha`
                // starts from before each worn layer subtracts its coverage —
                // because nothing here computes clothing morph masks yet
                // ([[viewer-bake-publish-morph-mask]]); that costs the
                // clothing-driven body morphs on observers, not the bake.
                let morph_mask = vec![u8::MAX; decoded.pixels.len().checked_div(4).unwrap_or(0)];
                match encode_baked_avatar_j2c(&decoded, &morph_mask) {
                    Ok(data) => {
                        let bytes = data.len();
                        writer.write(SlCommand(Command::UploadBakedTexture { data }));
                        publish.in_flight = Some(region.slot());
                        publish.deadline = Some(time.elapsed_secs() + UPLOAD_TIMEOUT_SECS);
                        debug!("uploading {} bake ({bytes} bytes)", region.name());
                        return;
                    }
                    Err(error) => {
                        warn!("failed to J2C-encode {} bake: {error}", region.name());
                    }
                }
            }
            // The queue is drained and nothing is in flight: publish the
            // appearance with the avatar's real worn shape (R12) and the body
            // size that shape computes to (R23), or neutral fallbacks if the
            // character assets are absent (placeholder-sphere mode).
            let (visual_params, size) = match library.as_ref() {
                Some(library) => {
                    let visual_params = inputs.visual_params(library.params());
                    let size = advertised_size(library, &visual_params);
                    (visual_params, size)
                }
                None => (neutral_visual_params(), AVATAR_SIZE),
            };
            publish.publish_serial = publish.publish_serial.wrapping_add(1).max(1);
            let serial = publish.publish_serial;
            publish_appearance(&publish, serial, visual_params, size, &mut writer);
            publish.stage = PublishStage::Done;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OwnBakePublish, PublishStage, publish_payload};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{BakeRegion, TextureKey, Uuid, avatar_texture};

    /// What the publish names is read out only once it has published, and
    /// sorted whatever order the regions were uploaded in.
    #[test]
    fn the_published_bakes_are_read_out_once_published_and_sorted() {
        let mut publish = OwnBakePublish::default();
        publish
            .uploaded
            .insert(8, TextureKey::from(Uuid::from_u128(0xB)));
        publish
            .uploaded
            .insert(9, TextureKey::from(Uuid::from_u128(0xA)));
        publish.stage = PublishStage::Uploading;
        assert_eq!(publish.published(), Vec::<Uuid>::new(), "still uploading");
        publish.stage = PublishStage::Done;
        assert_eq!(
            publish.published(),
            vec![Uuid::from_u128(0xA), Uuid::from_u128(0xB)]
        );
    }

    /// The cache's answer puts every texture it names on its slot and strikes
    /// that slot from the bake queue; a nil answer, a slot not asked by its
    /// hash and an answer to another query leave the queue alone.
    #[test]
    fn a_cache_answer_strikes_the_slots_it_names() {
        let mut publish = OwnBakePublish {
            pending: BakeRegion::ALL.into_iter().collect(),
            stage: PublishStage::Querying(7),
            ..OwnBakePublish::default()
        };
        for slot in [avatar_texture::HEAD_BAKED, avatar_texture::UPPER_BAKED] {
            let _hash = publish.hashes.insert(slot, Uuid::from_u128(0xCAC4E));
        }
        let head = Uuid::from_u128(0x4EAD);
        let answer = [
            (u8::try_from(avatar_texture::HEAD_BAKED).unwrap_or(0), head),
            (
                u8::try_from(avatar_texture::UPPER_BAKED).unwrap_or(0),
                Uuid::nil(),
            ),
            (u8::try_from(avatar_texture::LOWER_BAKED).unwrap_or(0), head),
        ];
        publish.answered(6, &answer);
        assert_eq!(
            publish.stage,
            PublishStage::Querying(7),
            "another query's answer"
        );
        publish.answered(7, &answer);
        assert_eq!(publish.stage, PublishStage::Uploading);
        assert_eq!(
            publish.uploaded.get(&avatar_texture::HEAD_BAKED),
            Some(&TextureKey::from(head))
        );
        assert_eq!(publish.uploaded.len(), 1, "only the hit");
        let queued: Vec<usize> = publish.pending.iter().map(|region| region.slot()).collect();
        assert!(
            !queued.contains(&avatar_texture::HEAD_BAKED),
            "the hit is not baked"
        );
        assert!(
            queued.contains(&avatar_texture::UPPER_BAKED),
            "a miss is baked"
        );
        assert!(
            queued.contains(&avatar_texture::LOWER_BAKED),
            "a slot not asked by its hash is baked"
        );
    }

    /// A publish names each baked slot's texture and, as its cache id, the
    /// slot's wearables hash — nil for a slot no wearable feeds.
    #[test]
    fn a_publish_names_each_slot_by_its_wearables_hash() {
        let mut uploaded = std::collections::HashMap::new();
        let _head = uploaded.insert(
            avatar_texture::HEAD_BAKED,
            TextureKey::from(Uuid::from_u128(0xB1)),
        );
        let _skirt = uploaded.insert(
            avatar_texture::SKIRT_BAKED,
            TextureKey::from(Uuid::from_u128(0xB2)),
        );
        let hash = Uuid::from_u128(0xCAC4E);
        let hashes = std::collections::HashMap::from([(avatar_texture::HEAD_BAKED, hash)]);
        let (entry, cache) = publish_payload(&uploaded, &hashes);
        assert_eq!(
            cache,
            vec![
                (hash, u8::try_from(avatar_texture::HEAD_BAKED).unwrap_or(0)),
                (
                    Uuid::nil(),
                    u8::try_from(avatar_texture::SKIRT_BAKED).unwrap_or(0)
                ),
            ]
        );
        assert_eq!(
            entry
                .faces
                .get(avatar_texture::HEAD_BAKED)
                .map(|face| face.texture_id.uuid()),
            Some(Uuid::from_u128(0xB1))
        );
        assert_eq!(entry.faces.len(), avatar_texture::COUNT);
    }
}
