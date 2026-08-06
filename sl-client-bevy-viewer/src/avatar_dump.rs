//! Avatar-state capture (viewer-avatar-state-dump-replay): on demand, write each
//! tracked avatar's render *inputs* — appearance visual-param bytes, currently
//! playing animation ids, and worn rigged-mesh asset ids — to a JSON file, so a
//! buggy avatar can be reproduced offline (with the headless replay analyzer)
//! after it logs out. The heavy geometry/textures/animations already persist in
//! the viewer's on-disk caches, keyed by the ids captured here.
//!
//! Opt-in and non-interfering: it does nothing unless `SL_VIEWER_DUMP_DIR` is set,
//! and only fires on the deliberate **Ctrl+Alt+D** chord.

use bevy::prelude::*;
use serde::Serialize;
use sl_client_bevy::Uuid;

use crate::animations::AnimationPlayback;
use crate::avatars::AvatarState;

/// One avatar's captured render inputs — everything needed, alongside the on-disk
/// caches, to reconstruct it offline.
#[derive(Serialize)]
struct AvatarDump {
    /// The avatar's agent id (also the file stem).
    agent: String,
    /// The raw `AvatarAppearance.visual_params` bytes, hex-encoded — drives the
    /// skeletal / morph shape deformation on replay.
    appearance_hex: String,
    /// The animation asset ids currently playing (their `.anim` assets are in the
    /// anim cache) — needed to reproduce a pose-driven defect (e.g. a face track).
    animations: Vec<String>,
    /// The worn rigged-mesh asset ids (their geometry + skin are in the mesh cache).
    rigged_meshes: Vec<String>,
}

/// Write a dump file per tracked avatar on **Ctrl+Alt+D**, into `SL_VIEWER_DUMP_DIR`.
/// A no-op unless that env var is set, so it never interferes with normal use.
pub(crate) fn dump_avatars_on_key(
    keyboard: Res<ButtonInput<KeyCode>>,
    state: Res<AvatarState>,
    playback: Res<AnimationPlayback>,
) {
    let Some(dir) = std::env::var_os("SL_VIEWER_DUMP_DIR") else {
        return;
    };
    let chord = keyboard.pressed(KeyCode::ControlLeft) && keyboard.pressed(KeyCode::AltLeft);
    if !(chord && keyboard.just_pressed(KeyCode::KeyD)) {
        return;
    }
    let dir = std::path::PathBuf::from(dir);
    // Bundle the actual asset bytes next to the manifests so the reproduction
    // survives a cache clear / eviction.
    let mesh_dst = dir.join("assets").join("meshes");
    let anim_dst = dir.join("assets").join("anims");
    for sub in [&dir, &mesh_dst, &anim_dst] {
        if let Err(error) = fs_err::create_dir_all(sub) {
            warn!("avatar dump: cannot create {sub:?}: {error}");
            return;
        }
    }
    let mesh_cache = crate::paths::asset_cache_dir("meshcache");
    let anim_cache = crate::paths::asset_cache_dir("animcache");
    let mut written = 0_u32;
    let mut assets = 0_u32;
    for agent in state.dumpable_agents() {
        let Some((appearance, meshes)) = state.dump_inputs(agent) else {
            continue;
        };
        let animations = playback.playing_anims(agent);
        // Copy each referenced asset out of the on-disk cache into the bundle.
        for &mesh in &meshes {
            assets =
                assets.saturating_add(copy_cached(mesh_cache.as_ref(), mesh, "mesh", &mesh_dst));
        }
        for &anim in &animations {
            assets =
                assets.saturating_add(copy_cached(anim_cache.as_ref(), anim, "asset", &anim_dst));
        }
        let dump = AvatarDump {
            agent: agent.uuid().to_string(),
            appearance_hex: to_hex(appearance),
            animations: animations.iter().map(ToString::to_string).collect(),
            rigged_meshes: meshes.iter().map(ToString::to_string).collect(),
        };
        let path = dir.join(format!("{}.json", dump.agent));
        match serde_json::to_string_pretty(&dump) {
            Ok(json) => match fs_err::write(&path, json) {
                Ok(()) => written = written.saturating_add(1),
                Err(error) => warn!("avatar dump: write {path:?}: {error}"),
            },
            Err(error) => warn!("avatar dump: serialize {}: {error}", dump.agent),
        }
    }
    info!("avatar dump: wrote {written} avatar(s) + {assets} asset(s) to {dir:?}");
}

/// Lowercase-hex-encode `bytes` (a `fold` + `write!`, the clippy-clean form —
/// `map(format!).collect()` trips `clippy::format_collect`).
fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut hex, byte| {
        let _written = write!(hex, "{byte:02x}");
        hex
    })
}

/// Copy the cached asset file for `id` (`<cache>/<first-char>/<uuid>.<ext>`, the
/// disk-cache layout) into `dest`. Returns 1 on a successful copy, 0 otherwise (no
/// cache dir, missing/partial entry, or already present) — best-effort bundling.
fn copy_cached(
    cache: Option<&std::path::PathBuf>,
    id: Uuid,
    ext: &str,
    dest: &std::path::Path,
) -> u32 {
    let Some(cache) = cache else { return 0 };
    let name = id.to_string();
    let Some(first) = name.get(..1) else { return 0 };
    let src = cache.join(first).join(format!("{name}.{ext}"));
    let dst = dest.join(format!("{name}.{ext}"));
    if dst.exists() {
        return 0;
    }
    match fs_err::copy(&src, &dst) {
        Ok(_bytes) => 1,
        Err(_) => 0,
    }
}
