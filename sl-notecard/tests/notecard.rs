//! Tests for the Linden-text notecard codec, over the shapes a live simulator
//! and a hostile asset actually produce.
//!
//! The unit tests inside the crate pin the happy path against a hand-built
//! sample. These pin the places where being "internally consistent" is not
//! enough: the two-line `metadata` field, the positional resolution of embedded
//! items, the chunk framing, and what a value that cannot be represented does
//! on the way out.

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_notecard::{AssetType, InventoryType, Notecard, NotecardVersion, SaleType};

    /// A boxed error so a test can `?` both notecard and UTF-8 failures.
    type TestError = Box<dyn std::error::Error>;

    /// The LLSD XML `LLInventoryItem::exportLegacyStream` writes for a
    /// thumbnail, which is what a real `metadata` field carries.
    const THUMBNAIL_METADATA: &str = "<llsd><map><key>thumbnail</key><map><key>asset_id</key>\
<uuid>eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee</uuid></map></map></llsd>";

    /// Wrap `items_chunk` (the body between `count` and its closing brace) and
    /// `text` into a whole version 2 container, the way the simulator writes
    /// it.
    fn container(count: usize, items_chunk: &str, text: &str) -> String {
        format!(
            "Linden text version 2\n{{\nLLEmbeddedItems version 1\n{{\ncount {count}\n\
{items_chunk}}}\nText length {}\n{text}}}\n",
            text.len()
        )
    }

    /// One embedded-item entry, with `fields` spliced into the item chunk after
    /// the permissions block.
    fn entry(ext_char_index: u32, fields: &str) -> String {
        format!(
            "{{\next char index {ext_char_index}\n\
\tinv_item\t0\n\
\t{{\n\
\t\titem_id\t11111111-1111-1111-1111-111111111111\n\
\t\tparent_id\t22222222-2222-2222-2222-222222222222\n\
\tpermissions 0\n\
\t{{\n\
\t\tbase_mask\t7fffffff\n\
\t\towner_mask\t7fffffff\n\
\t\tgroup_mask\t00000000\n\
\t\teveryone_mask\t00000000\n\
\t\tnext_owner_mask\t00082000\n\
\t\tcreator_id\taaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa\n\
\t\towner_id\tbbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb\n\
\t\tlast_owner_id\tcccccccc-cccc-cccc-cccc-cccccccccccc\n\
\t\tgroup_id\t00000000-0000-0000-0000-000000000000\n\
\t}}\n\
{fields}\
\t\ttype\tlandmark\n\
\t\tinv_type\tlandmark\n\
\t\tflags\t00000000\n\
\tsale_info\t0\n\
\t{{\n\
\t\tsale_type\tnot\n\
\t\tsale_price\t0\n\
\t}}\n\
\t\tname\tMy Landmark|\n\
\t\tdesc\tA place|\n\
\t\tcreation_date\t1700000000\n\
\t}}\n\
}}\n"
        )
    }

    /// The plain `asset_id` line every fixture below uses unless it is testing
    /// the obfuscated form.
    const ASSET_ID_FIELD: &str = "\t\tasset_id\tdddddddd-dddd-dddd-dddd-dddddddddddd\n";

    // -----------------------------------------------------------------------
    // The two-line `metadata` field.
    // -----------------------------------------------------------------------

    /// The simulator writes `metadata`'s LLSD on the keyword line and its `|`
    /// terminator on the **next** line (`toXML(...)` then `"|\n"`). The value
    /// is the XML alone, and the terminator must not survive as a field of its
    /// own.
    #[test]
    fn metadata_spans_two_lines() -> Result<(), TestError> {
        let fields = format!("\t\tmetadata\t{THUMBNAIL_METADATA}\n|\n{ASSET_ID_FIELD}");
        let bytes = container(1, &entry(0, &fields), "body\n");
        let decoded = Notecard::decode(bytes.as_bytes())?;
        let item = decoded.items.first().ok_or("no item")?;
        assert_eq!(item.metadata.as_deref(), Some(THUMBNAIL_METADATA));
        assert_eq!(
            item.unknown_fields,
            Vec::<String>::new(),
            "the `|` terminator is framing, not an unknown field"
        );
        Ok(())
    }

    /// And it re-encodes into the same two lines, so a notecard carrying a
    /// thumbnail round-trips byte-for-byte instead of gaining a bogus `\t\t|`
    /// line.
    #[test]
    fn metadata_re_encodes_byte_for_byte() -> Result<(), TestError> {
        let fields = format!("\t\tmetadata\t{THUMBNAIL_METADATA}\n|\n{ASSET_ID_FIELD}");
        let bytes = container(1, &entry(0, &fields), "body\n");
        let decoded = Notecard::decode(bytes.as_bytes())?;
        assert_eq!(String::from_utf8(decoded.encode())?, bytes);
        Ok(())
    }

    /// A writer that folds the terminator onto the metadata line itself still
    /// decodes to the same value — and is normalised to the reference's form.
    #[test]
    fn a_one_line_metadata_field_is_accepted() -> Result<(), TestError> {
        let fields = format!("\t\tmetadata\t{THUMBNAIL_METADATA}|\n{ASSET_ID_FIELD}");
        let bytes = container(1, &entry(0, &fields), "body\n");
        let decoded = Notecard::decode(bytes.as_bytes())?;
        let item = decoded.items.first().ok_or("no item")?;
        assert_eq!(item.metadata.as_deref(), Some(THUMBNAIL_METADATA));
        assert!(
            String::from_utf8(decoded.encode())?
                .contains(&format!("\t\tmetadata\t{THUMBNAIL_METADATA}\n|\n")),
            "re-encoded in the reference's two-line form"
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // A free-text value cannot change the shape of the stream.
    // -----------------------------------------------------------------------

    /// A `\n` in an item's name would otherwise write extra lines into the item
    /// chunk — letting the name itself set `asset_id`, which is how a hostile
    /// inventory name rewrites where an embedded item points on save.
    #[test]
    fn a_newline_in_a_name_cannot_forge_a_field() -> Result<(), TestError> {
        let fields = ASSET_ID_FIELD.to_owned();
        let bytes = container(1, &entry(0, &fields), "body\n");
        let mut notecard = Notecard::decode(bytes.as_bytes())?;
        let real_asset_id = notecard.items.first().ok_or("no item")?.asset_id;
        {
            let item = notecard.items.first_mut().ok_or("no item")?;
            item.name = "harmless\n\t\tasset_id\t99999999-9999-9999-9999-999999999999".to_owned();
        }
        let round_tripped = Notecard::decode(&notecard.encode())?;
        let item = round_tripped.items.first().ok_or("no item")?;
        assert_eq!(
            item.asset_id, real_asset_id,
            "the forged asset_id line never reached the stream"
        );
        assert!(
            !item.name.contains('\n'),
            "the newline was replaced, not written"
        );
        Ok(())
    }

    /// A `|` terminates `name` / `desc` on the wire, so one inside the value is
    /// replaced rather than silently truncating the field on the way back in.
    #[test]
    fn a_pipe_in_a_name_is_replaced_not_truncated() -> Result<(), TestError> {
        let bytes = container(1, &entry(0, ASSET_ID_FIELD), "body\n");
        let mut notecard = Notecard::decode(bytes.as_bytes())?;
        {
            let item = notecard.items.first_mut().ok_or("no item")?;
            item.name = "a|b".to_owned();
            item.description = "c|d".to_owned();
        }
        let round_tripped = Notecard::decode(&notecard.encode())?;
        let item = round_tripped.items.first().ok_or("no item")?;
        assert_eq!(item.name, "a b");
        assert_eq!(item.description, "c d");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Embedded items are numbered by position, not by `ext char index`.
    // -----------------------------------------------------------------------

    /// The reference reads `ext char index` into a local it never uses and
    /// numbers items by load order, so a lone item written with `ext char index
    /// 1` is still the item marker index 0 names. Resolving by the stored index
    /// instead would render nothing here and an item in Firestorm.
    #[test]
    fn a_lying_ext_char_index_does_not_move_the_item() -> Result<(), TestError> {
        let marker = sl_notecard::embedded_char(0).ok_or("no marker")?;
        let bytes = container(1, &entry(1, ASSET_ID_FIELD), &format!("see {marker}\n"));
        let decoded = Notecard::decode(bytes.as_bytes())?;
        let reference = decoded.embedded_references().first().copied();
        let index = reference.ok_or("no reference")?.index;
        assert_eq!(index, 0);
        assert_eq!(
            decoded.item_by_index(index).ok_or("no item")?.name,
            "My Landmark"
        );
        Ok(())
    }

    /// And the line is rewritten to the position on save, as
    /// `exportEmbeddedItemsStream` does — so output re-indexed by the reference
    /// cannot reattach items to different markers.
    #[test]
    fn encode_renumbers_ext_char_index_to_the_position() -> Result<(), TestError> {
        let chunk = format!("{}{}", entry(7, ASSET_ID_FIELD), entry(3, ASSET_ID_FIELD));
        let bytes = container(2, &chunk, "body\n");
        let decoded = Notecard::decode(bytes.as_bytes())?;
        let encoded = String::from_utf8(decoded.encode())?;
        assert!(encoded.contains("ext char index 0\n"), "first item is 0");
        assert!(encoded.contains("ext char index 1\n"), "second item is 1");
        assert!(!encoded.contains("ext char index 7\n"));
        assert!(!encoded.contains("ext char index 3\n"));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Chunk framing.
    // -----------------------------------------------------------------------

    /// An unrecognised chunk-shaped field would desynchronise a parser that
    /// skips its `{`: the nested `}` then closes the *item* and every following
    /// field lands in the wrong one. There is no recovering the framing, so the
    /// decode fails instead of returning silently mangled inventory.
    #[test]
    fn a_nested_unknown_chunk_is_an_error() {
        let fields = format!("{ASSET_ID_FIELD}\t\tfuture_block\t0\n\t{{\n\t\tvalue\t1\n\t}}\n");
        let bytes = container(1, &entry(0, &fields), "body\n");
        assert!(
            Notecard::decode(bytes.as_bytes()).is_err(),
            "a chunk the decoder cannot frame is an error"
        );
    }

    /// Likewise a `permissions` line with no chunk body: tolerating the missing
    /// `{` makes the parser eat the item's own closing brace.
    #[test]
    fn a_permissions_field_without_a_chunk_is_an_error() {
        let bytes = "Linden text version 2\n{\nLLEmbeddedItems version 1\n{\ncount 1\n{\n\
ext char index 0\n\tinv_item\t0\n\t{\n\t\tpermissions 0\n\t}\n}\n}\nText length 0\n}\n";
        assert!(
            Notecard::decode(bytes.as_bytes()).is_err(),
            "a permissions field with no brace cannot be framed"
        );
    }

    // -----------------------------------------------------------------------
    // Versions.
    // -----------------------------------------------------------------------

    /// The reference refuses to import an `LLEmbeddedItems` chunk that is not
    /// version 1 (`Invalid LLEmbeddedItems version`); accepting any `u32` and
    /// echoing it back writes a container no viewer will read.
    #[test]
    fn a_non_one_embedded_items_version_is_rejected() {
        let bytes = "Linden text version 2\n{\nLLEmbeddedItems version 2\n{\ncount 0\n}\n\
Text length 0\n}\n";
        assert!(
            Notecard::decode(bytes.as_bytes()).is_err(),
            "only LLEmbeddedItems version 1 exists"
        );
    }

    /// A version 1 container upgrades to version 2 on save, and its chunk
    /// version is written as the constant 1 either way.
    #[test]
    fn encode_always_writes_the_current_versions() -> Result<(), TestError> {
        let bytes = b"Linden text version 1\n{\nLLEmbeddedItems version 1\n{\ncount 0\n}\n\
Text length 2\nhi}\n";
        let decoded = Notecard::decode(bytes)?;
        assert_eq!(decoded.source_version, NotecardVersion::V1);
        let encoded = String::from_utf8(decoded.encode())?;
        assert!(encoded.starts_with("Linden text version 2\n"));
        assert!(encoded.contains("LLEmbeddedItems version 1\n"));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Hostile and malformed streams.
    // -----------------------------------------------------------------------

    /// A `shadow_id` that is not a UUID is an error, not a null asset id
    /// silently XOR-ed into a plausible-looking key.
    #[test]
    fn a_malformed_shadow_id_is_rejected() {
        let fields = "\t\tshadow_id\tnot-a-uuid\n";
        let bytes = container(1, &entry(0, fields), "body\n");
        assert!(
            Notecard::decode(bytes.as_bytes()).is_err(),
            "an unparsable shadow_id is an error, not a null key"
        );
    }

    /// An item chunk that never closes runs the cursor off the end rather than
    /// looping or returning a half-built item.
    #[test]
    fn an_unterminated_item_chunk_is_rejected() {
        let bytes = "Linden text version 2\n{\nLLEmbeddedItems version 1\n{\ncount 1\n{\n\
ext char index 0\n\tinv_item\t0\n\t{\n\t\titem_id\t11111111-1111-1111-1111-111111111111\n";
        assert!(
            Notecard::decode(bytes.as_bytes()).is_err(),
            "an item chunk that never closes is an error"
        );
    }

    /// A container line that is not UTF-8 is reported as such — the text body
    /// of a version 1 notecard is bytes, but the container around it is not.
    #[test]
    fn a_non_utf8_container_line_is_rejected() {
        let mut bytes =
            b"Linden text version 2\n{\nLLEmbeddedItems version 1\n{\ncount 0\n}\n".to_vec();
        bytes.extend_from_slice(b"Text length \xff\xfe\n}\n");
        assert!(
            Notecard::decode(&bytes).is_err(),
            "a container line that is not UTF-8 is an error"
        );
    }

    /// A declared `count` the stream could never hold must not size the item
    /// vector — a covenant decodes through this path.
    #[test]
    fn a_count_beyond_the_stream_is_rejected() {
        let bytes = format!(
            "Linden text version 2\n{{\nLLEmbeddedItems version 1\n{{\ncount {}\n",
            u32::MAX
        );
        assert!(
            Notecard::decode(bytes.as_bytes()).is_err(),
            "a count the stream cannot hold is an error, not an allocation"
        );
    }

    /// An empty stream is an error, not an empty notecard.
    #[test]
    fn an_empty_stream_is_rejected() {
        assert!(
            Notecard::decode(b"").is_err(),
            "an empty stream has no container header"
        );
    }

    // -----------------------------------------------------------------------
    // The type-name tables are each other's inverse.
    // -----------------------------------------------------------------------

    /// Every asset-type name the decoder classifies is written back verbatim,
    /// so an item's type survives a round-trip through the two ~30-arm tables.
    #[test]
    fn asset_type_names_round_trip() {
        for name in [
            "texture", "sound", "callcard", "landmark", "script", "clothing", "object", "notecard",
            "category", "lsltext", "lslbyte", "txtr_tga", "bodypart", "snd_wav", "img_tga", "jpeg",
            "animatn", "gesture", "simstate", "link", "link_f", "mesh", "widget", "person",
            "settings", "material", "gltf", "glbin",
        ] {
            let parsed = AssetType::from_type_name(name);
            assert_eq!(parsed.type_name(), name, "asset type {name}");
            assert!(
                !matches!(parsed, AssetType::Other(_)),
                "asset type {name} is classified, not preserved verbatim"
            );
        }
    }

    /// An unrecognised asset-type name is preserved verbatim rather than being
    /// mapped onto a neighbour.
    #[test]
    fn an_unknown_asset_type_name_is_preserved() {
        let parsed = AssetType::from_type_name("futuretype");
        assert_eq!(parsed, AssetType::Other("futuretype".to_owned()));
        assert_eq!(parsed.type_name(), "futuretype");
    }

    /// The same for the inventory-type table, whose `type_name` additionally
    /// distinguishes "absent" from "unrecognised".
    #[test]
    fn inventory_type_names_round_trip() {
        for name in [
            "texture",
            "sound",
            "callcard",
            "landmark",
            "object",
            "notecard",
            "category",
            "root",
            "script",
            "snapshot",
            "attach",
            "wearable",
            "animation",
            "gesture",
            "mesh",
            "widget",
            "person",
            "settings",
            "material",
            "gltf",
            "glbin",
        ] {
            let parsed = InventoryType::from_type_name(name);
            assert_eq!(parsed.type_name(), Some(name), "inventory type {name}");
            assert!(
                !matches!(parsed, InventoryType::Other(_)),
                "inventory type {name} is classified, not preserved verbatim"
            );
        }
        assert_eq!(
            InventoryType::None.type_name(),
            None,
            "an absent field stays absent"
        );
        assert_eq!(
            InventoryType::from_type_name("futuretype").type_name(),
            Some("futuretype")
        );
    }

    /// And the sale-type table.
    #[test]
    fn sale_type_names_round_trip() {
        for name in ["not", "orig", "copy", "cntn"] {
            let parsed = SaleType::from_type_name(name);
            assert_eq!(parsed.type_name(), name, "sale type {name}");
            assert!(!matches!(parsed, SaleType::Other(_)));
        }
        assert_eq!(SaleType::from_type_name("auct").type_name(), "auct");
    }
}
