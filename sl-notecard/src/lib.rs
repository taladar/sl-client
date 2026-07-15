//! Pure decoder / encoder for the Second Life / OpenSim **Linden-text**
//! notecard asset — the versioned container carrying the notecard text plus the
//! **embedded inventory items** a resident drops into the body (landmarks,
//! objects, other notecards). It mirrors `sl-prim`, `sl-sculpt` and the other
//! format crates: **Bevy-free and I/O-free**, its only substantive dependency
//! is `sl-types`, so it can be tested, fuzzed and reused (a CLI, an inventory
//! tool, a bulk exporter) with no session and no grid.
//!
//! A notecard is *not* plain text. On the wire it is:
//!
//! ```text
//! Linden text version 2
//! {
//! LLEmbeddedItems version 1
//! {
//! count 1
//! {
//! ext char index 0
//!     inv_item    0
//!     {
//!         item_id ...
//!         ...the legacy inventory-item chunk...
//!     }
//! }
//! }
//! Text length 13
//! Hello \u{100000}!
//! }
//! ```
//!
//! The text references each embedded item positionally: a Unicode code point
//! `FIRST_EMBEDDED_CHAR + index` (version 2) — or a byte `0x80 | index`
//! (the legacy version 1) — stands in the text where the item is shown inline.
//! [`Notecard::decode`] reproduces both the prose and where each item sits in
//! it; [`Notecard::encode`] round-trips a notecard it did not create without
//! corrupting items it does not understand, always writing the current
//! version 2 container the way the reference viewer does.
//!
//! The format follows Firestorm's `LLNotecard` / `LLInventoryItem` legacy
//! stream, reimplemented idiomatically rather than copied.

pub mod decode;
pub mod encode;
pub mod item;
pub mod types;

pub use item::{AssetIdEncoding, EmbeddedItem, InventoryItem, Permissions, SaleInfo};
pub use types::{AssetType, InventoryType, PermissionMask, SaleType};

/// The first Unicode code point that stands in the (version 2) text for an
/// embedded item: `FIRST_EMBEDDED_CHAR + index` marks embedded item `index`
/// (`LLTextEditor::FIRST_EMBEDDED_CHAR`).
pub const FIRST_EMBEDDED_CHAR: u32 = 0x0010_0000;

/// The last code point in the embedded-item range
/// (`LLTextEditor::LAST_EMBEDDED_CHAR`).
pub const LAST_EMBEDDED_CHAR: u32 = 0x0010_FFFF;

/// The version of the Linden-text container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotecardVersion {
    /// Version 1: the text is ASCII with `0x80 | index` embedded markers.
    V1,
    /// Version 2: the text is UTF-8 with `FIRST_EMBEDDED_CHAR + index` markers.
    V2,
}

/// A decoded Linden-text notecard: the embedded inventory items and the text
/// that references them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notecard {
    /// The container version the notecard was decoded from. [`Notecard::encode`]
    /// always writes [`V2`](NotecardVersion::V2) regardless, upgrading a
    /// version 1 notecard the way the reference viewer does on save.
    pub source_version: NotecardVersion,
    /// The `LLEmbeddedItems` chunk version (always 1 in practice).
    pub embedded_items_version: u32,
    /// The embedded inventory items, in stream order.
    pub items: Vec<EmbeddedItem>,
    /// The notecard text, with each embedded-item reference represented as the
    /// Unicode code point `FIRST_EMBEDDED_CHAR + index` — uniform across source
    /// versions, so a version 1 notecard's `0x80 | index` markers appear here as
    /// the same private-use code points a version 2 notecard uses.
    pub text: String,
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "decode and encode own their `impl Notecard` blocks in their own modules"
)]
impl Notecard {
    /// A single embedded-item reference found in the text.
    ///
    /// The index a reference points at, and the character offset (in `char`s,
    /// not bytes) where it sits in the text.
    #[must_use]
    pub fn embedded_references(&self) -> Vec<EmbeddedReference> {
        self.text
            .chars()
            .enumerate()
            .filter_map(|(offset, character)| {
                embedded_char_index(character).map(|index| EmbeddedReference { offset, index })
            })
            .collect()
    }

    /// The embedded item the text references by `index`
    /// ([`EmbeddedItem::char_index`]), if any.
    #[must_use]
    pub fn item_by_index(&self, index: u32) -> Option<&EmbeddedItem> {
        self.items.iter().find(|item| item.char_index == index)
    }
}

/// Where an embedded item is referenced in the notecard text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddedReference {
    /// The character offset (counting `char`s, not bytes) into the text.
    pub offset: usize,
    /// The embedded-item index this reference points at (matching
    /// [`EmbeddedItem::char_index`]).
    pub index: u32,
}

/// The embedded-item index a text character stands for, or `None` if the
/// character is ordinary text (outside the `FIRST_EMBEDDED_CHAR..=LAST` range).
#[must_use]
pub fn embedded_char_index(character: char) -> Option<u32> {
    let code = u32::from(character);
    (FIRST_EMBEDDED_CHAR..=LAST_EMBEDDED_CHAR)
        .contains(&code)
        .then(|| code.wrapping_sub(FIRST_EMBEDDED_CHAR))
}

/// The text character that stands for embedded-item `index`, or `None` if the
/// index is too large to fit the embedded-item code-point range.
#[must_use]
pub fn embedded_char(index: u32) -> Option<char> {
    index
        .checked_add(FIRST_EMBEDDED_CHAR)
        .filter(|code| *code <= LAST_EMBEDDED_CHAR)
        .and_then(char::from_u32)
}

#[cfg(test)]
mod tests {
    use crate::item::{
        AssetIdEncoding, EmbeddedItem, InventoryItem, Permissions, SaleInfo, xor_magic,
    };
    use crate::types::{AssetType, InventoryType, PermissionMask, SaleType};
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

    /// A single-landmark notecard matching [`FIXTURE`], built by hand so the
    /// decode / encode tests check against an independent construction.
    fn sample() -> Result<Notecard, uuid::Error> {
        let permissions = Permissions {
            base_mask: PermissionMask(0x7fff_ffff),
            owner_mask: PermissionMask(0x7fff_ffff),
            group_mask: PermissionMask(0),
            everyone_mask: PermissionMask(0),
            next_owner_mask: PermissionMask(0x0008_2000),
            creator_id: key("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")?,
            owner_id: key("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb")?,
            last_owner_id: key("cccccccc-cccc-cccc-cccc-cccccccccccc")?,
            group_id: key("00000000-0000-0000-0000-000000000000")?,
            group_owned: false,
        };
        let item = InventoryItem {
            item_id: key("11111111-1111-1111-1111-111111111111")?,
            parent_id: key("22222222-2222-2222-2222-222222222222")?,
            permissions,
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
            name: "My Landmark".to_owned(),
            description: "A place".to_owned(),
            creation_date: 1_700_000_000,
            unknown_fields: Vec::new(),
        };
        Ok(Notecard {
            source_version: NotecardVersion::V2,
            embedded_items_version: 1,
            items: vec![EmbeddedItem {
                char_index: 0,
                item,
            }],
            text: "Go here: \u{100000}\n".to_owned(),
        })
    }

    /// The exact bytes a simulator writes for [`sample`] (tabs and all).
    const FIXTURE: &str = "Linden text version 2\n\
{\n\
LLEmbeddedItems version 1\n\
{\n\
count 1\n\
{\n\
ext char index 0\n\
\tinv_item\t0\n\
\t{\n\
\t\titem_id\t11111111-1111-1111-1111-111111111111\n\
\t\tparent_id\t22222222-2222-2222-2222-222222222222\n\
\tpermissions 0\n\
\t{\n\
\t\tbase_mask\t7fffffff\n\
\t\towner_mask\t7fffffff\n\
\t\tgroup_mask\t00000000\n\
\t\teveryone_mask\t00000000\n\
\t\tnext_owner_mask\t00082000\n\
\t\tcreator_id\taaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa\n\
\t\towner_id\tbbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb\n\
\t\tlast_owner_id\tcccccccc-cccc-cccc-cccc-cccccccccccc\n\
\t\tgroup_id\t00000000-0000-0000-0000-000000000000\n\
\t}\n\
\t\tasset_id\tdddddddd-dddd-dddd-dddd-dddddddddddd\n\
\t\ttype\tlandmark\n\
\t\tinv_type\tlandmark\n\
\t\tflags\t00000000\n\
\tsale_info\t0\n\
\t{\n\
\t\tsale_type\tnot\n\
\t\tsale_price\t0\n\
\t}\n\
\t\tname\tMy Landmark|\n\
\t\tdesc\tA place|\n\
\t\tcreation_date\t1700000000\n\
\t}\n\
}\n\
}\n\
Text length 14\n\
Go here: \u{100000}\n\
}\n";

    #[test]
    fn encode_reproduces_the_simulator_bytes() -> Result<(), uuid::Error> {
        let encoded = sample()?.encode();
        assert_eq!(encoded, FIXTURE.as_bytes());
        Ok(())
    }

    #[test]
    fn decode_matches_the_hand_built_sample() -> TestResult {
        let decoded = Notecard::decode(FIXTURE.as_bytes())?;
        assert_eq!(decoded, sample()?);
        Ok(())
    }

    #[test]
    fn decode_then_encode_is_byte_exact() -> TestResult {
        let decoded = Notecard::decode(FIXTURE.as_bytes())?;
        assert_eq!(decoded.encode(), FIXTURE.as_bytes());
        Ok(())
    }

    #[test]
    fn references_locate_the_embedded_item_in_the_text() -> TestResult {
        let notecard = sample()?;
        let references = notecard.embedded_references();
        assert_eq!(references.len(), 1, "one embedded reference");
        let reference = references.first().ok_or("missing reference")?;
        // "Go here: " is nine characters, so the marker sits at offset 9.
        assert_eq!(reference.offset, 9);
        assert_eq!(reference.index, 0);
        let resolved = notecard.item_by_index(reference.index).ok_or("no item")?;
        assert_eq!(resolved.item.asset_type, AssetType::Landmark);
        Ok(())
    }

    #[test]
    fn shadow_id_is_undone_on_decode_and_reapplied_on_encode() -> TestResult {
        let mut notecard = sample()?;
        let real = key("dddddddd-dddd-dddd-dddd-dddddddddddd")?;
        {
            let item = notecard.items.first_mut().ok_or("no item")?;
            item.item.asset_id_encoding = AssetIdEncoding::Shadow;
        }
        let round_tripped = Notecard::decode(&notecard.encode())?;
        let item = round_tripped.items.first().ok_or("no item")?;
        // The real asset id survives even though it was stored obfuscated.
        assert_eq!(item.item.asset_id, real);
        assert_eq!(item.item.asset_id_encoding, AssetIdEncoding::Shadow);
        Ok(())
    }

    #[test]
    fn xor_magic_is_its_own_inverse() -> Result<(), uuid::Error> {
        let original = key("dddddddd-dddd-dddd-dddd-dddddddddddd")?;
        assert_eq!(xor_magic(xor_magic(original)), original);
        assert_ne!(xor_magic(original), original, "obfuscation changes the id");
        Ok(())
    }

    #[test]
    fn version_one_markers_upgrade_to_version_two_code_points() -> TestResult {
        // A minimal version 1 notecard with no embedded items and a raw 0x80
        // marker byte (the legacy byte-oriented text, not UTF-8) in a 3-byte
        // body "A\x80B".
        let mut bytes = Vec::new();
        bytes.extend_from_slice(
            b"Linden text version 1\n{\nLLEmbeddedItems version 1\n{\ncount 0\n}\nText length 3\n",
        );
        bytes.extend_from_slice(&[b'A', 0x80, b'B']);
        bytes.extend_from_slice(b"}\n");
        let decoded = Notecard::decode(&bytes)?;
        assert_eq!(decoded.source_version, NotecardVersion::V1);
        let expected = format!("A{}B", embedded_char(0).ok_or("bad char")?);
        assert_eq!(decoded.text, expected);
        // Re-encoding always writes version 2.
        let reencoded = String::from_utf8(decoded.encode())?;
        assert!(
            reencoded.starts_with("Linden text version 2\n"),
            "encode upgrades to v2"
        );
        Ok(())
    }

    #[test]
    fn empty_notecard_round_trips() -> TestResult {
        let empty = Notecard {
            source_version: NotecardVersion::V2,
            embedded_items_version: 1,
            items: Vec::new(),
            text: String::new(),
        };
        let round_tripped = Notecard::decode(&empty.encode())?;
        assert_eq!(round_tripped, empty);
        Ok(())
    }

    #[test]
    fn unknown_item_fields_are_preserved() -> TestResult {
        let mut notecard = sample()?;
        {
            let item = notecard.items.first_mut().ok_or("no item")?;
            item.item.unknown_fields.push("future_field\t42".to_owned());
        }
        let round_tripped = Notecard::decode(&notecard.encode())?;
        let item = round_tripped.items.first().ok_or("no item")?;
        assert_eq!(
            item.item.unknown_fields,
            vec!["future_field\t42".to_owned()]
        );
        Ok(())
    }

    #[test]
    fn unrecognised_type_names_survive_a_round_trip() -> TestResult {
        let mut notecard = sample()?;
        {
            let item = notecard.items.first_mut().ok_or("no item")?;
            item.item.asset_type = AssetType::Other("weird".to_owned());
            item.item.inventory_type = InventoryType::Other("odd".to_owned());
            item.item.sale_info.sale_type = SaleType::Other("xyzzy".to_owned());
        }
        let round_tripped = Notecard::decode(&notecard.encode())?;
        let item = round_tripped.items.first().ok_or("no item")?;
        assert_eq!(item.item.asset_type, AssetType::Other("weird".to_owned()));
        assert_eq!(
            item.item.inventory_type,
            InventoryType::Other("odd".to_owned())
        );
        assert_eq!(
            item.item.sale_info.sale_type,
            SaleType::Other("xyzzy".to_owned())
        );
        Ok(())
    }

    #[test]
    fn absent_inv_type_stays_absent() -> TestResult {
        let mut notecard = sample()?;
        {
            let item = notecard.items.first_mut().ok_or("no item")?;
            item.item.inventory_type = InventoryType::None;
        }
        let encoded = String::from_utf8(notecard.encode())?;
        assert!(!encoded.contains("inv_type"), "the field is omitted");
        let round_tripped = Notecard::decode(encoded.as_bytes())?;
        let item = round_tripped.items.first().ok_or("no item")?;
        assert_eq!(item.item.inventory_type, InventoryType::None);
        Ok(())
    }

    #[test]
    fn permission_mask_accessors_read_the_well_known_bits() {
        let unrestricted = PermissionMask(
            PermissionMask::MODIFY | PermissionMask::COPY | PermissionMask::TRANSFER,
        );
        assert!(unrestricted.can_modify(), "modify bit set");
        assert!(unrestricted.can_copy(), "copy bit set");
        assert!(unrestricted.can_transfer(), "transfer bit set");
        assert!(!unrestricted.can_move(), "move bit clear");
    }

    #[test]
    fn a_bad_version_is_rejected() {
        let bytes = b"Linden text version 9\n{\n";
        assert!(Notecard::decode(bytes).is_err(), "version 9 is unsupported");
    }

    #[test]
    fn a_truncated_text_body_is_rejected() {
        let bytes = "Linden text version 2\n{\nLLEmbeddedItems version 1\n{\ncount 0\n}\nText length 50\nshort\n}\n".as_bytes();
        assert!(
            Notecard::decode(bytes).is_err(),
            "declared length exceeds the body"
        );
    }
}
