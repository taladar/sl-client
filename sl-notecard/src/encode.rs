//! Encoding a [`Notecard`] back into a Linden-text byte stream.
//!
//! The writer mirrors Firestorm's `LLNotecard::exportStream` /
//! `LLInventoryItem::exportLegacyStream`, always emitting the current
//! **version 2** container (as the reference viewer does on save) with the same
//! field order, tab indentation and `%08x` mask formatting, so a notecard
//! decoded from a live grid re-encodes byte-for-byte.
//!
//! Two of those fields the writer does not take from the decoded notecard,
//! because the reference does not either: the `LLEmbeddedItems` chunk version
//! is the constant 1, and each item's `ext char index` is its position in the
//! table. Free-text values are sanitised on the way out, since a `|` or a
//! newline in one would otherwise change the shape of the stream rather than
//! its content.

use crate::item::{AssetIdEncoding, InventoryItem, Permissions, SaleInfo};
use crate::{EMBEDDED_ITEMS_VERSION, Notecard, PermissionMask};
use std::fmt::Write as _;

/// What a character that cannot survive the format is replaced with, the way
/// the reference's `replaceChar(mName, '|', ' ')` /
/// `replaceNonstandardASCII(mName, ' ')` do on import.
const SANITISED: &str = " ";

/// The line terminators no field value may contain, whatever it is.
///
/// A `\n` in a value would write extra lines into the item chunk, letting an
/// item's name or description forge an `asset_id` or a whole `permissions`
/// block on save.
const LINE_BREAKS: [char; 2] = ['\n', '\r'];

/// Additionally, the `|` that terminates a `name` / `desc` value on the wire —
/// a value containing one is truncated there on the way back in.
const FIELD_END: char = '|';

/// Make a value safe to write as one line of the container.
///
/// The reference never has to do this on export because it sanitises the same
/// characters on **import** (`llinventory.cpp`'s `replaceNonstandardASCII` /
/// `replaceChar`); this crate accepts a value from a caller who never went
/// through a decode, so it sanitises on the way out instead.
fn sanitise_line(value: &str) -> std::borrow::Cow<'_, str> {
    if value.contains(LINE_BREAKS) {
        std::borrow::Cow::Owned(value.replace(LINE_BREAKS, SANITISED))
    } else {
        std::borrow::Cow::Borrowed(value)
    }
}

/// [`sanitise_line`] plus the `|` field terminator, for the values written in
/// the `keyword\tvalue|` form.
fn sanitise_field(value: &str) -> std::borrow::Cow<'_, str> {
    if value.contains(FIELD_END) {
        std::borrow::Cow::Owned(sanitise_line(value).replace(FIELD_END, SANITISED))
    } else {
        sanitise_line(value)
    }
}

/// Append a permission mask as the simulator's eight-digit lowercase hex.
fn write_mask(out: &mut String, label: &str, mask: PermissionMask) -> std::fmt::Result {
    writeln!(out, "\t\t{label}\t{:08x}", mask.0)
}

/// Append a permissions chunk exactly as `LLPermissions::exportLegacyStream`.
fn write_permissions(out: &mut String, permissions: &Permissions) -> std::fmt::Result {
    out.push_str("\tpermissions 0\n\t{\n");
    write_mask(out, "base_mask", permissions.base_mask)?;
    write_mask(out, "owner_mask", permissions.owner_mask)?;
    write_mask(out, "group_mask", permissions.group_mask)?;
    write_mask(out, "everyone_mask", permissions.everyone_mask)?;
    write_mask(out, "next_owner_mask", permissions.next_owner_mask)?;
    writeln!(out, "\t\tcreator_id\t{}", permissions.creator_id)?;
    writeln!(out, "\t\towner_id\t{}", permissions.owner_id)?;
    writeln!(out, "\t\tlast_owner_id\t{}", permissions.last_owner_id)?;
    writeln!(out, "\t\tgroup_id\t{}", permissions.group_id)?;
    if permissions.group_owned {
        out.push_str("\t\tgroup_owned\t1\n");
    }
    out.push_str("\t}\n");
    Ok(())
}

/// Append a sale-info chunk exactly as `LLSaleInfo::exportLegacyStream`.
fn write_sale_info(out: &mut String, sale_info: &SaleInfo) -> std::fmt::Result {
    out.push_str("\tsale_info\t0\n\t{\n");
    writeln!(out, "\t\tsale_type\t{}", sale_info.sale_type.type_name())?;
    writeln!(out, "\t\tsale_price\t{}", sale_info.sale_price)?;
    out.push_str("\t}\n");
    Ok(())
}

/// Append a legacy inventory-item chunk exactly as
/// `LLInventoryItem::exportLegacyStream` (with the asset key included), in the
/// same field order the simulator uses.
fn write_item(out: &mut String, item: &InventoryItem) -> std::fmt::Result {
    out.push_str("\tinv_item\t0\n\t{\n");
    writeln!(out, "\t\titem_id\t{}", item.item_id)?;
    writeln!(out, "\t\tparent_id\t{}", item.parent_id)?;
    write_permissions(out, &item.permissions)?;
    if let Some(metadata) = &item.metadata {
        // Two lines, as `LLInventoryItem::exportLegacyStream` writes it: the
        // keyword and the LLSD XML, then the `|` terminator on its own line.
        writeln!(out, "\t\tmetadata\t{}", sanitise_line(metadata))?;
        out.push_str("|\n");
    }
    match item.asset_id_encoding {
        AssetIdEncoding::Plain => writeln!(out, "\t\tasset_id\t{}", item.asset_id)?,
        AssetIdEncoding::Shadow => writeln!(out, "\t\tshadow_id\t{}", item.shadow_id())?,
    }
    writeln!(out, "\t\ttype\t{}", item.asset_type.type_name())?;
    if let Some(inv_type) = item.inventory_type.type_name() {
        writeln!(out, "\t\tinv_type\t{inv_type}")?;
    }
    writeln!(out, "\t\tflags\t{:08x}", item.flags)?;
    write_sale_info(out, &item.sale_info)?;
    writeln!(out, "\t\tname\t{}|", sanitise_field(&item.name))?;
    writeln!(out, "\t\tdesc\t{}|", sanitise_field(&item.description))?;
    writeln!(out, "\t\tcreation_date\t{}", item.creation_date)?;
    for unknown in &item.unknown_fields {
        // A preserved line is one line: a newline smuggled in here would forge
        // fields the same way a name could. Its `|`, if any, is content.
        writeln!(out, "\t\t{}", sanitise_line(unknown))?;
    }
    out.push_str("\t}\n");
    Ok(())
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "encode owns its `impl Notecard` block, apart from decode's canonical impl"
)]
impl Notecard {
    /// Append the notecard's Linden-text serialisation to `out`.
    ///
    /// # Errors
    ///
    /// Returns a [`std::fmt::Error`] only if the underlying [`String`] writer
    /// fails, which it never does — [`encode`](Self::encode) relies on this.
    pub fn encode_into(&self, out: &mut String) -> std::fmt::Result {
        out.push_str("Linden text version 2\n{\n");
        writeln!(out, "LLEmbeddedItems version {EMBEDDED_ITEMS_VERSION}")?;
        out.push_str("{\n");
        writeln!(out, "count {}", self.items.len())?;
        for (index, item) in self.items.iter().enumerate() {
            out.push_str("{\n");
            // The index is the item's position, which is what the text's
            // markers resolve against — `exportEmbeddedItemsStream` numbers
            // them the same way rather than echoing anything it read.
            writeln!(out, "ext char index {index}")?;
            write_item(out, item)?;
            out.push_str("}\n");
        }
        out.push_str("}\n");
        writeln!(out, "Text length {}", self.text.len())?;
        out.push_str(&self.text);
        out.push_str("}\n");
        Ok(())
    }

    /// Encode the notecard as a Linden-text byte stream (always the current
    /// version 2 container).
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = String::new();
        // Writing to a `String` is infallible; the `Result` is only surfaced by
        // the generic `fmt::Write` signature.
        match self.encode_into(&mut out) {
            Ok(()) => {}
            Err(_infallible) => {}
        }
        out.into_bytes()
    }
}
