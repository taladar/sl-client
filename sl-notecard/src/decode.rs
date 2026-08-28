//! Decoding a Linden-text notecard byte stream into a [`Notecard`].
//!
//! The parser mirrors Firestorm's `LLNotecard::importStream` /
//! `LLInventoryItem::importLegacyStream`: a line-oriented walk over the
//! container and the embedded-item chunks, tolerant of the leading indentation
//! whitespace the simulator writes, followed by a fixed-length read of the raw
//! text body.
//!
//! It is stricter than the reference in one place, deliberately. The reference
//! skips a `{` it does not expect, which lets an unrecognised chunk-shaped
//! field close the *enclosing* chunk with its own `}` and misparse every item
//! after it. A chunk's braces are required here, and a stray one is an error —
//! there is no recovering the framing, and this is somebody's inventory.

use crate::item::{AssetIdEncoding, InventoryItem, Permissions, SaleInfo, xor_magic};
use crate::types::{AssetType, InventoryType, PermissionMask, SaleType};
use crate::{EMBEDDED_ITEMS_VERSION, Notecard, NotecardVersion, embedded_char};
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
    /// The `LLEmbeddedItems` chunk version is not 1, the only version the
    /// format has.
    #[error("unsupported LLEmbeddedItems version {0}")]
    UnsupportedEmbeddedItemsVersion(u32),
    /// A `{` opened a chunk where a field was expected, so the decoder can no
    /// longer tell where the enclosing chunk ends.
    #[error("unexpected {{ inside a {context} chunk: nested chunk {keyword:?} is not understood")]
    UnexpectedChunk {
        /// The chunk whose body the stray brace appeared in.
        context: &'static str,
        /// The field line that preceded the brace, which named the chunk.
        keyword: String,
    },
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

    /// The current read position, for [`rewind_to`](Cursor::rewind_to).
    const fn mark(&self) -> usize {
        self.pos
    }

    /// Put the read position back to a [`mark`](Cursor::mark), so a line that
    /// turned out not to belong here is left for the next reader.
    const fn rewind_to(&mut self, mark: usize) {
        self.pos = mark;
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

/// The value of a tab-then-`|`-terminated field (`name`, `desc`), i.e.
/// everything after the first tab up to the first `|` — matching the
/// reference's ` %254s%254[\t]%254[^|]` rescan of the line.
fn tabbed_value(line: &str) -> &str {
    let rest = line.split_once('\t').map_or("", |(_keyword, value)| value);
    rest.split_once('|').map_or(rest, |(value, _rest)| value)
}

/// The value of the `metadata` field: everything after the first tab, kept
/// whole.
///
/// Unlike `name` / `desc` the reference does **not** rescan this line for a `|`
/// terminator — it reads the value with a plain `%254s` and the `|` lives on
/// the following line (`llinventory.cpp`'s `toXML(...)` then `"|\n"`), where it
/// is warned about and dropped. A one-line variant with a trailing `|` is
/// accepted too so a writer that folds the terminator onto this line still
/// decodes to the same value.
fn metadata_value(line: &str) -> &str {
    let rest = line.split_once('\t').map_or("", |(_keyword, value)| value);
    rest.strip_suffix('|').unwrap_or(rest)
}

/// Consume the lone `|` line that terminates a `metadata` field, if it is the
/// next thing in the stream. A writer that put the terminator on the metadata
/// line itself leaves nothing to consume here, so its absence is not an error.
fn consume_metadata_terminator(cursor: &mut Cursor<'_>) {
    let mark = cursor.mark();
    if !matches!(next_nonblank(cursor, "metadata terminator"), Ok("|")) {
        cursor.rewind_to(mark);
    }
}

/// Consume the `{` that opens a chunk body, which the simulator always writes
/// on its own line after the chunk's keyword line.
fn expect_chunk_open(cursor: &mut Cursor<'_>, context: &'static str) -> Result<(), NotecardError> {
    expect_literal(cursor, "{", context)
}

/// Reject a `{` that opens an unrecognised nested chunk inside a chunk body.
///
/// The reference simply `continue`s past it, which desynchronises its parser:
/// the nested chunk's own `}` then closes the *enclosing* chunk and every
/// following field lands in the wrong item. There is no way to recover the
/// framing once that happens, so a decoder that returns a `Result` says so
/// instead of handing back silently mangled inventory.
fn nested_chunk_error(context: &'static str, previous_keyword: &str) -> NotecardError {
    NotecardError::UnexpectedChunk {
        context,
        keyword: previous_keyword.to_owned(),
    }
}

/// Parse a permissions chunk (`permissions 0 { ... }`), whose opening `{` has
/// not yet been consumed.
fn parse_permissions(cursor: &mut Cursor<'_>) -> Result<Permissions, NotecardError> {
    let mut permissions = Permissions::default();
    expect_chunk_open(cursor, "permissions open brace")?;
    let mut previous_keyword = String::new();
    loop {
        let line = next_nonblank(cursor, "permissions")?;
        if line == "{" {
            return Err(nested_chunk_error("permissions", &previous_keyword));
        }
        if line == "}" {
            break;
        }
        let keyword = keyword_of(line);
        let value = value_of(line);
        keyword.clone_into(&mut previous_keyword);
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
    expect_chunk_open(cursor, "sale_info open brace")?;
    let mut previous_keyword = String::new();
    loop {
        let line = next_nonblank(cursor, "sale_info")?;
        if line == "{" {
            return Err(nested_chunk_error("sale_info", &previous_keyword));
        }
        if line == "}" {
            break;
        }
        let keyword = keyword_of(line);
        let value = value_of(line);
        keyword.clone_into(&mut previous_keyword);
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

    expect_chunk_open(cursor, "inventory item open brace")?;
    let mut previous_keyword = String::new();
    loop {
        let line = next_nonblank(cursor, "inventory item")?;
        if line == "{" {
            return Err(nested_chunk_error("inventory item", &previous_keyword));
        }
        if line == "}" {
            break;
        }
        let keyword = keyword_of(line);
        let value = value_of(line);
        keyword.clone_into(&mut previous_keyword);
        match keyword {
            "item_id" => item_id = parse_key(value, "item_id")?,
            "parent_id" => parent_id = parse_key(value, "parent_id")?,
            "permissions" => permissions = parse_permissions(cursor)?,
            "sale_info" => sale_info = parse_sale_info(cursor)?,
            "metadata" => {
                metadata = Some(metadata_value(line).to_owned());
                consume_metadata_terminator(cursor);
            }
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

/// The fewest bytes one embedded-item entry can occupy. The shortest
/// well-formed entry — `{`, `ext char index 0`, `inv_item`, an empty item
/// chunk, `}` — is about 34 bytes with its newlines; this is a deliberate
/// floor well under that.
///
/// Used only to bound a reservation — see [`reserve_hint`].
const MIN_EMBEDDED_ITEM_BYTES: usize = 8;

/// How much to reserve up front for a declared `count` of embedded items,
/// given the bytes still unread: `count`, or what those bytes could actually
/// hold, whichever is smaller.
///
/// The `count` line is attacker-supplied text with no upper bound, and parcel
/// covenants decode through this path, so reserving from it directly lets
/// about seventy bytes of notecard ask for a multi-gigabyte allocation (or
/// overflow the capacity outright). The reference never preallocates here at
/// all (`llnotecard.cpp`); bounding the reservation keeps the speed for a real
/// notecard without letting the count size an allocation on its own. The parse
/// loop below stays the authority on how many items are actually present.
fn reserve_hint(count: usize, remaining: usize) -> usize {
    count.min(remaining.checked_div(MIN_EMBEDDED_ITEM_BYTES).unwrap_or(0))
}

/// Parse the `LLEmbeddedItems` chunk (header, count, and each `{ ext char
/// index / inv_item / item }` entry), whose header line has not yet been read.
///
/// The `ext char index` line is parsed for its shape but its value is
/// discarded: the reference reads it into a local it never uses
/// (`llnotecard.cpp`) and numbers the items by load order instead
/// (`LLEmbeddedItems::addItems`), so the returned order **is** the numbering
/// the text's markers resolve against.
fn parse_embedded_items(cursor: &mut Cursor<'_>) -> Result<Vec<InventoryItem>, NotecardError> {
    let header = next_nonblank(cursor, "LLEmbeddedItems header")?;
    let version = parse_u32(
        expect_prefix(header, "LLEmbeddedItems version ")?.trim(),
        "LLEmbeddedItems version",
    )?;
    if version != EMBEDDED_ITEMS_VERSION {
        return Err(NotecardError::UnsupportedEmbeddedItemsVersion(version));
    }
    expect_literal(cursor, "{", "LLEmbeddedItems open brace")?;

    let count_line = next_nonblank(cursor, "embedded item count")?;
    let count = parse_usize(expect_prefix(count_line, "count ")?.trim(), "count")?;

    let mut items = Vec::with_capacity(reserve_hint(count, cursor.remaining()));
    for _index in 0..count {
        expect_literal(cursor, "{", "embedded item entry open brace")?;
        let ext_line = next_nonblank(cursor, "ext char index")?;
        let _ignored_index = parse_u32(
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
        items.push(parse_item(cursor)?);
        expect_literal(cursor, "}", "embedded item entry close brace")?;
    }

    expect_literal(cursor, "}", "LLEmbeddedItems close brace")?;
    Ok(items)
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

        let items = parse_embedded_items(&mut cursor)?;

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
            items,
            text,
        })
    }
}
