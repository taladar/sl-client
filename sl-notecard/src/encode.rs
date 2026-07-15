//! Encoding a [`Notecard`] back into a Linden-text byte stream.
//!
//! The writer mirrors Firestorm's `LLNotecard::exportStream` /
//! `LLInventoryItem::exportLegacyStream`, always emitting the current
//! **version 2** container (as the reference viewer does on save) with the same
//! field order, tab indentation and `%08x` mask formatting, so a notecard
//! decoded from a live grid re-encodes byte-for-byte.

use crate::item::{AssetIdEncoding, InventoryItem, Permissions, SaleInfo};
use crate::{Notecard, PermissionMask};
use std::fmt::Write as _;

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
        writeln!(out, "\t\tmetadata\t{metadata}|")?;
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
    writeln!(out, "\t\tname\t{}|", item.name)?;
    writeln!(out, "\t\tdesc\t{}|", item.description)?;
    writeln!(out, "\t\tcreation_date\t{}", item.creation_date)?;
    for unknown in &item.unknown_fields {
        writeln!(out, "\t\t{unknown}")?;
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
        writeln!(
            out,
            "LLEmbeddedItems version {}",
            self.embedded_items_version
        )?;
        out.push_str("{\n");
        writeln!(out, "count {}", self.items.len())?;
        for embedded in &self.items {
            out.push_str("{\n");
            writeln!(out, "ext char index {}", embedded.char_index)?;
            write_item(out, &embedded.item)?;
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
