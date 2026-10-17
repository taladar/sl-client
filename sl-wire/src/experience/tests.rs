//! Round-trip tests for the experience cap codecs.

use pretty_assertions::assert_eq;
use sl_types::key::{AgentKey, ExperienceKey, GroupKey};
use uuid::Uuid;

use super::{
    ExperienceInfo, ExperienceLocation, ExperiencePermission, ExperienceProperties,
    ExperienceUpdate, PROPERTY_GRID, PROPERTY_INVALID, RegionExperienceLists,
    build_experience_ids_response, build_experience_infos_response,
    build_experience_permissions_response, build_experience_query_response,
    build_experience_search_response, build_experience_status_response,
    build_region_experiences_request, build_region_experiences_response,
    build_set_experience_permission_request, build_update_experience_request, experience_id_query,
    experience_info_query, experience_query, find_experience_query, forget_experience_query,
    group_experiences_query, parse_experience_id_query, parse_experience_ids,
    parse_experience_info_query, parse_experience_infos, parse_experience_permissions,
    parse_experience_query, parse_experience_query_reply, parse_experience_search_page,
    parse_experience_status, parse_find_experience_query, parse_forget_experience_query,
    parse_group_experiences_query, parse_region_experiences, parse_region_experiences_request,
    parse_set_experience_permission_request, parse_update_experience_request,
};
use crate::WireError;
use crate::llsd::parse_llsd_xml;

/// Parses a UUID in a test, surfacing a `String` error for the `?` operator.
fn uuid(text: &str) -> Result<Uuid, String> {
    Uuid::parse_str(text).map_err(|error| error.to_string())
}

/// Parses a UUID as an [`ExperienceKey`] in a test.
fn experience_key(text: &str) -> Result<ExperienceKey, String> {
    Ok(ExperienceKey::from(uuid(text)?))
}

/// `GetExperienceInfo` batches every id as a `public_id` query parameter under
/// the `id/` path, and its `experience_keys` decode into full records while
/// `error_ids` become `missing` placeholders.
#[test]
fn experience_info_query_and_decode() -> Result<(), String> {
    let id = experience_key("11111111-1111-1111-1111-111111111111")?;
    let suffix = experience_info_query(&[id]);
    assert_eq!(
        suffix,
        "/id/?page_size=1&public_id=11111111-1111-1111-1111-111111111111"
    );

    let reply = parse_llsd_xml(concat!(
        "<llsd><map><key>experience_keys</key><array><map>",
        "<key>public_id</key><uuid>11111111-1111-1111-1111-111111111111</uuid>",
        "<key>name</key><string>My Experience</string>",
        "<key>agent_id</key><uuid>22222222-2222-2222-2222-222222222222</uuid>",
        "<key>properties</key><integer>16</integer>",
        "<key>maturity</key><integer>13</integer>",
        "<key>description</key><string>fun</string>",
        "<key>slurl</key><string>http://maps/x</string>",
        "</map></array>",
        "<key>error_ids</key><array>",
        "<uuid>33333333-3333-3333-3333-333333333333</uuid></array>",
        "</map></llsd>"
    ))
    .map_err(|error| format!("{error:?}"))?;
    let infos = parse_experience_infos(&reply).map_err(|error| format!("{error:?}"))?;
    let [first, second] = infos.as_slice() else {
        return Err(format!("expected 2 infos, got {}", infos.len()));
    };
    assert_eq!(first.public_id, id);
    assert_eq!(first.name, "My Experience");
    assert!(first.properties.is_grid());
    assert_eq!(first.maturity, 13);
    assert!(!first.missing);
    assert!(second.missing);
    assert!(second.properties.is_invalid());
    Ok(())
}

/// An `experience_keys` element without a `public_id` is rejected: `public_id`
/// is the experience key every record is filed under (the only field the
/// Firestorm cache guards), so a record lacking it is meaningless and decoding
/// is a hard [`WireError::Llsd`].
#[test]
fn experience_info_missing_public_id_errors() -> Result<(), String> {
    let reply = parse_llsd_xml(concat!(
        "<llsd><map><key>experience_keys</key><array><map>",
        "<key>name</key><string>No Id</string>",
        "</map></array></map></llsd>"
    ))
    .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        parse_experience_infos(&reply),
        Err(WireError::Llsd(crate::LlsdError::MissingField {
            field: "public_id"
        }))
    );
    Ok(())
}

/// The search query escapes its text and carries the page / page-size.
#[test]
fn find_experience_query_escapes() {
    assert_eq!(
        find_experience_query("a b&c", 2),
        "?page=2&page_size=30&query=a%20b%26c"
    );
}

/// `experience_ids` and `{ experiences, blocked }` replies decode to id lists.
#[test]
fn id_list_and_permission_decode() -> Result<(), String> {
    let ids_reply = parse_llsd_xml(concat!(
        "<llsd><map><key>experience_ids</key><array>",
        "<uuid>11111111-1111-1111-1111-111111111111</uuid>",
        "<uuid>22222222-2222-2222-2222-222222222222</uuid>",
        "</array></map></llsd>"
    ))
    .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        parse_experience_ids(&ids_reply)
            .map_err(|error| format!("{error:?}"))?
            .len(),
        2
    );

    let prefs = parse_llsd_xml(concat!(
        "<llsd><map>",
        "<key>experiences</key><array><uuid>11111111-1111-1111-1111-111111111111</uuid></array>",
        "<key>blocked</key><array><uuid>22222222-2222-2222-2222-222222222222</uuid></array>",
        "</map></llsd>"
    ))
    .map_err(|error| format!("{error:?}"))?;
    let (allowed, blocked) =
        parse_experience_permissions(&prefs).map_err(|error| format!("{error:?}"))?;
    assert_eq!(allowed.len(), 1);
    assert_eq!(blocked.len(), 1);
    Ok(())
}

/// The `Allow` permission PUT body nests the permission under the id key.
#[test]
fn set_permission_body() -> Result<(), String> {
    let id = experience_key("11111111-1111-1111-1111-111111111111")?;
    let body = build_set_experience_permission_request(id, ExperiencePermission::Allow);
    assert_eq!(
        body,
        "<llsd><map><key>11111111-1111-1111-1111-111111111111</key><map><key>permission</key><string>Allow</string></map></map></llsd>"
    );
    Ok(())
}

/// The `UpdateExperience` POST body carries the editable fields and round-trips
/// the reply back through the info decoder (a bare experience map).
#[test]
fn update_experience_round_trip() -> Result<(), String> {
    let id = experience_key("11111111-1111-1111-1111-111111111111")?;
    let update = ExperienceUpdate {
        public_id: id,
        name: "Renamed".to_owned(),
        description: "desc".to_owned(),
        maturity: 13,
        properties: PROPERTY_GRID,
        slurl: Some(ExperienceLocation::Slurl(
            url::Url::parse("http://maps/y").map_err(|e| e.to_string())?,
        )),
        extended_metadata: String::new(),
        group: None,
    };
    let body = build_update_experience_request(&update);
    assert!(body.contains("<key>public_id</key><uuid>11111111-1111-1111-1111-111111111111</uuid>"));
    // No group is the nil id, as the reference sends the record's `group_id`.
    assert!(body.contains("<key>group_id</key><uuid>00000000-0000-0000-0000-000000000000</uuid>"));
    assert!(body.contains("<key>name</key><string>Renamed</string>"));
    assert!(body.contains("<key>properties</key><integer>16</integer>"));
    assert!(!body.contains("quota"));

    let reply = parse_llsd_xml(concat!(
        "<llsd><map>",
        "<key>public_id</key><uuid>11111111-1111-1111-1111-111111111111</uuid>",
        "<key>name</key><string>Renamed</string>",
        "</map></llsd>"
    ))
    .map_err(|error| format!("{error:?}"))?;
    let infos = parse_experience_infos(&reply).map_err(|error| format!("{error:?}"))?;
    let [info] = infos.as_slice() else {
        return Err(format!("expected 1 info, got {}", infos.len()));
    };
    assert_eq!(info.name, "Renamed");
    Ok(())
}

/// `RegionExperiences` round-trips its three id lists through the body builder
/// and the reply decoder.
#[test]
fn region_experiences_round_trip() -> Result<(), String> {
    let allowed = [experience_key("11111111-1111-1111-1111-111111111111")?];
    let trusted = [experience_key("22222222-2222-2222-2222-222222222222")?];
    let body = build_region_experiences_request(&allowed, &[], &trusted);
    assert!(body.contains(
        "<key>allowed</key><array><uuid>11111111-1111-1111-1111-111111111111</uuid></array>"
    ));
    assert!(body.contains("<key>blocked</key><array></array>"));
    assert!(body.contains(
        "<key>trusted</key><array><uuid>22222222-2222-2222-2222-222222222222</uuid></array>"
    ));

    let reply = parse_llsd_xml(&body).map_err(|error| format!("{error:?}"))?;
    let lists = parse_region_experiences(&reply).map_err(|error| format!("{error:?}"))?;
    assert_eq!(lists.allowed, allowed);
    assert!(lists.blocked.is_empty());
    assert_eq!(lists.trusted, trusted);
    // The POST body the builder writes carries no `default` — the reference's
    // `sendUpdate` does not send one — so the decoder must not invent one.
    assert_eq!(lists.default_experience, None);
    Ok(())
}

/// The reply's optional `default` key decodes into
/// [`RegionExperienceLists::default_experience`], and a reply without it (or
/// with an unreadable one) decodes as no default rather than as an error — the
/// reference reads the key by presence and then by `asUUID()`.
#[test]
fn region_experiences_default_is_optional() -> Result<(), String> {
    let default = experience_key("33333333-3333-3333-3333-333333333333")?;
    let decode = |xml: &str| -> Result<Option<ExperienceKey>, String> {
        let body = parse_llsd_xml(xml).map_err(|error| format!("{error:?}"))?;
        Ok(parse_region_experiences(&body)
            .map_err(|error| format!("{error:?}"))?
            .default_experience)
    };

    assert_eq!(
        decode(
            "<llsd><map><key>default</key><uuid>33333333-3333-3333-3333-333333333333</uuid></map></llsd>"
        )?,
        Some(default),
    );
    // A grid that spells it as a string is read the same way `llsd_uuid` reads
    // every other id in this family.
    assert_eq!(
        decode(
            "<llsd><map><key>default</key><string>33333333-3333-3333-3333-333333333333</string></map></llsd>"
        )?,
        Some(default),
    );
    assert_eq!(decode("<llsd><map /></llsd>")?, None);
    assert_eq!(
        decode("<llsd><map><key>default</key><undef /></map></llsd>")?,
        None,
    );
    Ok(())
}

/// The `{ status }` boolean decodes, and the property helpers read the bits.
#[test]
fn status_and_properties() -> Result<(), String> {
    let reply = parse_llsd_xml("<llsd><map><key>status</key><boolean>1</boolean></map></llsd>")
        .map_err(|error| format!("{error:?}"))?;
    assert!(parse_experience_status(&reply).map_err(|error| format!("{error:?}"))?);

    assert_eq!(
        build_experience_status_response(true),
        "<llsd><map><key>status</key><boolean>true</boolean></map></llsd>"
    );

    let props = ExperienceProperties(PROPERTY_GRID);
    assert!(props.is_grid());
    assert!(!props.is_private());
    assert_eq!(
        ExperienceInfo::default().properties,
        ExperienceProperties(0)
    );
    Ok(())
}

/// The `GetExperienceInfo` URL suffix round-trips through its parser, batching
/// every requested id back out of the `public_id` query parameters.
#[test]
fn experience_info_query_round_trip() -> Result<(), String> {
    let ids = [
        experience_key("11111111-1111-1111-1111-111111111111")?,
        experience_key("22222222-2222-2222-2222-222222222222")?,
    ];
    let suffix = experience_info_query(&ids);
    assert_eq!(parse_experience_info_query(&suffix), Ok(ids.to_vec()));
    Ok(())
}

/// The search query round-trips, recovering the percent-decoded text and page.
#[test]
fn find_experience_query_round_trip() {
    let suffix = find_experience_query("a b&c", 2);
    assert_eq!(
        parse_find_experience_query(&suffix),
        Some(("a b&c".to_owned(), 2))
    );
}

/// The bare-UUID query forms (group, forget) and the `experience_id=` form
/// each round-trip through their parsers.
#[test]
fn uuid_query_round_trips() -> Result<(), String> {
    let id = uuid("11111111-1111-1111-1111-111111111111")?;
    let key = ExperienceKey::from(id);
    assert_eq!(
        parse_group_experiences_query(&group_experiences_query(id)),
        Some(id)
    );
    assert_eq!(
        parse_forget_experience_query(&forget_experience_query(key)),
        Some(key)
    );
    assert_eq!(
        parse_experience_id_query(&experience_id_query(key)),
        Some(key)
    );
    Ok(())
}

/// The `ExperiencePreferences` PUT body round-trips builder → parser, and the
/// `{ experiences, blocked }` reply round-trips builder → parser.
#[test]
fn permission_request_and_reply_round_trip() -> Result<(), String> {
    let id = experience_key("11111111-1111-1111-1111-111111111111")?;
    let body = build_set_experience_permission_request(id, ExperiencePermission::Block);
    let parsed =
        parse_set_experience_permission_request(&body).map_err(|error| format!("{error:?}"))?;
    assert_eq!(parsed, (id, ExperiencePermission::Block));

    let allowed = [id];
    let blocked = [experience_key("22222222-2222-2222-2222-222222222222")?];
    let reply = build_experience_permissions_response(&allowed, &blocked);
    let parsed = parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?;
    let (allowed_out, blocked_out) =
        parse_experience_permissions(&parsed).map_err(|error| format!("{error:?}"))?;
    assert_eq!(allowed_out, allowed);
    assert_eq!(blocked_out, blocked);
    Ok(())
}

/// The `UpdateExperience` POST body round-trips builder → parser.
#[test]
fn update_experience_request_round_trip() -> Result<(), String> {
    let update = ExperienceUpdate {
        public_id: experience_key("11111111-1111-1111-1111-111111111111")?,
        name: "Renamed".to_owned(),
        description: "desc & more".to_owned(),
        maturity: 13,
        properties: PROPERTY_GRID,
        slurl: Some(ExperienceLocation::Last),
        extended_metadata: "<x/>".to_owned(),
        group: Some(GroupKey::from(uuid(
            "44444444-4444-4444-4444-444444444444",
        )?)),
    };
    let body = build_update_experience_request(&update);
    let parsed = parse_update_experience_request(&body).map_err(|error| format!("{error:?}"))?;
    assert_eq!(parsed, update);
    Ok(())
}

/// The `RegionExperiences` POST body and reply each round-trip through their
/// request parser / response builder.
#[test]
fn region_experiences_service_round_trip() -> Result<(), String> {
    let allowed = [experience_key("11111111-1111-1111-1111-111111111111")?];
    let trusted = [experience_key("22222222-2222-2222-2222-222222222222")?];
    let request = build_region_experiences_request(&allowed, &[], &trusted);
    let posted =
        parse_region_experiences_request(&request).map_err(|error| format!("{error:?}"))?;
    assert_eq!(posted.allowed, allowed);
    assert!(posted.blocked.is_empty());
    assert_eq!(posted.trusted, trusted);
    assert_eq!(posted.default_experience, None);

    let served = RegionExperienceLists {
        allowed: allowed.to_vec(),
        blocked: Vec::new(),
        trusted: trusted.to_vec(),
        default_experience: Some(experience_key("33333333-3333-3333-3333-333333333333")?),
    };
    let reply = build_region_experiences_response(&served);
    let decoded =
        parse_region_experiences(&parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?)
            .map_err(|error| format!("{error:?}"))?;
    assert_eq!(decoded, served);

    // An estate with no default writes the nil id, as Second Life does, and
    // the round trip still names nothing to be sticky about.
    let plain = RegionExperienceLists {
        default_experience: None,
        ..served
    };
    let reply = build_region_experiences_response(&plain);
    assert!(reply.contains("<key>default</key><uuid>00000000-0000-0000-0000-000000000000</uuid>"));
    let decoded =
        parse_region_experiences(&parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?)
            .map_err(|error| format!("{error:?}"))?;
    assert_eq!(decoded, plain);
    Ok(())
}

/// The `ExperienceQuery` GET round-trips query → parser, spelling the parcel id
/// and the comma-joined id list exactly as the reference builds them, and its
/// `{ experiences }` reply round-trips builder → parser in id order.
#[test]
fn experience_query_round_trip() -> Result<(), String> {
    let first = experience_key("11111111-1111-1111-1111-111111111111")?;
    let second = experience_key("22222222-2222-2222-2222-222222222222")?;
    let suffix = experience_query(7, &[first, second]);
    assert_eq!(
        suffix,
        "?parcelid=7&experiences=11111111-1111-1111-1111-111111111111,\
         22222222-2222-2222-2222-222222222222"
    );
    assert_eq!(
        parse_experience_query(&suffix),
        Some((7, vec![first, second]))
    );

    // Nothing is injecting: the reference writes the parameter name only before
    // the first id, so an empty list leaves it out altogether.
    assert_eq!(experience_query(-1, &[]), "?parcelid=-1");
    assert_eq!(
        parse_experience_query("?parcelid=-1"),
        Some((-1, Vec::new()))
    );
    assert_eq!(parse_experience_query("?experiences="), None);

    let reply = build_experience_query_response(&[(first, true), (second, false)]);
    let parsed = parse_experience_query_reply(
        &parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?,
    )
    .map_err(|error| format!("{error:?}"))?;
    assert_eq!(parsed, vec![(first, true), (second, false)]);
    Ok(())
}

/// An `ExperienceQuery` reply entry the viewer cannot read is skipped rather
/// than read as a refusal: the reference acts only on the entries that say
/// *no*, and a garbled one must not clear an experience's sky.
#[test]
fn experience_query_reply_skips_unreadable_entries() -> Result<(), String> {
    let reply = parse_llsd_xml(concat!(
        "<llsd><map><key>experiences</key><map>",
        "<key>11111111-1111-1111-1111-111111111111</key><boolean>0</boolean>",
        "<key>not-a-uuid</key><boolean>0</boolean>",
        "<key>22222222-2222-2222-2222-222222222222</key><string>no</string>",
        "</map></map></llsd>"
    ))
    .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        parse_experience_query_reply(&reply).map_err(|error| format!("{error:?}"))?,
        vec![(
            experience_key("11111111-1111-1111-1111-111111111111")?,
            false
        )]
    );
    Ok(())
}

/// The `experience_ids` reply round-trips builder → parser.
#[test]
fn experience_ids_response_round_trip() -> Result<(), String> {
    let ids = [
        experience_key("11111111-1111-1111-1111-111111111111")?,
        experience_key("22222222-2222-2222-2222-222222222222")?,
    ];
    let reply = build_experience_ids_response(&ids);
    let parsed =
        parse_experience_ids(&parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?)
            .map_err(|error| format!("{error:?}"))?;
    assert_eq!(parsed, ids);
    Ok(())
}

/// The `GetExperienceInfo` reply round-trips a full record through
/// `experience_keys` and a missing id through `error_ids`.
#[test]
fn experience_infos_response_round_trip() -> Result<(), String> {
    let real = ExperienceInfo {
        public_id: experience_key("11111111-1111-1111-1111-111111111111")?,
        name: "My Experience".to_owned(),
        owner: Some(AgentKey::from(uuid(
            "22222222-2222-2222-2222-222222222222",
        )?)),
        group: Some(GroupKey::from(uuid(
            "44444444-4444-4444-4444-444444444444",
        )?)),
        description: "fun & games".to_owned(),
        properties: ExperienceProperties(PROPERTY_GRID),
        maturity: 13,
        slurl: Some(ExperienceLocation::Slurl(
            url::Url::parse("http://maps/x").map_err(|e| e.to_string())?,
        )),
        ..ExperienceInfo::default()
    };
    let missing = ExperienceInfo {
        public_id: experience_key("33333333-3333-3333-3333-333333333333")?,
        properties: ExperienceProperties(PROPERTY_INVALID),
        missing: true,
        ..ExperienceInfo::default()
    };
    let reply = build_experience_infos_response(&[real.clone(), missing.clone()]);
    let infos =
        parse_experience_infos(&parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?)
            .map_err(|error| format!("{error:?}"))?;
    let [first, second] = infos.as_slice() else {
        return Err(format!("expected 2 infos, got {}", infos.len()));
    };
    assert_eq!(*first, real);
    assert_eq!(*second, missing);
    Ok(())
}

/// A `FindExperienceByName` reply round-trips its records *and* its two paging
/// markers, and a neighbour that does not exist is a key that is not there —
/// which is the only thing the reference viewer reads.
#[test]
fn experience_search_page_round_trips() -> Result<(), String> {
    let hit = ExperienceInfo {
        public_id: experience_key("11111111-1111-1111-1111-111111111111")?,
        name: "Magic Quest".to_owned(),
        properties: ExperienceProperties(PROPERTY_GRID),
        maturity: 13,
        ..ExperienceInfo::default()
    };
    let page = |next: Option<&str>, previous: Option<&str>| -> Result<_, String> {
        let reply = build_experience_search_response(std::slice::from_ref(&hit), next, previous);
        parse_experience_search_page(&parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?)
            .map_err(|error| format!("{error:?}"))
    };

    let middle = page(Some("?page=3&page_size=30&query=magic"), Some("?page=1"))?;
    assert_eq!(middle.infos, vec![hit.clone()]);
    assert!(middle.has_next_page);
    assert!(middle.has_previous_page);

    let only = page(None, None)?;
    assert_eq!(only.infos, vec![hit]);
    assert!(!only.has_next_page);
    assert!(!only.has_previous_page);
    Ok(())
}

/// A plain `{ experience_keys }` reply — a grid that models no paging at all —
/// decodes as a page with neither neighbour rather than as an error, so the
/// viewer simply offers no arrows.
#[test]
fn experience_search_page_without_markers_offers_no_paging() -> Result<(), String> {
    let reply = build_experience_infos_response(&[]);
    let page = parse_experience_search_page(
        &parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?,
    )
    .map_err(|error| format!("{error:?}"))?;
    assert_eq!(page, crate::ExperienceSearchPage::default());
    Ok(())
}

/// The owner and the group are two fields, as Second Life sends them: a
/// group's experience carries both (every record aditi served named an agent
/// owner, and some a group beside it), and either may be absent — each
/// combination round-trips through `to_llsd`/`from_llsd`.
#[test]
fn experience_owner_and_group_round_trip() -> Result<(), String> {
    let base = ExperienceInfo {
        public_id: experience_key("11111111-1111-1111-1111-111111111111")?,
        name: "X".to_owned(),
        ..ExperienceInfo::default()
    };
    let agent = Some(AgentKey::from(uuid(
        "22222222-2222-2222-2222-222222222222",
    )?));
    let group = Some(GroupKey::from(uuid(
        "33333333-3333-3333-3333-333333333333",
    )?));
    for (owner, group) in [(agent, None), (agent, group), (None, group), (None, None)] {
        let info = ExperienceInfo {
            owner,
            group,
            ..base.clone()
        };
        let decoded =
            ExperienceInfo::from_llsd(&info.to_llsd()).map_err(|error| format!("{error:?}"))?;
        assert_eq!((decoded.owner, decoded.group), (owner, group));
    }
    Ok(())
}

/// A search page as aditi serves it (2026-10-16): records whose `slurl` is a
/// location URL, empty, or the word `last`; a group experience with both ids;
/// `maturity` 254; `total_pages` and the paging URLs as relative paths. One
/// `last` used to refuse the whole page, so a search showed nothing.
#[test]
fn an_aditi_search_page_decodes() -> Result<(), String> {
    let reply = parse_llsd_xml(concat!(
        "<llsd><map><key>total_pages</key><integer>4</integer>",
        "<key>next_page_url</key><string>/experiences/search/name?page=3</string>",
        "<key>previous_page_url</key><string>/experiences/search/name?page=1</string>",
        "<key>experience_keys</key><array>",
        "<map><key>public_id</key><uuid>040979ee-0aa5-11e8-8c52-00304892026a</uuid>",
        "<key>agent_id</key><uuid>b4f26234-6778-43b3-b9e9-90e9773e84f1</uuid>",
        "<key>group_id</key><uuid>766cdef0-fb8e-ab2f-b238-6000cb093b52</uuid>",
        "<key>name</key><string>Bugsly Testing</string>",
        "<key>slurl</key><string>secondlife://Aditi/secondlife/Est%20Full3/172/28/23</string>",
        "<key>maturity</key><integer>13</integer><key>properties</key><integer>16</integer>",
        "<key>expiration</key><integer>600</integer><key>quota</key><integer>128</integer>",
        "<key>description</key><string /><key>extended_metadata</key><string /></map>",
        "<map><key>public_id</key><uuid>2d41af16-faff-11e3-b448-00304860d196</uuid>",
        "<key>agent_id</key><uuid>3de548e1-57be-cfea-2b78-83ae3ad95998</uuid>",
        "<key>group_id</key><uuid>00000000-0000-0000-0000-000000000000</uuid>",
        "<key>name</key><string>DanLoadtest-02</string><key>slurl</key><string>last</string>",
        "<key>maturity</key><integer>254</integer><key>expiration</key><integer>10</integer>",
        "</map>",
        "<map><key>public_id</key><uuid>016ad6ae-df8c-11e2-af8c-0030489203e2</uuid>",
        "<key>agent_id</key><uuid>870424b2-d6d3-4a89-975a-ec693c233319</uuid>",
        "<key>name</key><string>iTools Test XP Key</string><key>slurl</key><string />",
        "</map></array></map></llsd>",
    ))
    .map_err(|error| format!("{error:?}"))?;
    let page = parse_experience_search_page(&reply).map_err(|error| format!("{error:?}"))?;
    assert!(page.has_next_page && page.has_previous_page);
    let locations: Vec<_> = page.infos.iter().map(|info| info.slurl.clone()).collect();
    assert_eq!(
        locations,
        vec![
            Some(ExperienceLocation::Slurl(
                url::Url::parse("secondlife://Aditi/secondlife/Est%20Full3/172/28/23")
                    .map_err(|error| error.to_string())?
            )),
            Some(ExperienceLocation::Last),
            None,
        ]
    );
    let groups: Vec<_> = page.infos.iter().map(|info| info.group).collect();
    assert_eq!(
        groups,
        vec![
            Some(GroupKey::from(uuid(
                "766cdef0-fb8e-ab2f-b238-6000cb093b52"
            )?)),
            None,
            None,
        ]
    );
    assert!(page.infos.iter().all(|info| info.owner.is_some()));
    Ok(())
}

/// The location words round-trip, and a value that is neither a word nor a URL
/// is still refused rather than read as no location.
#[test]
fn experience_location_words_round_trip() -> Result<(), String> {
    for location in [ExperienceLocation::Last, ExperienceLocation::Home] {
        let wire = ExperienceLocation::to_wire(Some(&location));
        assert_eq!(
            ExperienceLocation::from_wire("slurl", &wire).map_err(|error| format!("{error:?}"))?,
            Some(location)
        );
    }
    assert_eq!(
        ExperienceLocation::from_wire("slurl", "").map_err(|error| format!("{error:?}"))?,
        None
    );
    if let Ok(location) = ExperienceLocation::from_wire("slurl", "not a location") {
        return Err(format!("a non-location was read as {location:?}"));
    }
    Ok(())
}

/// Second Life's `RegionExperiences` reply for an estate with no default: the
/// `default` key is there, holding the nil id, beside a `disabled` key and
/// `<undef/>` for the empty lists. The nil id is no default — it must not
/// become a sticky trusted row.
#[test]
fn an_aditi_region_reply_has_no_default() -> Result<(), String> {
    let reply = parse_llsd_xml(concat!(
        "<llsd><map><key>default</key><uuid>00000000-0000-0000-0000-000000000000</uuid>",
        "<key>blocked</key><undef /><key>disabled</key><undef /><key>trusted</key><undef />",
        "<key>allowed</key><array><uuid>233352b8-6e1b-11e6-b4e8-00304860d196</uuid>",
        "<uuid>2e364f80-1160-11e5-93aa-0030489203e2</uuid></array></map></llsd>",
    ))
    .map_err(|error| format!("{error:?}"))?;
    let lists = parse_region_experiences(&reply).map_err(|error| format!("{error:?}"))?;
    assert_eq!(lists.default_experience, None);
    assert_eq!(lists.allowed.len(), 2);
    assert!(lists.blocked.is_empty() && lists.trusted.is_empty());
    Ok(())
}

/// A status reply round-trips builder → parser for both truth values.
#[test]
fn status_response_round_trip() -> Result<(), String> {
    for value in [true, false] {
        let reply = build_experience_status_response(value);
        let parsed =
            parse_experience_status(&parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, value);
    }
    Ok(())
}
