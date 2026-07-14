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
//! - `masks` (P14.5) — parse the `<morph_masks>` table and sample per-vertex
//!   clothing-morph mask weights from a region's decoded baked texture.
//! - `skin` (P17.1) — the matrix-palette skinning math that deforms a rigged
//!   `sl-mesh` body/clothing with an avatar's posed skeleton instance.
//! - `physics` (P34.1) — ingest the `WT_PHYSICS` wearable: the breast / belly /
//!   butt spring-damper settings and the morph params each motion drives.
//!
//! P12.2 lands the [`skeleton`] module, P12.3 the [`basemesh`] module, P12.4 the
//! [`params`] module, P13.3 the [`morph`] module, P13.4 the [`resolve`] and
//! [`skeletal`] modules, and P17.1 the [`skin`] module.

pub mod bakecolor;
pub mod basemesh;
pub mod masks;
pub mod morph;
pub mod params;
pub mod physics;
pub mod resolve;
pub mod skeletal;
pub mod skeleton;
pub mod skin;
pub mod wearable;

pub use bakecolor::{combine_layer_color, global_color, global_color_params};
pub use basemesh::{
    BaseMesh, BaseMeshError, LodMesh, MeshTransform, MorphDelta, MorphTarget, SharedVertex,
    VertexSkinWeight,
};
pub use masks::{MaskTexture, MorphMask, MorphMasks, PartMorphMask};
pub use morph::{
    HAND_POSE_MORPH_PARAMS, MorphWeights, MorphedMesh, NORMAL_SOFTEN_FACTOR, PHYSICS_MORPH_PARAMS,
    RUNTIME_MORPH_PARAMS, hand_pose_morph_param, is_runtime_morph_param,
};
pub use params::{
    AppearanceValues, BoneOffset, DrivenParam, ParamEffect, ParamError, ParamGroup, ParamSex,
    ParamValue, VisualParam, VisualParams, VolumeMorph,
};
pub use physics::{
    BodyPhysics, PhysicsDrivenParam, PhysicsMotion, PhysicsMotionConfig, PhysicsSettings,
};
pub use resolve::ResolvedParams;
pub use skeletal::{BoneDeform, SkeletalDeformations};
pub use skeleton::{
    AttachmentPointDef, AttachmentPoints, CollisionVolume, Joint, JointSupport, Skeleton,
    SkeletonError,
};
pub use skin::SkinningPalette;
pub use wearable::{WearableAsset, WearableError};

pub use params::{ColorOp, ColorRamp};

pub use sl_proto::AttachmentPoint;
