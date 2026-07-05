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
//! - `morph` (P13.3) — blend the base-mesh morph-target deltas by the resolved
//!   visual-param weights so the body takes its real shape.
//! - `resolve` (P13.4) — driver → driven propagation and avatar-sex resolution,
//!   turning a partial appearance vector into every param's effective weight.
//! - `skeletal` (P13.4) — resolve `param_skeleton` params into per-bone scale /
//!   position deformations for the skeleton instance.
//!
//! P12.2 lands the [`skeleton`] module, P12.3 the [`basemesh`] module, P12.4 the
//! [`params`] module, P13.3 the [`morph`] module, and P13.4 the [`resolve`] and
//! [`skeletal`] modules.

pub mod basemesh;
pub mod morph;
pub mod params;
pub mod resolve;
pub mod skeletal;
pub mod skeleton;

pub use basemesh::{
    BaseMesh, BaseMeshError, LodMesh, MeshTransform, MorphDelta, MorphTarget, SharedVertex,
    VertexSkinWeight,
};
pub use morph::{MorphWeights, MorphedMesh};
pub use params::{
    AppearanceValues, BoneOffset, DrivenParam, ParamEffect, ParamError, ParamGroup, ParamSex,
    ParamValue, VisualParam, VisualParams,
};
pub use resolve::ResolvedParams;
pub use skeletal::{BoneDeform, SkeletalDeformations};
pub use skeleton::{
    AttachmentPointDef, AttachmentPoints, CollisionVolume, Joint, Skeleton, SkeletonError,
};

pub use sl_proto::AttachmentPoint;
