//! Avatar-state capture (viewer-avatar-state-dump-replay): on **Ctrl+Alt+D**,
//! write each nearby avatar's full render *inputs* — the avatar object and its
//! whole attachment tree (verbatim wire [`Object`]s), the decoded
//! [`AvatarAppearance`] (visual params + baked-texture entry) and the animations
//! it is playing — to a `<agent>.json` manifest, plus copy every mesh / texture /
//! animation those inputs reference out of the on-disk caches into the bundle's
//! `cache/` subtree (a drop-in cache). The [replay mode](crate::avatar_replay)
//! then rebuilds and *renders* the avatar offline, so a render-only defect can be
//! reproduced (and a fix tested) after the avatar has logged out or changed.
//!
//! Opt-in and non-interfering: the capture store and its systems are only added
//! when `SL_VIEWER_DUMP_DIR` is set (see `crate::run`), so a normal session
//! pays nothing. The heavy geometry/textures/animations are copied from the
//! viewer's own caches, keyed by the ids in the captured events.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, AvatarAppearance, CAP_GET_TEXTURE, MAX_FACES, Object, PlayingAnimation,
    RenderMaterialEntry, ScopedObjectId, SlCapabilities, SlEvent, SlIdentity, SlSessionEvent, Uuid,
    decode_texture_entry, pcode,
};

use crate::replay_bundle::{
    MANIFEST_VERSION, ReplayManifest, copy_cache_assets, run_texture_fetch, texture_fetch_urls,
};

/// The retained raw session events a capture needs: the avatar objects and their
/// attachment/linkset prims, each avatar's latest appearance, and each avatar's
/// latest animation set. Folded from the [`SlEvent`] stream every frame (only
/// while capture is enabled), so a **Ctrl+Alt+D** at any moment has a coherent,
/// verbatim snapshot to serialise.
#[derive(Debug, Resource, Default)]
pub struct ReplayCaptureStore {
    /// Every retained object by its scoped id: avatars ([`pcode::AVATAR`]) and
    /// any parented prim (an attachment root, or a child of one / of a linkset).
    objects: HashMap<ScopedObjectId, Object>,
    /// Each avatar's latest decoded appearance (visual params + baked textures).
    appearances: HashMap<AgentKey, AvatarAppearance>,
    /// Each avatar's latest playing-animation set.
    animations: HashMap<AgentKey, Vec<PlayingAnimation>>,
    /// Every legacy (`RenderMaterials`) `LLMaterial` resolved this session, keyed
    /// by its material id — so a dump can carry the ones its faces reference (the
    /// offline session cannot fetch the `RenderMaterials` cap).
    render_materials: HashMap<Uuid, RenderMaterialEntry>,
    /// The region's `GetTexture` capability URL, so the dump can fetch each
    /// referenced texture at full resolution (the local cache holds only the
    /// low-LOD prefix the live viewer happened to load).
    get_texture_cap: Option<String>,
}

/// Fold the object / appearance / animation session events into
/// [`ReplayCaptureStore`], retaining just what a capture can rebuild an avatar
/// from: avatars and parented prims (attachments), appearances and animation
/// sets. Added only when capture is enabled, so it never runs in a normal
/// session.
pub fn capture_replay_inputs(
    mut events: MessageReader<SlEvent>,
    mut capabilities: MessageReader<SlCapabilities>,
    mut store: ResMut<ReplayCaptureStore>,
) {
    for SlCapabilities(map) in capabilities.read() {
        if let Some(url) = map.get(CAP_GET_TEXTURE) {
            store.get_texture_cap = Some(url.clone());
        }
    }
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => {
                // Retain avatars and any parented prim (attachment roots have the
                // avatar as parent; child prims chain up to the root) — enough to
                // rebuild an avatar's whole attachment tree, without hoarding the
                // region's unrelated world prims.
                if object.pcode == pcode::AVATAR || object.parent_id.get() != 0 {
                    let _previous = store.objects.insert(object.scoped_id(), (**object).clone());
                }
            }
            SlSessionEvent::ObjectRemoved { local_id, .. } => {
                let _removed = store.objects.remove(local_id);
            }
            SlSessionEvent::AvatarAppearance(appearance) => {
                let _previous = store
                    .appearances
                    .insert(appearance.avatar_id, (**appearance).clone());
            }
            SlSessionEvent::AvatarAnimation {
                avatar_id,
                animations,
                ..
            } => {
                let _previous = store.animations.insert(*avatar_id, animations.clone());
            }
            SlSessionEvent::RenderMaterials(entries) => {
                for entry in entries {
                    let _previous = store
                        .render_materials
                        .insert(entry.material_id, entry.clone());
                }
            }
            _other => {}
        }
    }
}

/// Write a bundle (one `<agent>.json` manifest per captured avatar, plus a shared
/// `cache/` of the referenced assets) into `SL_VIEWER_DUMP_DIR` on **Ctrl+Alt+D**.
/// Added only when capture is enabled.
pub fn dump_avatars_on_key(
    keyboard: Res<ButtonInput<KeyCode>>,
    store: Res<ReplayCaptureStore>,
    identity: Res<SlIdentity>,
) {
    let Some(dir) = std::env::var_os("SL_VIEWER_DUMP_DIR") else {
        return;
    };
    let chord = keyboard.pressed(KeyCode::ControlLeft) && keyboard.pressed(KeyCode::AltLeft);
    if !(chord && keyboard.just_pressed(KeyCode::KeyD)) {
        return;
    }
    let dir = std::path::PathBuf::from(dir);
    if let Err(error) = fs_err::create_dir_all(&dir) {
        warn!("avatar dump: cannot create {dir:?}: {error}");
        return;
    }
    let now_unix = unix_now();
    // The full-resolution texture fetch inputs: the GetTexture cap and (for a
    // central-baking grid) the appearance service that serves baked-body textures.
    let get_texture_cap = store.get_texture_cap.clone();
    let appearance_service = identity
        .agent_appearance_service
        .as_ref()
        .map(ToString::to_string);
    let mut written = 0_u32;
    let mut assets = 0_u32;
    // The deduplicated (texture id -> full-resolution URL) fetch plan across every
    // dumped avatar (the shared `cache/` means one fetch serves all).
    let mut plan: HashMap<Uuid, String> = HashMap::new();
    // Each retained avatar object is one dumpable avatar.
    for avatar in store
        .objects
        .values()
        .filter(|object| object.pcode == pcode::AVATAR)
    {
        let manifest = build_manifest(&store, avatar);
        let agent = manifest.agent.to_string();
        let (counts, textures) = copy_cache_assets(&dir, &manifest, now_unix);
        assets = assets
            .saturating_add(counts.meshes)
            .saturating_add(counts.anims)
            .saturating_add(counts.materials);
        for (id, url) in texture_fetch_urls(
            &manifest,
            &textures,
            get_texture_cap.as_deref(),
            appearance_service.as_deref(),
        ) {
            let _previous = plan.entry(id).or_insert(url);
        }
        let path = dir.join(format!("{agent}.json"));
        match serde_json::to_vec_pretty(&manifest) {
            Ok(json) => match fs_err::write(&path, json) {
                Ok(()) => written = written.saturating_add(1),
                Err(error) => warn!("avatar dump: write {path:?}: {error}"),
            },
            Err(error) => warn!("avatar dump: serialize {agent}: {error}"),
        }
    }
    let plan: Vec<(Uuid, String)> = plan.into_iter().collect();
    let total = plan.len();
    info!(
        "avatar dump: wrote {written} avatar(s) + {assets} mesh/anim/material asset(s) to {}; \
         fetching {total} texture(s) at full resolution — the viewer will pause until done",
        dir.display(),
    );
    // Fetch on a worker thread (so `reqwest::blocking` is never nested in a tokio
    // runtime) but block the dump on its completion: a detached thread would be
    // killed when the operator closes the viewer, leaving the bundle's textures
    // incomplete. The brief pause is the price of a guaranteed-complete bundle.
    let fetched = std::thread::spawn(move || run_texture_fetch(&dir, &plan, now_unix))
        .join()
        .unwrap_or(0);
    info!("avatar dump: fetched {fetched}/{total} full-resolution texture(s); capture complete");
}

/// Build one avatar's [`ReplayManifest`]: the avatar object, its attachment tree,
/// its appearance and its animations, all as verbatim captured events.
fn build_manifest(store: &ReplayCaptureStore, avatar: &Object) -> ReplayManifest {
    let agent = avatar.full_id.uuid();
    let avatar_scoped = avatar.scoped_id();
    // The avatar object first, then every prim whose parent chain roots at it.
    let mut objects = vec![avatar.clone()];
    for object in store.objects.values() {
        if object.scoped_id() != avatar_scoped && wearer_of(store, object) == Some(avatar_scoped) {
            objects.push(object.clone());
        }
    }
    let avatar_key = AgentKey::from(agent);
    let render_materials = referenced_render_materials(store, &objects);
    ReplayManifest {
        version: MANIFEST_VERSION,
        agent,
        objects,
        appearance: store.appearances.get(&avatar_key).cloned(),
        animations: store
            .animations
            .get(&avatar_key)
            .cloned()
            .unwrap_or_default(),
        render_materials,
    }
}

/// The legacy (`RenderMaterials`) material entries whose id is referenced by any
/// face of `objects` — the ones the avatar's attachments actually use, resolved
/// from the retained set so replay can re-emit them offline.
fn referenced_render_materials(
    store: &ReplayCaptureStore,
    objects: &[Object],
) -> Vec<RenderMaterialEntry> {
    let mut wanted: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
    for object in objects {
        let entry = decode_texture_entry(&object.texture_entry, MAX_FACES);
        for face in &entry.faces {
            if let Some(material_id) = face.material_id {
                let _inserted = wanted.insert(material_id);
            }
        }
    }
    wanted
        .into_iter()
        .filter_map(|id| store.render_materials.get(&id).cloned())
        .collect()
}

/// Follow `object`'s parent chain up through the retained objects to the avatar
/// ([`pcode::AVATAR`]) that wears it, or `None` if the chain does not root at an
/// avatar (a world linkset) or is broken. Bounded against a cycle.
fn wearer_of(store: &ReplayCaptureStore, object: &Object) -> Option<ScopedObjectId> {
    let mut current = object;
    // A linkset/attachment chain is shallow; cap the walk well above any real
    // depth so a malformed cycle cannot loop forever.
    for _step in 0..64_u8 {
        if current.pcode == pcode::AVATAR {
            return Some(current.scoped_id());
        }
        if current.parent_id.get() == 0 {
            return None;
        }
        current = store.objects.get(&current.scoped_parent_id())?;
    }
    None
}

/// The current Unix time in whole seconds, for stamping bundle cache entries
/// (their own LRU bookkeeping). Falls back to `0` before the epoch.
fn unix_now() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u32::try_from(duration.as_secs()).unwrap_or(u32::MAX)
        })
}
