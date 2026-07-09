//! Linden tree & grass data and geometry for Second Life / OpenSim clients —
//! the `LLVOTree` / `LLVOGrass` counterpart of `sl-prim`, `sl-mesh` and
//! `sl-sculpt`.
//!
//! See the crate `README.md` for an overview. Trees and grass are their own
//! object classes (`PCODE_TREE` / `PCODE_NEW_TREE` / `PCODE_GRASS`) whose
//! visible form is selected by a one-byte *species* index carried in the
//! object's `state` field. The species indexes Linden's
//! `app_settings/trees.xml` table, which supplies the diffuse texture and the
//! parameters of the procedurally generated geometry.
//!
//! Like its sibling geometry crates this crate is deliberately **Bevy-free and
//! I/O-free**: it never fetches or decodes; it holds the species data and the
//! procedural geometry generation while the Bevy conversion stays in
//! `sl-client-bevy`.
//!
//! Currently implemented:
//!
//! - [`species`] — the `LLVOTree` species table ([`TreeSpecies`] /
//!   [`TREE_SPECIES`]) ported from `trees.xml`, with a [`tree_species`] lookup
//!   by species byte.
//! - [`geometry`] — procedural `LLVOTree` branch / leaf geometry
//!   ([`tree_geometry`] / [`billboard_geometry`]) generated from a
//!   [`TreeSpecies`], plus the [`TreeLod`] trunk levels of detail.

pub mod geometry;
mod noise;
pub mod species;

pub use geometry::{
    RADIUS_SCALE_FACTOR, TREE_LOD_LEVELS, TreeLod, TreeMesh, YAW_DEGREES, billboard_geometry,
    tree_geometry,
};
pub use species::{MAX_TREE_SPECIES, TREE_SPECIES, TreeSpecies, tree_species};
