//! Decoding an object asset's nested-block text into an [`ObjectAsset`].
//!
//! The grammar is line-oriented. Each prim is a `{'task_id':u…}` header line
//! followed by a brace-delimited block of `\t<keyword>\t<value>` lines, some of
//! which open a nested block of their own (`permissions`, `shape`, `faces`,
//! `sale_info`, `scratchpad`).
//!
//! Two deliberate departures from the reference's own readers
//! (`LLPermissions::importLegacyStream` and its siblings, which is all that
//! survives of this format in the viewer):
//!
//! - **an unknown keyword at prim level is kept**, not warned about and
//!   dropped, so a re-save writes back a field a newer simulator wrote (see
//!   [`PrimBlock::unknown`]);
//! - **a malformed value is an error**, where the reference's `sscanf` leaves
//!   the field at whatever it held before. A silently zeroed position is a prim
//!   in the wrong place with nothing to say so.

use sl_types::lsl::{Rotation, Vector};
use uuid::Uuid;

use crate::model::{
    IDENTITY_ROTATION, LegacyFace, LegacyPathParams, LegacyPermissions, LegacyProfileParams,
    LegacySaleInfo, LegacySaleType, LegacyShape, LinkState, ObjectAsset, PrimBlock, PrimPlacement,
    Scratchpad, UnknownField, ZERO_VECTOR,
};

/// An error decoding an object asset.
#[derive(Debug, thiserror::Error)]
pub enum ObjectAssetError {
    /// The asset is not valid UTF-8.
    #[error("object asset is not valid UTF-8: {source}")]
    NotUtf8 {
        /// The underlying decode error.
        source: core::str::Utf8Error,
    },
    /// The asset ended before a block it had opened was closed.
    #[error("unexpected end of object asset while reading {context}")]
    UnexpectedEnd {
        /// What the decoder was reading when the text ran out.
        context: &'static str,
    },
    /// A structural line was not what the grammar requires here.
    #[error("expected {expected} but found {found:?}")]
    Unexpected {
        /// What the decoder required.
        expected: &'static str,
        /// The line actually found.
        found: String,
    },
    /// A prim header line was not `{'task_id':u<uuid>}`.
    #[error("malformed prim header {line:?}")]
    MalformedHeader {
        /// The offending line.
        line: String,
    },
    /// An integer field could not be parsed.
    #[error("invalid {field} integer {value:?}")]
    InvalidInteger {
        /// The field being read.
        field: &'static str,
        /// The offending text.
        value: String,
    },
    /// A floating-point field could not be parsed.
    #[error("invalid {field} number {value:?}")]
    InvalidNumber {
        /// The field being read.
        field: &'static str,
        /// The offending text.
        value: String,
    },
    /// A UUID field could not be parsed.
    #[error("invalid {field} UUID {value:?}: {source}")]
    InvalidUuid {
        /// The field being read.
        field: &'static str,
        /// The offending text.
        value: String,
        /// The underlying parse error.
        source: uuid::Error,
    },
    /// A field that names one of a fixed set of keywords carried another.
    #[error("unknown {field} keyword {value:?}")]
    UnknownValue {
        /// The field being read.
        field: &'static str,
        /// The offending keyword.
        value: String,
    },
    /// A tuple field (a vector, a rotation, a colour) had the wrong number of
    /// components.
    #[error("{field} wants {expected} components, found {found} in {value:?}")]
    WrongComponentCount {
        /// The field being read.
        field: &'static str,
        /// How many components the field has.
        expected: usize,
        /// How many were present.
        found: usize,
        /// The offending text.
        value: String,
    },
    /// A `name` or `description` line had no `|` terminator, so where the value
    /// ends is unknowable.
    #[error("{field} value {value:?} has no `|` terminator")]
    MissingTerminator {
        /// The field being read.
        field: &'static str,
        /// The offending text.
        value: String,
    },
    /// A keyword inside a nested block is not one that block has.
    #[error("unknown keyword {keyword:?} in the {block} block")]
    UnknownNestedKeyword {
        /// The block being read.
        block: &'static str,
        /// The offending keyword.
        keyword: String,
    },
    /// A prim carried both the root motion pair and the child offset pair, so
    /// which one places it is unknowable.
    #[error("prim states both velocity/angvel and childpos/childrot")]
    ConflictingPlacement,
    /// The `faces` count and the number of face blocks disagree.
    #[error("faces declares {declared} faces but the block holds {found}")]
    FaceCountMismatch {
        /// The count on the `faces` line.
        declared: usize,
        /// How many face blocks were actually read.
        found: usize,
    },
}

impl ObjectAsset {
    /// Decodes an object asset from the bytes a grid serves for it.
    ///
    /// # Errors
    ///
    /// Returns an [`ObjectAssetError`] if the text is not UTF-8, if a block is
    /// unterminated, or if a field's value cannot be read as its type.
    pub fn decode(bytes: &[u8]) -> Result<Self, ObjectAssetError> {
        let text =
            core::str::from_utf8(bytes).map_err(|source| ObjectAssetError::NotUtf8 { source })?;
        Self::decode_str(text)
    }

    /// Decodes an object asset from its text form.
    ///
    /// # Errors
    ///
    /// As [`ObjectAsset::decode`], minus the UTF-8 check.
    pub fn decode_str(text: &str) -> Result<Self, ObjectAssetError> {
        let mut cursor = Cursor::new(text);
        let mut prims = Vec::new();
        while let Some(line) = cursor.next_nonblank() {
            let task_id = parse_header(line)?;
            prims.push(parse_prim(&mut cursor, task_id)?);
        }
        Ok(Self { prims })
    }
}

/// A line cursor over the asset text.
struct Cursor<'a> {
    /// The remaining lines.
    lines: core::iter::Peekable<core::str::Lines<'a>>,
}

impl<'a> Cursor<'a> {
    /// Start a cursor at the beginning of `text`.
    fn new(text: &'a str) -> Self {
        Self {
            lines: text.lines().peekable(),
        }
    }

    /// The next line that holds anything but whitespace, or `None` at the end.
    fn next_nonblank(&mut self) -> Option<&'a str> {
        self.lines.find(|line| !line.trim().is_empty())
    }

    /// The next line verbatim, blank or not.
    fn next_line(&mut self) -> Option<&'a str> {
        self.lines.next()
    }

    /// The next line without consuming it.
    fn peek(&mut self) -> Option<&'a str> {
        self.lines.peek().copied()
    }

    /// Consumes the `{` that opens a block.
    fn open_block(&mut self, context: &'static str) -> Result<(), ObjectAssetError> {
        let line = self
            .next_nonblank()
            .ok_or(ObjectAssetError::UnexpectedEnd { context })?;
        if line.trim() == "{" {
            Ok(())
        } else {
            Err(ObjectAssetError::Unexpected {
                expected: "{",
                found: line.to_owned(),
            })
        }
    }
}

/// The `task_id` a prim's header line names.
fn parse_header(line: &str) -> Result<Uuid, ObjectAssetError> {
    let trimmed = line.trim();
    let inner = trimmed
        .strip_prefix(HEADER_PREFIX)
        .and_then(|rest| rest.strip_suffix('}'))
        .ok_or_else(|| ObjectAssetError::MalformedHeader {
            line: line.to_owned(),
        })?;
    parse_uuid(inner, "task_id")
}

/// The literal that opens a prim's header line. The `u` is LLSD notation's
/// UUID marker: the header is a one-key LLSD map, and the rest of the prim is
/// not LLSD at all.
const HEADER_PREFIX: &str = "{'task_id':u";

/// Reads one prim block, the `{` after its header through the `}` that closes
/// it.
fn parse_prim(cursor: &mut Cursor<'_>, task_id: Uuid) -> Result<PrimBlock, ObjectAssetError> {
    cursor.open_block("a prim block")?;
    let mut prim = PrimBlock {
        task_id,
        ..PrimBlock::default()
    };
    // Which placement pair the prim carried, so a prim carrying both is
    // rejected rather than silently resolved by write order.
    let mut velocity: Option<Vector> = None;
    let mut angular_velocity: Option<Vector> = None;
    let mut child_position: Option<Vector> = None;
    let mut child_rotation: Option<Rotation> = None;
    loop {
        let line = cursor
            .next_nonblank()
            .ok_or(ObjectAssetError::UnexpectedEnd {
                context: "a prim block",
            })?;
        if line.trim() == "}" {
            break;
        }
        let (keyword, value) = split_keyword(line);
        match keyword {
            "name" => prim.name = parse_terminated(value, "name")?,
            "description" => prim.description = Some(parse_terminated(value, "description")?),
            "permissions" => prim.permissions = parse_permissions(cursor)?,
            "local_id" => prim.local_id = parse_u32(value, "local_id")?,
            "total_crc" => prim.total_crc = parse_u32(value, "total_crc")?,
            "type" => prim.pcode = parse_u8(value, "type")?,
            "task_valid" => prim.task_valid = parse_u32(value, "task_valid")?,
            "travel_access" => prim.travel_access = parse_u32(value, "travel_access")?,
            "displayopts" => prim.display_options = parse_u32(value, "displayopts")?,
            "displaytype" => value.trim().clone_into(&mut prim.display_type),
            "pos" => prim.position = parse_vector(value, "pos")?,
            "oldpos" => prim.old_position = parse_vector(value, "oldpos")?,
            "rotation" => prim.rotation = parse_rotation(value, "rotation")?,
            "velocity" => velocity = Some(parse_vector(value, "velocity")?),
            "angvel" => angular_velocity = Some(parse_vector(value, "angvel")?),
            "childpos" => child_position = Some(parse_vector(value, "childpos")?),
            "childrot" => child_rotation = Some(parse_rotation(value, "childrot")?),
            "scale" => prim.scale = parse_vector(value, "scale")?,
            "sit_offset" => prim.sit_offset = parse_vector(value, "sit_offset")?,
            "camera_eye_offset" => {
                prim.camera_eye_offset = parse_vector(value, "camera_eye_offset")?;
            }
            "camera_at_offset" => {
                prim.camera_at_offset = parse_vector(value, "camera_at_offset")?;
            }
            "sit_quat" => prim.sit_rotation = parse_rotation(value, "sit_quat")?,
            "sit_hint" => prim.sit_hint = parse_u32(value, "sit_hint")?,
            "state" => prim.state = parse_u8(value, "state")?,
            "material" => prim.material = parse_u8(value, "material")?,
            "soundid" => prim.sound.sound_id = parse_uuid(value.trim(), "soundid")?,
            "soundgain" => prim.sound.gain = parse_f32(value, "soundgain")?,
            "soundradius" => prim.sound.radius = parse_f32(value, "soundradius")?,
            "soundflags" => prim.sound.flags = parse_u8(value, "soundflags")?,
            "textcolor" => prim.text_color = parse_color(value, "textcolor")?,
            "selected" => prim.selected = parse_flag(value, "selected")?,
            "selector" => prim.selector = parse_uuid(value.trim(), "selector")?,
            "usephysics" => prim.flags.use_physics = parse_flag(value, "usephysics")?,
            "rotate_x" => prim.flags.rotate_x = parse_flag(value, "rotate_x")?,
            "rotate_y" => prim.flags.rotate_y = parse_flag(value, "rotate_y")?,
            "rotate_z" => prim.flags.rotate_z = parse_flag(value, "rotate_z")?,
            "phantom" => prim.flags.phantom = parse_flag(value, "phantom")?,
            "remote_script_access_pin" => {
                prim.remote_script_access_pin = parse_u32(value, "remote_script_access_pin")?;
            }
            "volume_detect" => prim.flags.volume_detect = parse_flag(value, "volume_detect")?,
            "block_grabs" => prim.flags.block_grabs = parse_flag(value, "block_grabs")?,
            "die_at_edge" => prim.flags.die_at_edge = parse_flag(value, "die_at_edge")?,
            "return_at_edge" => prim.flags.return_at_edge = parse_flag(value, "return_at_edge")?,
            "temporary" => prim.flags.temporary = parse_flag(value, "temporary")?,
            "sandbox" => prim.flags.sandbox = parse_flag(value, "sandbox")?,
            "sandboxhome" => prim.sandbox_home = parse_vector(value, "sandboxhome")?,
            "shape" => prim.shape = parse_shape(cursor)?,
            "faces" => prim.faces = parse_faces(cursor, value)?,
            "ps_next_crc" => prim.bookkeeping.ps_next_crc = parse_u32(value, "ps_next_crc")?,
            "gpw_bias" => prim.bookkeeping.gpw_bias = parse_f32(value, "gpw_bias")?,
            "ip" => prim.bookkeeping.ip = parse_u32(value, "ip")?,
            "complete" => prim.bookkeeping.complete = parse_truth(value, "complete")?,
            "delay" => prim.bookkeeping.delay = parse_u32(value, "delay")?,
            "nextstart" => prim.bookkeeping.next_start = parse_u64(value, "nextstart")?,
            "birthtime" => prim.bookkeeping.birth_time = parse_u64(value, "birthtime")?,
            "reztime" => prim.bookkeeping.rez_time = parse_u64(value, "reztime")?,
            "parceltime" => prim.bookkeeping.parcel_time = parse_u64(value, "parceltime")?,
            "tax_rate" => prim.bookkeeping.tax_rate = parse_f32(value, "tax_rate")?,
            "namevalue" => prim.name_values.push(value.to_owned()),
            "scratchpad" => prim.scratchpad = parse_scratchpad(cursor, value)?,
            "sale_info" => prim.sale_info = parse_sale_info(cursor)?,
            "orig_asset_id" => {
                prim.orig_asset_id = Some(parse_uuid(value.trim(), "orig_asset_id")?);
            }
            "orig_item_id" => prim.orig_item_id = Some(parse_uuid(value.trim(), "orig_item_id")?),
            "from_task_id" => prim.from_task_id = Some(parse_uuid(value.trim(), "from_task_id")?),
            "correct_family_id" => {
                prim.correct_family_id = parse_uuid(value.trim(), "correct_family_id")?;
            }
            "has_rezzed" => prim.has_rezzed = parse_flag(value, "has_rezzed")?,
            "pre_link_base_mask" => {
                prim.pre_link_base_mask = parse_hex_u32(value, "pre_link_base_mask")?;
            }
            "linked" => {
                prim.link = Some(LinkState::parse(value.trim()).ok_or_else(|| {
                    ObjectAssetError::UnknownValue {
                        field: "linked",
                        value: value.trim().to_owned(),
                    }
                })?);
            }
            "default_pay_price" => {
                prim.default_pay_price = parse_pay_price(value, "default_pay_price")?;
            }
            _unknown => prim.unknown.push(UnknownField {
                keyword: keyword.to_owned(),
                value: value.to_owned(),
            }),
        }
    }
    prim.placement = placement_from(velocity, angular_velocity, child_position, child_rotation)?;
    Ok(prim)
}

/// Resolves the two mutually exclusive placement pairs into one
/// [`PrimPlacement`], rejecting a prim that stated both.
fn placement_from(
    velocity: Option<Vector>,
    angular_velocity: Option<Vector>,
    child_position: Option<Vector>,
    child_rotation: Option<Rotation>,
) -> Result<PrimPlacement, ObjectAssetError> {
    let is_child = child_position.is_some() || child_rotation.is_some();
    let is_free = velocity.is_some() || angular_velocity.is_some();
    if is_child && is_free {
        return Err(ObjectAssetError::ConflictingPlacement);
    }
    if is_child {
        Ok(PrimPlacement::Child {
            position: child_position.unwrap_or(ZERO_VECTOR),
            rotation: child_rotation.unwrap_or(IDENTITY_ROTATION),
        })
    } else {
        Ok(PrimPlacement::Free {
            velocity: velocity.unwrap_or(ZERO_VECTOR),
            angular_velocity: angular_velocity.unwrap_or(ZERO_VECTOR),
        })
    }
}

/// Splits a field line into its keyword and everything after the single
/// separator (a tab or a space) that follows it. Leading indentation tabs are
/// dropped first; the value is returned verbatim, because a `name` may contain
/// spaces and a `namevalue` is only meaningful whole.
fn split_keyword(line: &str) -> (&str, &str) {
    let trimmed = line.trim_start_matches('\t');
    match trimmed.find(['\t', ' ']) {
        Some(index) => (
            trimmed.get(..index).unwrap_or(""),
            trimmed.get(index.saturating_add(1)..).unwrap_or(""),
        ),
        None => (trimmed, ""),
    }
}

/// The value of a `|`-terminated string field (`name`, `description`).
fn parse_terminated(value: &str, field: &'static str) -> Result<String, ObjectAssetError> {
    let end = value
        .find('|')
        .ok_or_else(|| ObjectAssetError::MissingTerminator {
            field,
            value: value.to_owned(),
        })?;
    Ok(value.get(..end).unwrap_or("").to_owned())
}

/// A decimal `u32` field.
fn parse_u32(value: &str, field: &'static str) -> Result<u32, ObjectAssetError> {
    value
        .trim()
        .parse()
        .map_err(|_parse| ObjectAssetError::InvalidInteger {
            field,
            value: value.trim().to_owned(),
        })
}

/// A decimal `u64` field.
fn parse_u64(value: &str, field: &'static str) -> Result<u64, ObjectAssetError> {
    value
        .trim()
        .parse()
        .map_err(|_parse| ObjectAssetError::InvalidInteger {
            field,
            value: value.trim().to_owned(),
        })
}

/// A decimal `u8` field (an object class, a state byte, a material).
fn parse_u8(value: &str, field: &'static str) -> Result<u8, ObjectAssetError> {
    value
        .trim()
        .parse()
        .map_err(|_parse| ObjectAssetError::InvalidInteger {
            field,
            value: value.trim().to_owned(),
        })
}

/// A decimal `i32` field.
fn parse_i32(value: &str, field: &'static str) -> Result<i32, ObjectAssetError> {
    value
        .trim()
        .parse()
        .map_err(|_parse| ObjectAssetError::InvalidInteger {
            field,
            value: value.trim().to_owned(),
        })
}

/// A hexadecimal mask field (`base_mask` and its siblings).
fn parse_hex_u32(value: &str, field: &'static str) -> Result<u32, ObjectAssetError> {
    u32::from_str_radix(value.trim(), 16).map_err(|_parse| ObjectAssetError::InvalidInteger {
        field,
        value: value.trim().to_owned(),
    })
}

/// A floating-point field.
fn parse_f32(value: &str, field: &'static str) -> Result<f32, ObjectAssetError> {
    value
        .trim()
        .parse()
        .map_err(|_parse| ObjectAssetError::InvalidNumber {
            field,
            value: value.trim().to_owned(),
        })
}

/// A UUID field.
fn parse_uuid(value: &str, field: &'static str) -> Result<Uuid, ObjectAssetError> {
    Uuid::parse_str(value.trim()).map_err(|source| ObjectAssetError::InvalidUuid {
        field,
        value: value.trim().to_owned(),
        source,
    })
}

/// A `0` / `1` boolean field.
fn parse_flag(value: &str, field: &'static str) -> Result<bool, ObjectAssetError> {
    Ok(parse_u32(value, field)? != 0)
}

/// A `TRUE` / `FALSE` boolean field — the one place the format spells a boolean
/// out (`complete`).
fn parse_truth(value: &str, field: &'static str) -> Result<bool, ObjectAssetError> {
    match value.trim() {
        "TRUE" => Ok(true),
        "FALSE" => Ok(false),
        other => Err(ObjectAssetError::UnknownValue {
            field,
            value: other.to_owned(),
        }),
    }
}

/// Splits a whitespace-separated tuple into exactly `expected` components.
fn parse_components<'a>(
    value: &'a str,
    field: &'static str,
    expected: usize,
) -> Result<Vec<&'a str>, ObjectAssetError> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() == expected {
        Ok(parts)
    } else {
        Err(ObjectAssetError::WrongComponentCount {
            field,
            expected,
            found: parts.len(),
            value: value.trim().to_owned(),
        })
    }
}

/// The `n`th component of an already length-checked tuple.
fn component(parts: &[&str], index: usize, field: &'static str) -> Result<f32, ObjectAssetError> {
    let text = parts.get(index).copied().unwrap_or("");
    parse_f32(text, field)
}

/// A three-component vector field.
fn parse_vector(value: &str, field: &'static str) -> Result<Vector, ObjectAssetError> {
    let parts = parse_components(value, field, 3)?;
    Ok(Vector {
        x: component(&parts, 0, field)?,
        y: component(&parts, 1, field)?,
        z: component(&parts, 2, field)?,
    })
}

/// A four-component rotation field, in the `x y z s` order the text writes.
fn parse_rotation(value: &str, field: &'static str) -> Result<Rotation, ObjectAssetError> {
    let parts = parse_components(value, field, 4)?;
    Ok(Rotation {
        x: component(&parts, 0, field)?,
        y: component(&parts, 1, field)?,
        z: component(&parts, 2, field)?,
        s: component(&parts, 3, field)?,
    })
}

/// A four-component RGBA colour field.
fn parse_color(value: &str, field: &'static str) -> Result<[f32; 4], ObjectAssetError> {
    let parts = parse_components(value, field, 4)?;
    Ok([
        component(&parts, 0, field)?,
        component(&parts, 1, field)?,
        component(&parts, 2, field)?,
        component(&parts, 3, field)?,
    ])
}

/// The five pay-button amounts.
fn parse_pay_price(value: &str, field: &'static str) -> Result<[i32; 5], ObjectAssetError> {
    let parts = parse_components(value, field, 5)?;
    let amount = |index: usize| -> Result<i32, ObjectAssetError> {
        parse_i32(parts.get(index).copied().unwrap_or(""), field)
    };
    Ok([amount(0)?, amount(1)?, amount(2)?, amount(3)?, amount(4)?])
}

/// Reads a nested block's field lines, handing each `(keyword, value)` pair to
/// `field` until the closing brace.
fn read_block<F>(
    cursor: &mut Cursor<'_>,
    context: &'static str,
    mut field: F,
) -> Result<(), ObjectAssetError>
where
    F: FnMut(&str, &str) -> Result<(), ObjectAssetError>,
{
    cursor.open_block(context)?;
    loop {
        let line = cursor
            .next_nonblank()
            .ok_or(ObjectAssetError::UnexpectedEnd { context })?;
        if line.trim() == "}" {
            return Ok(());
        }
        let (keyword, value) = split_keyword(line);
        field(keyword, value)?;
    }
}

/// The `permissions` block.
fn parse_permissions(cursor: &mut Cursor<'_>) -> Result<LegacyPermissions, ObjectAssetError> {
    let mut permissions = LegacyPermissions::default();
    read_block(cursor, "the permissions block", |keyword, value| {
        match keyword {
            // The reference's own reader still accepts `creator_mask` as the
            // pre-2004 spelling of `base_mask`, so a very old asset reads.
            "base_mask" | "creator_mask" => {
                permissions.base_mask = parse_hex_u32(value, "base_mask")?;
            }
            "owner_mask" => permissions.owner_mask = parse_hex_u32(value, "owner_mask")?,
            "group_mask" => permissions.group_mask = parse_hex_u32(value, "group_mask")?,
            "everyone_mask" => permissions.everyone_mask = parse_hex_u32(value, "everyone_mask")?,
            "next_owner_mask" => {
                permissions.next_owner_mask = parse_hex_u32(value, "next_owner_mask")?;
            }
            "creator_id" => permissions.creator_id = parse_uuid(value, "creator_id")?,
            "owner_id" => permissions.owner_id = parse_uuid(value, "owner_id")?,
            "last_owner_id" => permissions.last_owner_id = parse_uuid(value, "last_owner_id")?,
            "group_id" => permissions.group_id = parse_uuid(value, "group_id")?,
            "group_owned" => permissions.group_owned = parse_flag(value, "group_owned")?,
            unknown => {
                return Err(ObjectAssetError::UnknownNestedKeyword {
                    block: "permissions",
                    keyword: unknown.to_owned(),
                });
            }
        }
        Ok(())
    })?;
    Ok(permissions)
}

/// The `sale_info` block.
fn parse_sale_info(cursor: &mut Cursor<'_>) -> Result<LegacySaleInfo, ObjectAssetError> {
    let mut sale_info = LegacySaleInfo::default();
    read_block(cursor, "the sale_info block", |keyword, value| {
        match keyword {
            "sale_type" => {
                sale_info.sale_type = LegacySaleType::parse(value.trim()).ok_or_else(|| {
                    ObjectAssetError::UnknownValue {
                        field: "sale_type",
                        value: value.trim().to_owned(),
                    }
                })?;
            }
            "sale_price" => sale_info.sale_price = parse_i32(value, "sale_price")?,
            // The reference's reader accepts a deprecated `perm_mask` here and
            // hands it back to the *item* rather than the sale info; an object
            // asset has no item to hand it to, so it is ignored by name rather
            // than rejected as unknown.
            "perm_mask" => {}
            unknown => {
                return Err(ObjectAssetError::UnknownNestedKeyword {
                    block: "sale_info",
                    keyword: unknown.to_owned(),
                });
            }
        }
        Ok(())
    })?;
    Ok(sale_info)
}

/// The `shape` block, which holds a `path` block and a `profile` block.
fn parse_shape(cursor: &mut Cursor<'_>) -> Result<LegacyShape, ObjectAssetError> {
    let mut shape = LegacyShape::default();
    cursor.open_block("the shape block")?;
    loop {
        let line = cursor
            .next_nonblank()
            .ok_or(ObjectAssetError::UnexpectedEnd {
                context: "the shape block",
            })?;
        if line.trim() == "}" {
            return Ok(shape);
        }
        let (keyword, _value) = split_keyword(line);
        match keyword {
            "path" => shape.path = parse_path_params(cursor)?,
            "profile" => shape.profile = parse_profile_params(cursor)?,
            unknown => {
                return Err(ObjectAssetError::UnknownNestedKeyword {
                    block: "shape",
                    keyword: unknown.to_owned(),
                });
            }
        }
    }
}

/// The `path` block of a shape.
fn parse_path_params(cursor: &mut Cursor<'_>) -> Result<LegacyPathParams, ObjectAssetError> {
    let mut path = LegacyPathParams::default();
    read_block(cursor, "the path block", |keyword, value| {
        match keyword {
            "curve" => path.curve = parse_u8(value, "path curve")?,
            "begin" => path.begin = parse_f32(value, "path begin")?,
            "end" => path.end = parse_f32(value, "path end")?,
            "scale_x" => path.scale_x = parse_f32(value, "path scale_x")?,
            "scale_y" => path.scale_y = parse_f32(value, "path scale_y")?,
            "shear_x" => path.shear_x = parse_f32(value, "path shear_x")?,
            "shear_y" => path.shear_y = parse_f32(value, "path shear_y")?,
            "twist" => path.twist = parse_f32(value, "path twist")?,
            "twist_begin" => path.twist_begin = parse_f32(value, "path twist_begin")?,
            "radius_offset" => path.radius_offset = parse_f32(value, "path radius_offset")?,
            "taper_x" => path.taper_x = parse_f32(value, "path taper_x")?,
            "taper_y" => path.taper_y = parse_f32(value, "path taper_y")?,
            "revolutions" => path.revolutions = parse_f32(value, "path revolutions")?,
            "skew" => path.skew = parse_f32(value, "path skew")?,
            unknown => {
                return Err(ObjectAssetError::UnknownNestedKeyword {
                    block: "path",
                    keyword: unknown.to_owned(),
                });
            }
        }
        Ok(())
    })?;
    Ok(path)
}

/// The `profile` block of a shape.
fn parse_profile_params(cursor: &mut Cursor<'_>) -> Result<LegacyProfileParams, ObjectAssetError> {
    let mut profile = LegacyProfileParams::default();
    read_block(cursor, "the profile block", |keyword, value| {
        match keyword {
            "curve" => profile.curve = parse_u8(value, "profile curve")?,
            "begin" => profile.begin = parse_f32(value, "profile begin")?,
            "end" => profile.end = parse_f32(value, "profile end")?,
            "hollow" => profile.hollow = parse_f32(value, "profile hollow")?,
            unknown => {
                return Err(ObjectAssetError::UnknownNestedKeyword {
                    block: "profile",
                    keyword: unknown.to_owned(),
                });
            }
        }
        Ok(())
    })?;
    Ok(profile)
}

/// The `faces` list: a count, then that many face blocks.
fn parse_faces(cursor: &mut Cursor<'_>, count: &str) -> Result<Vec<LegacyFace>, ObjectAssetError> {
    let declared = parse_u32(count, "faces")?;
    let declared = usize::try_from(declared).unwrap_or(usize::MAX);
    let mut faces = Vec::new();
    while faces.len() < declared {
        // A truncated list is an error rather than a short read: the face count
        // and the prim's shape have to agree for anything downstream to index
        // a face at all.
        if cursor.peek().is_none() {
            return Err(ObjectAssetError::FaceCountMismatch {
                declared,
                found: faces.len(),
            });
        }
        faces.push(parse_face(cursor)?);
    }
    Ok(faces)
}

/// One face block of the `faces` list.
fn parse_face(cursor: &mut Cursor<'_>) -> Result<LegacyFace, ObjectAssetError> {
    let mut face = LegacyFace::default();
    read_block(cursor, "a face block", |keyword, value| {
        match keyword {
            "imageid" => face.image_id = parse_uuid(value, "imageid")?,
            "colors" => face.color = parse_color(value, "colors")?,
            "scales" => face.scale_s = parse_f32(value, "scales")?,
            "scalet" => face.scale_t = parse_f32(value, "scalet")?,
            "offsets" => face.offset_s = parse_f32(value, "offsets")?,
            "offsett" => face.offset_t = parse_f32(value, "offsett")?,
            "imagerot" => face.rotation = parse_f32(value, "imagerot")?,
            "bump" => face.bump = parse_u8(value, "bump")?,
            "fullbright" => face.fullbright = parse_flag(value, "fullbright")?,
            "media_flags" => face.media_flags = parse_u8(value, "media_flags")?,
            unknown => {
                return Err(ObjectAssetError::UnknownNestedKeyword {
                    block: "face",
                    keyword: unknown.to_owned(),
                });
            }
        }
        Ok(())
    })?;
    Ok(face)
}

/// The `scratchpad` block, kept verbatim.
fn parse_scratchpad(cursor: &mut Cursor<'_>, count: &str) -> Result<Scratchpad, ObjectAssetError> {
    let count = parse_u32(count, "scratchpad")?;
    cursor.open_block("the scratchpad block")?;
    let mut lines = Vec::new();
    loop {
        let line = cursor.next_line().ok_or(ObjectAssetError::UnexpectedEnd {
            context: "the scratchpad block",
        })?;
        if line.trim() == "}" {
            return Ok(Scratchpad { count, lines });
        }
        lines.push(line.to_owned());
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::{ObjectAsset, ObjectAssetError, split_keyword};

    /// The keyword split has to survive both separators the format uses: a tab
    /// after most keywords, a space after the block openers, and the stray
    /// trailing space the reference writes after `linked`.
    #[test]
    fn keywords_split_on_either_separator() {
        assert_eq!(split_keyword("\tname\tObject|"), ("name", "Object|"));
        assert_eq!(split_keyword("\tpermissions 0"), ("permissions", "0"));
        assert_eq!(split_keyword("\tlinked \tchild"), ("linked", "\tchild"));
        assert_eq!(split_keyword("\t\tcurve\t16"), ("curve", "16"));
        assert_eq!(split_keyword("\thas_rezzed\t0"), ("has_rezzed", "0"));
    }

    /// An empty asset is not an error: a grid that serves zero prims serves an
    /// object with no prims, and the decoder says so rather than inventing one.
    #[test]
    fn an_empty_asset_decodes_to_no_prims() -> Result<(), ObjectAssetError> {
        assert_eq!(ObjectAsset::decode(b"")?.prims.len(), 0);
        assert_eq!(ObjectAsset::decode(b"\n\n")?.prims.len(), 0);
        Ok(())
    }

    /// A prim that states both placement pairs cannot be placed, and saying so
    /// beats picking whichever the write order happened to leave.
    #[test]
    fn both_placement_pairs_is_an_error() {
        let text = concat!(
            "{'task_id':u00000000-0000-0000-0000-000000000001}\n",
            "{\n",
            "\tvelocity\t0\t0\t0\n",
            "\tchildpos\t1\t2\t3\n",
            "}\n",
        );
        let error = ObjectAsset::decode_str(text).err().map(|e| e.to_string());
        assert_eq!(
            error,
            Some("prim states both velocity/angvel and childpos/childrot".to_owned())
        );
    }
}
