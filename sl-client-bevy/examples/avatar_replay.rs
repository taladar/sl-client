//! Headless avatar-state **replay** analyzer (viewer-avatar-state-dump-replay):
//! reconstruct a dumped avatar's posed skeleton offline from a replay bundle (the
//! `<agent>.json` manifest plus the `cache/` drop-in caches the viewer copied out
//! of its own caches) and report the face-bone diagnostic — each mouth/brow
//! bone's world position and its distance from `mHead`, both at the deformed rest
//! and under the captured animation pose. Reproduces the protruding-tongue and
//! brow-spike defects without a live grid — the geometry-diagnosis counterpart of
//! the viewer's full `--replay` render mode.
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
use sl_asset::{AssetDiskCache, CacheLimits as AssetCacheLimits};
use sl_avatar::{
    AttachmentPoints, SkeletalDeformations, Skeleton, VisualParams, VolumeDeformations,
};
use sl_client_bevy::{AnimationPose, BevySkeleton, JointOverrides, Uuid, joint_position_overrides};
use sl_mesh::{CacheLimits as MeshCacheLimits, MeshDiskCache, MeshSkin, decode_skin, parse_header};
use sl_proto::{AvatarAppearance, Object, PlayingAnimation, SculptOrMeshKey};

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

    // The captured wire events, typed straight out of the manifest.
    let objects: Vec<Object> = manifest
        .get("objects")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    let appearance: Option<AvatarAppearance> = manifest
        .get("appearance")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .flatten();
    let animations: Vec<PlayingAnimation> = manifest
        .get("animations")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    let agent = manifest.get("agent").and_then(serde_json::Value::as_str);
    let appearance_bytes = appearance
        .map(|value| value.visual_params)
        .unwrap_or_default();
    // The worn rigged meshes: every object carrying a mesh asset.
    let meshes: Vec<Uuid> = objects
        .iter()
        .filter_map(|object| match object.extra.sculpt.as_ref()?.texture {
            SculptOrMeshKey::Mesh(mesh) => Some(mesh.uuid()),
            SculptOrMeshKey::Sculpt(_texture) => None,
        })
        .collect();

    // The bundle's drop-in caches (verbatim mesh / animation bytes).
    let mesh_cache = MeshDiskCache::open(
        bundle.join("cache").join("meshcache"),
        MeshCacheLimits::default(),
    )
    .ok();
    let anim_cache = AssetDiskCache::open(
        bundle.join("cache").join("animcache"),
        AssetCacheLimits::default(),
    )
    .ok();

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
    let deform = SkeletalDeformations::from_appearance(&params, &appearance_bytes);
    let volumes = VolumeDeformations::from_appearance(&params, &appearance_bytes);

    // Worn rigged meshes' joint-position overrides, merged (highest wins).
    let mut overrides = JointOverrides::default();
    let mut overriding_meshes = 0_usize;
    for &mesh in &meshes {
        if let Some(skin) = load_skin(mesh_cache.as_ref(), mesh) {
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
    let pose = resolve_pose(&bevy, anim_cache.as_ref(), &animations, time);

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

/// Load and decode a bundled mesh's skin block from the drop-in mesh cache, if
/// present.
fn load_skin(mesh_cache: Option<&MeshDiskCache>, mesh: Uuid) -> Option<MeshSkin> {
    let bytes = mesh_cache?.read(mesh)?;
    let (header, header_size) = parse_header(bytes.data())?;
    let skin_ref = header.skin?;
    let (start, end) = skin_ref.range(header_size);
    let slice = bytes.data().get(start..end)?;
    decode_skin(slice).ok()
}

/// Load and decode a bundled animation motion from the drop-in animation cache,
/// if present.
fn load_motion(anim_cache: Option<&AssetDiskCache>, anim: Uuid) -> Option<Motion> {
    let bytes = anim_cache?.read(anim)?;
    Motion::from_bytes(bytes.as_ref()).ok()
}

/// Resolve the per-joint animation pose from the captured animations: for each
/// joint, the track from the highest-effective-priority motion wins (a
/// `USE_MOTION` joint priority defers to the motion's base priority), sampled at
/// `time`.
fn resolve_pose(
    bevy: &BevySkeleton,
    anim_cache: Option<&AssetDiskCache>,
    animations: &[PlayingAnimation],
    time: f32,
) -> AnimationPose {
    let mut pose = AnimationPose::new();
    let mut winner: HashMap<usize, JointPriority> = HashMap::new();
    for animation in animations {
        let Some(motion) = load_motion(anim_cache, animation.anim_id) else {
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
