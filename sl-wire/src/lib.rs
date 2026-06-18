#![doc = include_str!("../README.md")]

mod control_flags;
mod endian;
mod error;
mod experience;
mod field;
mod header;
mod inventory;
mod llsd;
mod login;
mod material;
mod message;
/// Generated LLUDP message types and their (de)serialization, produced at build
/// time from the vendored `message_template.msg`.
pub mod messages;
mod parcel_flags;
mod voice;
mod zerocode;

pub use control_flags::ControlFlags;
pub use error::WireError;
pub use experience::{
    ExperienceInfo, ExperiencePermission, ExperienceProperties, ExperienceUpdate,
    PROPERTY_DISABLED, PROPERTY_GRID, PROPERTY_INVALID, PROPERTY_PRIVATE, PROPERTY_PRIVILEGED,
    PROPERTY_SUSPENDED, SEARCH_PAGE_SIZE, build_region_experiences_request,
    build_set_experience_permission_request, build_update_experience_request, experience_id_query,
    experience_info_query, find_experience_query, forget_experience_query, group_experiences_query,
    parse_experience_ids, parse_experience_infos, parse_experience_permissions,
    parse_experience_status, parse_region_experiences,
};
pub use field::{Reader, Writer};
pub use header::{PacketFlags, ParsedDatagram, encode_datagram, parse_datagram};
pub use inventory::{
    AIS_MAX_FOLDER_DEPTH, ais_category_children_fetch_url, ais_category_children_url,
    ais_category_url, ais_create_category_url, ais_item_url, build_ais_create_category_body,
    build_ais_move_body, build_ais_rename_category_body, build_ais_update_item_body,
    build_create_inventory_category_request,
};
pub use llsd::{
    AssetUploadResponse, EventQueueEvent, EventQueueResponse, Llsd, MEDIA_PERM_ALL,
    MEDIA_PERM_ANYONE, MEDIA_PERM_GROUP, MEDIA_PERM_NONE, MEDIA_PERM_OWNER, MediaEntry,
    ObjectMediaResponse, build_event_queue_request, build_fetch_inventory_request,
    build_group_member_data_request, build_group_notice_bucket,
    build_new_file_agent_inventory_request, build_object_media_get_request,
    build_object_media_navigate_request, build_object_media_update_request, build_seed_request,
    build_update_avatar_appearance_request, build_update_item_asset_request,
    build_upload_baked_texture_request, parse_asset_upload_response, parse_event_queue_response,
    parse_llsd_xml, parse_seed_response,
};
pub use login::{
    BuddyListEntry, Credential, HomeLocation, LoginFailure, LoginParseError, LoginRequest,
    LoginResponse, LoginServer, LoginSuccess, MfaChallenge, MfaPolicy, ParsedLoginRequest,
    SkeletonFolder, build_login_request, build_login_response, parse_login_request,
    parse_login_response, password_hash,
};
pub use material::{
    GLTF_MATERIAL_OVERRIDE_METHOD, GltfMaterialOverride, LegacyMaterial, MaterialOverrideUpdate,
    RenderMaterialEntry, build_modify_material_params_request, build_render_materials_request,
    parse_gltf_material_override, parse_render_materials_response,
};
pub use message::{Message, MessageId};
pub use messages::AnyMessage;
pub use parcel_flags::{ParcelFlags, RegionFlags, sim_access};
pub use voice::{
    IceCandidate, ParcelVoiceInfo, VOICE_SERVER_TYPE_VIVOX, VOICE_SERVER_TYPE_WEBRTC,
    VoiceAccountInfo, VoiceProvisionRequest, build_parcel_voice_info_request,
    build_provision_voice_account_request, build_voice_signaling_request,
};
pub use zerocode::{decode as zero_decode, encode as zero_encode};

/// Combines two UUIDs the way Second Life derives a legacy upload's asset id:
/// `MD5(a's 16 bytes ++ b's 16 bytes)` (LL's `LLUUID::combine` /
/// libomv's `UUID.Combine`). The simulator computes the stored asset's UUID as
/// `combine(transaction_id, secure_session_id)`, so a client can predict it (and
/// match the simulator's `RequestXfer`, whose `VFileID` is this asset id) before
/// the upload completes.
#[must_use]
pub fn combine_uuids(a: uuid::Uuid, b: uuid::Uuid) -> uuid::Uuid {
    let mut input = Vec::with_capacity(32);
    input.extend_from_slice(a.as_bytes());
    input.extend_from_slice(b.as_bytes());
    uuid::Uuid::from_bytes(md5::compute(&input).0)
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::{
        MediaEntry, ObjectMediaResponse, PacketFlags, Reader, WireError, Writer,
        build_group_notice_bucket, build_new_file_agent_inventory_request,
        build_object_media_get_request, build_object_media_navigate_request,
        build_object_media_update_request, build_update_item_asset_request, combine_uuids,
        encode_datagram, parse_asset_upload_response, parse_datagram, parse_llsd_xml, zero_decode,
        zero_encode,
    };

    #[test]
    fn group_notice_bucket_has_llsd_header() -> Result<(), WireError> {
        let item = uuid::Uuid::from_u128(0x4001);
        let owner = uuid::Uuid::from_u128(0x4002);
        let bucket = build_group_notice_bucket(item, owner);
        // OpenSim strips exactly 15 bytes of LLSD pre-header before parsing the
        // remaining map, so it must lead with the viewer's `<? LLSD/XML ?>\n`.
        let header = b"<? LLSD/XML ?>\n";
        assert_eq!(header.len(), 15);
        assert_eq!(bucket.get(..15), Some(header.as_slice()));
        // The remaining bytes are a parseable LLSD map carrying both ids.
        let body = String::from_utf8_lossy(bucket.get(15..).ok_or(WireError::ShortHeader)?);
        let parsed = parse_llsd_xml(&body).map_err(|_e| WireError::ShortHeader)?;
        assert!(body.contains(&item.to_string()));
        assert!(body.contains(&owner.to_string()));
        // Sanity: it really is a map.
        assert!(matches!(parsed, super::Llsd::Map(_)));
        Ok(())
    }

    #[test]
    fn field_round_trip() -> Result<(), WireError> {
        let mut w = Writer::new();
        w.put_u8(0x12);
        w.put_bool(true);
        w.put_u16(0xABCD);
        w.put_u32(0x0123_4567);
        w.put_u64(0x0011_2233_4455_6677);
        w.put_i16(-5);
        w.put_i32(-100_000);
        w.put_f32(1.5);
        w.put_f64(-2.25);
        w.put_variable1(b"hello")?;
        w.put_variable2(b"world")?;
        let bytes = w.into_bytes();

        let mut r = Reader::new(&bytes);
        assert_eq!(r.u8()?, 0x12);
        assert!(r.bool()?);
        assert_eq!(r.u16()?, 0xABCD);
        assert_eq!(r.u32()?, 0x0123_4567);
        assert_eq!(r.u64()?, 0x0011_2233_4455_6677);
        assert_eq!(r.i16()?, -5);
        assert_eq!(r.i32()?, -100_000);
        assert_eq!(r.f32()?.to_bits(), 1.5_f32.to_bits());
        assert_eq!(r.f64()?.to_bits(), (-2.25_f64).to_bits());
        assert_eq!(r.variable1()?, b"hello");
        assert_eq!(r.variable2()?, b"world");
        assert!(r.is_empty());
        Ok(())
    }

    #[test]
    fn reader_underflow_is_an_error() {
        let mut r = Reader::new(&[0x01, 0x02]);
        assert!(matches!(r.u32(), Err(WireError::UnexpectedEof { .. })));
    }

    #[test]
    fn little_endian_byte_order_on_the_wire() {
        let mut w = Writer::new();
        w.put_u32(0x0102_0304);
        // Little-endian: least significant byte first.
        assert_eq!(w.as_bytes(), &[0x04, 0x03, 0x02, 0x01]);
    }

    #[test]
    fn zerocode_round_trips() -> Result<(), WireError> {
        let cases: &[&[u8]] = &[
            &[],
            &[1, 2, 3],
            &[0, 0, 0, 0],
            &[1, 0, 0, 2, 0, 3],
            &[0xFF, 0x00, 0xFF, 0x00],
        ];
        for original in cases {
            let encoded = zero_encode(original);
            let decoded = zero_decode(&encoded)?;
            assert_eq!(&decoded, original);
        }
        Ok(())
    }

    #[test]
    fn zerocode_long_run_round_trips() -> Result<(), WireError> {
        let original = vec![0u8; 600];
        let encoded = zero_encode(&original);
        assert!(encoded.len() < original.len());
        assert_eq!(zero_decode(&encoded)?, original);
        Ok(())
    }

    #[test]
    fn zerocode_decodes_a_known_run() -> Result<(), WireError> {
        // `0x00 0x03` decodes to three zero bytes around literal data.
        assert_eq!(
            zero_decode(&[0x01, 0x00, 0x03, 0x02])?,
            vec![0x01, 0, 0, 0, 0x02]
        );
        Ok(())
    }

    #[test]
    fn zerocode_truncated_marker_errors() {
        assert!(matches!(
            zero_decode(&[0x01, 0x00]),
            Err(WireError::TruncatedZerocode)
        ));
    }

    #[test]
    fn datagram_header_round_trip() -> Result<(), WireError> {
        let body = [0xDE, 0xAD, 0xBE, 0xEF];
        let datagram = encode_datagram(PacketFlags::RELIABLE, 0x0001_0203, &body);
        // Sequence number is big-endian in the header.
        assert_eq!(datagram.get(1..5), Some(&[0x00, 0x01, 0x02, 0x03][..]));

        let parsed = parse_datagram(&datagram)?;
        assert_eq!(parsed.flags, PacketFlags::RELIABLE);
        assert_eq!(parsed.sequence, 0x0001_0203);
        assert!(parsed.extra.is_empty());
        assert!(parsed.acks.is_empty());
        assert_eq!(parsed.body, &body);
        Ok(())
    }

    #[test]
    fn parse_datagram_strips_appended_acks() -> Result<(), WireError> {
        // Hand-build a datagram with the ACK flag and two big-endian acks.
        let mut datagram = vec![PacketFlags::ACK.bits()];
        datagram.extend_from_slice(&[0x00, 0x00, 0x00, 0x09]); // sequence
        datagram.push(0x00); // extra length
        datagram.extend_from_slice(&[0xAA, 0xBB]); // body
        datagram.extend_from_slice(&[0x00, 0x00, 0x00, 0x07]); // ack 7 (big-endian)
        datagram.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]); // ack 8 (big-endian)
        datagram.push(0x02); // ack count

        let parsed = parse_datagram(&datagram)?;
        assert_eq!(parsed.sequence, 9);
        assert_eq!(parsed.acks, vec![7, 8]);
        assert_eq!(parsed.body, &[0xAA, 0xBB]);
        Ok(())
    }

    #[test]
    fn short_datagram_is_rejected() {
        assert!(matches!(
            parse_datagram(&[0x00, 0x01]),
            Err(WireError::ShortHeader)
        ));
    }

    #[test]
    fn parse_never_panics_on_arbitrary_bytes() {
        // Poke the parser with many short/odd inputs; it must always return
        // (Ok or Err), never panic, under the no-panic lints.
        for seed in 0usize..=2000 {
            let len = seed % 23;
            let bytes: Vec<u8> = (0..len)
                .map(|i| u8::try_from(seed.wrapping_add(i) % 256).unwrap_or(0))
                .collect();
            let _result = parse_datagram(&bytes);
            let _decoded = zero_decode(&bytes);
        }
    }

    #[test]
    fn combine_uuids_matches_md5_of_concatenated_bytes() {
        // `combine(a, b)` is MD5 of a's 16 bytes followed by b's 16 bytes (LL's
        // `LLUUID::combine`). Check against a hand-computed digest.
        let a = uuid::Uuid::from_u128(1);
        let b = uuid::Uuid::from_u128(3);
        let mut input = Vec::with_capacity(32);
        input.extend_from_slice(a.as_bytes());
        input.extend_from_slice(b.as_bytes());
        let expected = uuid::Uuid::from_bytes(md5::compute(&input).0);
        assert_eq!(combine_uuids(a, b), expected);
    }

    #[test]
    fn new_file_agent_inventory_request_carries_metadata() {
        let folder = uuid::Uuid::from_u128(0x00f0_1de7);
        let body = build_new_file_agent_inventory_request(
            folder,
            "texture",
            "texture",
            "My Pic",
            "a desc",
            0x0008_e000,
            0,
            0,
            0,
        );
        assert!(body.contains(&format!("<uuid>{folder}</uuid>")));
        assert!(body.contains("<key>asset_type</key><string>texture</string>"));
        assert!(body.contains("<key>inventory_type</key><string>texture</string>"));
        assert!(body.contains("<key>name</key><string>My Pic</string>"));
        assert!(body.contains("<key>expected_upload_cost</key><integer>0</integer>"));
    }

    #[test]
    fn upload_response_parses_both_steps_and_failure() -> Result<(), roxmltree::Error> {
        // Step 1: the uploader URL.
        let step1 = parse_asset_upload_response(
            "<llsd><map><key>state</key><string>upload</string>\
             <key>uploader</key><string>http://sim/up/42</string></map></llsd>",
        )?;
        assert_eq!(step1.state, "upload");
        assert_eq!(step1.uploader.as_deref(), Some("http://sim/up/42"));
        assert_eq!(step1.new_asset, None);

        // Step 2: completion with new_asset (string) + new_inventory_item (uuid).
        let asset = uuid::Uuid::from_u128(0x000a_55e7);
        let item = uuid::Uuid::from_u128(0x17e3);
        let step2 = parse_asset_upload_response(&format!(
            "<llsd><map><key>state</key><string>complete</string>\
             <key>new_asset</key><string>{asset}</string>\
             <key>new_inventory_item</key><uuid>{item}</uuid></map></llsd>"
        ))?;
        assert_eq!(step2.new_asset, Some(asset));
        assert_eq!(step2.new_inventory_item, Some(item));

        // A baked-texture completion has a nil inventory item → None.
        let baked = parse_asset_upload_response(&format!(
            "<llsd><map><key>state</key><string>complete</string>\
             <key>new_asset</key><string>{asset}</string>\
             <key>new_inventory_item</key>\
             <uuid>00000000-0000-0000-0000-000000000000</uuid></map></llsd>"
        ))?;
        assert_eq!(baked.new_asset, Some(asset));
        assert_eq!(baked.new_inventory_item, None);

        // An error response surfaces the message.
        let failed = parse_asset_upload_response(
            "<llsd><map><key>state</key><string>error</string>\
             <key>error</key><string>insufficient funds</string></map></llsd>",
        )?;
        assert_eq!(failed.state, "error");
        assert_eq!(failed.uploader, None);
        assert_eq!(failed.error.as_deref(), Some("insufficient funds"));
        Ok(())
    }

    #[test]
    fn update_item_asset_request_carries_item_id() {
        let item = uuid::Uuid::from_u128(0x17e3);
        let body = build_update_item_asset_request(item);
        assert!(body.contains(&format!("<key>item_id</key><uuid>{item}</uuid>")));
    }

    #[test]
    fn object_media_get_and_navigate_requests_carry_fields() {
        let object = uuid::Uuid::from_u128(0x000b_1ec7);
        let get = build_object_media_get_request(object);
        assert!(get.contains("<key>verb</key><string>GET</string>"));
        assert!(get.contains(&format!("<key>object_id</key><uuid>{object}</uuid>")));

        let navigate = build_object_media_navigate_request(object, 3, "https://example.com/a&b");
        assert!(navigate.contains(&format!("<key>object_id</key><uuid>{object}</uuid>")));
        // The URL is XML-escaped.
        assert!(
            navigate.contains("<key>current_url</key><string>https://example.com/a&amp;b</string>")
        );
        assert!(navigate.contains("<key>texture_index</key><integer>3</integer>"));
    }

    #[test]
    fn object_media_update_round_trips_through_a_get_response()
    -> Result<(), Box<dyn std::error::Error>> {
        let object = uuid::Uuid::from_u128(0x000b_1ec7);
        let entry = MediaEntry {
            current_url: "https://example.com/stream".to_owned(),
            home_url: "https://example.com/home".to_owned(),
            auto_play: true,
            auto_scale: true,
            width_pixels: 1024,
            height_pixels: 512,
            controls: 1,
            perms_interact: super::MEDIA_PERM_OWNER,
            whitelist_enable: true,
            whitelist: vec!["*.example.com".to_owned()],
            ..MediaEntry::default()
        };
        // A two-face update: face 0 has media, face 1 has none (an LLSD undef).
        let body = build_object_media_update_request(object, &[Some(entry.clone()), None]);
        assert!(body.contains("<key>verb</key><string>UPDATE</string>"));
        assert!(body.contains("<undef />"));

        // The UPDATE body is itself valid LLSD with the same `object_id` /
        // `object_media_data` shape the simulator echoes in a GET reply, so
        // decoding it back exercises the per-face serialize → parse round-trip.
        let parsed = parse_llsd_xml(&body)?;
        let response =
            ObjectMediaResponse::from_llsd(&parsed).ok_or("object_media body should decode")?;
        assert_eq!(response.object_id, object);
        assert_eq!(response.faces.len(), 2);
        assert_eq!(response.faces.first(), Some(&Some(entry)));
        assert_eq!(response.faces.get(1), Some(&None));
        Ok(())
    }
}
