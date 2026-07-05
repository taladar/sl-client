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

use std::path::Path;

use bevy::prelude::Resource;
use sl_client_bevy::{
    BaseMesh, BaseMeshError, BaseMeshSkin, BevySkeleton, ParamError, Skeleton, SkeletonError,
    VisualParams,
};
use tracing::warn;

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

/// One base part to load: a display label, its `lod = 0` `.llm` file name, and
/// how it binds to the skeleton.
#[derive(Debug, Clone, Copy)]
struct BasePartSpec {
    /// A short human-readable label, used only for log messages.
    label: &'static str,
    /// The base-part file name inside the `character/` directory.
    file: &'static str,
    /// How the part attaches to the skeleton.
    binding: PartBinding,
}

/// The standard base-body parts and their `lod = 0` files, as referenced by
/// `avatar_lad.xml`'s `<mesh>` table. The two eyeballs share one file but pin to
/// distinct eye joints; every other part is skinned to its own joint table.
const BASE_PARTS: &[BasePartSpec] = &[
    BasePartSpec {
        label: "head",
        file: "avatar_head.llm",
        binding: PartBinding::Skinned,
    },
    BasePartSpec {
        label: "hair",
        file: "avatar_hair.llm",
        binding: PartBinding::Skinned,
    },
    BasePartSpec {
        label: "eyelashes",
        file: "avatar_eyelashes.llm",
        binding: PartBinding::Skinned,
    },
    BasePartSpec {
        label: "upper body",
        file: "avatar_upper_body.llm",
        binding: PartBinding::Skinned,
    },
    BasePartSpec {
        label: "lower body",
        file: "avatar_lower_body.llm",
        binding: PartBinding::Skinned,
    },
    BasePartSpec {
        label: "skirt",
        file: "avatar_skirt.llm",
        binding: PartBinding::Skinned,
    },
    BasePartSpec {
        label: "left eye",
        file: "avatar_eye.llm",
        binding: PartBinding::Rigid("mEyeLeft"),
    },
    BasePartSpec {
        label: "right eye",
        file: "avatar_eye.llm",
        binding: PartBinding::Rigid("mEyeRight"),
    },
];

/// The canonical skeleton joint whose rest height the avatar object position is
/// taken to sit at (Second Life reports an avatar's position at roughly the
/// pelvis), used to plant the body vertically. See
/// [`AvatarAssetLibrary::pelvis_height`].
const PELVIS_JOINT: &str = "mPelvis";

/// A resolved base part: its decoded mesh and how it binds to the skeleton.
#[derive(Debug)]
pub(crate) struct LoadedPart {
    /// The decoded `lod = 0` base mesh (Second Life Z-up space).
    pub(crate) mesh: BaseMesh,
    /// How this part attaches to the skeleton.
    pub(crate) binding: LoadedBinding,
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
        let skeleton = BevySkeleton::from_skeleton(&skeleton);
        let params = VisualParams::from_xml(&fs_err::read_to_string(dir.join("avatar_lad.xml"))?)?;

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
            parts.push(LoadedPart { mesh, binding });
        }

        let library = Self {
            skeleton,
            parts,
            params,
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
            "loaded avatar assets: {} joints, {} base parts, {} visual params",
            self.skeleton.len(),
            self.parts.len(),
            self.params.all().len(),
        );
    }
}
