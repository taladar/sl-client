//! Load the standard Linden `character/` assets once into a resource (P13.2).
//!
//! The system-avatar base body is driven from client-side viewer files, not
//! fetched from the grid: the skeleton (`avatar_skeleton.xml`), the visual-param
//! table (`avatar_lad.xml`), and the base-body `.llm` meshes all live in a
//! viewer's `character/` directory. The `--viewer-assets <dir>` flag points the
//! viewer at that directory (e.g. an installed Firestorm / Second Life viewer);
//! [`AvatarAssetLibrary::load`] parses them here, keeping the pure `sl-avatar`
//! crate I/O-free.
//!
//! Only the finest (`lod = 0`) base part of each mesh is loaded — the un-morphed
//! rest body of Phase 13.2, before visual-param morphs (P13.3) and skeletal
//! scale (P13.4). Each part is bound to the skeleton in one of two ways: a
//! *skinned* part (head, hair, body, …) resolves its own joint-name table
//! against the skeleton into a [`BaseMeshSkin`]; a *rigid* part (the eyeballs,
//! which carry no skin weights) is pinned to a single named joint and simply
//! follows it. A part whose binding cannot be resolved is skipped with a warning
//! rather than failing the whole load.

use std::collections::HashMap;
use std::path::Path;

use bevy::prelude::Resource;
use sl_client_bevy::{
    AttachmentPoints, BaseMesh, BaseMeshError, BaseMeshSkin, BevySkeleton, MorphMasks, ParamError,
    Skeleton, SkeletonError, VisualParams, avatar_texture,
};
use tracing::warn;

/// Which baked-texture region a base part belongs to, driving the P13.5
/// conditional-visibility rules: a region is hidden when a worn attachment
/// replaces it (an `IMG_USE_BAKED_*` face), and the skirt region renders only
/// when the avatar's `TEX_SKIRT_BAKED` slot holds a visible bake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyRegion {
    /// The head (and eyelashes, which the reference viewer hides with the head).
    Head,
    /// The hair.
    Hair,
    /// The eyes (both eyeballs).
    Eyes,
    /// The upper body.
    Upper,
    /// The lower body.
    Lower,
    /// The skirt.
    Skirt,
}

impl BodyRegion {
    /// The avatar baked-texture slot this region's visibility keys off — the
    /// eyelashes ride with the head and the eyeballs with the eyes, matching the
    /// reference viewer's `updateMeshVisibility`.
    pub(crate) const fn baked_slot(self) -> usize {
        match self {
            Self::Head => avatar_texture::HEAD_BAKED,
            Self::Hair => avatar_texture::HAIR_BAKED,
            Self::Eyes => avatar_texture::EYES_BAKED,
            Self::Upper => avatar_texture::UPPER_BAKED,
            Self::Lower => avatar_texture::LOWER_BAKED,
            Self::Skirt => avatar_texture::SKIRT_BAKED,
        }
    }

    /// The `avatar_lad.xml` `<morph_masks>` `body_region` name this region matches,
    /// for the clothing-morph alpha masks (P14.5), or `None` for a region with no
    /// masked morphs. Only the head, upper body and lower body carry clothing
    /// morphs; the eyelashes ride with the head region but define no masked morphs
    /// of their own.
    pub(crate) const fn morph_mask_region(self) -> Option<&'static str> {
        match self {
            Self::Head => Some("head"),
            Self::Upper => Some("upper_body"),
            Self::Lower => Some("lower_body"),
            Self::Hair | Self::Eyes | Self::Skirt => None,
        }
    }
}

/// How a base part is attached to the avatar skeleton.
#[derive(Debug, Clone, Copy)]
enum PartBinding {
    /// The part carries per-vertex skin weights over its own joint-name table
    /// (resolved against the skeleton into a [`BaseMeshSkin`]).
    Skinned,
    /// The part carries no skin weights and is pinned rigidly to the named
    /// joint, following it (the eyeballs, one per eye joint).
    Rigid(&'static str),
}

/// One base part to load: a display label, its `lod = 0` `.llm` file name, how
/// it binds to the skeleton, and which baked region it belongs to.
#[derive(Debug, Clone, Copy)]
struct BasePartSpec {
    /// A short human-readable label, used only for log messages.
    label: &'static str,
    /// The base-part file name inside the `character/` directory.
    file: &'static str,
    /// How the part attaches to the skeleton.
    binding: PartBinding,
    /// Which baked region this part belongs to (for P13.5 visibility).
    region: BodyRegion,
}

/// The standard base-body parts and their `lod = 0` files, as referenced by
/// `avatar_lad.xml`'s `<mesh>` table. The two eyeballs share one file but pin to
/// distinct eye joints; every other part is skinned to its own joint table.
const BASE_PARTS: &[BasePartSpec] = &[
    BasePartSpec {
        label: "head",
        file: "avatar_head.llm",
        binding: PartBinding::Skinned,
        region: BodyRegion::Head,
    },
    BasePartSpec {
        label: "hair",
        file: "avatar_hair.llm",
        binding: PartBinding::Skinned,
        region: BodyRegion::Hair,
    },
    BasePartSpec {
        label: "eyelashes",
        file: "avatar_eyelashes.llm",
        binding: PartBinding::Skinned,
        region: BodyRegion::Head,
    },
    BasePartSpec {
        label: "upper body",
        file: "avatar_upper_body.llm",
        binding: PartBinding::Skinned,
        region: BodyRegion::Upper,
    },
    BasePartSpec {
        label: "lower body",
        file: "avatar_lower_body.llm",
        binding: PartBinding::Skinned,
        region: BodyRegion::Lower,
    },
    BasePartSpec {
        label: "skirt",
        file: "avatar_skirt.llm",
        binding: PartBinding::Skinned,
        region: BodyRegion::Skirt,
    },
    BasePartSpec {
        label: "left eye",
        file: "avatar_eye.llm",
        binding: PartBinding::Rigid("mEyeLeft"),
        region: BodyRegion::Eyes,
    },
    BasePartSpec {
        label: "right eye",
        file: "avatar_eye.llm",
        binding: PartBinding::Rigid("mEyeRight"),
        region: BodyRegion::Eyes,
    },
];

/// The canonical skeleton joint whose rest height the avatar object position is
/// taken to sit at (Second Life reports an avatar's position at roughly the
/// pelvis), used to plant the body vertically. See
/// [`AvatarAssetLibrary::pelvis_height`].
const PELVIS_JOINT: &str = "mPelvis";

/// A resolved base part: its decoded mesh, how it binds to the skeleton, and
/// which baked region it belongs to.
#[derive(Debug)]
pub(crate) struct LoadedPart {
    /// The decoded `lod = 0` base mesh (Second Life Z-up space).
    pub(crate) mesh: BaseMesh,
    /// How this part attaches to the skeleton.
    pub(crate) binding: LoadedBinding,
    /// Which baked region this part belongs to (for P13.5 visibility).
    pub(crate) region: BodyRegion,
}

/// One attachment point resolved against the loaded skeleton (P16.2): the joint
/// it hangs from and its fixed local offset from that joint (the `avatar_lad.xml`
/// `position` / `rotation` attributes, kept in Second Life Z-up space). The
/// viewer builds a per-avatar attachment-point node at this offset so a worn
/// rigid attachment seats where the reference viewer places it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AttachmentPointInfo {
    /// The index (in this library's skeleton) of the joint this point hangs from.
    pub(crate) joint_index: usize,
    /// The point's local translation from the joint, in metres (Second Life Z-up).
    pub(crate) position: [f32; 3],
    /// The point's local rotation from the joint, as Second Life Euler XYZ angles
    /// in degrees.
    pub(crate) rotation_euler_deg: [f32; 3],
}

/// A base part's resolved skeleton binding.
#[derive(Debug)]
pub(crate) enum LoadedBinding {
    /// A skinned part: its own joint-name table resolved against the skeleton.
    Skinned(BaseMeshSkin),
    /// A rigid part pinned to the skeleton joint at this index.
    Rigid(usize),
}

/// An error loading the `character/` assets from disk.
#[derive(thiserror::Error, Debug)]
pub(crate) enum AvatarAssetError {
    /// A `character/` file could not be read (the `fs_err` message already
    /// carries the offending path).
    #[error("reading avatar asset: {0}")]
    Read(#[from] std::io::Error),
    /// The skeleton XML could not be parsed.
    #[error("parsing avatar_skeleton.xml: {0}")]
    Skeleton(#[from] SkeletonError),
    /// The visual-param table could not be parsed.
    #[error("parsing avatar_lad.xml: {0}")]
    Params(#[from] ParamError),
    /// A base-part `.llm` mesh could not be decoded.
    #[error("decoding base mesh: {0}")]
    Mesh(#[from] BaseMeshError),
}

/// The parsed system-avatar assets: the skeleton, the resolved base parts, and
/// the visual-param table.
///
/// Loaded once from the `--viewer-assets` directory and inserted as a Bevy
/// resource; the viewer builds the shared render assets (Bevy meshes, inverse
/// bindposes) from it and spawns a fresh skeleton instance per avatar. The
/// visual-param table is loaded here so the morph phases (P13.3 / P13.4) reuse
/// it without re-reading the files.
#[derive(Resource, Debug)]
pub(crate) struct AvatarAssetLibrary {
    /// The avatar skeleton, converted to the Bevy joint-instance data.
    skeleton: BevySkeleton,
    /// The resolved base parts (those whose binding resolved).
    parts: Vec<LoadedPart>,
    /// The visual-param table (used by later morph phases).
    params: VisualParams,
    /// The `<morph_masks>` table driving the clothing-morph alpha masks (P14.5).
    masks: MorphMasks,
    /// The `<attachment_point>` table, mapping each attachment point to the
    /// skeleton joint an attached object hangs from (P16.1).
    attachment_points: AttachmentPoints,
}

impl AvatarAssetLibrary {
    /// Load and parse the standard `character/` assets from `dir`.
    ///
    /// # Errors
    ///
    /// Returns an [`AvatarAssetError`] if the skeleton, visual-param table, or a
    /// base-part mesh cannot be read or parsed. A base part whose skeleton
    /// binding does not resolve is skipped (logged), not an error.
    pub(crate) fn load(dir: &Path) -> Result<Self, AvatarAssetError> {
        let skeleton =
            Skeleton::from_xml(&fs_err::read_to_string(dir.join("avatar_skeleton.xml"))?)?;
        let mut skeleton = BevySkeleton::from_skeleton(&skeleton);
        // The reference viewer synthesizes an `mRoot` joint above `mPelvis` (it is
        // not in `avatar_skeleton.xml`); add it so the avatar-centre attachment
        // point (`joint="mRoot"`) resolves to a real joint entity (P16.1).
        skeleton.insert_synthetic_root("mRoot");
        // Parse the visual-param table and the morph-mask table from the one
        // `avatar_lad.xml` read.
        let lad = fs_err::read_to_string(dir.join("avatar_lad.xml"))?;
        let params = VisualParams::from_xml(&lad)?;
        let masks = MorphMasks::from_xml(&lad)?;
        let attachment_points = AttachmentPoints::from_xml(&lad)?;

        let mut parts = Vec::with_capacity(BASE_PARTS.len());
        for spec in BASE_PARTS {
            let mesh = BaseMesh::from_bytes(&fs_err::read(dir.join(spec.file))?)?;
            let binding = match spec.binding {
                PartBinding::Skinned => match skeleton.base_mesh_skin(&mesh) {
                    Some(skin) => LoadedBinding::Skinned(skin),
                    None => {
                        warn!("skipping avatar part `{}`: a joint is absent", spec.label);
                        continue;
                    }
                },
                PartBinding::Rigid(joint) => match skeleton.find(joint) {
                    Some(index) => LoadedBinding::Rigid(index),
                    None => {
                        warn!(
                            "skipping avatar part `{}`: joint `{joint}` absent",
                            spec.label
                        );
                        continue;
                    }
                },
            };
            parts.push(LoadedPart {
                mesh,
                binding,
                region: spec.region,
            });
        }

        let library = Self {
            skeleton,
            parts,
            params,
            masks,
            attachment_points,
        };
        library.log_summary();
        Ok(library)
    }

    /// The Bevy skeleton (joint rest transforms, parents, bind poses).
    pub(crate) const fn skeleton(&self) -> &BevySkeleton {
        &self.skeleton
    }

    /// The resolved base parts.
    pub(crate) fn parts(&self) -> &[LoadedPart] {
        &self.parts
    }

    /// The visual-param table, used to resolve an `AvatarAppearance.visual_params`
    /// vector into morph-target weights (P13.3).
    pub(crate) const fn params(&self) -> &VisualParams {
        &self.params
    }

    /// The `<morph_masks>` table, used to mask the clothing morphs per vertex from
    /// each region's decoded baked texture (P14.5).
    pub(crate) const fn masks(&self) -> &MorphMasks {
        &self.masks
    }

    /// The attachment-point table resolved against this library's skeleton
    /// (P16.1/P16.2): for each `<attachment_point>` in `avatar_lad.xml`, its raw
    /// numeric id paired with the index of the joint it hangs from and its fixed
    /// local offset from that joint (the `position` / `rotation` attributes). A
    /// point whose joint is absent from the skeleton (e.g. a HUD point's `mScreen`
    /// pseudo-joint) is omitted, so a body attachment always resolves to a real
    /// skeleton joint and a HUD point simply does not.
    pub(crate) fn attachment_points(&self) -> HashMap<u8, AttachmentPointInfo> {
        self.attachment_points
            .all()
            .iter()
            .filter_map(|def| {
                self.skeleton.find(&def.joint).map(|joint_index| {
                    (
                        def.id,
                        AttachmentPointInfo {
                            joint_index,
                            position: def.position,
                            rotation_euler_deg: def.rotation,
                        },
                    )
                })
            })
            .collect()
    }

    /// The rest height (Second Life Z, metres) of the pelvis joint — the offset
    /// used to plant the body so its pelvis sits at the reported avatar object
    /// position. Falls back to `0.0` if the joint is somehow absent.
    pub(crate) fn pelvis_height(&self) -> f32 {
        self.skeleton
            .find(PELVIS_JOINT)
            .and_then(|index| self.skeleton.local_transforms().get(index))
            .map_or(0.0, |transform| transform.translation.z)
    }

    /// Log a one-line summary of what was loaded.
    fn log_summary(&self) {
        tracing::info!(
            "loaded avatar assets: {} joints, {} base parts, {} visual params, {} morph masks, {} attachment points",
            self.skeleton.len(),
            self.parts.len(),
            self.params.all().len(),
            self.masks.len(),
            self.attachment_points.all().len(),
        );
    }
}
