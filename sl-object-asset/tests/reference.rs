//! The decoder against the two complete object assets the reference viewer
//! carries in its own test sources (see `tests/data/README.md`).
//!
//! These are the only public examples of the whole format, so they are the
//! contract: what the decoder makes of them is what the crate claims the format
//! *is*. Each field pinned here is one the crate would otherwise be free to get
//! silently wrong.

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_object_asset::{LegacySaleType, LinkState, ObjectAsset, PrimPlacement};
    use sl_types::lsl::{Rotation, Vector};
    use uuid::Uuid;

    /// A single attachment prim, as a simulator wrote it.
    const ATTACHMENT_PRIM: &str = include_str!("data/reference-attachment-prim.txt");

    /// A four-prim linkset, children first and the root last.
    const LINKSET: &str = include_str!("data/reference-linkset.txt");

    /// A decode failure, or a prim the asset turned out not to hold.
    type TestError = Box<dyn core::error::Error>;

    /// Asserts a decoded number is **exactly** the float the asset text names.
    ///
    /// Compared as bit patterns rather than with `==`, because that is what
    /// these assertions are for: the decoder must lose nothing, so an epsilon
    /// would be testing something weaker than the claim — and clippy is right
    /// that `==` on floats is usually the wrong tool.
    fn assert_exact(actual: f32, expected: f32, field: &str) {
        assert_eq!(actual.to_bits(), expected.to_bits(), "{field}");
    }

    /// [`assert_exact`] over the four channels of a colour.
    fn assert_exact_color(actual: [f32; 4], expected: [f32; 4], field: &str) {
        assert_eq!(
            actual.map(f32::to_bits),
            expected.map(f32::to_bits),
            "{field}"
        );
    }

    /// Every field of the reference's single attachment prim, so a change to
    /// any one keyword's reading shows up as a named failure rather than as a
    /// vaguely wrong prim.
    #[test]
    fn the_reference_attachment_prim_decodes_field_by_field() -> Result<(), TestError> {
        let asset = ObjectAsset::decode_str(ATTACHMENT_PRIM)?;
        assert_eq!(asset.prims.len(), 1, "one solitary prim");
        let prim = asset.prims.first().ok_or("the asset holds no prim")?;

        assert_eq!(
            prim.task_id,
            Uuid::parse_str("1fd77b79-a8e7-25a5-9454-02a4d948ba1c")?
        );
        assert_eq!(prim.name, "Object");
        assert_eq!(prim.description, None, "the prim carries no description");

        assert_eq!(prim.permissions.base_mask, 0x7fff_ffff);
        assert_eq!(prim.permissions.owner_mask, 0x7fff_ffff);
        assert_eq!(prim.permissions.group_mask, 0);
        assert_eq!(prim.permissions.everyone_mask, 0);
        assert_eq!(prim.permissions.next_owner_mask, 0x0008_2000);
        assert_eq!(
            prim.permissions.creator_id,
            Uuid::parse_str("3c115e51-04f4-523c-9fa6-98aff1034730")?
        );
        assert_eq!(prim.permissions.owner_id, prim.permissions.creator_id);
        assert_eq!(prim.permissions.last_owner_id, Uuid::nil());
        assert!(!prim.permissions.group_owned);

        assert_eq!(prim.local_id, 10_284);
        assert_eq!(prim.total_crc, 35);
        assert_eq!(prim.pcode, 1);
        assert_eq!(prim.task_valid, 2);
        assert_eq!(prim.travel_access, 21);
        assert_eq!(prim.display_options, 2);
        assert_eq!(prim.display_type, "v");

        assert_eq!(
            prim.position,
            Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0
            }
        );
        assert_eq!(
            prim.rotation,
            Rotation {
                x: 4.371_139_2e-8,
                y: 1.0,
                z: 4.371_139_2e-8,
                s: 0.0,
            },
            "the reference writes a float's full decimal expansion; it reads back as that float"
        );
        assert_eq!(
            prim.placement,
            PrimPlacement::Free {
                velocity: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
                angular_velocity: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
            },
            "an unlinked prim states a motion, not an offset from a root"
        );
        assert_eq!(
            prim.scale,
            Vector {
                x: 0.281_693_2,
                y: 0.281_693_2,
                z: 0.281_693_2
            }
        );
        assert_eq!(prim.sit_hint, 0);
        assert_eq!(prim.state, 80, "the attachment point, nibble-swapped");
        assert_eq!(prim.material, 3);
        assert_exact_color(prim.text_color, [0.0, 0.0, 0.0, 1.0], "textcolor");
        assert!(!prim.selected);
        assert_eq!(prim.selector, Uuid::nil());
        assert_eq!(prim.sound.sound_id, Uuid::nil());

        assert_eq!(prim.shape.path.curve, 16, "LL_PCODE_PATH_LINE");
        assert_exact(prim.shape.path.begin, 0.0, "path begin");
        assert_exact(prim.shape.path.end, 1.0, "path end");
        assert_exact(prim.shape.path.revolutions, 1.0, "path revolutions");
        assert_eq!(prim.shape.profile.curve, 1, "LL_PCODE_PROFILE_SQUARE");
        assert_exact(prim.shape.profile.hollow, 0.0, "profile hollow");

        assert_eq!(prim.faces.len(), 6, "a box has six faces");
        let face = prim.faces.first().ok_or("the prim has no faces")?;
        assert_eq!(
            face.image_id,
            Uuid::parse_str("89556747-24cb-43ed-920b-47caed15465f")?
        );
        assert_exact_color(face.color, [1.0, 1.0, 1.0, 1.0], "face colors");
        assert_exact(face.scale_s, 0.56, "face scales");
        assert_exact(face.scale_t, 0.56, "face scalet");
        assert_eq!(face.bump, 0);
        assert!(!face.fullbright);
        assert_eq!(face.media_flags, 0);

        assert_eq!(prim.bookkeeping.ps_next_crc, 1);
        assert_exact(prim.bookkeeping.gpw_bias, 1.0, "gpw_bias");
        assert_eq!(prim.bookkeeping.ip, 0);
        assert!(prim.bookkeeping.complete);
        assert_eq!(prim.bookkeeping.delay, 50_000);
        assert_eq!(prim.bookkeeping.next_start, 1_132_625_972_249_870);
        assert_eq!(prim.bookkeeping.birth_time, 1_132_625_953_120_694);
        assert_eq!(prim.bookkeeping.rez_time, prim.bookkeeping.birth_time);
        assert_eq!(prim.bookkeeping.parcel_time, prim.bookkeeping.birth_time);
        assert_exact(prim.bookkeeping.tax_rate, 1.016_15, "tax_rate");

        assert_eq!(
            prim.name_values,
            vec![
                "AttachmentOrientation VEC3 RW DS -3.141593, 0.000000, -3.141593".to_owned(),
                "AttachmentOffset VEC3 RW DS 0.000000, 0.000000, 0.000000".to_owned(),
                "AttachPt U32 RW S 5".to_owned(),
                "AttachItemID STRING RW SV 1f9975c0-2951-1b93-dd83-46e2b932fcc8".to_owned(),
            ],
            "the name-value declarations are kept verbatim, tokens and all"
        );

        assert_eq!(prim.scratchpad.count, 0);
        assert_eq!(prim.sale_info.sale_type, LegacySaleType::NotForSale);
        assert_eq!(prim.sale_info.sale_price, 10);
        assert_eq!(
            prim.orig_asset_id,
            Some(Uuid::parse_str("52019cdd-b464-ba19-e66d-3da751fef9da")?)
        );
        assert_eq!(
            prim.orig_item_id,
            Some(Uuid::parse_str("1f9975c0-2951-1b93-dd83-46e2b932fcc8")?)
        );
        assert_eq!(prim.from_task_id, None);
        assert_eq!(prim.correct_family_id, Uuid::nil());
        assert!(!prim.has_rezzed);
        assert_eq!(prim.pre_link_base_mask, 0x7fff_ffff);
        assert_eq!(prim.link, None, "no `linked` line: the prim stands alone");
        assert_eq!(prim.default_pay_price, [-2, 1, 5, 10, 20]);
        assert_eq!(prim.unknown, Vec::new(), "every keyword is one we know");
        Ok(())
    }

    /// The linkset: four prims, three of them children of the fourth, which is
    /// written last. The child pair (`childpos` / `childrot`) is the whole
    /// reason a linkset needs its own case.
    #[test]
    fn the_reference_linkset_decodes_as_three_children_and_a_root() -> Result<(), TestError> {
        let asset = ObjectAsset::decode_str(LINKSET)?;
        assert_eq!(asset.prims.len(), 4);
        assert_eq!(asset.children().count(), 3);

        let links: Vec<Option<LinkState>> = asset.prims.iter().map(|prim| prim.link).collect();
        assert_eq!(
            links,
            vec![
                Some(LinkState::Child),
                Some(LinkState::Child),
                Some(LinkState::Child),
                Some(LinkState::Root),
            ],
            "the root is written last, which is why `root()` reads the marker"
        );

        let child = asset.prims.first().ok_or("the asset holds no prim")?;
        assert_eq!(
            child.placement,
            PrimPlacement::Child {
                position: Vector {
                    x: -0.005,
                    y: -0.036,
                    z: 0.308,
                },
                rotation: Rotation {
                    x: -0.515_492_74,
                    y: -0.466_012,
                    z: 0.529_055_4,
                    s: 0.487_032_32,
                },
            },
            "a child states its offset from the root, not a velocity"
        );
        assert_eq!(child.pcode, 2);
        assert_eq!(child.description, None);
        assert_eq!(child.faces.len(), 6);

        let root = asset.root().ok_or("the linkset has no root")?;
        assert!(
            !root.name.is_empty(),
            "the root carries the object's name, which is the item's name on a take"
        );
        assert_eq!(root.description.as_deref(), Some("(No Description)"));
        assert_eq!(root.name_values.len(), 4, "the root is the worn attachment");
        assert_eq!(
            root.from_task_id,
            Some(Uuid::parse_str("3c115e51-04f4-523c-9fa6-98aff1034730")?),
            "only the root records the task it came from"
        );
        assert!(
            matches!(root.placement, PrimPlacement::Free { .. }),
            "the root states a motion"
        );
        assert_eq!(root.faces.len(), 3, "the root prim has three faces");
        Ok(())
    }

    /// Re-encoding a reference asset and reading it back yields the same model.
    ///
    /// Not byte equality: the reference writes each number at whatever
    /// precision its own `ostream` had (six significant digits for a position,
    /// a float's full exact expansion for a rotation) and this crate writes the
    /// shortest text that round-trips. What must hold is that no *field* is
    /// lost or changed on the way out and back.
    #[test]
    fn a_reference_asset_survives_a_re_encode() -> Result<(), TestError> {
        for asset in [
            ObjectAsset::decode_str(ATTACHMENT_PRIM)?,
            ObjectAsset::decode_str(LINKSET)?,
        ] {
            let round_tripped = ObjectAsset::decode(&asset.encode())?;
            assert_eq!(round_tripped, asset);
        }
        Ok(())
    }

    /// A keyword this crate has never heard of is carried through a re-save
    /// rather than dropped — the difference between an editor and a lossy one
    /// when a grid grows a field.
    #[test]
    fn an_unknown_keyword_survives_a_re_encode() -> Result<(), TestError> {
        let with_extra = ATTACHMENT_PRIM.replace(
            "\thas_rezzed\t0\n",
            "\thas_rezzed\t0\n\tsome_future_field\t42\n",
        );
        let asset = ObjectAsset::decode_str(&with_extra)?;
        let prim = asset.prims.first().ok_or("the asset holds no prim")?;
        let fields: Vec<(String, String)> = prim
            .unknown
            .iter()
            .map(|field| (field.keyword.clone(), field.value.clone()))
            .collect();
        assert_eq!(
            fields,
            vec![("some_future_field".to_owned(), "42".to_owned())]
        );
        let round_tripped = ObjectAsset::decode(&asset.encode())?;
        assert_eq!(round_tripped, asset);
        Ok(())
    }
}
