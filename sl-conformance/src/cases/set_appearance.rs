//! Publish the agent's own appearance over the legacy client-side bake path
//! (`AgentSetAppearance`) and query the simulator's baked-texture cache
//! (`AgentCachedTexture`) — the two wire messages a viewer uses to advertise a
//! locally-composited avatar and to skip re-uploading bakes the grid already
//! has.
//!
//! Two exchanges are exercised:
//!
//! 1. **Baked-texture cache query (verified, both grids).**
//!    [`Command::RequestCachedTextures`] sends an `AgentCachedTexture` for the
//!    classic baked slots (head / upper / lower / eyes / hair); the simulator
//!    answers with an `AgentCachedTextureResponse` surfaced as
//!    [`Event::CachedTextureResponse`] — the request serial echoed back plus one
//!    `(slot, cached texture id)` entry per queried slot (a nil id meaning that
//!    bake is not cached and would have to be uploaded). The case asserts the
//!    reply echoes the serial and carries exactly one entry per queried slot.
//!
//! 2. **Appearance publish.** [`Command::SetAppearance`] sends an
//!    `AgentSetAppearance` advertising a full 45-face avatar `TextureEntry` (the
//!    classic baked slots pointing at a real reference texture), a matching
//!    per-slot cache id, a neutral visual-parameter set and the avatar's bounding
//!    box. The simulator has no direct reply to a *single* avatar's own publish
//!    (the baked result is broadcast only to *other* observers), so on OpenSim
//!    the case re-queries the baked-texture cache afterwards — which both proves
//!    the circuit stayed healthy across the publish and, best-effort, surfaces
//!    whether the grid ingested it (a cache **hit** for the published id, or a
//!    `RebakeAvatarTextures` request for it). That server-side ingestion signal
//!    is recorded as a metric rather than asserted: whether it fires depends on
//!    the region's baked-texture-cache internals (asset presence, root/child
//!    presence on a multi-region grid), so a run is not failed for its absence —
//!    the wire exchange and the deterministic cache-query reply are what the case
//!    guarantees.
//!
//! **Grid divergence.** The legacy client-side bake is OpenSim's live appearance
//! path, so the publish-and-re-query round-trip runs there (green). Modern Second
//! Life bakes centrally (the "Sunshine" server-side bake
//! [`server-appearance-bake`](super::server_appearance_bake) drives), where
//! `AgentSetAppearance` is superseded; there the case exercises the cache query
//! and forms/sends the publish over the wire but records `partial`, because the
//! legacy publish is not how appearance is set on that grid — the mirror of
//! `server-appearance-bake`, which is `partial` on OpenSim.

use std::time::Instant;

use sl_client_tokio::{
    Command, Event, TextureEntry, TextureFace, TextureKey, Throttle, Uuid, Vector, avatar_texture,
    encode_texture_entry,
};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, REPLY_TIMEOUT, check, fixtures, is_aditi};

/// The baked avatar-texture slots the case exercises: the classic body bakes
/// every valid avatar carries (skirt is omitted — it exists only when a skirt is
/// worn, and the universal bakes are viewer-version dependent).
const BAKED_SLOTS: [usize; 5] = [
    avatar_texture::HEAD_BAKED,
    avatar_texture::UPPER_BAKED,
    avatar_texture::LOWER_BAKED,
    avatar_texture::EYES_BAKED,
    avatar_texture::HAIR_BAKED,
];

/// Base for the deterministic per-slot cache ids: the bake's cache hash a viewer
/// would compute from its wearables. Any non-nil id works — the simulator only
/// needs the publish and the re-query to agree — so this derives one per slot
/// instead of computing a real hash.
const CACHE_ID_BASE: u128 = 0x5e7a_bace_0000_0000_0000_0000_0000_0000;

/// The number of visual parameters advertised. The reference viewer sends one
/// quantized byte per tweakable morph; the exact count is not load-bearing (the
/// simulator accepts any length), so a full modern set of neutral midpoints is
/// used rather than a real slider read.
const VISUAL_PARAM_COUNT: usize = 253;

/// A neutral visual-parameter value (mid-range), so the published appearance is
/// a plausible avatar rather than an all-minimum deformation.
const NEUTRAL_VISUAL_PARAM: u8 = 128;

/// The advertised avatar bounding box (metres) — a plausible human size; not
/// load-bearing for the exchange.
const AVATAR_SIZE: Vector = Vector {
    x: 0.45,
    y: 0.6,
    z: 1.9,
};

/// The `AgentCachedTexture` serial for the baseline (pre-publish) query.
const BASELINE_SERIAL: i32 = 1;

/// The `AgentSetAppearance` serial for the publish (strictly increasing across
/// calls; the cache queries have their own serial space).
const PUBLISH_SERIAL: u32 = 1;

/// The `AgentCachedTexture` serial for the post-publish re-query.
const REQUERY_SERIAL: i32 = 2;

/// The deterministic per-slot cache id used both in the publish and the re-query
/// so the simulator can match them.
fn cache_id_for(slot: usize) -> Uuid {
    Uuid::from_u128(CACHE_ID_BASE | u128::try_from(slot).unwrap_or(0))
}

/// The queried slots for the publish/re-query: `(published cache id, slot index)`
/// per classic bake. Slots that do not fit a `u8` are skipped (none do).
fn published_slots() -> Vec<(Uuid, u8)> {
    BAKED_SLOTS
        .iter()
        .filter_map(|&slot| Some((cache_id_for(slot), u8::try_from(slot).ok()?)))
        .collect()
}

/// The queried slots for the baseline query: `(nil cache id, slot index)`, so a
/// fresh cache reports misses.
fn baseline_slots() -> Vec<(Uuid, u8)> {
    BAKED_SLOTS
        .iter()
        .filter_map(|&slot| Some((Uuid::nil(), u8::try_from(slot).ok()?)))
        .collect()
}

/// A full 45-face avatar `TextureEntry` whose classic baked slots all name
/// `baked` (every other face nil), ready to pack into an `AgentSetAppearance`.
fn baked_texture_entry(baked: TextureKey) -> TextureEntry {
    let nil = TextureFace::new(TextureKey::from(Uuid::nil()));
    let mut faces = vec![nil; avatar_texture::COUNT];
    for &slot in &BAKED_SLOTS {
        if let Some(face) = faces.get_mut(slot) {
            *face = TextureFace::new(baked);
        }
    }
    TextureEntry { faces }
}

/// A neutral full visual-parameter set for the appearance publish.
fn neutral_visual_params() -> Vec<u8> {
    vec![NEUTRAL_VISUAL_PARAM; VISUAL_PARAM_COUNT]
}

/// Publishes appearance (`AgentSetAppearance`) and queries the baked-texture
/// cache (`AgentCachedTexture`).
#[derive(Debug)]
pub struct SetAppearance;

impl GridTest for SetAppearance {
    fn name(&self) -> &'static str {
        "set-appearance"
    }

    fn description(&self) -> &'static str {
        "Publish appearance (AgentSetAppearance) and query the baked-texture cache"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            // A real, permanently-served texture to advertise on the baked slots,
            // so the published appearance names an asset both grids actually hold.
            let reference = fixtures::plywood_texture()?;

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            // --- Baked-texture cache query (both grids) ---
            // Query the classic bakes; the simulator answers an
            // `AgentCachedTextureResponse` echoing the serial with one entry per
            // queried slot. Region-active drains any login-time traffic, so this is
            // a genuine reply to our request.
            let baseline_slots = baseline_slots();
            let queried = i64::try_from(baseline_slots.len()).unwrap_or(-1);
            let published_slots = published_slots();
            session
                .send(Command::RequestCachedTextures {
                    serial: BASELINE_SERIAL,
                    slots: baseline_slots.clone(),
                })
                .await?;

            // Modern Second Life bakes centrally: the legacy `AgentCachedTexture`
            // may go unanswered and `AgentSetAppearance` is superseded. So on aditi
            // the query is best-effort, the publish is a wire exercise, and the run
            // records partial — the mirror of `server-appearance-bake`.
            if is_aditi(grid) {
                let answered = session
                    .wait_for(REPLY_TIMEOUT, |event| match event {
                        Event::CachedTextureResponse { serial, .. }
                            if *serial == BASELINE_SERIAL =>
                        {
                            Some(())
                        }
                        _other => None,
                    })
                    .await
                    .is_ok();
                let entry = baked_texture_entry(reference);
                session
                    .send(Command::SetAppearance {
                        serial: PUBLISH_SERIAL,
                        size: AVATAR_SIZE,
                        texture_entry: encode_texture_entry(&entry),
                        visual_params: neutral_visual_params(),
                        wearable_cache: published_slots.clone(),
                    })
                    .await?;
                let metrics = ctx.metrics();
                metrics.set("queried_slots", queried);
                metrics.set("cache_query_answered", i64::from(answered));
                ctx.mark_partial(
                    "server-side baking — AgentSetAppearance is superseded by central \
                     baking (see server-appearance-bake); exercised the legacy \
                     AgentCachedTexture query and formed the publish over the wire, \
                     but neither is how appearance is set on this grid",
                );
                return Ok(());
            }

            // OpenSim uses the legacy path, so the cache query is authoritative:
            // assert the reply echoes the serial and carries one entry per slot.
            let baseline = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::CachedTextureResponse { serial, textures }
                        if *serial == BASELINE_SERIAL =>
                    {
                        Some(textures.clone())
                    }
                    _other => None,
                })
                .await?;
            check(
                baseline.len() == baseline_slots.len(),
                &format!(
                    "cache response returned {} entries for {} queried slots",
                    baseline.len(),
                    baseline_slots.len()
                ),
            )?;
            let baseline_hits = baseline.iter().filter(|(_slot, id)| !id.is_nil()).count();
            let baseline_hits_metric = i64::try_from(baseline_hits).unwrap_or(-1);

            // --- Appearance publish ---
            // Advertise a full avatar TextureEntry with the reference texture on
            // every classic baked slot, a matching per-slot cache id, a neutral
            // visual-parameter set and the avatar's bounding box.
            let entry = baked_texture_entry(reference);
            session
                .send(Command::SetAppearance {
                    serial: PUBLISH_SERIAL,
                    size: AVATAR_SIZE,
                    texture_entry: encode_texture_entry(&entry),
                    visual_params: neutral_visual_params(),
                    wearable_cache: published_slots.clone(),
                })
                .await?;

            // Re-query the baked-texture cache with the same per-slot cache ids. A
            // well-formed reply proves the circuit stayed healthy across the
            // publish; best-effort, the reply (or a trailing rebake request)
            // reveals whether the grid ingested it.
            let reference_id = reference.uuid();
            let start = Instant::now();
            session
                .send(Command::RequestCachedTextures {
                    serial: REQUERY_SERIAL,
                    slots: published_slots.clone(),
                })
                .await?;
            let mut rebake_id: Option<Uuid> = None;
            let response = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::RebakeAvatarTextures { texture_id } => {
                        rebake_id = Some(texture_id.uuid());
                        None
                    }
                    Event::CachedTextureResponse { serial, textures }
                        if *serial == REQUERY_SERIAL =>
                    {
                        Some(textures.clone())
                    }
                    _other => None,
                })
                .await?;
            let verify_secs = start.elapsed().as_secs_f64();
            check(
                response.len() == published_slots.len(),
                &format!(
                    "re-query returned {} entries for {} queried slots",
                    response.len(),
                    published_slots.len()
                ),
            )?;
            let hits = response
                .iter()
                .filter(|(_slot, id)| *id == reference_id)
                .count();

            // A rebake may trail the cache reply; if neither is seen yet, give the
            // rebake a brief window so the ingestion metric is complete.
            if hits == 0 && rebake_id.is_none() {
                rebake_id = session
                    .wait_for(REPLY_TIMEOUT, |event| match event {
                        Event::RebakeAvatarTextures { texture_id } => Some(texture_id.uuid()),
                        _other => None,
                    })
                    .await
                    .ok();
            }
            let rebake_matched = rebake_id == Some(reference_id);
            let ingestion_confirmed = hits >= 1 || rebake_matched;

            let metrics = ctx.metrics();
            metrics.set("queried_slots", queried);
            metrics.set("baseline_hits", baseline_hits_metric);
            metrics.set_timing("verify_secs", verify_secs);
            metrics.set("requery_hits", i64::try_from(hits).unwrap_or(-1));
            metrics.set("rebake_requested", i64::from(rebake_matched));
            metrics.set("ingestion_confirmed", i64::from(ingestion_confirmed));

            Ok(())
        })
    }
}
