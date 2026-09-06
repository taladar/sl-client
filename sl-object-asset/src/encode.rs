//! Writing an [`ObjectAsset`] back out as the nested-block text a grid stores.
//!
//! The field order, the indentation and the choice of separator per line are
//! the reference's, taken from what survives of its writers
//! (`LLPermissions::exportLegacyStream`, `LLSaleInfo::exportLegacyStream`,
//! `LLPathParams` / `LLProfileParams::exportLegacyStream`) and from the two
//! complete assets its own tests carry. That matters because the format has no
//! self-description: a reader that walked keywords in a different order would
//! still read it, but a grid comparing two saves byte for byte would not.
//!
//! Numbers are written with Rust's shortest round-tripping float formatting
//! rather than the reference's own per-field `ostream` precision, which varies
//! from six significant digits to the float's full exact decimal expansion
//! within one prim. Every value this crate writes reads back as the same `f32`;
//! a reference asset re-encoded here is therefore *semantically* identical and
//! not byte-identical.

use core::fmt::Write as _;

use sl_types::lsl::{Rotation, Vector};
use uuid::Uuid;

use crate::model::{
    LegacyFace, LegacyPathParams, LegacyPermissions, LegacyProfileParams, LegacySaleInfo,
    LegacyShape, ObjectAsset, PrimBlock, PrimPlacement, Scratchpad,
};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "encode owns its `impl ObjectAsset` block, apart from decode's canonical impl"
)]
impl ObjectAsset {
    /// Encodes the asset as the bytes a grid would serve for it.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        self.encode_to_string().into_bytes()
    }

    /// Encodes the asset as its text form.
    #[must_use]
    pub fn encode_to_string(&self) -> String {
        let mut out = String::new();
        for prim in &self.prims {
            write_prim(&mut out, prim);
        }
        out
    }
}

/// Writes one prim: its header line and the block that follows it.
///
/// Every `write!` here is to a `String`, whose `fmt::Write` cannot fail, so the
/// results are bound and dropped rather than propagated — the alternative is a
/// `fmt::Result` on every function in this module for an error that cannot
/// happen.
fn write_prim(out: &mut String, prim: &PrimBlock) {
    let _written = writeln!(out, "{{'task_id':u{}}}", prim.task_id);
    out.push_str("{\n");
    let _written = writeln!(out, "\tname\t{}|", prim.name);
    write_permissions(out, &prim.permissions);
    let _written = writeln!(out, "\tlocal_id\t{}", prim.local_id);
    let _written = writeln!(out, "\ttotal_crc\t{}", prim.total_crc);
    let _written = writeln!(out, "\ttype\t{}", prim.pcode);
    let _written = writeln!(out, "\ttask_valid\t{}", prim.task_valid);
    let _written = writeln!(out, "\ttravel_access\t{}", prim.travel_access);
    let _written = writeln!(out, "\tdisplayopts\t{}", prim.display_options);
    let _written = writeln!(out, "\tdisplaytype\t{}", prim.display_type);
    write_vector(out, "pos", &prim.position);
    write_vector(out, "oldpos", &prim.old_position);
    write_rotation(out, "rotation", &prim.rotation);
    match &prim.placement {
        PrimPlacement::Free {
            velocity,
            angular_velocity,
        } => {
            write_vector(out, "velocity", velocity);
            write_vector(out, "angvel", angular_velocity);
        }
        PrimPlacement::Child { position, rotation } => {
            write_vector(out, "childpos", position);
            write_rotation(out, "childrot", rotation);
        }
    }
    write_vector(out, "scale", &prim.scale);
    write_vector(out, "sit_offset", &prim.sit_offset);
    write_vector(out, "camera_eye_offset", &prim.camera_eye_offset);
    write_vector(out, "camera_at_offset", &prim.camera_at_offset);
    write_rotation(out, "sit_quat", &prim.sit_rotation);
    let _written = writeln!(out, "\tsit_hint\t{}", prim.sit_hint);
    let _written = writeln!(out, "\tstate\t{}", prim.state);
    let _written = writeln!(out, "\tmaterial\t{}", prim.material);
    let _written = writeln!(out, "\tsoundid\t{}", prim.sound.sound_id);
    let _written = writeln!(out, "\tsoundgain\t{}", prim.sound.gain);
    let _written = writeln!(out, "\tsoundradius\t{}", prim.sound.radius);
    let _written = writeln!(out, "\tsoundflags\t{}", prim.sound.flags);
    write_color(out, "\ttextcolor\t", prim.text_color);
    write_flag(out, "selected", prim.selected);
    let _written = writeln!(out, "\tselector\t{}", prim.selector);
    write_flag(out, "usephysics", prim.flags.use_physics);
    write_flag(out, "rotate_x", prim.flags.rotate_x);
    write_flag(out, "rotate_y", prim.flags.rotate_y);
    write_flag(out, "rotate_z", prim.flags.rotate_z);
    write_flag(out, "phantom", prim.flags.phantom);
    let _written = writeln!(
        out,
        "\tremote_script_access_pin\t{}",
        prim.remote_script_access_pin
    );
    write_flag(out, "volume_detect", prim.flags.volume_detect);
    write_flag(out, "block_grabs", prim.flags.block_grabs);
    write_flag(out, "die_at_edge", prim.flags.die_at_edge);
    write_flag(out, "return_at_edge", prim.flags.return_at_edge);
    write_flag(out, "temporary", prim.flags.temporary);
    write_flag(out, "sandbox", prim.flags.sandbox);
    write_vector(out, "sandboxhome", &prim.sandbox_home);
    write_shape(out, &prim.shape);
    write_faces(out, &prim.faces);
    let _written = writeln!(out, "\tps_next_crc\t{}", prim.bookkeeping.ps_next_crc);
    let _written = writeln!(out, "\tgpw_bias\t{}", prim.bookkeeping.gpw_bias);
    let _written = writeln!(out, "\tip\t{}", prim.bookkeeping.ip);
    let _written = writeln!(
        out,
        "\tcomplete\t{}",
        if prim.bookkeeping.complete {
            "TRUE"
        } else {
            "FALSE"
        }
    );
    let _written = writeln!(out, "\tdelay\t{}", prim.bookkeeping.delay);
    let _written = writeln!(out, "\tnextstart\t{}", prim.bookkeeping.next_start);
    let _written = writeln!(out, "\tbirthtime\t{}", prim.bookkeeping.birth_time);
    let _written = writeln!(out, "\treztime\t{}", prim.bookkeeping.rez_time);
    let _written = writeln!(out, "\tparceltime\t{}", prim.bookkeeping.parcel_time);
    if let Some(description) = &prim.description {
        let _written = writeln!(out, "\tdescription\t{description}|");
    }
    let _written = writeln!(out, "\ttax_rate\t{}", prim.bookkeeping.tax_rate);
    for pair in &prim.name_values {
        let _written = writeln!(out, "\tnamevalue\t{pair}");
    }
    write_scratchpad(out, &prim.scratchpad);
    write_sale_info(out, &prim.sale_info);
    write_optional_uuid(out, "orig_asset_id", prim.orig_asset_id);
    write_optional_uuid(out, "orig_item_id", prim.orig_item_id);
    write_optional_uuid(out, "from_task_id", prim.from_task_id);
    let _written = writeln!(out, "\tcorrect_family_id\t{}", prim.correct_family_id);
    write_flag(out, "has_rezzed", prim.has_rezzed);
    let _written = writeln!(out, "\tpre_link_base_mask\t{:08x}", prim.pre_link_base_mask);
    if let Some(link) = prim.link {
        // The trailing space after the keyword is the reference's own: its
        // reader splits on whitespace and never sees it, and reproducing it is
        // what makes a re-save of an untouched prim compare equal.
        let _written = writeln!(out, "\tlinked \t{}", link.as_str());
    }
    let [free, first, second, third, fourth] = prim.default_pay_price;
    let _written = writeln!(
        out,
        "\tdefault_pay_price\t{free}\t{first}\t{second}\t{third}\t{fourth}"
    );
    for field in &prim.unknown {
        let _written = writeln!(out, "\t{}\t{}", field.keyword, field.value);
    }
    out.push_str("}\n");
}

/// Writes a `0` / `1` toggle line.
fn write_flag(out: &mut String, keyword: &str, value: bool) {
    let _written = writeln!(out, "\t{keyword}\t{}", u8::from(value));
}

/// Writes a tab-separated three-component vector line.
fn write_vector(out: &mut String, keyword: &str, value: &Vector) {
    let _written = writeln!(out, "\t{keyword}\t{}\t{}\t{}", value.x, value.y, value.z);
}

/// Writes a tab-separated four-component rotation line.
fn write_rotation(out: &mut String, keyword: &str, value: &Rotation) {
    let _written = writeln!(
        out,
        "\t{keyword}\t{}\t{}\t{}\t{}",
        value.x, value.y, value.z, value.s
    );
}

/// Writes a **space**-separated RGBA colour after the given line prefix — the
/// one place the format separates a tuple with spaces, because a colour is
/// streamed by `LLColor4`'s own operator rather than field by field.
fn write_color(out: &mut String, prefix: &str, value: [f32; 4]) {
    let [red, green, blue, alpha] = value;
    let _written = writeln!(out, "{prefix}{red} {green} {blue} {alpha}");
}

/// Writes an optional UUID line, omitting it entirely when there is none.
fn write_optional_uuid(out: &mut String, keyword: &str, value: Option<Uuid>) {
    if let Some(id) = value {
        let _written = writeln!(out, "\t{keyword}\t{id}");
    }
}

/// Writes the `permissions` block.
fn write_permissions(out: &mut String, permissions: &LegacyPermissions) {
    out.push_str("\tpermissions 0\n\t{\n");
    let _written = writeln!(out, "\t\tbase_mask\t{:08x}", permissions.base_mask);
    let _written = writeln!(out, "\t\towner_mask\t{:08x}", permissions.owner_mask);
    let _written = writeln!(out, "\t\tgroup_mask\t{:08x}", permissions.group_mask);
    let _written = writeln!(out, "\t\teveryone_mask\t{:08x}", permissions.everyone_mask);
    let _written = writeln!(
        out,
        "\t\tnext_owner_mask\t{:08x}",
        permissions.next_owner_mask
    );
    let _written = writeln!(out, "\t\tcreator_id\t{}", permissions.creator_id);
    let _written = writeln!(out, "\t\towner_id\t{}", permissions.owner_id);
    let _written = writeln!(out, "\t\tlast_owner_id\t{}", permissions.last_owner_id);
    let _written = writeln!(out, "\t\tgroup_id\t{}", permissions.group_id);
    if permissions.group_owned {
        // Written only when true, exactly as the reference does: an absent
        // line and a `0` mean the same thing to its reader.
        out.push_str("\t\tgroup_owned\t1\n");
    }
    out.push_str("\t}\n");
}

/// Writes the `sale_info` block.
fn write_sale_info(out: &mut String, sale_info: &LegacySaleInfo) {
    out.push_str("\tsale_info\t0\n\t{\n");
    let _written = writeln!(out, "\t\tsale_type\t{}", sale_info.sale_type.as_str());
    let _written = writeln!(out, "\t\tsale_price\t{}", sale_info.sale_price);
    out.push_str("\t}\n");
}

/// Writes the `shape` block and the `path` / `profile` blocks inside it.
fn write_shape(out: &mut String, shape: &LegacyShape) {
    out.push_str("\tshape 0\n\t{\n");
    write_path_params(out, &shape.path);
    write_profile_params(out, &shape.profile);
    out.push_str("\t}\n");
}

/// Writes the `path` block.
fn write_path_params(out: &mut String, path: &LegacyPathParams) {
    out.push_str("\t\tpath 0\n\t\t{\n");
    let _written = writeln!(out, "\t\t\tcurve\t{}", path.curve);
    let _written = writeln!(out, "\t\t\tbegin\t{}", path.begin);
    let _written = writeln!(out, "\t\t\tend\t{}", path.end);
    let _written = writeln!(out, "\t\t\tscale_x\t{}", path.scale_x);
    let _written = writeln!(out, "\t\t\tscale_y\t{}", path.scale_y);
    let _written = writeln!(out, "\t\t\tshear_x\t{}", path.shear_x);
    let _written = writeln!(out, "\t\t\tshear_y\t{}", path.shear_y);
    let _written = writeln!(out, "\t\t\ttwist\t{}", path.twist);
    let _written = writeln!(out, "\t\t\ttwist_begin\t{}", path.twist_begin);
    let _written = writeln!(out, "\t\t\tradius_offset\t{}", path.radius_offset);
    let _written = writeln!(out, "\t\t\ttaper_x\t{}", path.taper_x);
    let _written = writeln!(out, "\t\t\ttaper_y\t{}", path.taper_y);
    let _written = writeln!(out, "\t\t\trevolutions\t{}", path.revolutions);
    let _written = writeln!(out, "\t\t\tskew\t{}", path.skew);
    out.push_str("\t\t}\n");
}

/// Writes the `profile` block.
fn write_profile_params(out: &mut String, profile: &LegacyProfileParams) {
    out.push_str("\t\tprofile 0\n\t\t{\n");
    let _written = writeln!(out, "\t\t\tcurve\t{}", profile.curve);
    let _written = writeln!(out, "\t\t\tbegin\t{}", profile.begin);
    let _written = writeln!(out, "\t\t\tend\t{}", profile.end);
    let _written = writeln!(out, "\t\t\thollow\t{}", profile.hollow);
    out.push_str("\t\t}\n");
}

/// Writes the `faces` count and one block per face.
fn write_faces(out: &mut String, faces: &[LegacyFace]) {
    let _written = writeln!(out, "\tfaces\t{}", faces.len());
    for face in faces {
        write_face(out, face);
    }
}

/// Writes one face block.
fn write_face(out: &mut String, face: &LegacyFace) {
    out.push_str("\t{\n");
    let _written = writeln!(out, "\t\timageid\t{}", face.image_id);
    write_color(out, "\t\tcolors\t", face.color);
    let _written = writeln!(out, "\t\tscales\t{}", face.scale_s);
    let _written = writeln!(out, "\t\tscalet\t{}", face.scale_t);
    let _written = writeln!(out, "\t\toffsets\t{}", face.offset_s);
    let _written = writeln!(out, "\t\toffsett\t{}", face.offset_t);
    let _written = writeln!(out, "\t\timagerot\t{}", face.rotation);
    let _written = writeln!(out, "\t\tbump\t{}", face.bump);
    let _written = writeln!(out, "\t\tfullbright\t{}", u8::from(face.fullbright));
    let _written = writeln!(out, "\t\tmedia_flags\t{}", face.media_flags);
    out.push_str("\t}\n");
}

/// Writes the `scratchpad` block, whose body is whatever was read for it.
fn write_scratchpad(out: &mut String, scratchpad: &Scratchpad) {
    let _written = writeln!(out, "\tscratchpad\t{}", scratchpad.count);
    out.push_str("\t{\n");
    for line in &scratchpad.lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("\t}\n");
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use crate::decode::ObjectAssetError;
    use crate::model::{LinkState, ObjectAsset, PrimBlock, PrimPlacement};
    use sl_types::lsl::{Rotation, Vector};
    use uuid::Uuid;

    /// A prim written from the model's own defaults reads back as itself. The
    /// defaults are the interesting case: they are what a fixture that states
    /// only what it cares about will write.
    #[test]
    fn a_default_prim_round_trips() -> Result<(), ObjectAssetError> {
        let prim = PrimBlock {
            task_id: Uuid::from_u128(0x1234),
            name: "Fixture Prim".to_owned(),
            ..PrimBlock::default()
        };
        let asset = ObjectAsset::single(prim);
        let decoded = ObjectAsset::decode(&asset.encode())?;
        assert_eq!(decoded, asset);
        Ok(())
    }

    /// A two-prim linkset round trips with its root and child roles intact —
    /// including the child's offset pair, which is a different set of lines
    /// from the root's motion pair.
    #[test]
    fn a_linkset_round_trips() -> Result<(), ObjectAssetError> {
        let asset = ObjectAsset {
            prims: vec![
                PrimBlock {
                    task_id: Uuid::from_u128(2),
                    name: "Child".to_owned(),
                    link: Some(LinkState::Child),
                    placement: PrimPlacement::Child {
                        position: Vector {
                            x: 0.5,
                            y: -0.25,
                            z: 1.0,
                        },
                        rotation: Rotation {
                            x: 0.0,
                            y: 0.0,
                            // A quarter turn about Z.
                            z: core::f32::consts::FRAC_1_SQRT_2,
                            s: core::f32::consts::FRAC_1_SQRT_2,
                        },
                    },
                    ..PrimBlock::default()
                },
                PrimBlock {
                    task_id: Uuid::from_u128(1),
                    name: "Root".to_owned(),
                    description: Some("a two-prim fixture".to_owned()),
                    link: Some(LinkState::Root),
                    ..PrimBlock::default()
                },
            ],
        };
        let decoded = ObjectAsset::decode(&asset.encode())?;
        assert_eq!(decoded, asset);
        assert_eq!(
            decoded.root().map(|prim| prim.name.clone()),
            Some("Root".to_owned())
        );
        assert_eq!(decoded.children().count(), 1);
        Ok(())
    }

    /// Encoding is deterministic: the same model always produces the same
    /// bytes, which is what lets a grid compare two saves and what lets a
    /// fixture asset have a stable id.
    #[test]
    fn encoding_is_deterministic() {
        let asset = ObjectAsset::single(PrimBlock {
            task_id: Uuid::from_u128(7),
            name: "Twice".to_owned(),
            ..PrimBlock::default()
        });
        assert_eq!(asset.encode(), asset.encode());
    }
}
