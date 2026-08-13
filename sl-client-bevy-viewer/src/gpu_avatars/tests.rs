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

use super::render::{GpuAvatarReadbackData, palette_worst_diff};
use super::stage::{GpuAvatarStaging, StagedReadback, StagedSkinInstance};
use super::types::{
    GpuAvatarFrame, GpuComputeParams, GpuLocalPose, GpuRestJoint, GpuSkinInstance,
    compose_rest_joints, pose_rows, reference_fk,
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
// Ghost skin-joint resolution: never all-or-nothing (the missing-eyes class).
// ---------------------------------------------------------------------------

/// A submesh whose joint list contains entries that cannot resolve — an entity
/// that is no avatar joint at all, and a joint belonging to a *different*
/// avatar — still yields a **full-length** joint map (the bad entries pinned
/// to the fallback), with each failure reported by position, entity and
/// reason. A dropped submesh (the bug this guards) would have returned no map
/// at all and silently vanished from the pass-D instance table.
#[test]
fn resolve_joint_map_survives_unresolvable_joints() {
    use super::stage::resolve_joint_map;
    use sl_client_bevy::{AgentKey, Uuid};

    let mut world = World::new();
    let wearer = AgentKey::from(Uuid::from_u128(1));
    let other = AgentKey::from(Uuid::from_u128(2));
    let good_a = world.spawn_empty().id();
    let good_b = world.spawn_empty().id();
    let foreign = world.spawn_empty().id();
    let stranger = world.spawn_empty().id();

    let mut lookup = std::collections::HashMap::new();
    let _prev = lookup.insert(good_a, (wearer, 3_u32));
    let _prev = lookup.insert(good_b, (wearer, 7_u32));
    let _prev = lookup.insert(foreign, (other, 5_u32));

    let joints = vec![good_a, foreign, stranger, good_b];
    let (map, unresolved) = resolve_joint_map(&joints, wearer, &lookup, 42);

    assert_eq!(
        map,
        vec![3, 42, 42, 7],
        "resolvable joints keep their canonical index; unresolvable ones take \
         the fallback instead of dropping the submesh"
    );
    assert_eq!(unresolved.len(), 2, "both failures are reported");
    let foreign_report = unresolved
        .iter()
        .find(|entry| entry.joint == foreign)
        .copied();
    assert!(
        foreign_report.is_some_and(|entry| entry.position == 1 && entry.owner == Some(other)),
        "the other avatar's joint is reported with its owner: {unresolved:?}"
    );
    let stranger_report = unresolved
        .iter()
        .find(|entry| entry.joint == stranger)
        .copied();
    assert!(
        stranger_report.is_some_and(|entry| entry.position == 2 && entry.owner.is_none()),
        "the non-avatar entity is reported ownerless: {unresolved:?}"
    );
}

/// A fully resolvable joint list resolves untouched, with nothing reported.
#[test]
fn resolve_joint_map_passes_a_clean_list_through() {
    use super::stage::resolve_joint_map;
    use sl_client_bevy::{AgentKey, Uuid};

    let mut world = World::new();
    let wearer = AgentKey::from(Uuid::from_u128(1));
    let a = world.spawn_empty().id();
    let b = world.spawn_empty().id();
    let mut lookup = std::collections::HashMap::new();
    let _prev = lookup.insert(a, (wearer, 0_u32));
    let _prev = lookup.insert(b, (wearer, 9_u32));

    let (map, unresolved) = resolve_joint_map(&[a, b], wearer, &lookup, 42);
    assert_eq!(map, vec![0, 9]);
    assert!(unresolved.is_empty(), "nothing to report: {unresolved:?}");
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

/// `GpuComputeParams` packs to the WGSL `Params` uniform layout: size 32,
/// scalars at 0..=20.
#[test]
fn packing_compute_params_matches_wgsl() -> Result<(), TestError> {
    let params = GpuComputeParams {
        avatar_count: 1,
        joint_count: 2,
        instance_count: 3,
        max_skin_joints: 4,
        readback_instance: 5,
        readback_joint_count: 6,
        pad0: 0,
        pad1: 0,
    };
    let mut buffer = encase::UniformBuffer::new(Vec::<u8>::new());
    buffer.write(&params)?;
    let bytes = buffer.into_inner();
    assert_eq!(bytes.len(), 32, "GpuComputeParams must be 32 B");
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
        mode: Some(GpuAvatarsMode {
            // The render half is placement-agnostic (it writes whatever
            // instances are staged); ghost placement documents that the
            // fixture stages an explicit target entity.
            placement: super::GpuAvatarPlacement::Ghost,
            active: true,
            readback: true,
            // The test stages fixture data by hand instead of reading the
            // (absent) avatar state.
            live: false,
            ghost_offset: 2.0,
        }),
    });

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
    Ok(())
}
