//! Headless avatar-state **replay** analyzer (viewer-avatar-state-dump-replay):
//! reconstruct a dumped avatar's posed skeleton offline from a dump bundle (the
//! `<agent>.json` manifest plus the `assets/` the viewer copied out of its
//! caches) and report the face-bone diagnostic — each mouth/brow bone's world
//! position and its distance from `mHead`, both at the deformed rest and under
//! the captured animation pose. Reproduces the protruding-tongue and brow-spike
//! defects without a live grid.
//!
//! Usage (the `character/` dir supplies the standard skeleton + visual params):
//!
//! ```console
//! SL_VIEWER_ASSETS=<firestorm>/indra/newview/character \
//!   cargo run --release -p sl-client-bevy --example avatar_replay -- <dir>/<agent>.json
//! ```
//!
//! `SL_REPLAY_TIME` (seconds, default 1.0) picks the animation sample time.

use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use bevy::math::{Mat4, Quat, Vec3};
use sl_anim::{JointPriority, Motion};
use sl_avatar::{
    AttachmentPoints, SkeletalDeformations, Skeleton, VisualParams, VolumeDeformations,
};
use sl_client_bevy::{AnimationPose, BevySkeleton, JointOverrides, joint_position_overrides};
use sl_mesh::{decode_skin, parse_header};

/// The mouth/brow bones the diagnostic reports, in chain order.
const FACE_BONES: &[&str] = &[
    "mNeck",
    "mHead",
    "mFaceRoot",
    "mFaceForeheadCenter",
    "mFaceJaw",
    "mFaceLipLowerCenter",
    "mFaceTongueBase",
    "mFaceTongueTip",
];

/// Reconstruct the dumped avatar and print the face-bone diagnostic.
#[expect(
    clippy::print_stdout,
    reason = "this is a diagnostic command-line tool"
)]
fn main() -> Result<(), Box<dyn Error>> {
    let dump_path = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("usage: avatar_replay <dump.json>")?,
    );
    let bundle = dump_path.parent().unwrap_or_else(|| Path::new("."));
    let manifest: serde_json::Value = serde_json::from_slice(&fs_err::read(&dump_path)?)?;
    let agent = manifest.get("agent").and_then(serde_json::Value::as_str);
    let appearance = decode_hex(str_field(&manifest, "appearance_hex"));
    let animations = str_list(&manifest, "animations");
    let meshes = str_list(&manifest, "rigged_meshes");

    // The standard skeleton + visual-param table, built exactly as the viewer does.
    let char_dir = PathBuf::from(std::env::var("SL_VIEWER_ASSETS")?);
    let skeleton = Skeleton::from_xml(&fs_err::read_to_string(
        char_dir.join("avatar_skeleton.xml"),
    )?)?;
    let mut bevy = BevySkeleton::from_skeleton(&skeleton);
    bevy.insert_synthetic_root("mRoot");
    let lad = fs_err::read_to_string(char_dir.join("avatar_lad.xml"))?;
    let params = VisualParams::from_xml(&lad)?;
    bevy.insert_attachment_points(&AttachmentPoints::from_xml(&lad)?);

    // Shape → skeletal + volume deformation.
    let deform = SkeletalDeformations::from_appearance(&params, &appearance);
    let volumes = VolumeDeformations::from_appearance(&params, &appearance);

    // Worn rigged meshes' joint-position overrides, merged (highest wins).
    let mut overrides = JointOverrides::default();
    let mut overriding_meshes = 0_usize;
    for mesh in &meshes {
        if let Some(skin) = load_skin(bundle, mesh) {
            let mesh_overrides =
                joint_position_overrides(&skin, bevy.lookup(), bevy.local_transforms());
            if !mesh_overrides.is_empty() {
                overriding_meshes = overriding_meshes.saturating_add(1);
                overrides.merge(&mesh_overrides);
            }
        }
    }

    // The captured animation pose (per joint, the highest-priority track wins).
    let time: f32 = std::env::var("SL_REPLAY_TIME")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1.0);
    let pose = resolve_pose(&bevy, bundle, &animations, time);

    let posed = bevy.deformed_world_matrices(&deform, &volumes, &overrides, &pose);
    let rest =
        bevy.deformed_world_matrices(&deform, &volumes, &overrides, &AnimationPose::default());

    println!(
        "agent {}: {} anim(s), {} mesh(es) ({overriding_meshes} with overrides), t={time}s",
        agent.unwrap_or("?"),
        animations.len(),
        meshes.len(),
    );
    let head_rest = bone_world(&bevy, &rest, "mHead");
    let head_posed = bone_world(&bevy, &posed, "mHead");
    println!(
        "{:<22} {:>26}  {:>26}",
        "bone", "rest (world | |mHead|)", "posed (world | |mHead|)"
    );
    for &bone in FACE_BONES {
        let r = bone_world(&bevy, &rest, bone);
        let p = bone_world(&bevy, &posed, bone);
        println!(
            "{bone:<22} {}  {}",
            describe(r, head_rest),
            describe(p, head_posed),
        );
    }
    Ok(())
}

/// The string at `key` in the manifest, or `""`.
fn str_field<'a>(manifest: &'a serde_json::Value, key: &str) -> &'a str {
    manifest
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

/// The list of strings at `key` in the manifest.
fn str_list(manifest: &serde_json::Value, key: &str) -> Vec<String> {
    manifest
        .get(key)
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect()
}

/// Decode a hex string into bytes (ignoring a trailing odd nibble).
fn decode_hex(hex: &str) -> Vec<u8> {
    let bytes = hex.as_bytes();
    bytes
        .chunks_exact(2)
        .filter_map(|pair| {
            let text = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(text, 16).ok()
        })
        .collect()
}

/// Load and decode a bundled (or cached-layout) mesh's skin block, if present.
fn load_skin(bundle: &Path, mesh: &str) -> Option<sl_mesh::MeshSkin> {
    let bytes = read_asset(bundle, "meshes", mesh, "mesh")?;
    let (header, header_size) = parse_header(&bytes)?;
    let skin_ref = header.skin?;
    let (start, end) = skin_ref.range(header_size);
    let slice = bytes.get(start..end)?;
    decode_skin(slice).ok()
}

/// Load and decode a bundled animation motion, if present.
fn load_motion(bundle: &Path, anim: &str) -> Option<Motion> {
    let bytes = read_asset(bundle, "anims", anim, "asset")?;
    Motion::from_bytes(&bytes).ok()
}

/// Read a bundled asset file `assets/<kind>/<id>.<ext>`.
fn read_asset(bundle: &Path, kind: &str, id: &str, ext: &str) -> Option<Vec<u8>> {
    let path = bundle.join("assets").join(kind).join(format!("{id}.{ext}"));
    fs_err::read(path).ok()
}

/// Resolve the per-joint animation pose from the captured animations: for each
/// joint, the track from the highest-effective-priority motion wins (a
/// `USE_MOTION` joint priority defers to the motion's base priority), sampled at
/// `time`.
fn resolve_pose(
    bevy: &BevySkeleton,
    bundle: &Path,
    animations: &[String],
    time: f32,
) -> AnimationPose {
    let mut pose = AnimationPose::new();
    let mut winner: HashMap<usize, JointPriority> = HashMap::new();
    for anim in animations {
        let Some(motion) = load_motion(bundle, anim) else {
            continue;
        };
        for joint in &motion.joints {
            let Some(index) = bevy.find(&joint.name) else {
                continue;
            };
            let priority = if joint.priority == JointPriority::USE_MOTION {
                motion.base_priority
            } else {
                joint.priority
            };
            if winner
                .get(&index)
                .is_some_and(|current| *current >= priority)
            {
                continue;
            }
            let _prev = winner.insert(index, priority);
            if let Some(rotation) = joint.sample_rotation(time) {
                pose.set_rotation(index, Quat::from_array(rotation));
            }
            if let Some(position) = joint.sample_position(time) {
                pose.set_position(index, Vec3::from_array(position));
            }
        }
    }
    pose
}

/// The world-space translation of the named bone, if present.
fn bone_world(bevy: &BevySkeleton, world: &[Mat4], bone: &str) -> Option<Vec3> {
    bevy.find(bone)
        .and_then(|index| world.get(index))
        .map(|matrix| matrix.w_axis.truncate())
}

/// Format a bone position and its distance from the head as `(x,y,z) d=…`.
fn describe(bone: Option<Vec3>, head: Option<Vec3>) -> String {
    match bone {
        Some(position) => {
            let distance = head.map_or(f32::NAN, |head_pos| euclidean(position, head_pos));
            format!(
                "({:+.3},{:+.3},{:+.3}) d={distance:.3}",
                position.x, position.y, position.z
            )
        }
        None => "(absent)".to_owned(),
    }
}

/// Euclidean distance between two points, component-wise (kept off the glam `-`
/// operator the workspace `arithmetic_side_effects` lint watches).
fn euclidean(a: Vec3, b: Vec3) -> f32 {
    let (dx, dy, dz) = (a.x - b.x, a.y - b.y, a.z - b.z);
    dx.mul_add(dx, dy.mul_add(dy, dz * dz)).sqrt()
}
