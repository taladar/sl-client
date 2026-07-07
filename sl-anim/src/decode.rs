//! Decoding of the Linden keyframe-motion binary format, the `.anim` files a
//! viewer plays to pose an avatar's skeleton.
//!
//! A `.anim` file is a fixed header (version, base priority, duration, emote
//! name, loop points, ease-in/out durations, hand pose) followed by a list of
//! animated joints — each with a name, a per-joint priority, and quantised
//! rotation and position keyframe tracks — and finally an optional list of
//! collision-volume [`Constraint`]s. Every multi-byte field is little-endian.
//!
//! The quantised values are widened back to `f32` exactly as the reference
//! viewer does:
//!
//! - times are a `u16` fraction of the motion's [`duration`](Motion::duration),
//! - rotation angles are three `u16` components of an imaginary quaternion in
//!   `[-1, 1]`, from which the real part is recovered as
//!   `w = sqrt(max(0, 1 - x² - y² - z²))`,
//! - positions are three `u16` components in `[-5, 5]` metres (Z-up).
//!
//! Both the modern `1.0` encoding and the legacy `0.1` encoding (still used by
//! decades-old Second Life animation assets, which visual updates do not
//! replace) are decoded: the `0.1` form stores keyframe times as `f32` seconds
//! and rotations as `f32` Euler angles in degrees (`ZYX` order, built with the
//! reference viewer's `mayaQ`), and positions as `f32` metres clamped to the
//! same `[-5, 5]` range.
//!
//! This module is I/O-free: it decodes a borrowed `&[u8]` into an owned
//! [`Motion`]. The binary layout follows Firestorm
//! `LLKeyframeMotion::deserialize` / `serialize`
//! (`indra/llcharacter/llkeyframemotion.cpp`), reimplemented here idiomatically
//! without the workspace-forbidden `as` casts, slice indexing, or `unwrap` /
//! `expect` / `panic`.

/// The `version` a modern `.anim` file declares (`KEYFRAME_MOTION_VERSION`).
const CURRENT_VERSION: u16 = 1;
/// The `sub_version` a modern `.anim` file declares
/// (`KEYFRAME_MOTION_SUBVERSION`).
const CURRENT_SUB_VERSION: u16 = 0;

/// The `version` a legacy `0.1` `.anim` file declares. The legacy form (still
/// found in decades-old Second Life content) stores keyframe times as `f32`
/// seconds and rotations as `f32` Euler angles in degrees rather than the
/// modern quantised `u16` encoding.
const OLD_VERSION: u16 = 0;
/// The `sub_version` a legacy `0.1` `.anim` file declares.
const OLD_SUB_VERSION: u16 = 1;

/// Degrees-to-radians factor for the legacy Euler-angle rotation keys.
const DEG_TO_RAD: f32 = core::f32::consts::PI / 180.0;

/// The maximum motion duration the reference viewer accepts, in seconds
/// (`MAX_ANIM_DURATION`). A longer or non-finite duration is rejected as
/// corrupt.
const MAX_ANIM_DURATION: f32 = 60.0;

/// The half-range of a keyframe position component, in metres
/// (`LL_MAX_PELVIS_OFFSET`): positions are quantised over `[-5, 5]`.
const MAX_PELVIS_OFFSET: f32 = 5.0;

/// The most joint motions a single animation may carry
/// (`LL_CHARACTER_MAX_ANIMATED_JOINTS`).
const MAX_ANIMATED_JOINTS: u32 = 216;

/// The most collision-volume constraints the reference viewer reads before
/// treating the count as corrupt and ignoring the constraint block
/// (`MAX_CONSTRAINTS`).
const MAX_CONSTRAINTS: i32 = 10;

/// The largest `hand_pose` index the reference viewer accepts
/// (`LLHandMotion::NUM_HAND_POSES`).
const NUM_HAND_POSES: u32 = 14;

/// The fixed on-disk length of a constraint's source / target collision-volume
/// name field, in bytes (a NUL-padded ASCII string).
const CONSTRAINT_VOLUME_NAME_LEN: usize = 16;

/// A per-joint animation priority. Higher-priority joints win when several
/// motions animate the same joint (Phase 18.4 blending); a value of
/// [`USE_MOTION`](Self::USE_MOTION) defers to the motion's base priority.
///
/// Stored as the raw signed value the file carries (`LLJoint::JointPriority`);
/// the named constants cover the values the reference viewer emits, but any
/// value `>= USE_MOTION_PRIORITY` decodes so unknown priorities survive a round
/// trip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct JointPriority(i32);

impl JointPriority {
    /// Defer to the motion's [`base_priority`](Motion::base_priority)
    /// (`USE_MOTION_PRIORITY`, the lowest legal value).
    pub const USE_MOTION: Self = Self(-1);
    /// Lowest explicit priority (`LOW_PRIORITY`).
    pub const LOW: Self = Self(0);
    /// Medium priority (`MEDIUM_PRIORITY`).
    pub const MEDIUM: Self = Self(1);
    /// High priority (`HIGH_PRIORITY`).
    pub const HIGH: Self = Self(2);
    /// Higher priority (`HIGHER_PRIORITY`).
    pub const HIGHER: Self = Self(3);
    /// Highest named priority (`HIGHEST_PRIORITY`).
    pub const HIGHEST: Self = Self(4);
    /// Additive priority — the animation is layered on top rather than blended
    /// (`ADDITIVE_PRIORITY`, `LL_CHARACTER_MAX_PRIORITY`).
    pub const ADDITIVE: Self = Self(7);

    /// The raw signed priority value.
    #[must_use]
    pub const fn value(self) -> i32 {
        self.0
    }
}

/// The resting hand pose an animation selects for the joints it does not itself
/// animate (`LLHandMotion::eHandPose`). Stored as the raw index; the named
/// constants cover the poses the reference viewer defines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandPose(u32);

impl HandPose {
    /// `HAND_POSE_SPREAD` (index 0).
    pub const SPREAD: Self = Self(0);
    /// `HAND_POSE_RELAXED` (index 1).
    pub const RELAXED: Self = Self(1);
    /// `HAND_POSE_POINT` (index 2).
    pub const POINT: Self = Self(2);
    /// `HAND_POSE_FIST` (index 3).
    pub const FIST: Self = Self(3);
    /// `HAND_POSE_RELAXED_L` (index 4).
    pub const RELAXED_L: Self = Self(4);
    /// `HAND_POSE_POINT_L` (index 5).
    pub const POINT_L: Self = Self(5);
    /// `HAND_POSE_FIST_L` (index 6).
    pub const FIST_L: Self = Self(6);
    /// `HAND_POSE_RELAXED_R` (index 7).
    pub const RELAXED_R: Self = Self(7);
    /// `HAND_POSE_POINT_R` (index 8).
    pub const POINT_R: Self = Self(8);
    /// `HAND_POSE_FIST_R` (index 9).
    pub const FIST_R: Self = Self(9);
    /// `HAND_POSE_SALUTE_R` (index 10).
    pub const SALUTE_R: Self = Self(10);
    /// `HAND_POSE_TYPING` (index 11).
    pub const TYPING: Self = Self(11);
    /// `HAND_POSE_PEACE_R` (index 12).
    pub const PEACE_R: Self = Self(12);
    /// `HAND_POSE_PALM_R` (index 13).
    pub const PALM_R: Self = Self(13);

    /// The raw hand-pose index.
    #[must_use]
    pub const fn value(self) -> u32 {
        self.0
    }
}

/// The kind of collision-volume constraint an animation applies
/// (`EConstraintType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConstraintType {
    /// Constrain the source point to the target point (`CONSTRAINT_TYPE_POINT`).
    Point,
    /// Constrain the source point to the target plane (`CONSTRAINT_TYPE_PLANE`).
    Plane,
}

/// What a constraint's target is anchored to (`EConstraintTargetType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConstraintTargetType {
    /// Anchored to another collision volume on the body
    /// (`CONSTRAINT_TARGET_TYPE_BODY`); the volume is named by
    /// [`Constraint::target_volume`].
    Body,
    /// Anchored to the ground plane (`CONSTRAINT_TARGET_TYPE_GROUND`), signalled
    /// by the reserved target-volume name `GROUND`.
    Ground,
}

/// One rotation keyframe: the joint's orientation at [`time`](Self::time).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotationKey {
    /// The keyframe time, in seconds from the start of the motion
    /// (`0..=duration`).
    pub time: f32,
    /// The orientation as a unit quaternion `[x, y, z, w]` (SL Z-up). The real
    /// part `w` is recovered from the three stored imaginary components.
    pub rotation: [f32; 4],
}

/// One position keyframe: the joint's local position at [`time`](Self::time).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionKey {
    /// The keyframe time, in seconds from the start of the motion.
    pub time: f32,
    /// The local position in metres (SL Z-up), each component in `[-5, 5]`.
    pub position: [f32; 3],
}

/// The animation of a single named joint: its priority and its rotation and
/// position keyframe tracks. A track may be empty (the joint is only animated
/// in the other channel).
#[derive(Clone, Debug, PartialEq)]
pub struct JointMotion {
    /// The joint's name (e.g. `mPelvis`, `mChest`), matched against the target
    /// skeleton at play time.
    pub name: String,
    /// This joint's animation priority.
    pub priority: JointPriority,
    /// The rotation keyframes, in the order stored in the file.
    pub rotation_keys: Vec<RotationKey>,
    /// The position keyframes, in the order stored in the file.
    pub position_keys: Vec<PositionKey>,
}

/// A collision-volume constraint (inverse-kinematics anchor) an animation
/// carries, e.g. pinning a foot to the ground while a leg moves.
#[derive(Clone, Debug, PartialEq)]
pub struct Constraint {
    /// How many joints up the skeleton the constraint's IK chain spans, from
    /// the source volume's joint.
    pub chain_length: u8,
    /// The kind of constraint.
    pub constraint_type: ConstraintType,
    /// The name of the source collision volume the constraint acts from.
    pub source_volume: String,
    /// The offset from the source volume, in metres (Z-up).
    pub source_offset: [f32; 3],
    /// Whether the target is another body volume or the ground.
    pub target_type: ConstraintTargetType,
    /// The name of the target collision volume
    /// ([`Body`](ConstraintTargetType::Body) targets only; empty for a
    /// [`Ground`](ConstraintTargetType::Ground) target).
    pub target_volume: String,
    /// The offset from the target, in metres (Z-up).
    pub target_offset: [f32; 3],
    /// The target direction (Z-up); a zero vector means no target offset is
    /// applied.
    pub target_dir: [f32; 3],
    /// The time the constraint begins easing in, in seconds.
    pub ease_in_start_time: f32,
    /// The time the constraint has fully eased in, in seconds.
    pub ease_in_stop_time: f32,
    /// The time the constraint begins easing out, in seconds.
    pub ease_out_start_time: f32,
    /// The time the constraint has fully eased out, in seconds.
    pub ease_out_stop_time: f32,
}

/// A decoded Linden keyframe motion: the whole contents of a `.anim` file.
#[derive(Clone, Debug, PartialEq)]
pub struct Motion {
    /// The motion's base priority; a joint whose priority is
    /// [`JointPriority::USE_MOTION`] inherits this.
    pub base_priority: JointPriority,
    /// The total duration of the motion, in seconds (`0..=60`).
    pub duration: f32,
    /// The optional emote name the motion triggers on the face (empty when
    /// none).
    pub emote_name: String,
    /// The time within the motion at which a loop restarts, in seconds.
    pub loop_in_point: f32,
    /// The time within the motion at which a loop wraps back to
    /// [`loop_in_point`](Self::loop_in_point), in seconds.
    pub loop_out_point: f32,
    /// Whether the motion loops.
    pub loops: bool,
    /// How long the motion eases in when it starts, in seconds.
    pub ease_in_duration: f32,
    /// How long the motion eases out when it stops, in seconds.
    pub ease_out_duration: f32,
    /// The resting hand pose applied to unanimated hand joints.
    pub hand_pose: HandPose,
    /// The per-joint animation tracks.
    pub joints: Vec<JointMotion>,
    /// The collision-volume constraints (empty when the file carries none, or
    /// when a corrupt constraint count caused the block to be ignored, matching
    /// the reference viewer).
    pub constraints: Vec<Constraint>,
}

impl Motion {
    /// Decode a `.anim` (Linden keyframe-motion) file from its raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an [`AnimDecodeError`] if the data is truncated, declares an
    /// unsupported version, or fails one of the range / finiteness checks the
    /// reference viewer applies (bad priority, over-long duration, too many
    /// joints, a negative key count, a non-finite value, an out-of-range
    /// keyframe time, or an invalid constraint type).
    pub fn from_bytes(data: &[u8]) -> Result<Self, AnimDecodeError> {
        let mut cursor = Cursor::new(data);

        let version = cursor.read_u16("version")?;
        let sub_version = cursor.read_u16("sub_version")?;
        let old_version = if version == CURRENT_VERSION && sub_version == CURRENT_SUB_VERSION {
            false
        } else if version == OLD_VERSION && sub_version == OLD_SUB_VERSION {
            true
        } else {
            return Err(AnimDecodeError::UnsupportedVersion {
                version,
                sub_version,
            });
        };

        let base_priority = read_base_priority(cursor.read_i32("base_priority")?)?;

        let duration = cursor.read_f32("duration")?;
        if !duration.is_finite() || duration > MAX_ANIM_DURATION {
            return Err(AnimDecodeError::InvalidDuration { duration });
        }

        let emote_name = cursor.read_cstring("emote_name")?;

        let loop_in_point = read_finite(cursor.read_f32("loop_in_point")?, "loop_in_point")?;
        let loop_out_point = read_finite(cursor.read_f32("loop_out_point")?, "loop_out_point")?;
        let loops = cursor.read_i32("loop")? != 0;

        let ease_in_duration =
            read_finite(cursor.read_f32("ease_in_duration")?, "ease_in_duration")?;
        let ease_out_duration =
            read_finite(cursor.read_f32("ease_out_duration")?, "ease_out_duration")?;

        let hand_pose = read_hand_pose(cursor.read_u32("hand_pose")?)?;

        let num_joints = cursor.read_u32("num_joints")?;
        if num_joints == 0 {
            return Err(AnimDecodeError::NoJoints);
        }
        if num_joints > MAX_ANIMATED_JOINTS {
            return Err(AnimDecodeError::TooManyJoints { count: num_joints });
        }

        let mut joints = Vec::with_capacity(usize::try_from(num_joints).unwrap_or(0));
        for _ in 0..num_joints {
            joints.push(read_joint(&mut cursor, duration, old_version)?);
        }

        let constraints = read_constraints(&mut cursor, num_joints)?;

        Ok(Self {
            base_priority,
            duration,
            emote_name,
            loop_in_point,
            loop_out_point,
            loops,
            ease_in_duration,
            ease_out_duration,
            hand_pose,
            joints,
            constraints,
        })
    }
}

/// Validate and normalise a base priority. The reference viewer clamps a value
/// at or above [`JointPriority::ADDITIVE`] down to one below it, and rejects
/// anything below [`JointPriority::USE_MOTION`].
const fn read_base_priority(raw: i32) -> Result<JointPriority, AnimDecodeError> {
    if raw < JointPriority::USE_MOTION.value() {
        return Err(AnimDecodeError::BadBasePriority { priority: raw });
    }
    if raw >= JointPriority::ADDITIVE.value() {
        return Ok(JointPriority(
            JointPriority::ADDITIVE.value().saturating_sub(1),
        ));
    }
    Ok(JointPriority(raw))
}

/// Validate a joint priority: it must be at least [`JointPriority::USE_MOTION`].
const fn read_joint_priority(raw: i32) -> Result<JointPriority, AnimDecodeError> {
    if raw < JointPriority::USE_MOTION.value() {
        return Err(AnimDecodeError::BadJointPriority { priority: raw });
    }
    Ok(JointPriority(raw))
}

/// Validate a hand-pose index against `NUM_HAND_POSES`.
const fn read_hand_pose(raw: u32) -> Result<HandPose, AnimDecodeError> {
    if raw > NUM_HAND_POSES {
        return Err(AnimDecodeError::InvalidHandPose { value: raw });
    }
    Ok(HandPose(raw))
}

/// Return `value` if it is finite, else the
/// [`NonFinite`](AnimDecodeError::NonFinite) error tagged with `field`.
const fn read_finite(value: f32, field: &'static str) -> Result<f32, AnimDecodeError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(AnimDecodeError::NonFinite { field })
    }
}

/// Decode one [`JointMotion`]: name, priority, rotation track, position track.
/// `old_version` selects the legacy `0.1` keyframe encoding.
fn read_joint(
    cursor: &mut Cursor<'_>,
    duration: f32,
    old_version: bool,
) -> Result<JointMotion, AnimDecodeError> {
    let name = cursor.read_cstring("joint_name")?;
    let priority = read_joint_priority(cursor.read_i32("joint_priority")?)?;

    let num_rot_keys = read_key_count(cursor.read_i32("num_rot_keys")?, "num_rot_keys")?;
    let mut rotation_keys = Vec::with_capacity(num_rot_keys);
    for _ in 0..num_rot_keys {
        rotation_keys.push(read_rotation_key(cursor, duration, old_version)?);
    }

    let num_pos_keys = read_key_count(cursor.read_i32("num_pos_keys")?, "num_pos_keys")?;
    let mut position_keys = Vec::with_capacity(num_pos_keys);
    for _ in 0..num_pos_keys {
        position_keys.push(read_position_key(cursor, duration, old_version)?);
    }

    Ok(JointMotion {
        name,
        priority,
        rotation_keys,
        position_keys,
    })
}

/// Validate a signed keyframe count (must be non-negative) and widen it to a
/// `usize`.
fn read_key_count(raw: i32, field: &'static str) -> Result<usize, AnimDecodeError> {
    usize::try_from(raw).map_err(|_ignored| AnimDecodeError::NegativeKeyCount { field })
}

/// Decode one rotation keyframe. In the modern encoding this is a `u16` time
/// and three `u16` imaginary-quaternion components, widened and completed to a
/// unit quaternion. In the legacy `0.1` encoding it is an `f32` time and three
/// `f32` Euler angles (degrees, `ZYX` order) built into a quaternion by
/// [`maya_q_zyx`].
fn read_rotation_key(
    cursor: &mut Cursor<'_>,
    duration: f32,
    old_version: bool,
) -> Result<RotationKey, AnimDecodeError> {
    if old_version {
        let time = read_finite(cursor.read_f32("rot_time")?, "rot_time")?;
        let x = read_finite(cursor.read_f32("rot_angle_x")?, "rot_angle_x")?;
        let y = read_finite(cursor.read_f32("rot_angle_y")?, "rot_angle_y")?;
        let z = read_finite(cursor.read_f32("rot_angle_z")?, "rot_angle_z")?;
        return Ok(RotationKey {
            time,
            rotation: maya_q_zyx(x, y, z),
        });
    }
    let time = u16_to_f32(cursor.read_u16("rot_time")?, 0.0, duration);
    if time < 0.0 || time > duration {
        return Err(AnimDecodeError::KeyTimeOutOfRange { time, duration });
    }
    let x = u16_to_f32(cursor.read_u16("rot_angle_x")?, -1.0, 1.0);
    let y = u16_to_f32(cursor.read_u16("rot_angle_y")?, -1.0, 1.0);
    let z = u16_to_f32(cursor.read_u16("rot_angle_z")?, -1.0, 1.0);
    Ok(RotationKey {
        time,
        rotation: unpack_quaternion(x, y, z),
    })
}

/// Decode one position keyframe. In the modern encoding this is a `u16` time
/// and three `u16` position components in `[-5, 5]` metres. In the legacy `0.1`
/// encoding it is an `f32` time and three `f32` metre components, clamped to the
/// same `[-5, 5]` range.
fn read_position_key(
    cursor: &mut Cursor<'_>,
    duration: f32,
    old_version: bool,
) -> Result<PositionKey, AnimDecodeError> {
    if old_version {
        let time = read_finite(cursor.read_f32("pos_time")?, "pos_time")?;
        let x = read_finite(cursor.read_f32("pos_x")?, "pos_x")?
            .clamp(-MAX_PELVIS_OFFSET, MAX_PELVIS_OFFSET);
        let y = read_finite(cursor.read_f32("pos_y")?, "pos_y")?
            .clamp(-MAX_PELVIS_OFFSET, MAX_PELVIS_OFFSET);
        let z = read_finite(cursor.read_f32("pos_z")?, "pos_z")?
            .clamp(-MAX_PELVIS_OFFSET, MAX_PELVIS_OFFSET);
        return Ok(PositionKey {
            time,
            position: [x, y, z],
        });
    }
    let time = u16_to_f32(cursor.read_u16("pos_time")?, 0.0, duration);
    let x = u16_to_f32(
        cursor.read_u16("pos_x")?,
        -MAX_PELVIS_OFFSET,
        MAX_PELVIS_OFFSET,
    );
    let y = u16_to_f32(
        cursor.read_u16("pos_y")?,
        -MAX_PELVIS_OFFSET,
        MAX_PELVIS_OFFSET,
    );
    let z = u16_to_f32(
        cursor.read_u16("pos_z")?,
        -MAX_PELVIS_OFFSET,
        MAX_PELVIS_OFFSET,
    );
    Ok(PositionKey {
        time,
        position: [x, y, z],
    })
}

/// Decode the constraint block. A count outside `0..=MAX_CONSTRAINTS` is treated
/// as corrupt and the block is skipped (no constraints), matching the reference
/// viewer, which logs a warning and continues.
fn read_constraints(
    cursor: &mut Cursor<'_>,
    num_joints: u32,
) -> Result<Vec<Constraint>, AnimDecodeError> {
    let num_constraints = cursor.read_i32("num_constraints")?;
    if !(0..=MAX_CONSTRAINTS).contains(&num_constraints) {
        return Ok(Vec::new());
    }
    let count = usize::try_from(num_constraints).unwrap_or(0);
    let mut constraints = Vec::with_capacity(count);
    for _ in 0..count {
        constraints.push(read_constraint(cursor, num_joints)?);
    }
    Ok(constraints)
}

/// Decode a single [`Constraint`].
fn read_constraint(
    cursor: &mut Cursor<'_>,
    num_joints: u32,
) -> Result<Constraint, AnimDecodeError> {
    let chain_length = cursor.read_u8("chain_length")?;
    if u32::from(chain_length) > num_joints {
        return Err(AnimDecodeError::ConstraintChainTooLong {
            chain_length,
            joints: num_joints,
        });
    }

    let constraint_type = match cursor.read_u8("constraint_type")? {
        0 => ConstraintType::Point,
        1 => ConstraintType::Plane,
        other => return Err(AnimDecodeError::InvalidConstraintType { value: other }),
    };

    let source_volume = cursor.read_fixed_name(CONSTRAINT_VOLUME_NAME_LEN, "source_volume")?;
    let source_offset = read_finite_vec3(cursor, "source_offset")?;

    let target_name = cursor.read_fixed_name(CONSTRAINT_VOLUME_NAME_LEN, "target_volume")?;
    let (target_type, target_volume) = if target_name == "GROUND" {
        (ConstraintTargetType::Ground, String::new())
    } else {
        (ConstraintTargetType::Body, target_name)
    };

    let target_offset = read_finite_vec3(cursor, "target_offset")?;
    let target_dir = read_finite_vec3(cursor, "target_dir")?;

    let ease_in_start_time = read_finite(cursor.read_f32("ease_in_start")?, "ease_in_start")?;
    let ease_in_stop_time = read_finite(cursor.read_f32("ease_in_stop")?, "ease_in_stop")?;
    let ease_out_start_time = read_finite(cursor.read_f32("ease_out_start")?, "ease_out_start")?;
    let ease_out_stop_time = read_finite(cursor.read_f32("ease_out_stop")?, "ease_out_stop")?;

    Ok(Constraint {
        chain_length,
        constraint_type,
        source_volume,
        source_offset,
        target_type,
        target_volume,
        target_offset,
        target_dir,
        ease_in_start_time,
        ease_in_stop_time,
        ease_out_start_time,
        ease_out_stop_time,
    })
}

/// Read a little-endian `Vector3` and reject it if any component is non-finite.
fn read_finite_vec3(
    cursor: &mut Cursor<'_>,
    field: &'static str,
) -> Result<[f32; 3], AnimDecodeError> {
    let x = read_finite(cursor.read_f32(field)?, field)?;
    let y = read_finite(cursor.read_f32(field)?, field)?;
    let z = read_finite(cursor.read_f32(field)?, field)?;
    Ok([x, y, z])
}

/// Widen a quantised `u16` back to an `f32` in `[lower, upper]`, reproducing the
/// reference viewer's `U16_to_F32` — including its snap of near-zero results to
/// exactly zero.
fn u16_to_f32(value: u16, lower: f32, upper: f32) -> f32 {
    /// The reciprocal of `u16::MAX`, the quantisation step (`OOU16MAX`).
    const OO_U16_MAX: f32 = 1.0 / 65535.0;
    let delta = upper - lower;
    let scaled = f32::from(value) * OO_U16_MAX * delta + lower;
    let max_error = delta * OO_U16_MAX;
    if scaled.abs() < max_error {
        0.0
    } else {
        scaled
    }
}

/// Complete a three-component imaginary quaternion `(x, y, z)` to a unit
/// quaternion `[x, y, z, w]`, matching `LLQuaternion::unpackFromVector3`.
fn unpack_quaternion(x: f32, y: f32, z: f32) -> [f32; 4] {
    let t = 1.0 - (x * x + y * y + z * z);
    let w = if t > 0.0 { t.sqrt() } else { 0.0 };
    [x, y, z, w]
}

/// Build a quaternion from the legacy `0.1` format's three Euler angles
/// `(x, y, z)` in degrees, composed in `ZYX` order (`zQ * yQ * xQ`), matching
/// the reference viewer's `mayaQ(x, y, z, StringToOrder("ZYX"))`.
fn maya_q_zyx(x_deg: f32, y_deg: f32, z_deg: f32) -> [f32; 4] {
    let x_q = axis_quaternion([1.0, 0.0, 0.0], x_deg * DEG_TO_RAD);
    let y_q = axis_quaternion([0.0, 1.0, 0.0], y_deg * DEG_TO_RAD);
    let z_q = axis_quaternion([0.0, 0.0, 1.0], z_deg * DEG_TO_RAD);
    multiply_quaternions(multiply_quaternions(z_q, y_q), x_q)
}

/// A rotation quaternion `[x, y, z, w]` of `angle` radians about the unit
/// `axis`, matching `LLQuaternion(F32 angle, const LLVector3& vec)`.
fn axis_quaternion(axis: [f32; 3], angle: f32) -> [f32; 4] {
    let half = angle * 0.5;
    let s = half.sin();
    let c = half.cos();
    [axis[0] * s, axis[1] * s, axis[2] * s, c]
}

/// Multiply two quaternions `[x, y, z, w]` using the reference viewer's
/// left-to-right convention (`operator*(const LLQuaternion&, const
/// LLQuaternion&)`), so a chain `a * b` composes exactly as the C++ does.
fn multiply_quaternions(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        bw * ax + bx * aw + by * az - bz * ay,
        bw * ay + by * aw + bz * ax - bx * az,
        bw * az + bz * aw + bx * ay - by * ax,
        bw * aw - bx * ax - by * ay - bz * az,
    ]
}

/// A forward-only cursor over the `.anim` bytes, reading little-endian
/// primitives without slice indexing or the `from_le_bytes` family (both
/// forbidden by the workspace lints), mirroring `sl_avatar`'s base-mesh cursor.
struct Cursor<'a> {
    /// The full animation byte slice.
    data: &'a [u8],
    /// The current read offset into [`data`](Self::data).
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// Create a cursor at the start of `data`.
    const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Take the next `len` bytes, advancing the cursor, or an EOF error tagged
    /// with `field`.
    fn take(&mut self, len: usize, field: &'static str) -> Result<&'a [u8], AnimDecodeError> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or(AnimDecodeError::UnexpectedEof { field })?;
        let slice = self
            .data
            .get(self.pos..end)
            .ok_or(AnimDecodeError::UnexpectedEof { field })?;
        self.pos = end;
        Ok(slice)
    }

    /// Read a single byte.
    fn read_u8(&mut self, field: &'static str) -> Result<u8, AnimDecodeError> {
        self.take(1, field)?
            .first()
            .copied()
            .ok_or(AnimDecodeError::UnexpectedEof { field })
    }

    /// Read a little-endian `u16`.
    fn read_u16(&mut self, field: &'static str) -> Result<u16, AnimDecodeError> {
        let bytes = self.take(2, field)?;
        Ok(bytes.iter().enumerate().fold(0_u16, |acc, (index, &byte)| {
            let shift = u32::try_from(index).unwrap_or(0).saturating_mul(8);
            acc | (u16::from(byte).checked_shl(shift).unwrap_or(0))
        }))
    }

    /// Read a little-endian `u32`.
    fn read_u32(&mut self, field: &'static str) -> Result<u32, AnimDecodeError> {
        let bytes = self.take(4, field)?;
        Ok(bytes.iter().enumerate().fold(0_u32, |acc, (index, &byte)| {
            let shift = u32::try_from(index).unwrap_or(0).saturating_mul(8);
            acc | (u32::from(byte).checked_shl(shift).unwrap_or(0))
        }))
    }

    /// Read a little-endian signed `i32` (bit-reinterpreting the `u32`, so the
    /// two's-complement value survives without an `as` cast or the
    /// `from_le_bytes` family).
    fn read_i32(&mut self, field: &'static str) -> Result<i32, AnimDecodeError> {
        Ok(self.read_u32(field)?.cast_signed())
    }

    /// Read a little-endian IEEE-754 `f32` (via its bit pattern, avoiding the
    /// lint-forbidden `from_le_bytes`).
    fn read_f32(&mut self, field: &'static str) -> Result<f32, AnimDecodeError> {
        Ok(f32::from_bits(self.read_u32(field)?))
    }

    /// Read a NUL-terminated ASCII string, consuming the terminator.
    fn read_cstring(&mut self, field: &'static str) -> Result<String, AnimDecodeError> {
        let rest = self
            .data
            .get(self.pos..)
            .ok_or(AnimDecodeError::UnexpectedEof { field })?;
        let nul = rest
            .iter()
            .position(|&byte| byte == 0)
            .ok_or(AnimDecodeError::UnexpectedEof { field })?;
        let text = rest.get(..nul).unwrap_or_default();
        let value = String::from_utf8_lossy(text).into_owned();
        self.pos = self
            .pos
            .checked_add(nul)
            .and_then(|next| next.checked_add(1))
            .ok_or(AnimDecodeError::UnexpectedEof { field })?;
        Ok(value)
    }

    /// Read a fixed-length, NUL-padded ASCII name field, returning the string up
    /// to the first NUL (or the whole field if there is none).
    fn read_fixed_name(
        &mut self,
        len: usize,
        field: &'static str,
    ) -> Result<String, AnimDecodeError> {
        let bytes = self.take(len, field)?;
        let end = bytes.iter().position(|&byte| byte == 0).unwrap_or(len);
        let text = bytes.get(..end).unwrap_or(bytes);
        Ok(String::from_utf8_lossy(text).into_owned())
    }
}

/// An error returned while decoding a `.anim` keyframe-motion file.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum AnimDecodeError {
    /// The stream ended before a required field could be read in full.
    #[error("unexpected end of animation data while reading {field}")]
    UnexpectedEof {
        /// The field the decoder was reading when the data ran out.
        field: &'static str,
    },
    /// The header declared a version this decoder does not support (only the
    /// current `1.0` keyframe-motion version and the legacy `0.1` Euler-angle
    /// form are decoded).
    #[error("unsupported animation version {version}.{sub_version}")]
    UnsupportedVersion {
        /// The declared major version.
        version: u16,
        /// The declared sub version.
        sub_version: u16,
    },
    /// The base priority was below the lowest legal value
    /// ([`JointPriority::USE_MOTION`]).
    #[error("bad animation base priority {priority}")]
    BadBasePriority {
        /// The declared base priority.
        priority: i32,
    },
    /// The duration was non-finite or exceeded `MAX_ANIM_DURATION`.
    #[error("invalid animation duration {duration}")]
    InvalidDuration {
        /// The declared duration.
        duration: f32,
    },
    /// A field that must be finite was `NaN` or infinite.
    #[error("non-finite value while reading {field}")]
    NonFinite {
        /// The field that held the non-finite value.
        field: &'static str,
    },
    /// The header declared zero joint motions.
    #[error("animation has no joints")]
    NoJoints,
    /// The header declared more joints than `MAX_ANIMATED_JOINTS`.
    #[error("too many animated joints: {count}")]
    TooManyJoints {
        /// The declared joint count.
        count: u32,
    },
    /// A joint priority was below the lowest legal value
    /// ([`JointPriority::USE_MOTION`]).
    #[error("bad joint priority {priority}")]
    BadJointPriority {
        /// The declared joint priority.
        priority: i32,
    },
    /// A keyframe-count field was negative.
    #[error("negative key count while reading {field}")]
    NegativeKeyCount {
        /// The field that held the negative count.
        field: &'static str,
    },
    /// The hand pose exceeded the largest accepted index
    /// (`NUM_HAND_POSES`).
    #[error("invalid hand pose {value}")]
    InvalidHandPose {
        /// The declared hand-pose index.
        value: u32,
    },
    /// A rotation keyframe's time fell outside `0..=duration`.
    #[error("keyframe time {time} out of range for duration {duration}")]
    KeyTimeOutOfRange {
        /// The decoded keyframe time.
        time: f32,
        /// The motion duration it exceeded.
        duration: f32,
    },
    /// A constraint declared an unknown type (not
    /// [`Point`](ConstraintType::Point) or [`Plane`](ConstraintType::Plane)).
    #[error("invalid constraint type {value}")]
    InvalidConstraintType {
        /// The declared constraint-type discriminant.
        value: u8,
    },
    /// A constraint's IK chain was longer than the number of animated joints.
    #[error("constraint chain length {chain_length} exceeds joint count {joints}")]
    ConstraintChainTooLong {
        /// The declared chain length.
        chain_length: u8,
        /// The number of animated joints in the motion.
        joints: u32,
    },
}
