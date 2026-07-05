//! Pure Second Life / OpenSim avatar decoding for the visual viewer — the
//! system-avatar counterpart of `sl-mesh` (LLMesh) and `sl-texture` (J2C).
//!
//! See the crate `README.md` for an overview. This crate parses the standard
//! Linden `character/` assets and drives the avatar shape / skinning math,
//! staying deliberately Bevy-free and I/O-free (it parses from `&[u8]` / `&str`
//! and produces geometry in Second Life's right-handed **Z-up** space); the
//! Bevy skeleton-instance / `SkinnedMesh` conversion lives in `sl-client-bevy`.
//!
//! The pieces are added over the course of Phase 12 of the viewer road map:
//!
//! - `skeleton` (P12.2) — parse `avatar_skeleton.xml` into a joint hierarchy
//!   with rest transforms and collision volumes, plus the attachment-point /
//!   HUD-point maps from `avatar_lad.xml`.
//! - `basemesh` (P12.3) — decode the legacy base-body `.llm` meshes into
//!   per-part positions / normals / UVs / skin weights and morph-target deltas.
//! - `params` (P12.4) — parse the `avatar_lad.xml` visual-param table and map an
//!   `AvatarAppearance.visual_params` byte vector onto typed param values.
//!
//! P12.2 lands the [`skeleton`] module; the `basemesh` / `params` modules follow
//! in the subsequent points.

pub mod skeleton;

pub use skeleton::{
    AttachmentPointDef, AttachmentPoints, CollisionVolume, Joint, Skeleton, SkeletonError,
};

pub use sl_proto::AttachmentPoint;
