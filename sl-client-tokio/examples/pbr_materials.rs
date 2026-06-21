//! Live exercise for PBR / materials (ROADMAP #25): logs in, opens the
//! bandwidth throttle, and watches the scene stream in. For every object it
//! decodes the per-face `TextureEntry` and collects the legacy-material ids the
//! faces reference, then fetches those materials over the `RenderMaterials`
//! capability (the path stock OpenSim implements) and prints the decoded
//! normal/specular parameters.
//!
//! It also surfaces any GLTF (PBR) material **overrides** the simulator pushes
//! (`Event::GltfMaterialOverride`) — these only appear on a Second Life region
//! (stock OpenSim does not send them), where they arrive as raw, undecoded
//! per-face notation documents.
//!
//! Run against the local OpenSim or a Second Life region. Configure via the same
//! environment variables as `tokio_login_hold_logout` (`SL_LOGIN_URI`, `SL_FIRST`,
//! `SL_LAST`, `SL_PASSWORD`). Note: stock OpenSim only returns a material over
//! `RenderMaterials` once a viewer has applied one, so an empty result there
//! still confirms the cap round-trips.

use std::collections::BTreeSet;
use std::time::Duration;

use sl_client_tokio::{
    Client, Command, DisconnectReason, Event, LoginParams, LoginRequest, ReflectionProbeFlags,
    Throttle, Uuid, decode_texture_entry,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// How long to let the scene stream in before fetching the harvested materials.
const SCENE_SETTLE: Duration = Duration::from_secs(8);

/// The maximum number of prim faces a single `TextureEntry` can describe.
const MAX_FACES: usize = 32;

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

/// Collects the non-nil per-face material ids referenced by a raw
/// `TextureEntry` blob into `out`.
fn harvest_material_ids(texture_entry: &[u8], out: &mut BTreeSet<Uuid>) {
    if texture_entry.is_empty() {
        return;
    }
    let entry = decode_texture_entry(texture_entry, MAX_FACES);
    for face in &entry.faces {
        if !face.material_id.is_nil() {
            out.insert(face.material_id);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let login_uri = env_or("SL_LOGIN_URI", "http://127.0.0.1:9000/");
    let first = std::env::var("SL_FIRST")?;
    let last = std::env::var("SL_LAST")?;
    let password = std::env::var("SL_PASSWORD")?;
    let start = env_or("SL_START", "last").parse::<sl_client_tokio::StartLocation>()?;
    let channel = env_or("SL_CHANNEL", "sl-client-pbr-materials");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));

    info!("logging in as {first} {last}...");
    let request = LoginRequest::new(first, last, password, start, channel, version);
    let client = Client::connect(LoginParams { login_uri, request }).await?;
    info!("login succeeded; running session");

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(512);
    let (command_tx, command_rx) = mpsc::channel::<Command>(16);
    let (diag_tx, _diag_rx) = mpsc::channel(16);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

    let mut material_ids: BTreeSet<Uuid> = BTreeSet::new();
    let mut requested = false;

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::RegionHandshakeComplete => {
                info!("region handshake complete; opening throttle to stream the scene");
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_1000()))
                    .await
                    .ok();
                // After the scene settles, fetch whatever materials we harvested.
                let command_tx = command_tx.clone();
                tokio::spawn(async move {
                    sleep(SCENE_SETTLE).await;
                    command_tx.send(Command::Logout).await.ok();
                });
            }
            Event::ObjectAdded(object) | Event::ObjectUpdated(object) => {
                if let Some(probe) = object.extra.reflection_probe {
                    info!(
                        "object {} is a reflection probe: ambiance={} clip={} box={} dynamic={} mirror={}",
                        object.local_id,
                        probe.ambiance,
                        probe.clip_distance,
                        probe.flags.contains(ReflectionProbeFlags::BOX_VOLUME),
                        probe.flags.contains(ReflectionProbeFlags::DYNAMIC),
                        probe.flags.contains(ReflectionProbeFlags::MIRROR),
                    );
                }
                if !object.extra.render_material.is_empty() {
                    info!(
                        "object {} has {} per-face GLTF material ref(s)",
                        object.local_id,
                        object.extra.render_material.len(),
                    );
                }
                let before = material_ids.len();
                harvest_material_ids(&object.texture_entry, &mut material_ids);
                if material_ids.len() != before && !requested {
                    // We have at least one material to ask about; fetch the set.
                    requested = true;
                    let ids: Vec<Uuid> = material_ids.iter().copied().collect();
                    info!("fetching {} material(s) over RenderMaterials", ids.len());
                    command_tx
                        .send(Command::RequestRenderMaterials { material_ids: ids })
                        .await
                        .ok();
                }
            }
            Event::RenderMaterials(materials) => {
                info!("RenderMaterials returned {} material(s)", materials.len());
                for entry in &materials {
                    info!(
                        "  {} normal_map={} specular_map={} alpha_mode={} spec_exp={}",
                        entry.material_id,
                        entry.material.normal_map,
                        entry.material.specular_map,
                        entry.material.diffuse_alpha_mode,
                        entry.material.specular_exponent,
                    );
                }
            }
            Event::GltfMaterialOverride {
                region_handle,
                local_id,
                faces,
                overrides,
            } => {
                info!(
                    "GLTF override on object {local_id} (region {region_handle:#x}): faces {faces:?}, {} raw override doc(s)",
                    overrides.len(),
                );
            }
            Event::MaterialParamsResult { success, message } => {
                info!("ModifyMaterialParams result: success={success} message={message:?}");
            }
            Event::LoggedOut => {
                info!(
                    "logged out cleanly (harvested {} material id(s))",
                    material_ids.len()
                );
                break;
            }
            Event::Disconnected(reason) => {
                match reason {
                    DisconnectReason::Timeout => warn!("disconnected: inactivity timeout"),
                    other => warn!("disconnected: {other:?}"),
                }
                break;
            }
            _ => {}
        }
    }

    run.await??;
    Ok(())
}
