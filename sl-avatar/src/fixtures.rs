//! The miniature avatar assets this crate's own tests decode, exposed so that
//! sibling crates can decode the same bytes.
//!
//! These are test data, not shipped content, but they are deliberately public.
//! The consumers need a *real* decode rather than a synthesized value —
//! [`BaseMesh`](crate::basemesh::BaseMesh)'s fields are private, so
//! `from_bytes` is its only constructor — and the alternative to sharing is a
//! second copy of the same bytes in each consumer, which rots silently when
//! the real fixture changes. One copy, referenced by name, means reshaping a
//! fixture breaks its consumers loudly at compile time, which is the correct
//! direction for that failure.
//!
//! Keeping the reference by name rather than by relative path also keeps every
//! consumer's `include!` arguments inside its own directory, which is what lets
//! the commit hooks scope a crate's checks to that crate.
//!
//! They cost ~4 KiB of `.rodata` in a consumer that names them, and nothing at
//! all in one that does not.

/// A four-bone / two-volume skeleton — `mPelvis` → `mTorso` (+`BELLY`) →
/// `mChest`, plus `mHipRight` and `PELVIS` — whose collision volumes carry
/// authored non-identity rotations, so a consumer that drops the volume
/// rotation renders visibly wrong rather than merely imprecisely.
pub const MINI_SKELETON: &str = include_str!("../tests/fixtures/mini_skeleton.xml");

/// A single base-body part weighted against [`MINI_SKELETON`], carrying skin
/// weights and morph-target deltas.
///
/// Because the part is skinned, a consumer that gives its mesh the
/// `JOINT_INDEX` / `JOINT_WEIGHT` attributes must also give it a skeleton: in
/// Bevy those attributes specialize the skinned pipeline, and a skinned
/// pipeline handed a model-only bind group is a wgpu validation error that
/// kills the process.
pub const MINI_BASEMESH: &[u8] = include_bytes!("../tests/fixtures/mini_basemesh.llm");

/// A minimal `avatar_lad.xml`: chest and skull attachment points plus one HUD
/// point, enough to exercise attachment-point resolution without the real
/// several-megabyte table.
pub const MINI_LAD: &str = include_str!("../tests/fixtures/mini_lad.xml");

#[cfg(test)]
mod tests {
    use super::{MINI_BASEMESH, MINI_LAD, MINI_SKELETON};
    use crate::basemesh::BaseMesh;
    use crate::skeleton::{AttachmentPoints, Skeleton};
    use pretty_assertions::assert_eq;

    /// The error type these tests bubble into, so a decode failure reports
    /// itself rather than being unwrapped.
    type TestError = Box<dyn core::error::Error>;

    /// Every exported fixture decodes, so a consumer that names one can rely
    /// on the bytes being well-formed instead of discovering otherwise at run
    /// time in a renderer.
    #[test]
    fn the_exported_fixtures_decode() -> Result<(), TestError> {
        let skeleton = Skeleton::from_xml(MINI_SKELETON)?;
        assert_eq!(skeleton.len(), 4, "four joints");
        assert_eq!(skeleton.collision_volume_count(), 2, "two volumes");

        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;
        assert!(
            !mesh.positions().is_empty(),
            "the decoded part carries geometry"
        );

        let points = AttachmentPoints::from_xml(MINI_LAD)?;
        assert_eq!(points.all().len(), 3, "chest, skull and one HUD point");
        Ok(())
    }
}
