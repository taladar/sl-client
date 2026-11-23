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
//! when `SL_VIEWER_DUMP_DIR` is set (see `sl_client_bevy_viewer::run`), so a normal session
//! pays nothing. The heavy geometry/textures/animations are copied from the
//! viewer's own caches, keyed by the ids in the captured events.

use std::collections::{BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, AvatarAppearance, MAX_FACES, Object, PlayingAnimation, RenderMaterialEntry,
    ScopedObjectId, SlCapabilities, SlEvent, SlIdentity, SlSessionEvent, Uuid,
    decode_texture_entry, pcode,
};

use crate::replay_bundle::{
    BundleGaps, CaptureCaps, FetchPlan, MANIFEST_VERSION, ReplayManifest, RetryPolicy,
    absent_render_materials, add_render_materials, copy_cache_assets, load_bundle,
    run_bundle_fetch, run_render_material_fetch, texture_fetch_urls,
};

/// The environment variable that arms the capture: the directory a bundle is
/// written into. Unset, [`AvatarDumpPlugin`] adds nothing at all.
pub const DUMP_DIR_ENV: &str = "SL_VIEWER_DUMP_DIR";

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
    /// The region's capability URLs, so the dump can fetch what the local caches
    /// lack or hold only part of.
    caps: CaptureCaps,
}

/// Avatar-state capture (`viewer-avatar-state-dump-replay`): retain the raw
/// avatar / appearance / animation events each frame and write a bundle per
/// avatar on **Ctrl+Alt+D**.
///
/// Present only when `SL_VIEWER_DUMP_DIR` is set, so a normal session pays
/// nothing — neither the per-frame retention nor a scheduler dispatch. The
/// variable is read here rather than by the caller because it is this module's
/// switch: the dump systems check it too, and having one place decide whether
/// they exist and another decide whether they act is how a capture comes to be
/// half-armed.
#[derive(Debug, Default, Clone, Copy)]
pub struct AvatarDumpPlugin;

impl Plugin for AvatarDumpPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var_os(DUMP_DIR_ENV).is_none() {
            return;
        }
        app.init_resource::<ReplayCaptureStore>()
            .add_systems(Update, (capture_replay_inputs, dump_avatars_on_key));
    }
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
        store.caps.update(map);
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
    paths: Option<Res<sl_viewer_platform::paths::ViewerPaths>>,
) {
    let Some(dir) = std::env::var_os(DUMP_DIR_ENV) else {
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
    let caps = store.caps.clone();
    // The appearance service serves baked-body textures on a central-baking grid.
    let appearance_service = identity
        .agent_appearance_service
        .as_ref()
        .map(ToString::to_string);
    // Each retained avatar object is one dumpable avatar.
    let mut manifests: Vec<ReplayManifest> = store
        .objects
        .values()
        .filter(|object| object.pcode == pcode::AVATAR)
        .map(|avatar| build_manifest(&store, avatar))
        .collect();
    // An avatar captured into this directory earlier and not here now is completed
    // too: what its bundle lacks can still be fetched by id after it has left.
    let present: BTreeSet<Uuid> = manifests.iter().map(|manifest| manifest.agent).collect();
    match load_bundle(&dir) {
        Ok(earlier) => manifests.extend(
            earlier
                .into_iter()
                .filter(|manifest| !present.contains(&manifest.agent)),
        ),
        Err(error) => warn!("avatar dump: cannot read the earlier captures: {error}"),
    }
    // The legacy materials no reply had delivered yet come first: they travel in
    // the manifests, and they name normal and specular maps to fetch below.
    let absent: BTreeSet<Uuid> = manifests.iter().flat_map(absent_render_materials).collect();
    info!(
        "avatar dump: capturing {} avatar(s), fetching {} legacy material(s) not yet received \
         — the viewer will pause until done",
        manifests.len(),
        absent.len(),
    );
    if !absent.is_empty() {
        if let Some(cap) = caps.render_materials.clone() {
            let fetched = sl_client_bevy::log_context::spawn_thread(move || {
                run_render_material_fetch(&cap, &absent)
            })
            .join()
            .unwrap_or_default();
            for manifest in &mut manifests {
                add_render_materials(manifest, &fetched);
            }
        } else {
            warn!("avatar dump: the region offers no RenderMaterials capability");
        }
    }
    let mut written = 0_u32;
    let mut assets = 0_u32;
    // What is still to download across every avatar (the shared `cache/` means one
    // fetch serves all), and what nothing can serve.
    let mut plan = FetchPlan::default();
    let mut gaps = BundleGaps::default();
    for manifest in &manifests {
        let agent = manifest.agent.to_string();
        gaps.render_materials
            .extend(absent_render_materials(manifest));
        let (counts, textures) = copy_cache_assets(paths.as_deref(), &dir, manifest, now_unix);
        assets = assets
            .saturating_add(counts.meshes)
            .saturating_add(counts.anims)
            .saturating_add(counts.materials);
        plan.add_assets(manifest);
        let urls = texture_fetch_urls(
            manifest,
            &textures,
            caps.texture.as_deref(),
            appearance_service.as_deref(),
        );
        let served: BTreeSet<Uuid> = urls.iter().map(|(id, _url)| *id).collect();
        gaps.textures.extend(textures.difference(&served));
        for (id, url) in urls {
            let _previous = plan.textures.entry(id).or_insert(url);
        }
        let path = dir.join(format!("{agent}.json"));
        match serde_json::to_vec_pretty(manifest) {
            Ok(json) => match fs_err::write(&path, json) {
                Ok(()) => written = written.saturating_add(1),
                Err(error) => warn!("avatar dump: write {path:?}: {error}"),
            },
            Err(error) => warn!("avatar dump: serialize {agent}: {error}"),
        }
    }
    info!(
        "avatar dump: wrote {written} avatar(s) + {assets} cached asset(s) to {}; fetching {} \
         mesh(es) whole and {} texture(s) at full resolution, plus any animation or PBR \
         material not cached",
        dir.display(),
        plan.meshes.len(),
        plan.textures.len(),
    );
    // Fetch on a worker thread (so `reqwest::blocking` is never nested in a tokio
    // runtime) but block the dump on its completion: a detached thread would be
    // killed when the operator closes the viewer, leaving the bundle incomplete.
    // The pause is the price of a bundle that holds everything it references.
    let requested = (
        plan.meshes.clone(),
        plan.anims.clone(),
        plan.materials.clone(),
        plan.textures.keys().copied().collect::<BTreeSet<Uuid>>(),
    );
    let fetched = sl_client_bevy::log_context::spawn_thread(move || {
        run_bundle_fetch(
            &dir,
            plan,
            &caps,
            now_unix,
            RetryPolicy::Brief,
            &std::sync::atomic::AtomicBool::new(false),
        )
    })
    .join()
    .unwrap_or_else(|_panicked| BundleGaps {
        render_materials: BTreeSet::new(),
        meshes: requested.0,
        anims: requested.1,
        materials: requested.2,
        textures: requested.3,
    });
    gaps.meshes.extend(fetched.meshes);
    gaps.anims.extend(fetched.anims);
    gaps.materials.extend(fetched.materials);
    gaps.textures.extend(fetched.textures);
    let missing = gaps.by_kind();
    if missing.is_empty() {
        info!("avatar dump: capture complete, every referenced asset is in the bundle");
    }
    for (kind, ids) in missing {
        let listed: Vec<String> = ids.iter().map(ToString::to_string).collect();
        warn!(
            "avatar dump: capture INCOMPLETE, {} {kind}(s) missing: {}",
            ids.len(),
            listed.join(", ")
        );
    }
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
