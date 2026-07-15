//! Decoding a Linden-text notecard byte stream into a [`Notecard`].
//!
//! The parser mirrors Firestorm's `LLNotecard::importStream` /
//! `LLInventoryItem::importLegacyStream`: a line-oriented walk over the
//! container and the embedded-item chunks, tolerant of the leading indentation
//! whitespace the simulator writes, followed by a fixed-length read of the raw
//! text body.

use crate::item::{AssetIdEncoding, EmbeddedItem, InventoryItem, Permissions, SaleInfo, xor_magic};
use crate::types::{AssetType, InventoryType, PermissionMask, SaleType};
use crate::{Notecard, NotecardVersion, embedded_char};
use sl_types::key::{Key, NULL_KEY};
use uuid::Uuid;

/// An error decoding a notecard byte stream.
#[derive(Debug, thiserror::Error)]
pub enum NotecardError {
    /// The stream ended before a required token was read.
    #[error("unexpected end of notecard data while reading {context}")]
    UnexpectedEof {
        /// What the decoder was looking for when the stream ran out.
        context: &'static str,
    },
    /// A structural token did not match what the container requires here.
    #[error("expected {expected:?} but found {found:?}")]
    Unexpected {
        /// The literal or prefix the decoder expected.
        expected: &'static str,
        /// The line actually found.
        found: String,
    },
    /// An integer field could not be parsed.
    #[error("invalid {field} integer {value:?}")]
    InvalidInteger {
        /// The field being parsed.
        field: &'static str,
        /// The offending text.
        value: String,
    },
    /// A UUID field could not be parsed.
    #[error("invalid {field} UUID {value:?}: {source}")]
    InvalidUuid {
        /// The field being parsed.
        field: &'static str,
        /// The offending text.
        value: String,
        /// The underlying parse error.
        source: uuid::Error,
    },
    /// The container version is neither 1 nor 2.
    #[error("unsupported Linden text version {0}")]
    UnsupportedVersion(u32),
    /// A line was not valid UTF-8.
    #[error("notecard line is not valid UTF-8: {source}")]
    InvalidLine {
        /// The underlying decode error.
        source: std::str::Utf8Error,
    },
    /// The version 2 text body was not valid UTF-8.
    #[error("notecard text is not valid UTF-8: {source}")]
    InvalidText {
        /// The underlying decode error.
        source: std::str::Utf8Error,
    },
    /// The declared text length exceeds the bytes actually present.
    #[error("declared text length {declared} exceeds the {available} bytes remaining")]
    TextLengthOverflow {
        /// The `Text length` field value.
        declared: usize,
        /// The bytes left in the stream.
        available: usize,
    },
}

/// A byte cursor that yields lines and fixed-length spans without any manual
/// index arithmetic escaping into the parser.
struct Cursor<'a> {
    /// The full stream being decoded.
    data: &'a [u8],
    /// The read position within [`data`](Cursor::data).
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// Start a cursor at the beginning of `data`.
    const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// The next line (without its trailing `\n`), or `None` at end of stream.
    fn next_line(&mut self) -> Option<&'a [u8]> {
        let rest = self.data.get(self.pos..)?;
        if rest.is_empty() {
            return None;
        }
        match rest.iter().position(|&byte| byte == b'\n') {
            Some(index) => {
                let line = rest.get(..index)?;
                self.pos = self.pos.saturating_add(index).saturating_add(1);
                Some(line)
            }
            None => {
                self.pos = self.data.len();
                Some(rest)
            }
        }
    }

    /// The next `count` raw bytes, or `None` if fewer than `count` remain.
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(count)?;
        let slice = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    /// The number of bytes still unread.
    const fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }
}

/// The next non-blank line, trimmed of surrounding whitespace.
fn next_nonblank<'a>(
    cursor: &mut Cursor<'a>,
    context: &'static str,
) -> Result<&'a str, NotecardError> {
    loop {
        let line = cursor
            .next_line()
            .ok_or(NotecardError::UnexpectedEof { context })?;
        let text =
            std::str::from_utf8(line).map_err(|source| NotecardError::InvalidLine { source })?;
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }
}

/// Consume a line that must equal `expected`.
fn expect_literal(
    cursor: &mut Cursor<'_>,
    expected: &'static str,
    context: &'static str,
) -> Result<(), NotecardError> {
    let line = next_nonblank(cursor, context)?;
    if line == expected {
        Ok(())
    } else {
        Err(NotecardError::Unexpected {
            expected,
            found: line.to_owned(),
        })
    }
}

/// Strip a required `prefix` off `line`, returning the remainder.
fn expect_prefix<'a>(line: &'a str, prefix: &'static str) -> Result<&'a str, NotecardError> {
    line.strip_prefix(prefix)
        .ok_or_else(|| NotecardError::Unexpected {
            expected: prefix,
            found: line.to_owned(),
        })
}

/// The keyword (first whitespace-delimited token) of a field line.
fn keyword_of(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or("")
}

/// The value (second whitespace-delimited token) of a simple field line.
fn value_of(line: &str) -> &str {
    line.split_whitespace().nth(1).unwrap_or("")
}

/// Parse a `u32` field.
fn parse_u32(value: &str, field: &'static str) -> Result<u32, NotecardError> {
    value
        .parse()
        .map_err(|_ignored| NotecardError::InvalidInteger {
            field,
            value: value.to_owned(),
        })
}

/// Parse a `usize` field.
fn parse_usize(value: &str, field: &'static str) -> Result<usize, NotecardError> {
    value
        .parse()
        .map_err(|_ignored| NotecardError::InvalidInteger {
            field,
            value: value.to_owned(),
        })
}

/// Parse an `i64` field.
fn parse_i64(value: &str, field: &'static str) -> Result<i64, NotecardError> {
    value
        .parse()
        .map_err(|_ignored| NotecardError::InvalidInteger {
            field,
            value: value.to_owned(),
        })
}

/// Parse an `i32` field.
fn parse_i32(value: &str, field: &'static str) -> Result<i32, NotecardError> {
    value
        .parse()
        .map_err(|_ignored| NotecardError::InvalidInteger {
            field,
            value: value.to_owned(),
        })
}

/// Parse a hexadecimal `u32` field (a permission mask or the flags bitfield).
fn parse_hex_u32(value: &str, field: &'static str) -> Result<u32, NotecardError> {
    u32::from_str_radix(value, 16).map_err(|_ignored| NotecardError::InvalidInteger {
        field,
        value: value.to_owned(),
    })
}

/// Parse a [`Key`] field.
fn parse_key(value: &str, field: &'static str) -> Result<Key, NotecardError> {
    Uuid::parse_str(value)
        .map(Key)
        .map_err(|source| NotecardError::InvalidUuid {
            field,
            value: value.to_owned(),
            source,
        })
}

/// The value of a tab-then-`|`-terminated field (`name`, `desc`, `metadata`),
/// i.e. everything after the first tab up to the first `|`.
fn tabbed_value(line: &str) -> &str {
    let rest = line.split_once('\t').map_or("", |(_keyword, value)| value);
    rest.split_once('|').map_or(rest, |(value, _rest)| value)
}

/// Parse a permissions chunk (`permissions 0 { ... }`), whose opening `{` has
/// not yet been consumed.
fn parse_permissions(cursor: &mut Cursor<'_>) -> Result<Permissions, NotecardError> {
    let mut permissions = Permissions::default();
    loop {
        let line = next_nonblank(cursor, "permissions")?;
        if line == "{" {
            continue;
        }
        if line == "}" {
            break;
        }
        let keyword = keyword_of(line);
        let value = value_of(line);
        match keyword {
            "base_mask" | "creator_mask" => {
                permissions.base_mask = PermissionMask(parse_hex_u32(value, "base_mask")?);
            }
            "owner_mask" => {
                permissions.owner_mask = PermissionMask(parse_hex_u32(value, "owner_mask")?);
            }
            "group_mask" => {
                permissions.group_mask = PermissionMask(parse_hex_u32(value, "group_mask")?);
            }
            "everyone_mask" => {
                permissions.everyone_mask = PermissionMask(parse_hex_u32(value, "everyone_mask")?);
            }
            "next_owner_mask" => {
                permissions.next_owner_mask =
                    PermissionMask(parse_hex_u32(value, "next_owner_mask")?);
            }
            "creator_id" => permissions.creator_id = parse_key(value, "creator_id")?,
            "owner_id" => permissions.owner_id = parse_key(value, "owner_id")?,
            "last_owner_id" => permissions.last_owner_id = parse_key(value, "last_owner_id")?,
            "group_id" => permissions.group_id = parse_key(value, "group_id")?,
            "group_owned" => permissions.group_owned = parse_i32(value, "group_owned")? != 0,
            _other => {}
        }
    }
    Ok(permissions)
}

/// Parse a sale-info chunk (`sale_info 0 { ... }`), whose opening `{` has not
/// yet been consumed.
fn parse_sale_info(cursor: &mut Cursor<'_>) -> Result<SaleInfo, NotecardError> {
    let mut sale_info = SaleInfo::default();
    loop {
        let line = next_nonblank(cursor, "sale_info")?;
        if line == "{" {
            continue;
        }
        if line == "}" {
            break;
        }
        let keyword = keyword_of(line);
        let value = value_of(line);
        match keyword {
            "sale_type" => sale_info.sale_type = SaleType::from_type_name(value),
            "sale_price" => sale_info.sale_price = parse_i32(value, "sale_price")?,
            _other => {}
        }
    }
    Ok(sale_info)
}

/// Parse the legacy inventory-item chunk that follows an `inv_item 0` line,
/// starting at the item's opening `{`.
fn parse_item(cursor: &mut Cursor<'_>) -> Result<InventoryItem, NotecardError> {
    let mut item_id = NULL_KEY;
    let mut parent_id = NULL_KEY;
    let mut permissions = Permissions::default();
    let mut metadata = None;
    let mut asset_id = NULL_KEY;
    let mut asset_id_encoding = AssetIdEncoding::Plain;
    let mut asset_type = AssetType::Notecard;
    let mut inventory_type = InventoryType::None;
    let mut flags = 0u32;
    let mut sale_info = SaleInfo::default();
    let mut name = String::new();
    let mut description = String::new();
    let mut creation_date = 0i64;
    let mut unknown_fields = Vec::new();

    loop {
        let line = next_nonblank(cursor, "inventory item")?;
        if line == "{" {
            continue;
        }
        if line == "}" {
            break;
        }
        let keyword = keyword_of(line);
        let value = value_of(line);
        match keyword {
            "item_id" => item_id = parse_key(value, "item_id")?,
            "parent_id" => parent_id = parse_key(value, "parent_id")?,
            "permissions" => permissions = parse_permissions(cursor)?,
            "sale_info" => sale_info = parse_sale_info(cursor)?,
            "metadata" => metadata = Some(tabbed_value(line).to_owned()),
            "asset_id" => {
                asset_id = parse_key(value, "asset_id")?;
                asset_id_encoding = AssetIdEncoding::Plain;
            }
            "shadow_id" => {
                asset_id = xor_magic(parse_key(value, "shadow_id")?);
                asset_id_encoding = AssetIdEncoding::Shadow;
            }
            "type" => asset_type = AssetType::from_type_name(value),
            "inv_type" => inventory_type = InventoryType::from_type_name(value),
            "flags" => flags = parse_hex_u32(value, "flags")?,
            "name" => tabbed_value(line).clone_into(&mut name),
            "desc" => tabbed_value(line).clone_into(&mut description),
            "creation_date" => creation_date = parse_i64(value, "creation_date")?,
            _other => unknown_fields.push(line.to_owned()),
        }
    }

    Ok(InventoryItem {
        item_id,
        parent_id,
        permissions,
        metadata,
        asset_id,
        asset_id_encoding,
        asset_type,
        inventory_type,
        flags,
        sale_info,
        name,
        description,
        creation_date,
        unknown_fields,
    })
}

/// Parse the `LLEmbeddedItems` chunk (header, count, and each `{ ext char
/// index / inv_item / item }` entry), whose header line has not yet been read.
fn parse_embedded_items(
    cursor: &mut Cursor<'_>,
) -> Result<(u32, Vec<EmbeddedItem>), NotecardError> {
    let header = next_nonblank(cursor, "LLEmbeddedItems header")?;
    let version = parse_u32(
        expect_prefix(header, "LLEmbeddedItems version ")?.trim(),
        "LLEmbeddedItems version",
    )?;
    expect_literal(cursor, "{", "LLEmbeddedItems open brace")?;

    let count_line = next_nonblank(cursor, "embedded item count")?;
    let count = parse_usize(expect_prefix(count_line, "count ")?.trim(), "count")?;

    let mut items = Vec::with_capacity(count);
    for _index in 0..count {
        expect_literal(cursor, "{", "embedded item entry open brace")?;
        let ext_line = next_nonblank(cursor, "ext char index")?;
        let char_index = parse_u32(
            expect_prefix(ext_line, "ext char index ")?.trim(),
            "ext char index",
        )?;
        let inv_line = next_nonblank(cursor, "inv_item marker")?;
        if keyword_of(inv_line) != "inv_item" {
            return Err(NotecardError::Unexpected {
                expected: "inv_item",
                found: inv_line.to_owned(),
            });
        }
        let item = parse_item(cursor)?;
        expect_literal(cursor, "}", "embedded item entry close brace")?;
        items.push(EmbeddedItem { char_index, item });
    }

    expect_literal(cursor, "}", "LLEmbeddedItems close brace")?;
    Ok((version, items))
}

/// Decode the raw text body, mapping the version's embedded markers to the
/// uniform `FIRST_EMBEDDED_CHAR + index` code points.
fn decode_text(bytes: &[u8], version: NotecardVersion) -> Result<String, NotecardError> {
    match version {
        NotecardVersion::V2 => std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|source| NotecardError::InvalidText { source }),
        NotecardVersion::V1 => {
            let mut text = String::with_capacity(bytes.len());
            for &byte in bytes {
                if byte & 0x80 == 0 {
                    text.push(char::from(byte));
                } else if let Some(character) = embedded_char(u32::from(byte & 0x7f)) {
                    text.push(character);
                }
            }
            Ok(text)
        }
    }
}

impl Notecard {
    /// Decode a Linden-text notecard byte stream.
    ///
    /// # Errors
    ///
    /// Returns [`NotecardError`] if the container header, an embedded-item
    /// chunk, or the text-length field is malformed, or if a version 2 text
    /// body is not valid UTF-8.
    pub fn decode(data: &[u8]) -> Result<Self, NotecardError> {
        let mut cursor = Cursor::new(data);

        let version_line = next_nonblank(&mut cursor, "Linden text header")?;
        let version_number = parse_u32(
            expect_prefix(version_line, "Linden text version ")?.trim(),
            "Linden text version",
        )?;
        let source_version = match version_number {
            1 => NotecardVersion::V1,
            2 => NotecardVersion::V2,
            other => return Err(NotecardError::UnsupportedVersion(other)),
        };

        expect_literal(&mut cursor, "{", "container open brace")?;

        let (embedded_items_version, items) = parse_embedded_items(&mut cursor)?;

        let length_line = next_nonblank(&mut cursor, "Text length")?;
        let text_length = parse_usize(
            expect_prefix(length_line, "Text length ")?.trim(),
            "Text length",
        )?;

        let available = cursor.remaining();
        let text_bytes = cursor
            .take(text_length)
            .ok_or(NotecardError::TextLengthOverflow {
                declared: text_length,
                available,
            })?;
        let text = decode_text(text_bytes, source_version)?;

        Ok(Self {
            source_version,
            embedded_items_version,
            items,
            text,
        })
    }
}
