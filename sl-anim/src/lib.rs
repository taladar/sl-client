//! Pure Second Life / OpenSim **animation** decoding: the Linden keyframe-motion
//! binary format (`.anim`) a viewer plays to pose an avatar's skeleton.
//!
//! See the crate `README.md` for an overview. Like its siblings `sl-mesh`
//! (LLMesh), `sl-texture` (J2C), and `sl-avatar` (skeleton / base body) the
//! crate is deliberately **Bevy-free and I/O-free**: it decodes a borrowed
//! `&[u8]` into an owned [`Motion`] in Second Life's right-handed **Z-up** metre
//! space and never opens a file or fetches from the grid. Resolving an
//! animation UUID to its bytes and driving a skeleton from the decoded tracks
//! live in the runtime / `sl-client-bevy` layers.
//!
//! The pieces are:
//!
//! - [`decode`] — the keyframe-motion binary decoder and its owned model.
//! - [`registry`] — the fixed-UUID built-in agent-animation registry (which
//!   UUIDs are the standard walks/stands/emotes, and whether each is a
//!   downloadable `.anim` asset or a procedurally synthesised motion). Named for
//!   its role, to avoid the `module_name_repetitions` lint (as [`decode`] is).
//! - `sample` — playback-time mapping and per-joint keyframe interpolation, plus
//!   the ease-in/out [`pose_weight`](Motion::pose_weight), the inherent
//!   [`Motion`] / [`JointMotion`] methods a skeleton driver samples each frame
//!   (P18.3 / P18.4). Its API is those methods, so nothing is re-exported.
//! - [`blend`] — resolving several motions' contributions to one joint by
//!   priority (P18.4), the pure counterpart of the reference `LLJointStateBlender`.

pub mod blend;
pub mod decode;
pub mod registry;
mod sample;

pub use blend::{BlendedJoint, JointContribution, MAX_JOINT_CONTRIBUTIONS, blend_joint};
pub use decode::{
    AnimDecodeError, Constraint, ConstraintTargetType, ConstraintType, HandPose, JointMotion,
    JointPriority, Motion, PositionKey, RotationKey,
};
pub use registry::{BUILTIN_ANIMATIONS, BuiltinAnimation, BuiltinKind, builtin_animation};
