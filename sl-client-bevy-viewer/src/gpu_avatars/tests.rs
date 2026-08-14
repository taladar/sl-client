//! Phase 1a verification (`roadmap/context/gpu-avatars.md` §9.2):
//!
//! - **Buffer-packing pins**: every `#[derive(ShaderType)]` struct's byte
//!   layout is asserted against the offsets the WGSL declarations mean, so a
//!   reordered field cannot silently skew the GPU's view of the data.
//! - **Golden FK tests** (the decisive gate): [`reference_fk`] — the Rust
//!   mirror of the WGSL pass-C recurrence — against
//!   [`BevySkeleton::deformed_world_matrices`] itself, **bit-exact**, one
//!   fixture per semantic branch (pelvis-additive, volume-additive,
//!   absolute-position replace, override-wins, lock-scale,
//!   parent-local-scale-scales-child-offset).
//! - **Headless GPU test**: the real WGSL passes C+D run over the same
//!   fixture, writing into Bevy's live `SkinUniforms` buffer at the offset
//!   Bevy allocated for a real skinned mesh, and the pipeline's own readback
//!   channel must show the GPU palette equal to the CPU reference (1e-4).

use std::sync::{Arc, Mutex};

use pretty_assertions::assert_eq;

use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::log::LogPlugin;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::encase;
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::winit::WinitPlugin;
use sl_client_bevy::{
    AnimationPose, BevySkeleton, JointOverrides, SkeletalDeformations, Skeleton, VisualParams,
    VolumeDeformations,
};

use sl_anim::{
    HandPose, JointContribution, JointMotion, JointPriority, Motion, PositionKey, RotationKey,
    blend_joint,
};
use sl_client_bevy::{AssetKey, Uuid, sample_motion};

use super::render::{GpuAvatarReadbackData, palette_worst_diff};
use super::stage::{GpuAvatarStaging, StagedReadback, StagedSkinInstance};
use super::types::{
    ClipArena, GpuAvatarFrame, GpuClipHeader, GpuComputeParams, GpuCorrection, GpuJointTrack,
    GpuLocalPose, GpuPlayState, GpuRestJoint, GpuSampleJob, GpuSkinInstance, JOINT_NONE,
    MAX_ACTIVE_CLIPS, PLAY_STOPPED_NONE, POSE_FLAG_POS, POSE_FLAG_ROT, compose_rest_joints,
    mirror_local_pose, mirror_playback_time, mirror_sample_track, pose_rows, reference_fk,
};
use super::{GpuAvatarsMode, GpuAvatarsPlugin};
use crate::face_material::{FaceMaterial, SlFaceMaterialPlugin, inert_face_material};
use crate::render_test::TestError;

/// The four-bone / two-volume test skeleton shared with the `sl-client-bevy`
/// golden tests (`mPelvis` → `mTorso` (+`BELLY`) → `mChest`, `mHipRight`,
/// `PELVIS`), whose collision volumes carry authored non-identity rotations.
const MINI_SKELETON: &str = include_str!("../../../sl-avatar/tests/fixtures/mini_skeleton.xml");

/// A visual-param table whose skeletal param scales and offsets `mTorso` —
/// the parent-local-scale-scales-child-offset branch (`mChest`'s offset rides
/// `mTorso`'s scale) plus a plain rest-position offset.
const TORSO_SCALE_LAD: &str = r#"<?xml version="1.0"?>
<linden_avatar version="2.0">
  <skeleton file_name="avatar_skeleton.xml">
    <param id="1" group="0" name="Torso_Stretch" value_min="0" value_max="1" value_default="0">
      <param_skeleton>
        <bone name="mTorso" scale="0.05 0.1 0.2" offset="0.01 -0.02 0.03"/>
        <bone name="mChest" scale="0 0 0" offset="0 0 0.05"/>
      </param_skeleton>
    </param>
  </skeleton>
</linden_avatar>"#;

/// A visual-param table whose morph param displaces the `BELLY` collision
/// volume — the volume rest scale/position fold.
const BELLY_VOLUME_LAD: &str = r#"<?xml version="1.0"?>
<linden_avatar version="2.0">
  <mesh type="upperBodyMesh" lod="0" file_name="avatar_upper_body.llm">
    <param id="104" group="0" name="Big_Belly_Torso" value_min="0" value_max="1" value_default="0">
      <param_morph>
        <volume_morph name="BELLY" scale="0.075 0.04 0.03" pos="0.07 0 -0.07"/>
      </param_morph>
    </param>
  </mesh>
</linden_avatar>"#;

/// The fixture skeleton: the mini skeleton with the synthetic `mRoot`
/// appended — deliberately, because the appended root creates the **forward
/// parent reference** the FK's identity fallback must reproduce.
fn fixture_skeleton() -> Result<BevySkeleton, TestError> {
    let skeleton = Skeleton::from_xml(MINI_SKELETON)?;
    let mut bevy = BevySkeleton::from_skeleton(&skeleton);
    bevy.insert_synthetic_root("mRoot");
    Ok(bevy)
}

/// A non-trivial root affine (an SL→Bevy-style axis rotation plus a world
/// translation), so the root-compose is exercised beyond identity.
fn fixture_root() -> Mat4 {
    Mat4::from_rotation_translation(
        Quat::from_rotation_x(-core::f32::consts::FRAC_PI_2),
        Vec3::new(10.0, 3.0, -7.0),
    )
}

/// The torso-stretch skeletal deformation at full weight.
fn fixture_deform() -> Result<SkeletalDeformations, TestError> {
    let params = VisualParams::from_xml(TORSO_SCALE_LAD)?;
    Ok(SkeletalDeformations::from_appearance(&params, &[255]))
}

/// The belly volume displacement at full weight.
fn fixture_volumes() -> Result<VolumeDeformations, TestError> {
    let params = VisualParams::from_xml(BELLY_VOLUME_LAD)?;
    Ok(VolumeDeformations::from_appearance(&params, &[255]))
}

/// Assert the Rust pass-C mirror reproduces `deformed_world_matrices` (plus
/// the root compose) **bit-exactly** on the given inputs, and return the
/// GPU-side world matrices for the caller's has-teeth checks.
fn assert_fk_matches_cpu(
    skeleton: &BevySkeleton,
    deform: &SkeletalDeformations,
    volumes: &VolumeDeformations,
    overrides: &JointOverrides,
    pose: &AnimationPose,
    root: Mat4,
) -> Result<Vec<Mat4>, TestError> {
    let rest = compose_rest_joints(skeleton, deform, volumes, overrides);
    let rows = pose_rows(pose, skeleton.len());
    let gpu = reference_fk(&rest, &rows, root);
    let cpu = skeleton.deformed_world_matrices(deform, volumes, overrides, pose);
    assert_eq!(gpu.len(), cpu.len(), "joint count diverges");
    for (index, (gpu_world, cpu_world)) in gpu.iter().zip(&cpu).enumerate() {
        let expected = root.mul_mat4(cpu_world);
        let worst = gpu_world
            .to_cols_array()
            .iter()
            .zip(expected.to_cols_array().iter())
            .map(|(got, want)| (got - want).abs())
            .fold(0.0_f32, f32::max);
        // `worst` is a fold of absolute values, so it is non-negative by
        // construction: `<= 0.0` is exactly "bit-equal" without a float `==`.
        assert!(
            worst <= 0.0,
            "joint {index} ({:?}): the pass-C mirror is not bit-exact against \
             deformed_world_matrices (worst component diff {worst:e})\n gpu: {gpu_world}\n cpu: {expected}",
            skeleton.joint_name(index)
        );
    }
    Ok(gpu)
}

// ---------------------------------------------------------------------------
// Golden FK tests — one per `deformed_world_matrices` branch (§9.2).
// ---------------------------------------------------------------------------

/// Shaped rest pose (no animation): the appearance scale/offset fold, the
/// volume-morph fold, and — because `mTorso` is scaled — the
/// parent-local-scale-scales-child-offset branch on `mChest`. Also proves the
/// synthetic root's forward-parent identity fallback.
#[test]
fn golden_shaped_rest_pose_is_bit_exact() -> Result<(), TestError> {
    let skeleton = fixture_skeleton()?;
    let gpu = assert_fk_matches_cpu(
        &skeleton,
        &fixture_deform()?,
        &fixture_volumes()?,
        &JointOverrides::default(),
        &AnimationPose::default(),
        fixture_root(),
    )?;
    // Teeth: the deformation actually moved the chest off its undeformed
    // place (mTorso's +0.2 Z scale stretches mChest's offset).
    let plain = assert_fk_matches_cpu(
        &skeleton,
        &SkeletalDeformations::default(),
        &VolumeDeformations::default(),
        &JointOverrides::default(),
        &AnimationPose::default(),
        fixture_root(),
    )?;
    let chest = skeleton.find("mChest").ok_or("mChest missing")?;
    let deformed = gpu.get(chest).ok_or("chest matrix")?.w_axis;
    let rest = plain.get(chest).ok_or("chest rest matrix")?.w_axis;
    assert!(
        (deformed - rest).length() > 1.0e-3,
        "the fixture deformation did not move mChest — the golden case has no teeth"
    );
    Ok(())
}

/// An `mPelvis` position key is **additive** onto the (deformed) rest.
#[test]
fn golden_pelvis_position_key_is_additive() -> Result<(), TestError> {
    let skeleton = fixture_skeleton()?;
    let pelvis = skeleton.find("mPelvis").ok_or("mPelvis missing")?;
    let mut pose = AnimationPose::new();
    pose.set_position(pelvis, Vec3::new(0.1, -0.2, 0.3));
    pose.set_rotation(pelvis, Quat::from_rotation_z(0.4));
    let gpu = assert_fk_matches_cpu(
        &skeleton,
        &fixture_deform()?,
        &fixture_volumes()?,
        &JointOverrides::default(),
        &pose,
        fixture_root(),
    )?;
    // Teeth: additive means the pelvis sits at rest + key (the fixture pelvis
    // rests at z=1.067, so a replace would land somewhere else entirely).
    let rest_world = assert_fk_matches_cpu(
        &skeleton,
        &fixture_deform()?,
        &fixture_volumes()?,
        &JointOverrides::default(),
        &AnimationPose::default(),
        fixture_root(),
    )?;
    let moved = gpu.get(pelvis).ok_or("pelvis matrix")?.w_axis.truncate();
    let rest = rest_world
        .get(pelvis)
        .ok_or("pelvis rest matrix")?
        .w_axis
        .truncate();
    let delta = fixture_root().transform_vector3(Vec3::new(0.1, -0.2, 0.3));
    assert!(
        (moved - (rest + delta)).length() < 1.0e-5,
        "pelvis position key was not additive: moved {moved}, rest {rest}, delta {delta}"
    );
    Ok(())
}

/// A collision-volume position key (the body-physics channel) is additive
/// too — even though the volume also carries a shape-morph displacement.
#[test]
fn golden_volume_position_key_is_additive() -> Result<(), TestError> {
    let skeleton = fixture_skeleton()?;
    let belly = skeleton.find("BELLY").ok_or("BELLY missing")?;
    let mut pose = AnimationPose::new();
    pose.set_position(belly, Vec3::new(0.0, 0.0, 0.05));
    let gpu = assert_fk_matches_cpu(
        &skeleton,
        &fixture_deform()?,
        &fixture_volumes()?,
        &JointOverrides::default(),
        &pose,
        fixture_root(),
    )?;
    let rest_world = assert_fk_matches_cpu(
        &skeleton,
        &fixture_deform()?,
        &fixture_volumes()?,
        &JointOverrides::default(),
        &AnimationPose::default(),
        fixture_root(),
    )?;
    let moved = gpu.get(belly).ok_or("belly matrix")?.w_axis;
    let rest = rest_world.get(belly).ok_or("belly rest matrix")?.w_axis;
    assert!(
        (moved - rest).length() > 1.0e-3,
        "the volume position key did not move BELLY — no teeth"
    );
    Ok(())
}

/// A position key on an ordinary bone is its **absolute** local position
/// (replaces the deformed rest — the Bento neutral-face semantics).
#[test]
fn golden_absolute_position_key_replaces_the_rest() -> Result<(), TestError> {
    let skeleton = fixture_skeleton()?;
    let chest = skeleton.find("mChest").ok_or("mChest missing")?;
    let mut pose = AnimationPose::new();
    pose.set_position(chest, Vec3::new(-0.015, 0.0, 0.4));
    let gpu = assert_fk_matches_cpu(
        &skeleton,
        &fixture_deform()?,
        &fixture_volumes()?,
        &JointOverrides::default(),
        &pose,
        fixture_root(),
    )?;
    let rest_world = assert_fk_matches_cpu(
        &skeleton,
        &fixture_deform()?,
        &fixture_volumes()?,
        &JointOverrides::default(),
        &AnimationPose::default(),
        fixture_root(),
    )?;
    let moved = gpu.get(chest).ok_or("chest matrix")?.w_axis;
    let rest = rest_world.get(chest).ok_or("chest rest matrix")?.w_axis;
    assert!(
        (moved - rest).length() > 1.0e-3,
        "the absolute position key did not move mChest — no teeth"
    );
    Ok(())
}

/// A rig joint-position override **wins** over an absolute position key: the
/// key is ignored and the joint stays at the override.
#[test]
fn golden_override_wins_over_an_absolute_key() -> Result<(), TestError> {
    let skeleton = fixture_skeleton()?;
    let chest = skeleton.find("mChest").ok_or("mChest missing")?;
    let mut overrides = JointOverrides::default();
    overrides.set_position(chest, Vec3::new(-0.02, 0.01, 0.25));
    let mut pose = AnimationPose::new();
    pose.set_position(chest, Vec3::new(0.5, 0.5, 0.5));
    let with_key = assert_fk_matches_cpu(
        &skeleton,
        &fixture_deform()?,
        &fixture_volumes()?,
        &overrides,
        &pose,
        fixture_root(),
    )?;
    let without_key = assert_fk_matches_cpu(
        &skeleton,
        &fixture_deform()?,
        &fixture_volumes()?,
        &overrides,
        &AnimationPose::default(),
        fixture_root(),
    )?;
    let a = with_key.get(chest).ok_or("chest with key")?;
    let b = without_key.get(chest).ok_or("chest without key")?;
    assert!(
        a.abs_diff_eq(*b, 0.0),
        "the override did not win over the absolute key: with {a}, without {b}"
    );
    Ok(())
}

/// Under an override with `lock_scale_if_joint_position`, the joint keeps its
/// **default** scale — the appearance scale deformation must not stretch it.
#[test]
fn golden_lock_scale_pins_an_overridden_joint() -> Result<(), TestError> {
    let skeleton = fixture_skeleton()?;
    let torso = skeleton.find("mTorso").ok_or("mTorso missing")?;
    let mut overrides = JointOverrides::default();
    overrides.set_position(torso, Vec3::new(0.0, 0.0, 0.09));
    overrides.set_lock_scale(true);
    let gpu = assert_fk_matches_cpu(
        &skeleton,
        &fixture_deform()?,
        &fixture_volumes()?,
        &overrides,
        &AnimationPose::default(),
        fixture_root(),
    )?;
    // Teeth: the fixture deform scales mTorso by +0.2 Z; the lock must pin it
    // back to the rest 1.0.
    let (scale, _rot, _pos) = gpu
        .get(torso)
        .ok_or("torso matrix")?
        .to_scale_rotation_translation();
    assert!(
        (scale.z - 1.0).abs() < 1.0e-4,
        "lock_scale did not pin the overridden mTorso to its default scale: {scale}"
    );
    Ok(())
}

/// Animated rotations compose through the (scaled, offset) chain exactly like
/// the CPU recurrence — including a rotation on the volume-carrying parent.
#[test]
fn golden_animated_rotations_compose_bit_exact() -> Result<(), TestError> {
    let skeleton = fixture_skeleton()?;
    let pelvis = skeleton.find("mPelvis").ok_or("mPelvis missing")?;
    let torso = skeleton.find("mTorso").ok_or("mTorso missing")?;
    let mut pose = AnimationPose::new();
    pose.set_rotation(pelvis, Quat::from_euler(EulerRot::XYZ, 0.2, -0.3, 0.15));
    pose.set_rotation(torso, Quat::from_euler(EulerRot::XYZ, -0.1, 0.25, 0.05));
    let _gpu = assert_fk_matches_cpu(
        &skeleton,
        &fixture_deform()?,
        &fixture_volumes()?,
        &JointOverrides::default(),
        &pose,
        fixture_root(),
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Buffer-packing pins: the ShaderType byte layouts the WGSL mirrors assume.
// ---------------------------------------------------------------------------

/// Serialize one value through encase's storage-buffer (std430) layout.
fn std430_bytes<T: encase::ShaderType + encase::internal::WriteInto>(
    value: &T,
) -> Result<Vec<u8>, TestError> {
    let mut buffer = encase::StorageBuffer::new(Vec::<u8>::new());
    buffer.write(value)?;
    Ok(buffer.into_inner())
}

/// The little-endian `f32` at byte offset `at`.
fn f32_at(bytes: &[u8], at: usize) -> f32 {
    bytes
        .get(at..at.saturating_add(4))
        .and_then(|slice| slice.try_into().ok())
        .map_or(f32::NAN, f32::from_ne_bytes)
}

/// The little-endian `u32` at byte offset `at`.
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    bytes
        .get(at..at.saturating_add(4))
        .and_then(|slice| slice.try_into().ok())
        .map_or(u32::MAX, u32::from_ne_bytes)
}

/// `GpuRestJoint` packs to the WGSL `RestJoint` layout: stride 48, `parent`
/// at 12, `rest_rot` at 16, `local_scale` at 32, `flags` at 44.
#[test]
fn packing_rest_joint_matches_wgsl() -> Result<(), TestError> {
    let rows = vec![
        GpuRestJoint {
            rest_pos: Vec3::new(1.0, 2.0, 3.0),
            parent: 7,
            rest_rot: Vec4::new(4.0, 5.0, 6.0, 8.0),
            local_scale: Vec3::new(9.0, 10.0, 11.0),
            flags: 5,
        },
        GpuRestJoint::default(),
    ];
    let bytes = std430_bytes(&rows)?;
    assert_eq!(
        bytes.len(),
        96,
        "two GpuRestJoint rows must stride 48 B each"
    );
    assert!((f32_at(&bytes, 0) - 1.0).abs() < f32::EPSILON);
    assert_eq!(u32_at(&bytes, 12), 7, "parent must sit at offset 12");
    assert!(
        (f32_at(&bytes, 16) - 4.0).abs() < f32::EPSILON,
        "rest_rot at 16"
    );
    assert!(
        (f32_at(&bytes, 32) - 9.0).abs() < f32::EPSILON,
        "local_scale at 32"
    );
    assert_eq!(u32_at(&bytes, 44), 5, "flags must sit at offset 44");
    Ok(())
}

/// `GpuLocalPose` packs to the WGSL `LocalPose` layout: stride 32, `pos` at
/// 16, `flags` at 28.
#[test]
fn packing_local_pose_matches_wgsl() -> Result<(), TestError> {
    let rows = vec![
        GpuLocalPose {
            rot: Vec4::new(1.0, 2.0, 3.0, 4.0),
            pos: Vec3::new(5.0, 6.0, 7.0),
            flags: 3,
        },
        GpuLocalPose::default(),
    ];
    let bytes = std430_bytes(&rows)?;
    assert_eq!(
        bytes.len(),
        64,
        "two GpuLocalPose rows must stride 32 B each"
    );
    assert!((f32_at(&bytes, 16) - 5.0).abs() < f32::EPSILON, "pos at 16");
    assert_eq!(u32_at(&bytes, 28), 3, "flags must sit at offset 28");
    Ok(())
}

/// `GpuAvatarFrame` packs to the WGSL `AvatarFrame` layout: stride 80, `slot`
/// at 64.
#[test]
fn packing_avatar_frame_matches_wgsl() -> Result<(), TestError> {
    let rows = vec![
        GpuAvatarFrame {
            root: Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0)),
            slot: 9,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        },
        GpuAvatarFrame::default(),
    ];
    let bytes = std430_bytes(&rows)?;
    assert_eq!(
        bytes.len(),
        160,
        "two GpuAvatarFrame rows must stride 80 B each"
    );
    assert_eq!(u32_at(&bytes, 64), 9, "slot must sit at offset 64");
    // Column-major mat4: the translation lands in the w column (offset 48).
    assert!(
        (f32_at(&bytes, 48) - 1.0).abs() < f32::EPSILON,
        "w column at 48"
    );
    Ok(())
}

/// `GpuSkinInstance` packs to the WGSL `SkinInstance` layout: stride 32,
/// fields at 0/4/8/12/16.
#[test]
fn packing_skin_instance_matches_wgsl() -> Result<(), TestError> {
    let rows = vec![
        GpuSkinInstance {
            avatar_slot: 1,
            palette_offset: 2,
            joint_count: 3,
            joint_map_offset: 4,
            ibp_offset: 5,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        },
        GpuSkinInstance::default(),
    ];
    let bytes = std430_bytes(&rows)?;
    assert_eq!(
        bytes.len(),
        64,
        "two GpuSkinInstance rows must stride 32 B each"
    );
    for (offset, want) in [(0, 1), (4, 2), (8, 3), (12, 4), (16, 5)] {
        assert_eq!(u32_at(&bytes, offset), want, "field at offset {offset}");
    }
    Ok(())
}

/// `GpuComputeParams` packs to the WGSL `Params` uniform layout: the Phase 1
/// scalars keep their offsets 0..=20 (the Phase 2 tail is pinned by
/// `packing_compute_params_phase2_matches_wgsl`).
#[test]
fn packing_compute_params_matches_wgsl() -> Result<(), TestError> {
    let params = GpuComputeParams {
        avatar_count: 1,
        joint_count: 2,
        instance_count: 3,
        max_skin_joints: 4,
        readback_instance: 5,
        readback_joint_count: 6,
        ..GpuComputeParams::default()
    };
    let mut buffer = encase::UniformBuffer::new(Vec::<u8>::new());
    buffer.write(&params)?;
    let bytes = buffer.into_inner();
    for (offset, want) in [(0, 1), (4, 2), (8, 3), (12, 4), (16, 5), (20, 6)] {
        assert_eq!(u32_at(&bytes, offset), want, "field at offset {offset}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The headless GPU test: real WGSL passes into the real SkinUniforms buffer.
// ---------------------------------------------------------------------------

/// The rendered frame's edge, in pixels (also the no-GPU-adapter probe, as in
/// the spike tests).
const FRAME: u32 = 128;

/// Frames to run before reading back — sized for asynchronous pipeline
/// compilation on this harness (the spike's measured value).
const FRAMES_TO_RUN: usize = 400;

/// Where a completed pixel readback's bytes land (the adapter probe).
type Cell = Arc<Mutex<Option<Vec<u8>>>>;

/// A quad mesh fully weighted onto joint 0 of a two-joint skin — the minimal
/// real skinned mesh (the spike's fixture): it takes Bevy's skinned pipeline
/// and registers a palette range in `SkinUniforms`.
fn skinned_quad() -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-1.0_f32, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0_f32, 0.0, 1.0]; 4])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0_f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_JOINT_INDEX,
        VertexAttributeValues::Uint16x4(vec![[0, 0, 0, 0]; 4]),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_JOINT_WEIGHT,
        vec![[1.0_f32, 0.0, 0.0, 0.0]; 4],
    )
    .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

/// **The Phase 1a GPU gate**: run the real `fk` + `palettes` WGSL passes over
/// the golden fixture, writing into the palette range Bevy allocated for a
/// real skinned mesh, and assert — through the pipeline's own compute-copy
/// readback channel — that the GPU-written palette equals the CPU reference
/// (`reference_fk`, itself bit-exact against `deformed_world_matrices`) to
/// 1e-4 per component.
///
/// Skips (loudly) when no frame comes back: a machine with no GPU adapter
/// cannot answer, mirroring the readback test tier.
#[test]
fn the_gpu_palette_matches_the_cpu_reference() -> Result<(), TestError> {
    // The fixture: the golden skeleton, a shaped + animated pose, a
    // non-trivial root, and a two-entry skin over pelvis + torso with
    // non-identity inverse bindposes.
    let skeleton = fixture_skeleton()?;
    let deform = fixture_deform()?;
    let volumes = fixture_volumes()?;
    let overrides = JointOverrides::default();
    let pelvis = skeleton.find("mPelvis").ok_or("mPelvis missing")?;
    let torso = skeleton.find("mTorso").ok_or("mTorso missing")?;
    let mut pose = AnimationPose::new();
    pose.set_rotation(pelvis, Quat::from_euler(EulerRot::XYZ, 0.2, -0.3, 0.15));
    pose.set_position(pelvis, Vec3::new(0.1, -0.05, 0.2));
    pose.set_rotation(torso, Quat::from_euler(EulerRot::XYZ, -0.1, 0.25, 0.05));
    let root = fixture_root();

    let rest = compose_rest_joints(&skeleton, &deform, &volumes, &overrides);
    let rows = pose_rows(&pose, skeleton.len());
    let world = reference_fk(&rest, &rows, root);
    let joint_map: Vec<u32> = vec![
        u32::try_from(pelvis).map_err(|_error| "pelvis index")?,
        u32::try_from(torso).map_err(|_error| "torso index")?,
    ];
    let ibps = [
        Mat4::from_translation(Vec3::new(0.0, -1.0, 0.3)),
        Mat4::from_rotation_z(0.5),
    ];
    let expected: Vec<Mat4> = joint_map
        .iter()
        .zip(ibps.iter())
        .map(|(&canonical, ibp)| {
            let canonical = usize::try_from(canonical).unwrap_or(usize::MAX);
            world
                .get(canonical)
                .copied()
                .unwrap_or(Mat4::IDENTITY)
                .mul_mat4(ibp)
        })
        .collect();
    let joint_count = u32::try_from(skeleton.len()).map_err(|_error| "joint count")?;
    let row_capacity = skeleton.len();

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<LogPlugin>(),
    )
    .add_plugins(ScheduleRunnerPlugin::run_loop(core::time::Duration::ZERO))
    .add_plugins(SlFaceMaterialPlugin)
    .add_plugins(GpuAvatarsPlugin {
        mode: GpuAvatarsMode {
            active: true,
            readback: true,
            // The test stages fixture data by hand instead of reading the
            // (absent) avatar state.
            live: false,
        },
    });

    // Phase 5: turn the posed-bounds pass on for this fixture (the live app
    // gates its infra on `mode.live`, off here) so the test runs the exact
    // fk → bounds → palettes dispatch sequence a live run does. The `bounds`
    // pass binds its own (bounds) layout at slot 0 and pass D re-binds the pose
    // layout after it; if that re-bind were missing the compute pass would hit
    // the wgpu bind-group-layout validation error and the palette readback
    // below would not match.
    app.init_resource::<super::render::GpuAvatarBounds>()
        .add_plugins(bevy::render::extract_resource::ExtractResourcePlugin::<
            super::render::GpuAvatarBoundsTarget,
        >::default())
        .add_systems(Startup, super::render::init_gpu_avatar_bounds);

    // Keep the skinned mesh's transform dirty every frame (same-value write):
    // a static instance can extract before its skin registers and would keep
    // a stale `current_skin_index` forever — the spike's staleness finding.
    app.add_systems(
        Update,
        |mut meshes: Query<&mut Transform, With<SkinnedMesh>>| {
            for mut transform in &mut meshes {
                transform.set_changed();
            }
        },
    );

    let pixels: Cell = Cell::default();
    let pixels_in_observer = Arc::clone(&pixels);
    let rest_for_startup = rest.clone();
    let rows_for_startup = rows.clone();
    let expected_for_startup = expected.clone();
    let joint_map_for_startup = joint_map.clone();

    app.add_systems(
        Startup,
        move |mut commands: Commands,
              mut meshes: ResMut<Assets<Mesh>>,
              mut materials: ResMut<Assets<FaceMaterial>>,
              mut images: ResMut<Assets<Image>>,
              mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>| {
            // The render target + pixel readback: the no-GPU-adapter probe.
            let mut target =
                Image::new_target_texture(FRAME, FRAME, TextureFormat::Rgba8UnormSrgb, None);
            target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
            let target = images.add(target);
            commands.spawn((
                Camera3d::default(),
                RenderTarget::Image(target.clone().into()),
                bevy::camera::Hdr,
                Msaa::Off,
                Transform::from_xyz(0.0, 0.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
            ));
            let pixels_cell = Arc::clone(&pixels_in_observer);
            commands.spawn(Readback::texture(target)).observe(
                move |readback: On<ReadbackComplete>| {
                    if let Ok(mut slot) = pixels_cell.lock() {
                        *slot = Some(readback.data.clone());
                    }
                },
            );

            // Two joint entities (their transforms are what Bevy's own CPU
            // path uploads — which the compute then overwrites).
            let joints = vec![
                commands.spawn(Transform::from_xyz(0.0, 0.0, 0.0)).id(),
                commands.spawn(Transform::from_xyz(0.0, 1.0, 0.0)).id(),
            ];
            let inverse_bindposes = bindposes.add(SkinnedMeshInverseBindposes::from(vec![
                Mat4::from_translation(Vec3::new(0.0, -1.0, 0.3)),
                Mat4::from_rotation_z(0.5),
            ]));
            let quad = commands
                .spawn((
                    Mesh3d(meshes.add(skinned_quad())),
                    MeshMaterial3d(materials.add(inert_face_material(StandardMaterial {
                        base_color: Color::srgb(1.0, 0.0, 0.0),
                        unlit: true,
                        ..default()
                    }))),
                    Transform::IDENTITY,
                    SkinnedMesh {
                        inverse_bindposes,
                        joints,
                    },
                    NoFrustumCulling,
                ))
                .id();

            // The hand-built staging snapshot: one avatar in slot 0, one
            // instance (the quad standing in for a ghost), and the readback
            // request with the CPU-reference palette as `expected`.
            commands.insert_resource(GpuAvatarStaging {
                joint_count,
                slot_capacity: 1,
                frames: vec![GpuAvatarFrame {
                    root,
                    slot: 0,
                    pad0: 0,
                    pad1: 0,
                    pad2: 0,
                }],
                local_pose: rows_for_startup.clone(),
                rest: Arc::new(rest_for_startup.clone()),
                rest_generation: 1,
                joint_map: Arc::new(joint_map_for_startup.clone()),
                ibps: Arc::new(vec![
                    Mat4::from_translation(Vec3::new(0.0, -1.0, 0.3)),
                    Mat4::from_rotation_z(0.5),
                ]),
                pool_generation: 1,
                instances: vec![StagedSkinInstance {
                    target: quad,
                    avatar_slot: 0,
                    joint_count: 2,
                    joint_map_offset: 0,
                    ibp_offset: 0,
                }],
                readback: Some(StagedReadback {
                    target: quad,
                    label: "headless fixture".to_owned(),
                    joint_count: 2,
                    expected: expected_for_startup.clone(),
                }),
                // Phase 1 upload semantics: the CPU-staged local pose above
                // is consumed as-is; passes A+B stay idle.
                ..GpuAvatarStaging::default()
            });
        },
    );

    // Sanity: the staged local_pose must cover slot 0's full row block.
    assert_eq!(rows.len(), row_capacity, "fixture rows must cover N_J");

    app.finish();
    app.cleanup();
    for _frame in 0..FRAMES_TO_RUN {
        app.update();
    }

    let frame = pixels.lock().ok().and_then(|mut slot| slot.take());
    if frame.is_none() {
        warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
        return Ok(());
    }

    let bytes = app
        .world()
        .get_resource::<GpuAvatarReadbackData>()
        .map(|data| data.bytes.clone())
        .ok_or("the readback data resource is missing")?;
    assert!(
        !bytes.is_empty(),
        "the machine renders but the GPU-avatar readback never completed — the compute \
         pipeline did not run (pipeline creation failure or the instance never resolved \
         in SkinUniforms)"
    );
    let worst = palette_worst_diff(&bytes, 2).ok_or(
        "the readback completed but its expected half is implausible (all zeros) — the \
         readback pass never executed over a resolved instance",
    )?;
    assert!(
        worst <= 1.0e-4,
        "the GPU-written palette diverges from the CPU reference (worst component diff \
         {worst:e}) — the WGSL FK does not reproduce deformed_world_matrices; the avatar \
         ghost would NOT match the CPU-posed original"
    );

    // Phase 5: the palette match above already proves fk → bounds → palettes ran
    // without a bind-group-layout validation error (a mismatch fails the whole
    // compute pass). Additionally confirm the `bounds` pass wrote slot 0 a
    // plausible, non-degenerate world AABB when its readback has landed (it
    // rides the same `Readback` mechanism as the palette, but its own timing).
    let bounds_bytes = app
        .world()
        .get_resource::<super::render::GpuAvatarBounds>()
        .map(|data| data.bytes.clone())
        .unwrap_or_default();
    match super::render::bounds_at(&bounds_bytes, 0) {
        Some((min, max)) => assert!(
            max.y - min.y > 0.0,
            "the posed-bounds pass wrote a degenerate slot-0 AABB (min {min:?} max {max:?})"
        ),
        None => warn!(
            "the bounds readback has not landed this run (timing); the palette path already \
             confirmed the fk → bounds → palettes sequence is validation-clean"
        ),
    }
    Ok(())
}

/// Build a slot-indexed bounds-readback byte buffer (the layout `bounds_at`
/// parses: 32 B per slot — min `xyz` + pad, max `xyz` + pad, native-endian)
/// covering slots `0..=max(slot)`, each listed slot filled with its `(min,
/// max)` and every other left zero (an unwritten slot).
fn encode_bounds(entries: &[(u32, Vec3, Vec3)]) -> Vec<u8> {
    let max_slot = entries.iter().map(|(slot, _, _)| *slot).max().unwrap_or(0);
    let count = usize::try_from(max_slot).unwrap_or(0).saturating_add(1);
    let mut bytes: Vec<u8> = Vec::new();
    for slot in 0..count {
        match entries
            .iter()
            .find(|(candidate, _, _)| usize::try_from(*candidate).ok() == Some(slot))
        {
            Some((_, min, max)) => {
                for component in [min.x, min.y, min.z, 0.0, max.x, max.y, max.z, 0.0] {
                    bytes.extend_from_slice(&component.to_ne_bytes());
                }
            }
            None => bytes.extend(std::iter::repeat_n(0_u8, 32)),
        }
    }
    bytes
}

/// Phase 5 culling: the `Aabb` [`apply_gpu_avatar_bounds`] derives from a
/// read-back world bound must actually **cull** a skinned avatar/crowd submesh
/// when the camera frustum excludes it — the live symptom was a flat
/// `extract_skins` that never dropped when the crowd was off-screen, i.e. an
/// effectively always-visible AABB. This drives the real apply system
/// (GPU-free) and tests the applied AABB against a real [`Frustum`] the way
/// Bevy's `check_visibility` does, for **both** a real avatar (whose entity
/// transform equals its GPU pose root) and a crowd copy (whose entity transform
/// is a static grid cell offset from where the GPU bound sits).
#[test]
fn applied_bounds_cull_an_offscreen_avatar() -> Result<(), TestError> {
    use bevy::camera::primitives::Aabb;
    use bevy::camera::{CameraProjection, PerspectiveProjection};

    use super::render::GpuAvatarBounds;
    use super::stage::{GpuAvatarRegistry, apply_gpu_avatar_bounds};
    use super::{GpuSkinBinding, PoseSlotKey};

    // Slot 0 — a "real avatar": the entity transform equals the GPU pose root,
    // so its world bound sits right at the entity (10 m ahead of the origin).
    // Slot 1 — a "crowd copy": the entity transform is a static grid cell 2 m
    // aside, but the GPU bound is 10 m ahead and 3 m up (as if the shared
    // base-root placed it there); the world→local round-trip must still put the
    // cull box at the GPU bound, not at the grid cell.
    let bytes = encode_bounds(&[
        (0, Vec3::new(-1.0, -1.0, -11.0), Vec3::new(1.0, 1.0, -9.0)),
        (1, Vec3::new(-1.0, 2.0, -11.0), Vec3::new(1.0, 4.0, -9.0)),
    ]);

    let mut world = World::new();
    let mut registry = GpuAvatarRegistry::default();
    registry.set_slot_for_test(PoseSlotKey::Crowd(0), 0);
    registry.set_slot_for_test(PoseSlotKey::Crowd(1), 1);
    world.insert_resource(registry);
    world.insert_resource(GpuAvatarBounds { bytes });

    let real = world
        .spawn((
            GpuSkinBinding {
                slot: PoseSlotKey::Crowd(0),
                canonical: Arc::from(Vec::<u32>::new()),
            },
            GlobalTransform::from(Transform::from_xyz(0.0, 0.0, -10.0)),
        ))
        .id();
    let crowd = world
        .spawn((
            GpuSkinBinding {
                slot: PoseSlotKey::Crowd(1),
                canonical: Arc::from(Vec::<u32>::new()),
            },
            GlobalTransform::from(Transform::from_xyz(2.0, 0.0, 0.0)),
        ))
        .id();

    let mut schedule = Schedule::default();
    schedule.add_systems(apply_gpu_avatar_bounds);
    schedule.run(&mut world);

    // A camera at the origin, looking toward the bounds (`-Z`) and away (`+Z`).
    let projection = PerspectiveProjection::default();
    let toward = CameraProjection::compute_frustum(
        &projection,
        &GlobalTransform::from(
            Transform::from_xyz(0.0, 0.0, 0.0).looking_at(Vec3::new(0.0, 0.0, -1.0), Vec3::Y),
        ),
    );
    let away = CameraProjection::compute_frustum(
        &projection,
        &GlobalTransform::from(
            Transform::from_xyz(0.0, 0.0, 0.0).looking_at(Vec3::new(0.0, 0.0, 1.0), Vec3::Y),
        ),
    );

    for (label, entity) in [("real avatar", real), ("crowd copy", crowd)] {
        let aabb = world
            .get::<Aabb>(entity)
            .copied()
            .ok_or("apply_gpu_avatar_bounds left the entity with no Aabb")?;
        let global = world
            .get::<GlobalTransform>(entity)
            .copied()
            .ok_or("the entity lost its GlobalTransform")?;
        // A posed bound is ~1–2 m + margin; the never-cull default is 1e4 m.
        assert!(
            aabb.half_extents.max_element() < 10.0,
            "{label}: expected a posed-sized AABB, got half-extents {:?} (the generous \
             1e4 default → never culled)",
            aabb.half_extents
        );
        let affine = global.affine();
        assert!(
            toward.intersects_obb(&aabb, &affine, true, false),
            "{label}: in front of the camera, it must be visible"
        );
        assert!(
            !away.intersects_obb(&aabb, &affine, true, false),
            "{label}: behind the camera, the applied AABB must frustum-cull it — a flat \
             always-visible AABB is the live bug"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Phase 2 golden tests (§9.2): the pass A/B Rust mirrors against `sl_anim`
// itself — sample (loop wrap, binary-search edges, quat nlerp/slerp), blend
// (priority ties, recency, 4-cap, weight budget, zero-weight skip, cubic
// ease), the idle adjusters, corrections, and a playback-clock soak.
// ---------------------------------------------------------------------------

/// The golden clip fixture's joint-name → canonical-index mapping (a
/// four-joint canonical space: pelvis 0, torso 1, chest 2, "spare" 3).
fn golden_joint_index(name: &str) -> Option<usize> {
    match name {
        "mPelvis" => Some(0),
        "mTorso" => Some(1),
        "mChest" => Some(2),
        _other => None,
    }
}

/// A clip with rotation + position tracks, uneven key times, a
/// negative-hemisphere key pair (exercising the slerp branch), a loop window
/// strictly inside the duration, and non-zero eases.
fn golden_motion(loops: bool) -> Motion {
    Motion {
        base_priority: JointPriority::HIGH,
        duration: 2.0,
        emote_name: String::new(),
        loop_in_point: 0.25,
        loop_out_point: 1.75,
        loops,
        ease_in_duration: 0.4,
        ease_out_duration: 0.3,
        hand_pose: HandPose::RELAXED,
        joints: vec![
            JointMotion {
                name: "mPelvis".to_owned(),
                priority: JointPriority::USE_MOTION,
                rotation_keys: vec![
                    RotationKey {
                        time: 0.0,
                        rotation: [0.0, 0.0, 0.0, 1.0],
                    },
                    RotationKey {
                        time: 0.5,
                        rotation: [0.0, 0.0, 0.707_106_77, 0.707_106_77],
                    },
                    // Opposite hemisphere vs the previous key: dot < 0, so
                    // interpolation takes the slerp branch.
                    RotationKey {
                        time: 1.3,
                        rotation: [0.0, 0.0, 0.923_879_5, -0.382_683_43],
                    },
                    RotationKey {
                        time: 1.75,
                        rotation: [0.1, -0.2, 0.3, 0.926_909_6],
                    },
                ],
                position_keys: vec![
                    PositionKey {
                        time: 0.0,
                        position: [0.0, 0.0, 1.0],
                    },
                    PositionKey {
                        time: 1.0,
                        position: [0.1, -0.2, 1.2],
                    },
                    PositionKey {
                        time: 1.75,
                        position: [-0.05, 0.15, 0.9],
                    },
                ],
            },
            JointMotion {
                name: "mTorso".to_owned(),
                priority: JointPriority::HIGHEST,
                rotation_keys: vec![
                    RotationKey {
                        time: 0.2,
                        rotation: [0.258_819_04, 0.0, 0.0, 0.965_925_8],
                    },
                    RotationKey {
                        time: 1.6,
                        rotation: [0.0, 0.382_683_43, 0.0, 0.923_879_5],
                    },
                ],
                position_keys: Vec::new(),
            },
            // A joint the canonical skeleton lacks: dropped at upload, and
            // skipped by the CPU resolver alike.
            JointMotion {
                name: "mNotASkeletonJoint".to_owned(),
                priority: JointPriority::LOW,
                rotation_keys: vec![RotationKey {
                    time: 0.0,
                    rotation: [1.0, 0.0, 0.0, 0.0],
                }],
                position_keys: Vec::new(),
            },
        ],
        constraints: Vec::new(),
    }
}

/// Assert two f32s are **bit-comparable** (identical or both NaN-free equal):
/// the mirrors run the same Rust float ops in the same order as `sl_anim`.
#[track_caller]
fn assert_bit_equal(actual: f32, expected: f32, what: &str) {
    let diff = (actual - expected).abs();
    assert!(
        diff <= 0.0,
        "{what}: mirror {actual} != reference {expected} (diff {diff:e})"
    );
}

/// **Pass A golden**: the arena-sampling mirror reproduces
/// [`sample_motion`] bit-for-bit across loop wrap, before-first / past-last
/// clamps, exact key hits, the zero-span guard and both nlerp branches —
/// for a looping and a one-shot clip.
#[test]
fn golden_mirror_sample_matches_sample_motion() -> Result<(), TestError> {
    for loops in [true, false] {
        let motion = golden_motion(loops);
        let mut arena = ClipArena::default();
        let clip_id = arena
            .ensure_clip(
                AssetKey::from(Uuid::from_u128(if loops { 11 } else { 10 })),
                &motion,
                4,
                golden_joint_index,
            )
            .ok_or("clip upload")?;
        let slices = arena.slices();
        let header = *slices
            .headers
            .get(usize::try_from(clip_id)?)
            .ok_or("header")?;
        assert_eq!(
            header.track_count, 2,
            "the unknown joint's track is dropped"
        );
        // Phases covering: negative clamp, zero, mid-key, exact key hits,
        // the loop-out edge, past-loop wrap positions, far past the end.
        let phases = [
            -0.5_f32, 0.0, 0.1, 0.2, 0.35, 0.5, 0.77, 1.0, 1.3, 1.5, 1.6, 1.75, 1.9, 2.0, 2.6,
            3.25, 7.83, 60.0,
        ];
        for &phase in &phases {
            let time = mirror_playback_time(&header, phase);
            let reference = sample_motion(&motion, phase);
            for t in 0..header.track_count {
                let track_index = header.track_offset.checked_add(t).ok_or("track index")?;
                let track = *slices
                    .tracks
                    .get(usize::try_from(track_index)?)
                    .ok_or("track")?;
                let row = mirror_sample_track(slices, track_index, time);
                // Find the reference sample for the track's joint by name.
                let name = match track.joint {
                    0 => "mPelvis",
                    1 => "mTorso",
                    other => return Err(format!("unexpected joint {other}").into()),
                };
                let sampled = reference
                    .iter()
                    .find(|joint| joint.name == name)
                    .ok_or("reference sample")?;
                assert_eq!(
                    track.priority, sampled.priority,
                    "effective priority resolved at upload"
                );
                match sampled.rotation {
                    Some(rotation) => {
                        assert_eq!(row.flags & POSE_FLAG_ROT, POSE_FLAG_ROT);
                        for (component, (got, want)) in row
                            .rot
                            .to_array()
                            .iter()
                            .zip(rotation.to_array().iter())
                            .enumerate()
                        {
                            assert_bit_equal(
                                *got,
                                *want,
                                &format!("{name} rot[{component}] at phase {phase}"),
                            );
                        }
                    }
                    None => assert_eq!(row.flags & POSE_FLAG_ROT, 0),
                }
                match sampled.position {
                    Some(position) => {
                        assert_eq!(row.flags & POSE_FLAG_POS, POSE_FLAG_POS);
                        for (component, (got, want)) in row
                            .pos
                            .to_array()
                            .iter()
                            .zip(position.to_array().iter())
                            .enumerate()
                        {
                            assert_bit_equal(
                                *got,
                                *want,
                                &format!("{name} pos[{component}] at phase {phase}"),
                            );
                        }
                    }
                    None => assert_eq!(row.flags & POSE_FLAG_POS, 0),
                }
            }
        }
    }
    Ok(())
}

/// One blend-fixture clip: a single `mPelvis` rotation (+ optional position)
/// track at the given explicit priority, with the given eases.
fn blend_clip(priority: JointPriority, rotation: [f32; 4], ease_in: f32, ease_out: f32) -> Motion {
    Motion {
        base_priority: JointPriority::LOW,
        duration: 30.0,
        emote_name: String::new(),
        loop_in_point: 0.0,
        loop_out_point: 30.0,
        loops: true,
        ease_in_duration: ease_in,
        ease_out_duration: ease_out,
        hand_pose: HandPose::RELAXED,
        joints: vec![JointMotion {
            name: "mPelvis".to_owned(),
            priority,
            rotation_keys: vec![RotationKey {
                time: 0.0,
                rotation,
            }],
            position_keys: vec![PositionKey {
                time: 0.0,
                position: [rotation[0], rotation[1], rotation[2]],
            }],
        }],
        constraints: Vec::new(),
    }
}

/// **Pass B golden**: the blend mirror reproduces `resolve_pose`'s
/// composition of [`Motion::pose_weight`] + [`sample_motion`] +
/// [`blend_joint`] bit-for-bit over a five-way contest exercising every §9.2
/// branch: a priority tie broken by recency, the 4-slot cap, the running
/// weight budget under a partial (cubic-eased) weight, and a zero-weight
/// (fully stopped) motion skipped without occupying a slot.
#[test]
fn golden_mirror_blend_matches_blend_joint() -> Result<(), TestError> {
    let now = 100.0_f32;
    // (motion, start, stopped_at, order): five clips on one joint.
    let setups: Vec<(Motion, f32, Option<f32>, u64)> = vec![
        // Two equal-priority full-weight motions: recency breaks the tie.
        (
            blend_clip(JointPriority::HIGH, [0.0, 0.0, 0.0, 1.0], 0.0, 0.0),
            50.0,
            None,
            4,
        ),
        (
            blend_clip(
                JointPriority::HIGH,
                [0.0, 0.0, 0.707_106_77, 0.707_106_77],
                0.0,
                0.0,
            ),
            60.0,
            None,
            7,
        ),
        // A partial cubic ease-in weight: started 0.5 s ago with a 2 s ease.
        (
            blend_clip(
                JointPriority::MEDIUM,
                [0.258_819_04, 0.0, 0.0, 0.965_925_8],
                2.0,
                1.0,
            ),
            99.5,
            None,
            9,
        ),
        // A low-priority full-weight motion (the 4th slot).
        (
            blend_clip(
                JointPriority::LOW,
                [0.0, 0.382_683_43, 0.0, 0.923_879_5],
                0.0,
                0.0,
            ),
            40.0,
            None,
            2,
        ),
        // Stopped long past its ease-out tail: weight 0, skipped entirely —
        // it must NOT occupy a blend slot (resolve_pose's gather semantics).
        (
            blend_clip(JointPriority::HIGHEST, [1.0, 0.0, 0.0, 0.0], 0.0, 0.5),
            10.0,
            Some(20.0),
            11,
        ),
        // A 6th, lowest-recency low-priority motion, pushed out by the 4-cap.
        (
            blend_clip(JointPriority::LOW, [0.5, 0.5, 0.5, 0.5], 0.0, 0.0),
            30.0,
            None,
            1,
        ),
    ];

    // Upload all clips and build the staged playback + jobs.
    let mut arena = ClipArena::default();
    let mut plays = vec![GpuPlayState::default(); MAX_ACTIVE_CLIPS];
    let mut jobs: Vec<GpuSampleJob> = Vec::new();
    let mut cache_len = 0_u32;
    for (index, (motion, start, stopped_at, order)) in setups.iter().enumerate() {
        let clip_id = arena
            .ensure_clip(
                AssetKey::from(Uuid::from_u128(
                    u128::try_from(index)?.checked_add(100).ok_or("clip id")?,
                )),
                motion,
                4,
                golden_joint_index,
            )
            .ok_or("clip upload")?;
        let phase = now - start;
        let cache_base = cache_len;
        jobs.push(GpuSampleJob {
            clip_id,
            cache_base,
            phase,
            pad0: 0,
        });
        cache_len = cache_len
            .checked_add(arena.track_count(clip_id))
            .ok_or("cache len")?;
        *plays.get_mut(index).ok_or("slot")? = GpuPlayState {
            clip_id,
            cache_base,
            start: *start,
            stopped_at: stopped_at.unwrap_or(PLAY_STOPPED_NONE),
            order: u32::try_from(*order)?,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        };
    }
    let rows = mirror_local_pose(
        arena.slices(),
        &plays,
        &jobs,
        cache_len,
        4,
        now,
        None,
        JOINT_NONE,
        JOINT_NONE,
        &[],
    );

    // The CPU reference: resolve_pose's gather (skip weight <= 0) into
    // blend_joint, over the same motions.
    let mut contributions: Vec<JointContribution> = Vec::new();
    for (motion, start, stopped_at, order) in &setups {
        let elapsed = now - start;
        let weight = motion.pose_weight(elapsed, *stopped_at);
        if weight <= 0.0 {
            continue;
        }
        for sampled in sample_motion(motion, elapsed) {
            if golden_joint_index(sampled.name) != Some(0) {
                continue;
            }
            contributions.push(JointContribution {
                priority: sampled.priority,
                order: *order,
                weight,
                rotation: sampled.rotation.map(|rotation| rotation.to_array()),
                position: sampled.position.map(|position| position.to_array()),
            });
        }
    }
    assert_eq!(contributions.len(), 5, "the stopped clip is gathered out");
    let blended = blend_joint(&mut contributions);
    let pelvis = rows.first().ok_or("pelvis row")?;
    let expected_rot = blended.rotation.ok_or("blended rotation")?;
    assert_eq!(pelvis.flags & POSE_FLAG_ROT, POSE_FLAG_ROT);
    for (component, (got, want)) in pelvis
        .rot
        .to_array()
        .iter()
        .zip(expected_rot.iter())
        .enumerate()
    {
        assert_bit_equal(*got, *want, &format!("blended rot[{component}]"));
    }
    let expected_pos = blended.position.ok_or("blended position")?;
    assert_eq!(pelvis.flags & POSE_FLAG_POS, POSE_FLAG_POS);
    for (component, (got, want)) in pelvis
        .pos
        .to_array()
        .iter()
        .zip(expected_pos.iter())
        .enumerate()
    {
        assert_bit_equal(*got, *want, &format!("blended pos[{component}]"));
    }
    // Joints without any track keep no channels.
    let torso = rows.get(1).ok_or("torso row")?;
    assert_eq!(torso.flags, 0, "no track, no channels");
    Ok(())
}

/// **Idle golden**: the mirror's chest/torso composition reproduces
/// [`crate::procedural::apply_idle_adjustments`] bit-for-bit, both over an
/// empty base (identity) and over a blended keyframe base.
#[test]
fn golden_mirror_idle_matches_procedural() -> Result<(), TestError> {
    use sl_client_bevy::AnimationPose;

    let now = 42.0_f32;
    let idle_now =
        (now * crate::animations::POSE_IDLE_HZ).floor() / crate::animations::POSE_IDLE_HZ;
    // A clip driving mChest so the chest idle composes over a blended base.
    let mut chest_clip = blend_clip(JointPriority::HIGH, [0.0, 0.0, 0.5, 0.866_025_4], 0.0, 0.0);
    for joint in &mut chest_clip.joints {
        joint.name = "mChest".to_owned();
    }
    let mut arena = ClipArena::default();
    let clip_id = arena
        .ensure_clip(
            AssetKey::from(Uuid::from_u128(200)),
            &chest_clip,
            4,
            golden_joint_index,
        )
        .ok_or("clip upload")?;
    let mut plays = vec![GpuPlayState::default(); MAX_ACTIVE_CLIPS];
    *plays.first_mut().ok_or("slot")? = GpuPlayState {
        clip_id,
        cache_base: 0,
        start: 10.0,
        stopped_at: PLAY_STOPPED_NONE,
        order: 1,
        pad0: 0,
        pad1: 0,
        pad2: 0,
    };
    let jobs = vec![GpuSampleJob {
        clip_id,
        cache_base: 0,
        phase: now - 10.0,
        pad0: 0,
    }];
    let rows = mirror_local_pose(
        arena.slices(),
        &plays,
        &jobs,
        arena.track_count(clip_id),
        4,
        now,
        Some(idle_now),
        2,
        1,
        &[],
    );

    // The CPU reference: the blended keyframe pose + apply_idle_adjustments.
    let elapsed = now - 10.0;
    let weight = chest_clip.pose_weight(elapsed, None);
    assert!(weight > 0.0);
    let mut pose = AnimationPose::new();
    for sampled in sample_motion(&chest_clip, elapsed) {
        let Some(index) = golden_joint_index(sampled.name) else {
            continue;
        };
        // A single motion: blend_joint copies the lone contribution outright.
        if let Some(rotation) = sampled.rotation {
            pose.set_rotation(index, rotation);
        }
        if let Some(position) = sampled.position {
            pose.set_position(index, position);
        }
    }
    crate::procedural::apply_idle_adjustments(&mut pose, idle_now, golden_joint_index);
    let expected_chest = pose.rotation(2).ok_or("chest rotation")?;
    let chest_row = rows.get(2).ok_or("chest row")?;
    for (component, (got, want)) in chest_row
        .rot
        .to_array()
        .iter()
        .zip(expected_chest.to_array().iter())
        .enumerate()
    {
        assert_bit_equal(*got, *want, &format!("chest idle rot[{component}]"));
    }
    let expected_torso = pose.rotation(1).ok_or("torso rotation")?;
    let torso_row = rows.get(1).ok_or("torso row")?;
    for (component, (got, want)) in torso_row
        .rot
        .to_array()
        .iter()
        .zip(expected_torso.to_array().iter())
        .enumerate()
    {
        assert_bit_equal(*got, *want, &format!("torso idle rot[{component}]"));
    }
    Ok(())
}

/// **Corrections golden**: a staged correction replaces exactly the channels
/// it carries, leaving the other channel of the joint at the blend's value.
#[test]
fn golden_mirror_corrections_replace_channels() -> Result<(), TestError> {
    let arena = ClipArena::default();
    let plays = vec![GpuPlayState::default(); MAX_ACTIVE_CLIPS];
    let correction = GpuLocalPose {
        rot: bevy::math::Vec4::new(0.0, 0.0, 0.707_106_77, 0.707_106_77),
        pos: Vec3::ZERO,
        flags: POSE_FLAG_ROT,
    };
    let rows = mirror_local_pose(
        arena.slices(),
        &plays,
        &[],
        0,
        4,
        5.0,
        None,
        JOINT_NONE,
        JOINT_NONE,
        &[(3, correction)],
    );
    let corrected = rows.get(3).ok_or("corrected row")?;
    assert_eq!(corrected.flags, POSE_FLAG_ROT);
    assert!(corrected.rot.abs_diff_eq(correction.rot, 0.0));
    let untouched = rows.first().ok_or("row 0")?;
    assert_eq!(untouched.flags, 0);
    Ok(())
}

/// **Playback-clock soak** (§9.2): a looping walk-class clip sampled hours
/// into its playback (with a drifting walk-speed skew folded into the
/// phase, as the scheduler does) stays bit-equal to [`sample_motion`] across
/// thousands of wrap positions — the loop wrap and binary search do not
/// degrade with elapsed magnitude.
#[test]
fn golden_mirror_sample_soaks_loop_wrap() -> Result<(), TestError> {
    let motion = golden_motion(true);
    let mut arena = ClipArena::default();
    let clip_id = arena
        .ensure_clip(
            AssetKey::from(Uuid::from_u128(300)),
            &motion,
            4,
            golden_joint_index,
        )
        .ok_or("clip upload")?;
    let slices = arena.slices();
    let header = *slices
        .headers
        .get(usize::try_from(clip_id)?)
        .ok_or("header")?;
    let mut drift = 0.0_f32;
    for step in 0..3000_u32 {
        // A wall clock hours in, plus an accumulating walk-speed skew.
        let step_f = u16::try_from(step).map(f32::from)?;
        drift += 0.003;
        let phase = 10_000.0 + step_f * 0.033 + drift;
        let time = mirror_playback_time(&header, phase);
        assert!(
            time >= header.loop_in && time <= header.loop_out,
            "wrapped time {time} escaped the loop window at phase {phase}"
        );
        let reference = sample_motion(&motion, phase);
        for t in 0..header.track_count {
            let track_index = header.track_offset.checked_add(t).ok_or("track index")?;
            let track = *slices
                .tracks
                .get(usize::try_from(track_index)?)
                .ok_or("track")?;
            let row = mirror_sample_track(slices, track_index, time);
            let name = if track.joint == 0 {
                "mPelvis"
            } else {
                "mTorso"
            };
            let sampled = reference
                .iter()
                .find(|joint| joint.name == name)
                .ok_or("reference sample")?;
            if let Some(rotation) = sampled.rotation {
                for (got, want) in row.rot.to_array().iter().zip(rotation.to_array().iter()) {
                    assert_bit_equal(*got, *want, &format!("soak rot at phase {phase}"));
                }
            }
            if let Some(position) = sampled.position {
                for (got, want) in row.pos.to_array().iter().zip(position.to_array().iter()) {
                    assert_bit_equal(*got, *want, &format!("soak pos at phase {phase}"));
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Phase 2 buffer-packing pins.
// ---------------------------------------------------------------------------

/// `GpuClipHeader` packs to the WGSL `ClipHeader` layout: stride 48, `flags`
/// at 20, `track_of_joint_offset` at 32.
#[test]
fn packing_clip_header_matches_wgsl() -> Result<(), TestError> {
    let rows = vec![
        GpuClipHeader {
            duration: 1.0,
            loop_in: 2.0,
            loop_out: 3.0,
            ease_in: 4.0,
            ease_out: 5.0,
            flags: 6,
            track_count: 7,
            track_offset: 8,
            track_of_joint_offset: 9,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        },
        GpuClipHeader::default(),
    ];
    let bytes = std430_bytes(&rows)?;
    assert_eq!(bytes.len(), 96, "two GpuClipHeader rows must stride 48 B");
    assert!((f32_at(&bytes, 0) - 1.0).abs() < f32::EPSILON);
    assert_eq!(u32_at(&bytes, 20), 6, "flags at 20");
    assert_eq!(u32_at(&bytes, 24), 7, "track_count at 24");
    assert_eq!(u32_at(&bytes, 32), 9, "track_of_joint_offset at 32");
    Ok(())
}

/// `GpuJointTrack` packs to the WGSL `JointTrack` layout: stride 32,
/// `priority` at 4, `pos_count` at 20.
#[test]
fn packing_joint_track_matches_wgsl() -> Result<(), TestError> {
    let rows = vec![
        GpuJointTrack {
            joint: 1,
            priority: -2,
            rot_offset: 3,
            rot_count: 4,
            pos_offset: 5,
            pos_count: 6,
            pad0: 0,
            pad1: 0,
        },
        GpuJointTrack::default(),
    ];
    let bytes = std430_bytes(&rows)?;
    assert_eq!(bytes.len(), 64, "two GpuJointTrack rows must stride 32 B");
    assert_eq!(u32_at(&bytes, 0), 1, "joint at 0");
    assert_eq!(
        u32_at(&bytes, 4),
        u32::MAX.wrapping_sub(1),
        "priority -2 at 4"
    );
    assert_eq!(u32_at(&bytes, 20), 6, "pos_count at 20");
    Ok(())
}

/// `GpuSampleJob` packs to the WGSL `SampleJob` layout: stride 16, `phase`
/// at 8.
#[test]
fn packing_sample_job_matches_wgsl() -> Result<(), TestError> {
    let rows = vec![
        GpuSampleJob {
            clip_id: 1,
            cache_base: 2,
            phase: 3.0,
            pad0: 0,
        },
        GpuSampleJob::default(),
    ];
    let bytes = std430_bytes(&rows)?;
    assert_eq!(bytes.len(), 32, "two GpuSampleJob rows must stride 16 B");
    assert_eq!(u32_at(&bytes, 0), 1, "clip_id at 0");
    assert!((f32_at(&bytes, 8) - 3.0).abs() < f32::EPSILON, "phase at 8");
    Ok(())
}

/// `GpuPlayState` packs to the WGSL `PlayState` layout: stride 32, `start`
/// at 8, `order` at 16.
#[test]
fn packing_play_state_matches_wgsl() -> Result<(), TestError> {
    let rows = vec![
        GpuPlayState {
            clip_id: 1,
            cache_base: 2,
            start: 3.0,
            stopped_at: 4.0,
            order: 5,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        },
        GpuPlayState::default(),
    ];
    let bytes = std430_bytes(&rows)?;
    assert_eq!(bytes.len(), 64, "two GpuPlayState rows must stride 32 B");
    assert!((f32_at(&bytes, 8) - 3.0).abs() < f32::EPSILON, "start at 8");
    assert_eq!(u32_at(&bytes, 16), 5, "order at 16");
    Ok(())
}

/// `GpuCorrection` packs to the WGSL `Correction` layout: stride 48, `rot`
/// at 16, `pos` at 32.
#[test]
fn packing_correction_matches_wgsl() -> Result<(), TestError> {
    let rows = vec![
        GpuCorrection {
            avatar: 1,
            joint: 2,
            flags: 3,
            pad0: 0,
            rot: Vec4::new(4.0, 5.0, 6.0, 7.0),
            pos: Vec3::new(8.0, 9.0, 10.0),
            pad1: 0,
        },
        GpuCorrection::default(),
    ];
    let bytes = std430_bytes(&rows)?;
    assert_eq!(bytes.len(), 96, "two GpuCorrection rows must stride 48 B");
    assert_eq!(u32_at(&bytes, 0), 1, "avatar at 0");
    assert!((f32_at(&bytes, 16) - 4.0).abs() < f32::EPSILON, "rot at 16");
    assert!((f32_at(&bytes, 32) - 8.0).abs() < f32::EPSILON, "pos at 32");
    Ok(())
}

/// The widened `GpuComputeParams` packs to the WGSL `Params` uniform layout:
/// size 64, `now` at 32, `chest_joint` at 40, `flags` at 48.
#[test]
fn packing_compute_params_phase2_matches_wgsl() -> Result<(), TestError> {
    let params = GpuComputeParams {
        avatar_count: 1,
        joint_count: 2,
        instance_count: 3,
        max_skin_joints: 4,
        readback_instance: 5,
        readback_joint_count: 6,
        sample_job_count: 7,
        correction_count: 8,
        now: 9.0,
        idle_now: 10.0,
        chest_joint: 11,
        torso_joint: 12,
        flags: 13,
        pad0: 0,
        pad1: 0,
        pad2: 0,
    };
    let mut buffer = encase::UniformBuffer::new(Vec::<u8>::new());
    buffer.write(&params)?;
    let bytes = buffer.into_inner();
    assert_eq!(bytes.len(), 64, "GpuComputeParams must be 64 B");
    assert_eq!(u32_at(&bytes, 24), 7, "sample_job_count at 24");
    assert!((f32_at(&bytes, 32) - 9.0).abs() < f32::EPSILON, "now at 32");
    assert_eq!(u32_at(&bytes, 40), 11, "chest_joint at 40");
    assert_eq!(u32_at(&bytes, 48), 13, "flags at 48");
    Ok(())
}

/// **The Phase 2 GPU gate**: run the real `sample` + `blend` + `fk` +
/// `palettes` WGSL passes over a staged clip arena / playback / job /
/// correction fixture — a two-clip priority contest with partial ease
/// weights, the procedural idle adjusters on, and one sparse correction —
/// and assert, through the pipeline's own compute-copy readback channel,
/// that the GPU-written palette equals the CPU mirror pipeline
/// ([`mirror_local_pose`] → [`reference_fk`]) to 1e-4 per component.
///
/// Skips (loudly) when no frame comes back: a machine with no GPU adapter
/// cannot answer, mirroring the readback test tier.
#[test]
fn the_gpu_sampled_blended_palette_matches_the_cpu_mirror() -> Result<(), TestError> {
    let skeleton = fixture_skeleton()?;
    let deform = fixture_deform()?;
    let volumes = fixture_volumes()?;
    let overrides = JointOverrides::default();
    let pelvis = skeleton.find("mPelvis").ok_or("mPelvis missing")?;
    let torso = skeleton.find("mTorso").ok_or("mTorso missing")?;
    let chest = skeleton.find("mChest").ok_or("mChest missing")?;
    let hip = skeleton.find("mHipRight").ok_or("mHipRight missing")?;
    let joint_count = u32::try_from(skeleton.len()).map_err(|_error| "joint count")?;
    let root = fixture_root();
    let now = 5.0_f32;
    let idle_now =
        (now * crate::animations::POSE_IDLE_HZ).floor() / crate::animations::POSE_IDLE_HZ;

    // Two clips contesting the pelvis: the golden multi-track clip at full
    // weight, and a competing single-key clip still easing in.
    let clip_a = golden_motion(true);
    let mut clip_b = blend_clip(
        JointPriority::HIGH,
        [0.0, 0.258_819_04, 0.0, 0.965_925_8],
        0.5,
        0.25,
    );
    clip_b.base_priority = JointPriority::MEDIUM;
    let mut arena = ClipArena::default();
    let skeleton_index = |name: &str| skeleton.find(name);
    let id_a = arena
        .ensure_clip(
            AssetKey::from(Uuid::from_u128(400)),
            &clip_a,
            joint_count,
            skeleton_index,
        )
        .ok_or("clip a upload")?;
    let id_b = arena
        .ensure_clip(
            AssetKey::from(Uuid::from_u128(401)),
            &clip_b,
            joint_count,
            skeleton_index,
        )
        .ok_or("clip b upload")?;
    let (start_a, start_b) = (1.0_f32, 4.8_f32);
    let jobs = vec![
        GpuSampleJob {
            clip_id: id_a,
            cache_base: 0,
            phase: now - start_a,
            pad0: 0,
        },
        GpuSampleJob {
            clip_id: id_b,
            cache_base: arena.track_count(id_a),
            phase: now - start_b,
            pad0: 0,
        },
    ];
    let cache_len = arena
        .track_count(id_a)
        .checked_add(arena.track_count(id_b))
        .ok_or("cache len")?;
    let mut plays = vec![GpuPlayState::default(); MAX_ACTIVE_CLIPS];
    *plays.first_mut().ok_or("slot 0")? = GpuPlayState {
        clip_id: id_a,
        cache_base: 0,
        start: start_a,
        stopped_at: PLAY_STOPPED_NONE,
        order: 1,
        pad0: 0,
        pad1: 0,
        pad2: 0,
    };
    *plays.get_mut(1).ok_or("slot 1")? = GpuPlayState {
        clip_id: id_b,
        cache_base: arena.track_count(id_a),
        start: start_b,
        stopped_at: PLAY_STOPPED_NONE,
        order: 2,
        pad0: 0,
        pad1: 0,
        pad2: 0,
    };
    // One sparse correction: replace the right hip's rotation outright (an
    // "IK result").
    let hip_u32 = u32::try_from(hip).map_err(|_error| "hip index")?;
    let correction_value = GpuLocalPose {
        rot: Vec4::new(0.182_574_18, 0.365_148_37, 0.547_722_5, 0.730_296_74),
        pos: Vec3::ZERO,
        flags: POSE_FLAG_ROT,
    };
    let chest_u32 = u32::try_from(chest).map_err(|_error| "chest index")?;
    let torso_u32 = u32::try_from(torso).map_err(|_error| "torso index")?;

    // The CPU mirror expectation: passes A+B (sample, blend, idle,
    // corrections) then pass C (reference FK) then pass D (× ibps).
    let rows = mirror_local_pose(
        arena.slices(),
        &plays,
        &jobs,
        cache_len,
        joint_count,
        now,
        Some(idle_now),
        chest_u32,
        torso_u32,
        &[(hip_u32, correction_value)],
    );
    // Teeth: the fixture actually exercises blend + idle + correction.
    assert!(
        rows.get(pelvis)
            .is_some_and(|row| row.flags & POSE_FLAG_ROT != 0),
        "the pelvis must be animated by the contest"
    );
    assert!(
        rows.get(chest)
            .is_some_and(|row| row.flags & POSE_FLAG_ROT != 0),
        "the chest must carry the idle breathe"
    );
    assert!(
        rows.get(hip).is_some_and(|row| row.flags == POSE_FLAG_ROT),
        "the hip must carry the correction"
    );
    let rest = compose_rest_joints(&skeleton, &deform, &volumes, &overrides);
    let world = reference_fk(&rest, &rows, root);
    let joint_map: Vec<u32> = vec![
        u32::try_from(pelvis).map_err(|_error| "pelvis index")?,
        u32::try_from(torso).map_err(|_error| "torso index")?,
    ];
    let ibps = [
        Mat4::from_translation(Vec3::new(0.0, -1.0, 0.3)),
        Mat4::from_rotation_z(0.5),
    ];
    let expected: Vec<Mat4> = joint_map
        .iter()
        .zip(ibps.iter())
        .map(|(&canonical, ibp)| {
            let canonical = usize::try_from(canonical).unwrap_or(usize::MAX);
            world
                .get(canonical)
                .copied()
                .unwrap_or(Mat4::IDENTITY)
                .mul_mat4(ibp)
        })
        .collect();

    let (clip_headers, clip_tracks, track_of_joint, key_times, key_values, clip_generation) =
        arena.staged();

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<LogPlugin>(),
    )
    .add_plugins(ScheduleRunnerPlugin::run_loop(core::time::Duration::ZERO))
    .add_plugins(SlFaceMaterialPlugin)
    .add_plugins(GpuAvatarsPlugin {
        mode: GpuAvatarsMode {
            active: true,
            readback: true,
            live: false,
        },
    });
    app.add_systems(
        Update,
        |mut meshes: Query<&mut Transform, With<SkinnedMesh>>| {
            for mut transform in &mut meshes {
                transform.set_changed();
            }
        },
    );

    let pixels: Cell = Cell::default();
    let pixels_in_observer = Arc::clone(&pixels);
    let rest_for_startup = rest.clone();
    let expected_for_startup = expected.clone();
    let joint_map_for_startup = joint_map.clone();
    let jobs_for_startup = jobs.clone();
    let plays_for_startup = plays.clone();

    app.add_systems(
        Startup,
        move |mut commands: Commands,
              mut meshes: ResMut<Assets<Mesh>>,
              mut materials: ResMut<Assets<FaceMaterial>>,
              mut images: ResMut<Assets<Image>>,
              mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>| {
            let mut target =
                Image::new_target_texture(FRAME, FRAME, TextureFormat::Rgba8UnormSrgb, None);
            target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
            let target = images.add(target);
            commands.spawn((
                Camera3d::default(),
                RenderTarget::Image(target.clone().into()),
                bevy::camera::Hdr,
                Msaa::Off,
                Transform::from_xyz(0.0, 0.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
            ));
            let pixels_cell = Arc::clone(&pixels_in_observer);
            commands.spawn(Readback::texture(target)).observe(
                move |readback: On<ReadbackComplete>| {
                    if let Ok(mut slot) = pixels_cell.lock() {
                        *slot = Some(readback.data.clone());
                    }
                },
            );
            let joints = vec![
                commands.spawn(Transform::from_xyz(0.0, 0.0, 0.0)).id(),
                commands.spawn(Transform::from_xyz(0.0, 1.0, 0.0)).id(),
            ];
            let inverse_bindposes = bindposes.add(SkinnedMeshInverseBindposes::from(vec![
                Mat4::from_translation(Vec3::new(0.0, -1.0, 0.3)),
                Mat4::from_rotation_z(0.5),
            ]));
            let quad = commands
                .spawn((
                    Mesh3d(meshes.add(skinned_quad())),
                    MeshMaterial3d(materials.add(inert_face_material(StandardMaterial {
                        base_color: Color::srgb(0.0, 1.0, 0.0),
                        unlit: true,
                        ..default()
                    }))),
                    Transform::IDENTITY,
                    SkinnedMesh {
                        inverse_bindposes,
                        joints,
                    },
                    NoFrustumCulling,
                ))
                .id();
            commands.insert_resource(GpuAvatarStaging {
                joint_count,
                slot_capacity: 1,
                frames: vec![GpuAvatarFrame {
                    root,
                    slot: 0,
                    pad0: 0,
                    pad1: 0,
                    pad2: 0,
                }],
                // Phase 2: the local pose is GPU-computed by passes A+B.
                local_pose: Vec::new(),
                rest: Arc::new(rest_for_startup.clone()),
                rest_generation: 1,
                joint_map: Arc::new(joint_map_for_startup.clone()),
                ibps: Arc::new(vec![
                    Mat4::from_translation(Vec3::new(0.0, -1.0, 0.3)),
                    Mat4::from_rotation_z(0.5),
                ]),
                pool_generation: 1,
                instances: vec![StagedSkinInstance {
                    target: quad,
                    avatar_slot: 0,
                    joint_count: 2,
                    joint_map_offset: 0,
                    ibp_offset: 0,
                }],
                readback: Some(StagedReadback {
                    target: quad,
                    label: "phase-2 headless fixture".to_owned(),
                    joint_count: 2,
                    expected: expected_for_startup.clone(),
                }),
                blend: true,
                clip_headers: Arc::clone(&clip_headers),
                clip_tracks: Arc::clone(&clip_tracks),
                track_of_joint: Arc::clone(&track_of_joint),
                key_times: Arc::clone(&key_times),
                key_values: Arc::clone(&key_values),
                clip_generation,
                jobs: jobs_for_startup.clone(),
                cache_len,
                playback: Arc::new(plays_for_startup.clone()),
                playback_generation: 1,
                corrections: vec![GpuCorrection {
                    avatar: 0,
                    joint: hip_u32,
                    flags: correction_value.flags,
                    pad0: 0,
                    rot: correction_value.rot,
                    pos: correction_value.pos,
                    pad1: 0,
                }],
                now,
                idle_now,
                chest_joint: chest_u32,
                torso_joint: torso_u32,
                param_flags: 0,
            });
        },
    );

    app.finish();
    app.cleanup();
    for _frame in 0..FRAMES_TO_RUN {
        app.update();
    }

    let frame = pixels.lock().ok().and_then(|mut slot| slot.take());
    if frame.is_none() {
        warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
        return Ok(());
    }

    let bytes = app
        .world()
        .get_resource::<GpuAvatarReadbackData>()
        .map(|data| data.bytes.clone())
        .ok_or("the readback data resource is missing")?;
    assert!(
        !bytes.is_empty(),
        "the machine renders but the GPU-avatar readback never completed — the compute \
         pipeline did not run"
    );
    let worst = palette_worst_diff(&bytes, 2).ok_or(
        "the readback completed but its expected half is implausible (all zeros) — the \
         readback pass never executed over a resolved instance",
    )?;
    assert!(
        worst <= 1.0e-4,
        "the GPU sample+blend+FK palette diverges from the CPU mirror (worst component \
         diff {worst:e}) — passes A/B do not reproduce sample_motion/blend_joint"
    );
    Ok(())
}
