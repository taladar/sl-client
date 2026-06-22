//! Round-trip tests for the experience cap codecs.

use pretty_assertions::assert_eq;
use sl_types::key::AgentKey;
use uuid::Uuid;

use super::{
    ExperienceInfo, ExperiencePermission, ExperienceProperties, ExperienceUpdate, PROPERTY_GRID,
    PROPERTY_INVALID, build_experience_ids_response, build_experience_infos_response,
    build_experience_permissions_response, build_experience_status_response,
    build_region_experiences_request, build_region_experiences_response,
    build_set_experience_permission_request, build_update_experience_request, experience_id_query,
    experience_info_query, find_experience_query, forget_experience_query, group_experiences_query,
    parse_experience_id_query, parse_experience_ids, parse_experience_info_query,
    parse_experience_infos, parse_experience_permissions, parse_experience_status,
    parse_find_experience_query, parse_forget_experience_query, parse_group_experiences_query,
    parse_region_experiences, parse_region_experiences_request,
    parse_set_experience_permission_request, parse_update_experience_request,
};
use crate::llsd::parse_llsd_xml;

/// Parses a UUID in a test, surfacing a `String` error for the `?` operator.
fn uuid(text: &str) -> Result<Uuid, String> {
    Uuid::parse_str(text).map_err(|error| error.to_string())
}

/// `GetExperienceInfo` batches every id as a `public_id` query parameter under
/// the `id/` path, and its `experience_keys` decode into full records while
/// `error_ids` become `missing` placeholders.
#[test]
fn experience_info_query_and_decode() -> Result<(), String> {
    let id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").map_err(|e| e.to_string())?;
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
    let infos = parse_experience_infos(&reply);
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
    assert_eq!(parse_experience_ids(&ids_reply).len(), 2);

    let prefs = parse_llsd_xml(concat!(
        "<llsd><map>",
        "<key>experiences</key><array><uuid>11111111-1111-1111-1111-111111111111</uuid></array>",
        "<key>blocked</key><array><uuid>22222222-2222-2222-2222-222222222222</uuid></array>",
        "</map></llsd>"
    ))
    .map_err(|error| format!("{error:?}"))?;
    let (allowed, blocked) = parse_experience_permissions(&prefs);
    assert_eq!(allowed.len(), 1);
    assert_eq!(blocked.len(), 1);
    Ok(())
}

/// The `Allow` permission PUT body nests the permission under the id key.
#[test]
fn set_permission_body() -> Result<(), String> {
    let id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").map_err(|e| e.to_string())?;
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
    let id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").map_err(|e| e.to_string())?;
    let update = ExperienceUpdate {
        public_id: id,
        name: "Renamed".to_owned(),
        description: "desc".to_owned(),
        maturity: 13,
        properties: PROPERTY_GRID,
        slurl: "http://maps/y".to_owned(),
        extended_metadata: String::new(),
    };
    let body = build_update_experience_request(&update);
    assert!(body.contains("<key>public_id</key><uuid>11111111-1111-1111-1111-111111111111</uuid>"));
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
    let infos = parse_experience_infos(&reply);
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
    let allowed =
        [Uuid::parse_str("11111111-1111-1111-1111-111111111111").map_err(|e| e.to_string())?];
    let trusted =
        [Uuid::parse_str("22222222-2222-2222-2222-222222222222").map_err(|e| e.to_string())?];
    let body = build_region_experiences_request(&allowed, &[], &trusted);
    assert!(body.contains(
        "<key>allowed</key><array><uuid>11111111-1111-1111-1111-111111111111</uuid></array>"
    ));
    assert!(body.contains("<key>blocked</key><array></array>"));
    assert!(body.contains(
        "<key>trusted</key><array><uuid>22222222-2222-2222-2222-222222222222</uuid></array>"
    ));

    let reply = parse_llsd_xml(&body).map_err(|error| format!("{error:?}"))?;
    let (allowed_out, blocked_out, trusted_out) = parse_region_experiences(&reply);
    assert_eq!(allowed_out, allowed);
    assert!(blocked_out.is_empty());
    assert_eq!(trusted_out, trusted);
    Ok(())
}

/// The `{ status }` boolean decodes, and the property helpers read the bits.
#[test]
fn status_and_properties() -> Result<(), String> {
    let reply = parse_llsd_xml("<llsd><map><key>status</key><boolean>1</boolean></map></llsd>")
        .map_err(|error| format!("{error:?}"))?;
    assert!(parse_experience_status(&reply));

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
        uuid("11111111-1111-1111-1111-111111111111")?,
        uuid("22222222-2222-2222-2222-222222222222")?,
    ];
    let suffix = experience_info_query(&ids);
    assert_eq!(parse_experience_info_query(&suffix), ids);
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
    assert_eq!(
        parse_group_experiences_query(&group_experiences_query(id)),
        Some(id)
    );
    assert_eq!(
        parse_forget_experience_query(&forget_experience_query(id)),
        Some(id)
    );
    assert_eq!(
        parse_experience_id_query(&experience_id_query(id)),
        Some(id)
    );
    Ok(())
}

/// The `ExperiencePreferences` PUT body round-trips builder → parser, and the
/// `{ experiences, blocked }` reply round-trips builder → parser.
#[test]
fn permission_request_and_reply_round_trip() -> Result<(), String> {
    let id = uuid("11111111-1111-1111-1111-111111111111")?;
    let body = build_set_experience_permission_request(id, ExperiencePermission::Block);
    let parsed =
        parse_set_experience_permission_request(&body).map_err(|error| format!("{error:?}"))?;
    assert_eq!(parsed, Some((id, ExperiencePermission::Block)));

    let allowed = [id];
    let blocked = [uuid("22222222-2222-2222-2222-222222222222")?];
    let reply = build_experience_permissions_response(&allowed, &blocked);
    let parsed = parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?;
    let (allowed_out, blocked_out) = parse_experience_permissions(&parsed);
    assert_eq!(allowed_out, allowed);
    assert_eq!(blocked_out, blocked);
    Ok(())
}

/// The `UpdateExperience` POST body round-trips builder → parser.
#[test]
fn update_experience_request_round_trip() -> Result<(), String> {
    let update = ExperienceUpdate {
        public_id: uuid("11111111-1111-1111-1111-111111111111")?,
        name: "Renamed".to_owned(),
        description: "desc & more".to_owned(),
        maturity: 13,
        properties: PROPERTY_GRID,
        slurl: "http://maps/y".to_owned(),
        extended_metadata: "<x/>".to_owned(),
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
    let allowed = [uuid("11111111-1111-1111-1111-111111111111")?];
    let trusted = [uuid("22222222-2222-2222-2222-222222222222")?];
    let request = build_region_experiences_request(&allowed, &[], &trusted);
    let (allowed_out, blocked_out, trusted_out) =
        parse_region_experiences_request(&request).map_err(|error| format!("{error:?}"))?;
    assert_eq!(allowed_out, allowed);
    assert!(blocked_out.is_empty());
    assert_eq!(trusted_out, trusted);

    let reply = build_region_experiences_response(&allowed, &[], &trusted);
    let (allowed_out, blocked_out, trusted_out) =
        parse_region_experiences(&parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?);
    assert_eq!(allowed_out, allowed);
    assert!(blocked_out.is_empty());
    assert_eq!(trusted_out, trusted);
    Ok(())
}

/// The `experience_ids` reply round-trips builder → parser.
#[test]
fn experience_ids_response_round_trip() -> Result<(), String> {
    let ids = [
        uuid("11111111-1111-1111-1111-111111111111")?,
        uuid("22222222-2222-2222-2222-222222222222")?,
    ];
    let reply = build_experience_ids_response(&ids);
    let parsed =
        parse_experience_ids(&parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?);
    assert_eq!(parsed, ids);
    Ok(())
}

/// The `GetExperienceInfo` reply round-trips a full record through
/// `experience_keys` and a missing id through `error_ids`.
#[test]
fn experience_infos_response_round_trip() -> Result<(), String> {
    let real = ExperienceInfo {
        public_id: uuid("11111111-1111-1111-1111-111111111111")?,
        name: "My Experience".to_owned(),
        agent_id: AgentKey::from(uuid("22222222-2222-2222-2222-222222222222")?),
        description: "fun & games".to_owned(),
        properties: ExperienceProperties(PROPERTY_GRID),
        maturity: 13,
        slurl: "http://maps/x".to_owned(),
        ..ExperienceInfo::default()
    };
    let missing = ExperienceInfo {
        public_id: uuid("33333333-3333-3333-3333-333333333333")?,
        properties: ExperienceProperties(PROPERTY_INVALID),
        missing: true,
        ..ExperienceInfo::default()
    };
    let reply = build_experience_infos_response(&[real.clone(), missing.clone()]);
    let infos =
        parse_experience_infos(&parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?);
    let [first, second] = infos.as_slice() else {
        return Err(format!("expected 2 infos, got {}", infos.len()));
    };
    assert_eq!(*first, real);
    assert_eq!(*second, missing);
    Ok(())
}

/// A status reply round-trips builder → parser for both truth values.
#[test]
fn status_response_round_trip() -> Result<(), String> {
    for value in [true, false] {
        let reply = build_experience_status_response(value);
        let parsed =
            parse_experience_status(&parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?);
        assert_eq!(parsed, value);
    }
    Ok(())
}
