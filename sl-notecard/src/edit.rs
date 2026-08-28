//! Reconciling an **edited** notecard body back into a well-formed [`Notecard`].
//!
//! An editor hands back a text buffer in which the embedded-item markers
//! (`FIRST_EMBEDDED_CHAR + index`, one private-use code point per embedded item)
//! sit inline among the prose. After an edit the markers no longer line up with
//! the item table: the resident may have deleted a marker (dropping that item),
//! moved one, or copy-pasted one (duplicating the item). This mirrors what the
//! reference viewer's `LLViewerTextEditor::getEmbeddedText` does on save — walk
//! the edited text, resolve each marker against the *original* item table, and
//! rebuild the table so every surviving marker `FIRST_EMBEDDED_CHAR + i` lines
//! up with a fresh item at index `i`, in order of first appearance.

use crate::item::InventoryItem;
use crate::{Notecard, NotecardVersion, embedded_char, embedded_char_index};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "edit owns its `impl Notecard` block, apart from decode's canonical impl"
)]
impl Notecard {
    /// Produce the notecard that results from replacing this notecard's body
    /// with `edited_text`, reconciling the embedded-item table against the
    /// markers the edited text actually contains.
    ///
    /// The embedded-item markers in `edited_text` are interpreted against
    /// **this** notecard's item table (the one the editor was loaded with), so
    /// the caller edits the prose freely without tracking item indices:
    ///
    /// - A marker whose item survives is kept, with its item cloned into the new
    ///   table and renumbered by order of first appearance.
    /// - A marker whose item was deleted from the text simply does not appear,
    ///   so its item is dropped.
    /// - A **duplicated** marker (the resident copy-pasted an embedded item)
    ///   yields an independent item per occurrence, matching the reference
    ///   viewer's copy-on-paste behaviour, rather than two markers aliasing one
    ///   table entry.
    /// - A marker pointing at an index with no item (a stray private-use code
    ///   point the resident typed, or a dangling reference) is dropped from the
    ///   text entirely, since it names nothing.
    ///
    /// The result always carries [`NotecardVersion::V2`] as its source version
    /// — an edit produces a fresh version 2 notecard.
    #[must_use]
    pub fn with_edited_text(&self, edited_text: &str) -> Self {
        let mut items: Vec<InventoryItem> = Vec::new();
        let mut text = String::with_capacity(edited_text.len());
        for character in edited_text.chars() {
            match embedded_char_index(character) {
                None => text.push(character),
                Some(old_index) => {
                    let Some(source) = self.item_by_index(old_index) else {
                        // A marker naming no item: drop it rather than emit a
                        // dangling reference.
                        continue;
                    };
                    let new_index = u32::try_from(items.len()).unwrap_or(u32::MAX);
                    let Some(new_marker) = embedded_char(new_index) else {
                        // More surviving markers than the private-use range can
                        // hold; drop the overflow rather than corrupt the text.
                        continue;
                    };
                    items.push(InventoryItem::clone(source));
                    text.push(new_marker);
                }
            }
        }
        Self {
            source_version: NotecardVersion::V2,
            items,
            text,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::item::{AssetIdEncoding, InventoryItem, Permissions, SaleInfo};
    use crate::types::{AssetType, InventoryType, SaleType};
    use crate::{Notecard, NotecardVersion, embedded_char};
    use pretty_assertions::assert_eq;
    use sl_types::key::Key;
    use uuid::Uuid;

    /// A boxed error so tests can `?` both notecard and UUID parse failures.
    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Parse a UUID string into a [`Key`] for building fixtures.
    fn key(text: &str) -> Result<Key, uuid::Error> {
        Uuid::parse_str(text).map(Key)
    }

    /// A named embedded item, so tests can tell surviving items apart.
    fn item(name: &str) -> Result<InventoryItem, uuid::Error> {
        Ok(InventoryItem {
            item_id: key("11111111-1111-1111-1111-111111111111")?,
            parent_id: key("22222222-2222-2222-2222-222222222222")?,
            permissions: Permissions::default(),
            metadata: None,
            asset_id: key("dddddddd-dddd-dddd-dddd-dddddddddddd")?,
            asset_id_encoding: AssetIdEncoding::Plain,
            asset_type: AssetType::Landmark,
            inventory_type: InventoryType::Landmark,
            flags: 0,
            sale_info: SaleInfo {
                sale_type: SaleType::NotForSale,
                sale_price: 0,
            },
            name: name.to_owned(),
            description: String::new(),
            creation_date: 0,
            unknown_fields: Vec::new(),
        })
    }

    /// The embedded items' names, in table order — so a test asserts the whole
    /// surviving table without indexing into it.
    fn item_names(notecard: &Notecard) -> Vec<&str> {
        notecard
            .items
            .iter()
            .map(|item| item.name.as_str())
            .collect()
    }

    /// A two-item notecard: `A<0>B<1>C` with items named "first" and "second".
    fn two_item_notecard() -> Result<Notecard, Box<dyn std::error::Error>> {
        Ok(Notecard {
            source_version: NotecardVersion::V2,
            items: vec![item("first")?, item("second")?],
            text: format!(
                "A{}B{}C",
                embedded_char(0).ok_or("bad char")?,
                embedded_char(1).ok_or("bad char")?,
            ),
        })
    }

    #[test]
    fn untouched_markers_round_trip_the_item_table() -> TestResult {
        let notecard = two_item_notecard()?;
        let reconciled = notecard.with_edited_text(&notecard.text);
        assert_eq!(item_names(&reconciled), vec!["first", "second"]);
        assert_eq!(reconciled.text, notecard.text);
        Ok(())
    }

    #[test]
    fn deleting_a_marker_drops_its_item_and_renumbers() -> TestResult {
        let notecard = two_item_notecard()?;
        // The resident deleted the first marker, keeping only the second.
        let edited = format!("ABC{}D", embedded_char(1).ok_or("bad char")?);
        let reconciled = notecard.with_edited_text(&edited);
        assert_eq!(reconciled.items.len(), 1, "only the second item survives");
        let survivor = reconciled.items.first().ok_or("no item")?;
        assert_eq!(survivor.name, "second");
        // The surviving marker in the text is now the index-0 marker.
        assert_eq!(
            reconciled.text,
            format!("ABC{}D", embedded_char(0).ok_or("bad char")?)
        );
        Ok(())
    }

    #[test]
    fn reordering_markers_renumbers_by_appearance() -> TestResult {
        let notecard = two_item_notecard()?;
        // The resident swapped the two markers' positions.
        let edited = format!(
            "{}{}",
            embedded_char(1).ok_or("bad char")?,
            embedded_char(0).ok_or("bad char")?,
        );
        let reconciled = notecard.with_edited_text(&edited);
        assert_eq!(
            item_names(&reconciled),
            vec!["second", "first"],
            "renumbered by first appearance"
        );
        Ok(())
    }

    #[test]
    fn duplicating_a_marker_clones_the_item() -> TestResult {
        let notecard = two_item_notecard()?;
        // The resident copy-pasted the first marker.
        let marker0 = embedded_char(0).ok_or("bad char")?;
        let edited = format!("{marker0}{marker0}");
        let reconciled = notecard.with_edited_text(&edited);
        assert_eq!(
            item_names(&reconciled),
            vec!["first", "first"],
            "each occurrence gets its own item"
        );
        // The two markers in the reconciled text are the index-0 and index-1
        // markers, in that order, so each names its own table entry.
        assert_eq!(
            reconciled.text,
            format!(
                "{}{}",
                embedded_char(0).ok_or("bad char")?,
                embedded_char(1).ok_or("bad char")?
            )
        );
        Ok(())
    }

    #[test]
    fn a_marker_for_no_item_is_dropped() -> TestResult {
        let notecard = two_item_notecard()?;
        // Index 5 names no item in the table.
        let stray = embedded_char(5).ok_or("bad char")?;
        let edited = format!("hello{stray}world");
        let reconciled = notecard.with_edited_text(&edited);
        assert_eq!(reconciled.items.len(), 0);
        assert_eq!(reconciled.text, "helloworld");
        Ok(())
    }

    #[test]
    fn plain_text_edit_keeps_no_items() -> TestResult {
        let notecard = two_item_notecard()?;
        let reconciled = notecard.with_edited_text("just prose now");
        assert_eq!(reconciled.items.len(), 0);
        assert_eq!(reconciled.text, "just prose now");
        assert_eq!(reconciled.source_version, NotecardVersion::V2);
        Ok(())
    }

    #[test]
    fn edited_notecard_re_encodes_and_decodes() -> TestResult {
        let notecard = two_item_notecard()?;
        let edited = format!("intro {}outro", embedded_char(1).ok_or("bad char")?);
        let reconciled = notecard.with_edited_text(&edited);
        let round_tripped = Notecard::decode(&reconciled.encode())?;
        assert_eq!(round_tripped, reconciled);
        Ok(())
    }
}
