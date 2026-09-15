//! Every attachment point the vendored `character/` directory declares resolves
//! to something a worn object can hang off.
//!
//! A worn attachment is seated by looking its raw point id up in one of two
//! tables the viewer builds from `avatar_lad.xml`: the body table
//! ([`AvatarAssetLibrary::attachment_points`]), whose entries carry the skeleton
//! joint the point hangs from, and the HUD table
//! ([`AvatarAssetLibrary::hud_attachment_points`]), whose entries hang off the
//! `mScreen` pseudo-joint instead. A point that lands in **neither** — because
//! its `joint` attribute names something the skeleton does not carry — is not an
//! error anywhere: it is silently dropped, and the only symptom is that an
//! object worn there never appears, on that point alone, with nothing logged
//! (roadmap `viewer-prim-attachment-worn-but-not-rendered`).
//!
//! So pin the partition here, where a re-copied `character/` directory or a
//! skeleton change is cheap to catch, rather than on a grid one attachment point
//! at a time.

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;
    use sl_viewer_kit::avatar_assets::AvatarAssetLibrary;

    /// A boxed test error, so the test can use `?`.
    type TestError = Box<dyn core::error::Error>;

    /// The vendored `character/` directory, the viewer's own default.
    fn character_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("viewer-assets")
            .join("character")
    }

    /// Every attachment point in the vendored `avatar_lad.xml` resolves: the body
    /// points to a real skeleton joint, the HUD points to the `mScreen` table, and
    /// the two partition the whole declared id set with nothing left over.
    #[test]
    fn every_vendored_attachment_point_resolves() -> Result<(), TestError> {
        let library = AvatarAssetLibrary::load(&character_dir())?;
        let body: BTreeSet<u8> = library.attachment_points().into_keys().collect();
        let hud: BTreeSet<u8> = library.hud_attachment_points().into_keys().collect();

        // The `avatar_lad.xml` this workspace vendors declares 55 points, ids 1
        // through 55 with no gaps — Linden's own numbering, and what the wire's
        // un-swizzled `state` byte indexes into.
        let declared: BTreeSet<u8> = (1..=55).collect();
        let resolved: BTreeSet<u8> = body.union(&hud).copied().collect();
        assert_eq!(
            resolved, declared,
            "every declared attachment point must resolve to a body joint or the HUD screen"
        );

        // The two tables are disjoint: a point hangs off the skeleton or off the
        // screen, never both, and the seating code picks its table by the point's
        // id alone (`is_hud_point`).
        let both: Vec<u8> = body.intersection(&hud).copied().collect();
        assert_eq!(
            both,
            Vec::<u8>::new(),
            "no attachment point may be both a body point and a HUD point"
        );

        // The HUD points are exactly the eight `mScreen` ones the reference
        // viewer creates for `isSelf()` — ids 31..=38.
        let expected_hud: BTreeSet<u8> = (31..=38).collect();
        assert_eq!(hud, expected_hud, "the HUD points are ids 31 through 38");

        // Each body point's joint index addresses a real joint, which is what the
        // per-avatar attachment-point node is spawned from: an index past the end
        // makes the node — and so the worn object — silently absent.
        let joints = library.skeleton().len();
        for (point_id, info) in library.attachment_points() {
            assert!(
                info.joint_index < joints,
                "attachment point {point_id} binds joint index {} of {joints}",
                info.joint_index
            );
        }
        Ok(())
    }

    /// The body points an object worn on stays drawn in mouselook, pinned to the
    /// vendored `avatar_lad.xml`: every point but the head ones. The own-avatar
    /// first-person view hides what is worn on the rest, so a re-copied
    /// `character/` directory that lost the attribute would put a worn hat or
    /// mesh head back in front of the camera — the parser reads a missing flag
    /// as `false`, which would hide everything instead.
    #[test]
    fn only_the_head_points_are_hidden_in_first_person() -> Result<(), TestError> {
        let library = AvatarAssetLibrary::load(&character_dir())?;
        let hidden: BTreeSet<u8> = library
            .attachment_points()
            .into_iter()
            .filter(|(_point_id, info)| !info.visible_in_first_person)
            .map(|(point_id, _info)| point_id)
            .collect();
        // Skull, Mouth, Chin, Left / Right Ear, Left / Right Eyeball, Nose, and
        // the Bento face points Jaw, Alt Left / Right Ear, Alt Left / Right Eye
        // and Tongue.
        let expected: BTreeSet<u8> = [2, 11, 12, 13, 14, 15, 16, 17, 47, 48, 49, 50, 51, 52]
            .into_iter()
            .collect();
        assert_eq!(hidden, expected);
        Ok(())
    }
}
