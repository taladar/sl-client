//! The `LLVOTree` species table, ported from Linden's
//! `app_settings/trees.xml`.
//!
//! A tree object (`PCODE_TREE` / `PCODE_NEW_TREE`) carries a one-byte species
//! selector in its `state` field. That byte indexes this fixed table, which
//! gives each species its diffuse texture and the parameters of the branch /
//! leaf geometry the viewer generates procedurally
//! (Firestorm's `LLVOTree::TreeSpeciesData`).
//!
//! The values here are ported verbatim from `trees.xml`. As in Firestorm the
//! `depth` and `trunk_depth` attributes are parsed as integers (the XML writes
//! a few of them with a fractional part, e.g. `trunk_depth="0.1"`, which
//! truncates towards zero).

use sl_types::key::{Key, TextureKey};
use uuid::uuid;

/// One `LLVOTree` species: the diffuse texture plus the parameters of the
/// procedurally generated branch / leaf geometry.
///
/// Ported from `app_settings/trees.xml` / Firestorm's
/// `LLVOTree::TreeSpeciesData`. Field names follow the XML attributes; the
/// corresponding `TreeSpeciesData` member is noted where it differs.
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `TreeSpecies` reads clearly"
)]
pub struct TreeSpecies {
    /// The species index (`species_id`), matching this entry's position in
    /// [`TREE_SPECIES`].
    pub species_id: u8,
    /// Human-readable species name (`name`), e.g. `"Pine 1"`.
    pub name: &'static str,
    /// Diffuse texture applied to the trunk and leaf cards (`texture_id` /
    /// `mTextureID`).
    pub texture_id: TextureKey,
    /// Droop of the branches from vertical, in degrees (`droop` / `mDroop`).
    pub droop: f32,
    /// Twist applied at each recursion, in degrees (`twist` / `mTwist`).
    pub twist: f32,
    /// Number of branches spawned at each recursion (`branches` /
    /// `mBranches`).
    pub branches: f32,
    /// Number of recursions from trunk to branch tips (`depth` / `mDepth`).
    pub depth: u8,
    /// Scale multiplier applied at each recursion (`scale_step` /
    /// `mScaleStep`).
    pub scale_step: f32,
    /// Trunk recursion depth (`trunk_depth` / `mTrunkDepth`).
    pub trunk_depth: u8,
    /// Length scale of a branch (`branch_length` / `mBranchLength`).
    pub branch_length: f32,
    /// Length scale of the trunk (`trunk_length` / `mTrunkLength`).
    pub trunk_length: f32,
    /// Scale of the leaf texture card when rendering (`leaf_scale` /
    /// `mLeafScale`).
    pub leaf_scale: f32,
    /// Scale of the distance-billboard imposter (`billboard_scale` /
    /// `mBillboardScale`).
    pub billboard_scale: f32,
    /// Height-to-width aspect ratio of the billboard (`billboard_ratio` /
    /// `mBillboardRatio`).
    pub billboard_ratio: f32,
    /// Width-to-length ratio of the trunk (`trunk_aspect` / `mTrunkAspect`).
    pub trunk_aspect: f32,
    /// Width-to-length ratio of a branch (`branch_aspect` / `mBranchAspect`).
    pub branch_aspect: f32,
    /// Random leaf rotation, in degrees (`leaf_rotate` / `mRandomLeafRotate`).
    pub leaf_rotate: f32,
    /// Amount of Perlin-noise deformation (`noise_mag` / `mNoiseMag`).
    pub noise_mag: f32,
    /// Scaling of the noise function in Perlin space (`noise_scale` /
    /// `mNoiseScale`).
    pub noise_scale: f32,
    /// Taper amount from base to tip (`taper` / `mTaper`).
    pub taper: f32,
    /// Vertical repeats of the trunk texture (`repeat_z` / `mRepeatTrunkZ`).
    pub repeat_trunk_z: f32,
}

/// Build a [`TextureKey`] from a compile-time UUID literal.
const fn tex(id: uuid::Uuid) -> TextureKey {
    TextureKey(Key(id))
}

/// Number of defined tree species (`species_id` `0..MAX_TREE_SPECIES`).
pub const MAX_TREE_SPECIES: u8 = 21;

/// The `LLVOTree` species table, ported from `app_settings/trees.xml`.
///
/// Indexed by species byte: `TREE_SPECIES[n].species_id == n`. Prefer
/// [`tree_species`] for a bounds-checked lookup from an on-wire species value.
pub static TREE_SPECIES: [TreeSpecies; 21] = [
    TreeSpecies {
        species_id: 0,
        name: "Pine 1",
        texture_id: tex(uuid!("0187babf-6c0d-5891-ebed-4ecab1426683")),
        droop: 60.0,
        twist: 5.0,
        branches: 5.0,
        depth: 1,
        scale_step: 0.7,
        trunk_depth: 6,
        branch_length: 8.0,
        trunk_length: 11.5,
        leaf_scale: 22.0,
        billboard_scale: 39.5,
        billboard_ratio: 1.1,
        trunk_aspect: 0.1,
        branch_aspect: 0.05,
        leaf_rotate: 20.0,
        noise_mag: 0.5,
        noise_scale: 2.5,
        taper: 0.8,
        repeat_trunk_z: 3.0,
    },
    TreeSpecies {
        species_id: 1,
        name: "Oak",
        texture_id: tex(uuid!("8a515889-eac9-fb55-8eba-d2dc09eb32c8")),
        droop: 35.0,
        twist: 3.0,
        branches: 4.0,
        depth: 3,
        scale_step: 0.7,
        trunk_depth: 0,
        branch_length: 3.0,
        trunk_length: 3.8,
        leaf_scale: 7.0,
        billboard_scale: 10.25,
        billboard_ratio: 1.0,
        trunk_aspect: 0.15,
        branch_aspect: 0.07,
        leaf_rotate: 0.0,
        noise_mag: 1.2,
        noise_scale: 4.0,
        taper: 0.3,
        repeat_trunk_z: 4.0,
    },
    TreeSpecies {
        species_id: 2,
        name: "Tropical Bush 1",
        texture_id: tex(uuid!("5bc11cd6-2f40-071e-a8da-0903394204f9")),
        droop: 10.0,
        twist: 0.0,
        branches: 6.0,
        depth: 1,
        scale_step: 0.5,
        trunk_depth: 1,
        branch_length: 0.5,
        trunk_length: 0.15,
        leaf_scale: 7.5,
        billboard_scale: 5.0,
        billboard_ratio: 1.25,
        trunk_aspect: 1.0,
        branch_aspect: 0.08,
        leaf_rotate: 0.0,
        noise_mag: 1.0,
        noise_scale: 1.0,
        taper: 0.2,
        repeat_trunk_z: 1.0,
    },
    TreeSpecies {
        species_id: 3,
        name: "Palm 1",
        texture_id: tex(uuid!("ca4e8c27-473c-eb1c-2f5d-50ee3f07d85c")),
        droop: 0.0,
        twist: 0.0,
        branches: 3.0,
        depth: 1,
        scale_step: 0.5,
        trunk_depth: 0,
        branch_length: 0.7,
        trunk_length: 9.0,
        leaf_scale: 10.0,
        billboard_scale: 13.25,
        billboard_ratio: 1.0,
        trunk_aspect: 0.035,
        branch_aspect: 0.03,
        leaf_rotate: 0.0,
        noise_mag: 0.2,
        noise_scale: 6.0,
        taper: 0.7,
        repeat_trunk_z: 10.0,
    },
    TreeSpecies {
        species_id: 4,
        name: "Dogwood",
        texture_id: tex(uuid!("64367bd1-697e-b3e6-0b65-3f862a577366")),
        droop: 30.0,
        twist: 0.0,
        branches: 3.0,
        depth: 2,
        scale_step: 0.7,
        trunk_depth: 1,
        branch_length: 2.75,
        trunk_length: 4.0,
        leaf_scale: 5.5,
        billboard_scale: 10.0,
        billboard_ratio: 1.0,
        trunk_aspect: 0.06,
        branch_aspect: 0.05,
        leaf_rotate: 0.0,
        noise_mag: 1.5,
        noise_scale: 2.0,
        taper: 0.8,
        repeat_trunk_z: 3.0,
    },
    TreeSpecies {
        species_id: 5,
        name: "Tropical Bush 2",
        texture_id: tex(uuid!("cdd9a9fc-6d0b-f90d-8416-c72b6019bca8")),
        droop: 10.0,
        twist: 0.0,
        branches: 3.0,
        depth: 1,
        scale_step: 0.5,
        trunk_depth: 1,
        branch_length: 0.5,
        trunk_length: 0.15,
        leaf_scale: 6.0,
        billboard_scale: 4.5,
        billboard_ratio: 0.9,
        trunk_aspect: 1.0,
        branch_aspect: 0.08,
        leaf_rotate: 0.0,
        noise_mag: 1.0,
        noise_scale: 1.0,
        taper: 0.2,
        repeat_trunk_z: 1.0,
    },
    TreeSpecies {
        species_id: 6,
        name: "Palm 2",
        texture_id: tex(uuid!("2d784476-d0db-9979-0cff-9408745a7cf3")),
        droop: 0.0,
        twist: 0.0,
        branches: 3.0,
        depth: 1,
        scale_step: 0.5,
        trunk_depth: 0,
        branch_length: 0.7,
        trunk_length: 10.0,
        leaf_scale: 7.5,
        billboard_scale: 13.5,
        billboard_ratio: 1.0,
        trunk_aspect: 0.035,
        branch_aspect: 0.03,
        leaf_rotate: 0.0,
        noise_mag: 0.2,
        noise_scale: 6.0,
        taper: 0.6,
        repeat_trunk_z: 12.0,
    },
    TreeSpecies {
        species_id: 7,
        name: "Cypress 1",
        texture_id: tex(uuid!("fb2ae204-3fd1-df33-594f-c9f882830e66")),
        droop: 30.0,
        twist: 0.0,
        branches: 3.0,
        depth: 4,
        scale_step: 0.5,
        trunk_depth: 0,
        branch_length: 10.0,
        trunk_length: 10.0,
        leaf_scale: 70.0,
        billboard_scale: 22.5,
        billboard_ratio: 1.0,
        trunk_aspect: 0.05,
        branch_aspect: 0.03,
        leaf_rotate: 0.0,
        noise_mag: 1.2,
        noise_scale: 1.0,
        taper: 0.5,
        repeat_trunk_z: 6.0,
    },
    TreeSpecies {
        species_id: 8,
        name: "Cypress 2",
        texture_id: tex(uuid!("30047cec-269d-408e-0c30-b2603b887268")),
        droop: 30.0,
        twist: 0.0,
        branches: 3.0,
        depth: 4,
        scale_step: 0.6,
        trunk_depth: 3,
        branch_length: 7.5,
        trunk_length: 10.0,
        leaf_scale: 35.0,
        billboard_scale: 25.0,
        billboard_ratio: 0.8,
        trunk_aspect: 0.05,
        branch_aspect: 0.04,
        leaf_rotate: 0.0,
        noise_mag: 1.2,
        noise_scale: 1.0,
        taper: 0.5,
        repeat_trunk_z: 5.0,
    },
    TreeSpecies {
        species_id: 9,
        name: "Pine 2",
        texture_id: tex(uuid!("d691a01c-13b7-578d-57c0-5caef0b4e7e1")),
        droop: 50.0,
        twist: 7.5,
        branches: 4.0,
        depth: 2,
        scale_step: 0.7,
        trunk_depth: 6,
        branch_length: 6.0,
        trunk_length: 10.0,
        leaf_scale: 15.5,
        billboard_scale: 33.0,
        billboard_ratio: 1.35,
        trunk_aspect: 0.1,
        branch_aspect: 0.08,
        leaf_rotate: 5.0,
        noise_mag: 0.5,
        noise_scale: 2.5,
        taper: 0.7,
        repeat_trunk_z: 3.0,
    },
    TreeSpecies {
        species_id: 10,
        name: "Plumeria",
        texture_id: tex(uuid!("6de37e4e-7029-61f5-54b8-f5e63f983f58")),
        droop: 8.0,
        twist: 7.0,
        branches: 3.0,
        depth: 2,
        scale_step: 0.6,
        trunk_depth: 0,
        branch_length: 3.0,
        trunk_length: 0.1,
        leaf_scale: 20.0,
        billboard_scale: 10.0,
        billboard_ratio: 1.35,
        trunk_aspect: 0.1,
        branch_aspect: 0.075,
        leaf_rotate: 0.0,
        noise_mag: 0.0,
        noise_scale: 0.0,
        taper: 0.85,
        repeat_trunk_z: 2.0,
    },
    TreeSpecies {
        species_id: 11,
        name: "Winter Pine 1",
        texture_id: tex(uuid!("10d2a01a-0818-84b9-4b96-c2eb63256519")),
        droop: 90.0,
        twist: 2.5,
        branches: 6.0,
        depth: 1,
        scale_step: 0.66,
        trunk_depth: 8,
        branch_length: 0.0,
        trunk_length: 4.0,
        leaf_scale: 6.75,
        billboard_scale: 12.5,
        billboard_ratio: 0.6,
        trunk_aspect: 0.1,
        branch_aspect: 0.05,
        leaf_rotate: 0.0,
        noise_mag: 0.0,
        noise_scale: 2.5,
        taper: 0.85,
        repeat_trunk_z: 2.0,
    },
    TreeSpecies {
        species_id: 12,
        name: "Winter Aspen",
        texture_id: tex(uuid!("7c0cf89b-44b1-1ce2-dd74-07102a98ac2a")),
        droop: 85.0,
        twist: 3.0,
        branches: 5.0,
        depth: 1,
        scale_step: 0.6,
        trunk_depth: 8,
        branch_length: 3.0,
        trunk_length: 4.5,
        leaf_scale: 8.0,
        billboard_scale: 12.0,
        billboard_ratio: 0.675,
        trunk_aspect: 0.06,
        branch_aspect: 0.05,
        leaf_rotate: 0.0,
        noise_mag: 0.75,
        noise_scale: 2.5,
        taper: 0.8,
        repeat_trunk_z: 2.0,
    },
    TreeSpecies {
        species_id: 13,
        name: "Winter Pine 2",
        texture_id: tex(uuid!("67931331-0c02-4876-1255-28770896c6a2")),
        droop: 140.0,
        twist: 5.0,
        branches: 6.0,
        depth: 1,
        scale_step: 0.6,
        trunk_depth: 7,
        branch_length: 0.0,
        trunk_length: 3.0,
        leaf_scale: 5.0,
        billboard_scale: 7.5,
        billboard_ratio: 0.5,
        trunk_aspect: 0.1,
        branch_aspect: 0.05,
        leaf_rotate: 0.0,
        noise_mag: 0.75,
        noise_scale: 2.5,
        taper: 0.5,
        repeat_trunk_z: 2.0,
    },
    TreeSpecies {
        species_id: 14,
        name: "Eucalyptus",
        texture_id: tex(uuid!("a6162133-724b-54df-a12f-51cd070ad6f3")),
        droop: 20.0,
        twist: 5.0,
        branches: 3.6,
        depth: 4,
        scale_step: 0.6,
        trunk_depth: 0,
        branch_length: 12.0,
        trunk_length: 8.0,
        leaf_scale: 33.0,
        billboard_scale: 24.0,
        billboard_ratio: 1.3,
        trunk_aspect: 0.15,
        branch_aspect: 0.08,
        leaf_rotate: 0.0,
        noise_mag: 0.0,
        noise_scale: 0.0,
        taper: 0.675,
        repeat_trunk_z: 3.0,
    },
    TreeSpecies {
        species_id: 15,
        name: "Fern",
        texture_id: tex(uuid!("8872f2b8-31db-42d8-580a-b3e4a91262de")),
        droop: 12.0,
        twist: 0.0,
        branches: 7.0,
        depth: 1,
        scale_step: 0.5,
        trunk_depth: 0,
        branch_length: 0.01,
        trunk_length: 0.0,
        leaf_scale: 4.0,
        billboard_scale: 3.5,
        billboard_ratio: 0.85,
        trunk_aspect: 1.0,
        branch_aspect: 0.08,
        leaf_rotate: 0.0,
        noise_mag: 1.0,
        noise_scale: 1.0,
        taper: 0.2,
        repeat_trunk_z: 1.0,
    },
    TreeSpecies {
        species_id: 16,
        name: "Eelgrass",
        texture_id: tex(uuid!("96b4de31-f4fa-337d-ec78-451e3609769e")),
        droop: 0.0,
        twist: 0.0,
        branches: 5.0,
        depth: 1,
        scale_step: 0.5,
        trunk_depth: 1,
        branch_length: 0.5,
        trunk_length: 0.15,
        leaf_scale: 5.0,
        billboard_scale: 3.0,
        billboard_ratio: 1.0,
        trunk_aspect: 1.0,
        branch_aspect: 0.08,
        leaf_rotate: 0.0,
        noise_mag: 1.0,
        noise_scale: 1.0,
        taper: 0.2,
        repeat_trunk_z: 1.0,
    },
    TreeSpecies {
        species_id: 17,
        name: "Sea Sword",
        texture_id: tex(uuid!("5894e2e7-ab8d-edfa-e61c-18cf16854ba3")),
        droop: 0.0,
        twist: 0.0,
        branches: 6.0,
        depth: 1,
        scale_step: 0.7,
        trunk_depth: 1,
        branch_length: 0.0,
        trunk_length: 0.0,
        leaf_scale: 2.0,
        billboard_scale: 2.0,
        billboard_ratio: 1.0,
        trunk_aspect: 1.0,
        branch_aspect: 1.0,
        leaf_rotate: 0.0,
        noise_mag: 0.5,
        noise_scale: 0.0,
        taper: 0.0,
        repeat_trunk_z: 1.0,
    },
    TreeSpecies {
        species_id: 18,
        name: "Kelp 1",
        texture_id: tex(uuid!("2caf1179-7861-6ff3-4b7d-46e17780bdfa")),
        droop: -15.0,
        twist: 0.0,
        branches: 1.0,
        depth: 1,
        scale_step: 1.0,
        trunk_depth: 3,
        branch_length: 2.5,
        trunk_length: 0.75,
        leaf_scale: 1.85,
        billboard_scale: 4.9,
        billboard_ratio: 1.0,
        trunk_aspect: 0.04,
        branch_aspect: 0.05,
        leaf_rotate: 0.0,
        noise_mag: 1.0,
        noise_scale: 2.0,
        taper: 0.8,
        repeat_trunk_z: 2.0,
    },
    TreeSpecies {
        species_id: 19,
        name: "Beach Grass 1",
        texture_id: tex(uuid!("18fb888b-e8f1-dce7-7da7-321d651ea6b0")),
        droop: 0.0,
        twist: 0.0,
        branches: 4.0,
        depth: 1,
        scale_step: 0.7,
        trunk_depth: 1,
        branch_length: 0.0,
        trunk_length: 0.0,
        leaf_scale: 4.0,
        billboard_scale: 2.5,
        billboard_ratio: 1.2,
        trunk_aspect: 1.0,
        branch_aspect: 1.0,
        leaf_rotate: 0.0,
        noise_mag: 0.5,
        noise_scale: 0.0,
        taper: 0.0,
        repeat_trunk_z: 1.0,
    },
    TreeSpecies {
        species_id: 20,
        name: "Kelp 2",
        texture_id: tex(uuid!("2a4880b6-b7a3-690a-2049-bfbe38eafb9f")),
        droop: -15.0,
        twist: 0.0,
        branches: 1.0,
        depth: 1,
        scale_step: 1.0,
        trunk_depth: 3,
        branch_length: 2.5,
        trunk_length: 1.35,
        leaf_scale: 2.0,
        billboard_scale: 4.9,
        billboard_ratio: 1.0,
        trunk_aspect: 0.025,
        branch_aspect: 0.05,
        leaf_rotate: 0.0,
        noise_mag: 1.0,
        noise_scale: 2.0,
        taper: 0.8,
        repeat_trunk_z: 2.0,
    },
];

/// Look up a tree species by its on-wire species byte.
///
/// Returns `None` for a species value outside the defined range
/// (`0..MAX_TREE_SPECIES`). Firestorm clamps an unknown species to a valid one
/// with a warning; callers that must always render something can fall back to
/// species `0`.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `tree_species` reads clearly"
)]
pub fn tree_species(species: u8) -> Option<&'static TreeSpecies> {
    TREE_SPECIES.get(usize::from(species))
}

#[cfg(test)]
mod tests {
    use super::{MAX_TREE_SPECIES, TREE_SPECIES, tree_species};
    use pretty_assertions::assert_eq;
    use uuid::uuid;

    #[test]
    fn table_is_indexed_by_species_id() {
        for (index, species) in TREE_SPECIES.iter().enumerate() {
            assert_eq!(usize::from(species.species_id), index);
        }
    }

    #[test]
    fn covers_all_defined_species() {
        assert_eq!(TREE_SPECIES.len(), usize::from(MAX_TREE_SPECIES));
        assert_eq!(MAX_TREE_SPECIES, 21);
    }

    #[test]
    fn lookup_in_range_and_out_of_range() {
        assert_eq!(tree_species(0).map(|s| s.name), Some("Pine 1"));
        assert_eq!(tree_species(20).map(|s| s.name), Some("Kelp 2"));
        assert!(tree_species(21).is_none());
        assert!(tree_species(255).is_none());
    }

    #[test]
    fn known_species_texture_ids() {
        assert_eq!(
            tree_species(0).map(|s| s.texture_id.uuid()),
            Some(uuid!("0187babf-6c0d-5891-ebed-4ecab1426683")),
        );
        assert_eq!(
            tree_species(1).map(|s| s.texture_id.uuid()),
            Some(uuid!("8a515889-eac9-fb55-8eba-d2dc09eb32c8")),
        );
    }

    #[test]
    fn integer_depths_truncate_like_firestorm() {
        // trees.xml writes `trunk_depth="0.1"` for Fern (species 15) and
        // `trunk_depth="3.0"` for Cypress 2 (species 8); both parse as ints.
        assert_eq!(tree_species(15).map(|s| s.trunk_depth), Some(0));
        assert_eq!(tree_species(8).map(|s| s.trunk_depth), Some(3));
    }
}
