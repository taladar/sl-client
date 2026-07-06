//! Publish our **own** avatar's client-side bake to the grid (P15.4).
//!
//! On a legacy client-side-baking grid (OpenSim, and any grid without central
//! server baking) the simulator and *other* viewers only see our avatar textured
//! if the *client* uploads its composited bakes and advertises them. P15.3 draped
//! the composite over our own body locally; this module finishes the loop:
//!
//! 1. composite each bake region ([`composite_own_region`], the same canonical
//!    bytes P15.3 draped);
//! 2. J2C-encode it ([`sl_texture::encode_j2c`]) and upload it over the legacy
//!    `UploadBakedTexture` capability, one region at a time (the upload reply
//!    carries no correlation id, so the uploads are serialised);
//! 3. once every region is uploaded, advertise the baked-texture ids in an
//!    `AgentSetAppearance` ([`Command::SetAppearance`]) so the sim broadcasts our
//!    textured avatar to other observers.
//!
//! The whole thing is a one-shot, gated on the region advertising
//! `UploadBakedTexture`: Second Life bakes centrally (P14) and does not advertise
//! that capability, so this runs only where the legacy path is the live one
//! (OpenSim). Publishing a real *shape* is out of scope — the appearance carries
//! a neutral visual-parameter set (the bake textures are what P15.4 delivers);
//! matching the worn shape is left to the deferred high-level appearance API.

use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;
use sl_client_bevy::{
    BakeRegion, CAP_UPLOAD_BAKED_TEXTURE, Command, SlCapabilities, SlCommand, SlEvent,
    SlSessionEvent, TextureEntry, TextureFace, TextureKey, Uuid, Vector, avatar_texture,
    encode_texture_entry,
};
use sl_texture::encode_j2c;

use crate::avatars::composite_own_region;
use crate::bake_inputs::OwnBakeInputs;

/// How long, in seconds, to wait for one region's `UploadBakedTexture` reply
/// before giving up on it, so a lost reply cannot wedge the publish.
const UPLOAD_TIMEOUT_SECS: f32 = 30.0;

/// The `AgentSetAppearance` serial for the publish. A one-shot per session, so a
/// fixed strictly-positive serial is enough (0 would reset the sequence).
const PUBLISH_SERIAL: u32 = 1;

/// The advertised avatar bounding box (metres) — a plausible human size. Not
/// load-bearing for the bake exchange; a real box is a shape-fidelity follow-up.
const AVATAR_SIZE: Vector = Vector {
    x: 0.45,
    y: 0.6,
    z: 1.9,
};

/// The number of neutral visual parameters advertised (a full modern set); the
/// simulator accepts any length, and the exact count is not load-bearing.
const VISUAL_PARAM_COUNT: usize = 253;

/// A neutral (mid-range) visual-parameter value, so the published appearance is a
/// plausible avatar rather than an all-minimum deformation.
const NEUTRAL_VISUAL_PARAM: u8 = 128;

/// Base for the deterministic per-slot cache ids advertised alongside the bakes.
/// Any non-nil id works — it only has to be self-consistent — so this derives one
/// per slot instead of computing a real wearable-cache hash.
const CACHE_ID_BASE: u128 = 0x5e7a_bace_0000_0000_0000_0000_0000_0000;

/// The stage of the one-shot own-avatar bake publish.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum PublishStage {
    /// Waiting for the bake inputs and the `UploadBakedTexture` capability.
    #[default]
    Idle,
    /// Encoding and uploading each region's bake, one at a time.
    Uploading,
    /// Every region uploaded and the appearance published (or nothing to do).
    Done,
}

/// Our own avatar's client-side bake publish state (P15.4): the upload queue, the
/// slot awaiting a reply, and the baked-texture ids uploaded so far.
#[derive(Resource, Default)]
pub(crate) struct OwnBakePublish {
    /// The publish stage.
    stage: PublishStage,
    /// Whether the current region advertises the legacy `UploadBakedTexture`
    /// capability (present on OpenSim, absent on server-baking Second Life).
    upload_cap: bool,
    /// The bake regions still to encode and upload.
    pending: VecDeque<BakeRegion>,
    /// The baked slot currently awaiting an `AssetUploaded` / `AssetUploadFailed`
    /// reply (`None` when free to start the next region).
    in_flight: Option<usize>,
    /// The wall-clock deadline (`Time::elapsed_secs`) for the in-flight upload.
    deadline: Option<f32>,
    /// The uploaded baked-texture asset id per baked slot.
    uploaded: HashMap<usize, TextureKey>,
}

/// The deterministic cache id advertised for a baked slot.
fn cache_id_for(slot: usize) -> Uuid {
    Uuid::from_u128(CACHE_ID_BASE | u128::try_from(slot).unwrap_or(0))
}

/// A full neutral visual-parameter set for the appearance publish.
fn neutral_visual_params() -> Vec<u8> {
    vec![NEUTRAL_VISUAL_PARAM; VISUAL_PARAM_COUNT]
}

/// Advertise the uploaded bakes in an `AgentSetAppearance`: a full 45-face avatar
/// [`TextureEntry`] whose uploaded baked slots name their new asset id (every
/// other face nil), a neutral visual-parameter set, and one derived cache id per
/// uploaded slot.
fn publish_appearance(
    uploaded: &HashMap<usize, TextureKey>,
    writer: &mut MessageWriter<SlCommand>,
) {
    if uploaded.is_empty() {
        debug!("client-side bake produced no uploaded regions; nothing to publish");
        return;
    }
    let nil = TextureFace::new(TextureKey::from(Uuid::nil()));
    let mut faces = vec![nil; avatar_texture::COUNT];
    let mut wearable_cache = Vec::new();
    for (&slot, &baked) in uploaded {
        if let Some(face) = faces.get_mut(slot) {
            *face = TextureFace::new(baked);
        }
        if let Ok(slot_u8) = u8::try_from(slot) {
            wearable_cache.push((cache_id_for(slot), slot_u8));
        }
    }
    let entry = TextureEntry { faces };
    writer.write(SlCommand(Command::SetAppearance {
        serial: PUBLISH_SERIAL,
        size: AVATAR_SIZE,
        texture_entry: encode_texture_entry(&entry),
        visual_params: neutral_visual_params(),
        wearable_cache,
    }));
    info!(
        "published client-side bake appearance ({} baked slot(s))",
        uploaded.len()
    );
}

/// Drive the one-shot client-side bake publish (P15.4): once the bake inputs are
/// ready and the region advertises `UploadBakedTexture`, encode and upload each
/// region's bake one at a time, then publish an `AgentSetAppearance` naming them.
pub(crate) fn drive_bake_publish(
    time: Res<Time>,
    inputs: Res<OwnBakeInputs>,
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
            _other => {}
        }
    }

    match publish.stage {
        PublishStage::Idle => {
            // Second Life advertises no `UploadBakedTexture` (it bakes centrally),
            // so gating on the capability keeps this to the legacy grids that need
            // it (OpenSim).
            if !inputs.is_ready() || !publish.upload_cap {
                return;
            }
            publish.pending = BakeRegion::ALL.into_iter().collect();
            publish.stage = PublishStage::Uploading;
            info!("publishing client-side bake (UploadBakedTexture) for own avatar");
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
                match encode_j2c(&decoded) {
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
            // The queue is drained and nothing is in flight: publish the appearance.
            publish_appearance(&publish.uploaded, &mut writer);
            publish.stage = PublishStage::Done;
        }
        PublishStage::Done => {}
    }
}
