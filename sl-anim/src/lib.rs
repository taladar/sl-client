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

pub mod decode;

pub use decode::{
    AnimDecodeError, Constraint, ConstraintTargetType, ConstraintType, HandPose, JointMotion,
    JointPriority, Motion, PositionKey, RotationKey,
};
