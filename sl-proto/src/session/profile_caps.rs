//! The profile capabilities: `AgentProfile`, `UploadAgentProfileImage`,
//! `SetDisplayName` and `SearchStatRequest` — the request bodies a runtime
//! sends and the answers it hands back.
//!
//! Shapes as Second Life answers them (`profiles`, 2026-10-15,
//! `book/src/gridspec/profiles.md`): an `AgentProfile` GET answers every key
//! of the profile in one map — a text the avatar never set is `undef`, the
//! partner is a *string*, `online` is left out for an avatar that does not show
//! the agent its status, and `hide_age` is left out altogether; a PUT of any
//! subset of the writable keys answers `200` with no body and takes them all.

use sl_types::key::{AgentKey, ClassifiedKey, GroupKey, TextureKey};
use sl_wire::Llsd;
use uuid::Uuid;

use super::conversions::llsd_map;
use crate::types::{
    AvatarGroupMembership, AvatarPick, AvatarProperties, ClassifiedClickStats,
    ClassifiedSearchClicks, ProfileImage, ProfileUpdate,
};
use crate::{Event, PickKey};

/// The `AVATAR_ALLOW_PUBLISH` profile flag.
const ALLOW_PUBLISH: u32 = 0x1;
/// The `AVATAR_MATURE_PUBLISH` profile flag.
const MATURE_PUBLISH: u32 = 0x2;
/// The `AVATAR_IDENTIFIED` profile flag.
const IDENTIFIED: u32 = 0x4;
/// The `AVATAR_TRANSACTED` profile flag.
const TRANSACTED: u32 = 0x8;
/// The `AVATAR_ONLINE` profile flag.
const ONLINE: u32 = 0x10;

/// The key a runtime stamps an `UploadAgentProfileImage` outcome with: which
/// field the image was uploaded for, as the first request named it.
const PROFILE_IMAGE_ASSET: &str = "profile-image-asset";

/// The key of the asset an `UploadAgentProfileImage` upload made.
const NEW_ASSET: &str = "new_asset";

/// The key a runtime stamps a `SearchStatRequest` answer with: the classified
/// the counts are for, which the answer itself does not name.
const CLASSIFIED_ID: &str = "classified_id";

/// The `AgentProfile` URL of `avatar`'s profile, for a GET of it — or, for
/// the agent's own id, a PUT of its profile, and for another avatar's, a PUT
/// of the agent's notes about it.
#[must_use]
pub fn agent_profile_url(cap_url: &str, avatar: AgentKey) -> String {
    format!("{cap_url}/{}", avatar.uuid())
}

/// The LLSD-XML body of an `AgentProfile` PUT that writes `update`: every
/// field it holds, in one map, which Second Life takes whole (the reference
/// PUTs one key at a time, as its panels save them).
#[must_use]
pub fn agent_profile_put_body(update: &ProfileUpdate) -> String {
    let mut entries = vec![
        ("sl_about_text", Llsd::String(update.about_text.clone())),
        ("fl_about_text", Llsd::String(update.fl_about_text.clone())),
        ("sl_image_id", Llsd::Uuid(update.image_id.uuid())),
        ("fl_image_id", Llsd::Uuid(update.fl_image_id.uuid())),
        ("allow_publish", Llsd::Boolean(update.allow_publish)),
    ];
    if let Some(hide_age) = update.hide_age {
        entries.push(("hide_age", Llsd::Boolean(hide_age)));
    }
    llsd_map(entries).to_llsd_xml()
}

/// The LLSD-XML body of an `AgentProfile` PUT of the agent's notes about
/// another avatar (to that avatar's [`agent_profile_url`]).
#[must_use]
pub fn agent_profile_notes_body(notes: &str) -> String {
    llsd_map(vec![("notes", Llsd::String(notes.to_owned()))]).to_llsd_xml()
}

/// The LLSD-XML body of the first `UploadAgentProfileImage` POST, which names
/// the field the image is for and is answered with the uploader URL.
#[must_use]
pub fn profile_image_upload_body(image: ProfileImage) -> String {
    llsd_map(vec![(
        PROFILE_IMAGE_ASSET,
        Llsd::String(image.field().to_owned()),
    )])
    .to_llsd_xml()
}

/// What a runtime hands the session, tagged `UploadAgentProfileImage`, when
/// an upload of `image` is over: the asset it made, or none where it failed.
#[must_use]
pub fn profile_image_upload_outcome(image: ProfileImage, asset: Option<Uuid>) -> Llsd {
    let mut entries = vec![(PROFILE_IMAGE_ASSET, Llsd::String(image.field().to_owned()))];
    if let Some(asset) = asset {
        entries.push((NEW_ASSET, Llsd::Uuid(asset)));
    }
    llsd_map(entries)
}

/// The LLSD-XML body of a `SetDisplayName` POST: `{ display_name: [<old>,
/// <new>] }`, the old name first, as the People API wants both.
#[must_use]
pub fn set_display_name_body(old: &str, new: &str) -> String {
    llsd_map(vec![(
        "display_name",
        Llsd::Array(vec![
            Llsd::String(old.to_owned()),
            Llsd::String(new.to_owned()),
        ]),
    )])
    .to_llsd_xml()
}

/// The LLSD-XML body of a `SearchStatRequest` POST for `classified`.
#[must_use]
pub fn search_stat_request_body(classified: ClassifiedKey) -> String {
    llsd_map(vec![(CLASSIFIED_ID, Llsd::Uuid(classified.uuid()))]).to_llsd_xml()
}

/// A `SearchStatRequest` answer stamped with the classified it is for, as a
/// runtime hands it to the session.
#[must_use]
pub fn stamp_search_stat_answer(answer: Llsd, classified: ClassifiedKey) -> Llsd {
    match answer {
        Llsd::Map(mut map) => {
            map.insert(CLASSIFIED_ID.to_owned(), Llsd::Uuid(classified.uuid()));
            Llsd::Map(map)
        }
        other => other,
    }
}

/// The profile field a first `UploadAgentProfileImage` POST names — what a
/// simulator reads of it.
#[must_use]
pub fn parse_profile_image_upload_body(body: &str) -> Option<ProfileImage> {
    let body = sl_wire::parse_llsd_xml(body).ok()?;
    ProfileImage::from_field(body.get(PROFILE_IMAGE_ASSET)?.as_str()?)
}

/// The old and the new name a `SetDisplayName` POST names
/// ([`set_display_name_body`]) — what a simulator reads of it.
#[must_use]
pub fn parse_set_display_name_body(body: &str) -> Option<(String, String)> {
    let body = sl_wire::parse_llsd_xml(body).ok()?;
    let names = body.get("display_name")?.as_array()?;
    match names {
        [old, new] => Some((old.as_str()?.to_owned(), new.as_str()?.to_owned())),
        _ => None,
    }
}

/// The classified a `SearchStatRequest` POST asks about
/// ([`search_stat_request_body`]) — what a simulator reads of it.
#[must_use]
pub fn parse_search_stat_request_body(body: &str) -> Option<ClassifiedKey> {
    let body = sl_wire::parse_llsd_xml(body).ok()?;
    uuid_of(body.get(CLASSIFIED_ID)).map(ClassifiedKey::from)
}

/// The LLSD-XML a simulator answers a `SearchStatRequest` with: the six
/// counters, as Second Life names them, and no classified id.
#[must_use]
pub fn search_stat_answer_body(stats: &ClassifiedClickStats) -> String {
    let search = stats.search.unwrap_or(ClassifiedSearchClicks {
        teleport: 0,
        map: 0,
        profile: 0,
    });
    llsd_map(vec![
        ("teleport_clicks", Llsd::Integer(stats.teleport)),
        ("map_clicks", Llsd::Integer(stats.map)),
        ("profile_clicks", Llsd::Integer(stats.profile)),
        ("search_teleport_clicks", Llsd::Integer(search.teleport)),
        ("search_map_clicks", Llsd::Integer(search.map)),
        ("search_profile_clicks", Llsd::Integer(search.profile)),
    ])
    .to_llsd_xml()
}

/// An id that a profile answer writes either as an LLSD uuid or as a string
/// (Second Life writes the partner as a string).
fn uuid_of(value: Option<&Llsd>) -> Option<Uuid> {
    match value? {
        Llsd::Uuid(id) => Some(*id),
        Llsd::String(text) => Uuid::parse_str(text).ok(),
        _ => None,
    }
}

/// A text of a profile answer: what the avatar wrote, empty where it never
/// wrote anything (`undef`).
fn text_of(value: Option<&Llsd>) -> String {
    value.and_then(Llsd::as_str).unwrap_or_default().to_owned()
}

/// A flag of a profile answer, false where it is missing.
fn flag_of(value: Option<&Llsd>) -> bool {
    value.and_then(Llsd::as_bool).unwrap_or(false)
}

/// The `member_since` date written as the UDP `BornOn` is (`M/D/YYYY`), so
/// that one profile reads the same over both roads; any other form is kept
/// as it came.
fn born_on(value: Option<&Llsd>) -> String {
    let Some(Llsd::Date(date) | Llsd::String(date)) = value else {
        return String::new();
    };
    let day = date.get(..10).unwrap_or(date);
    let mut parts = day.split('-').map(str::parse::<u32>);
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(Ok(year)), Some(Ok(month)), Some(Ok(day)), None) => format!("{month}/{day}/{year}"),
        _ => date.clone(),
    }
}

/// The account caption as the UDP `CharterMember` carries it: the caption
/// index as one character where the answer names one (`charter_member`), the
/// caption itself where it names that instead (`caption`).
fn caption(body: &Llsd) -> String {
    if let Some(index) = body
        .get("charter_member")
        .and_then(Llsd::as_i32)
        .and_then(|index| u8::try_from(index).ok())
    {
        return char::from(index).to_string();
    }
    text_of(body.get("caption"))
}

/// The events an `AgentProfile` GET answer brings: the profile, the groups
/// and picks it lists, and the agent's notes about the avatar — as the UDP
/// replies would bring them, so a consumer reads one profile however it came.
/// `None` where the answer names no avatar.
pub(crate) fn agent_profile_events(body: &Llsd) -> Option<Vec<Event>> {
    let avatar = uuid_of(body.get("id"))?;
    let online = body.get("online").and_then(Llsd::as_bool);
    let mut flags = 0;
    for (key, flag) in [
        ("allow_publish", ALLOW_PUBLISH),
        ("mature_profile", MATURE_PUBLISH),
        ("identified", IDENTIFIED),
        ("transacted", TRANSACTED),
    ] {
        if flag_of(body.get(key)) {
            flags |= flag;
        }
    }
    if online == Some(true) {
        flags |= ONLINE;
    }
    let properties = AvatarProperties {
        avatar_id: AgentKey::from(avatar),
        image_id: TextureKey::from(uuid_of(body.get("sl_image_id")).unwrap_or_else(Uuid::nil)),
        fl_image_id: TextureKey::from(uuid_of(body.get("fl_image_id")).unwrap_or_else(Uuid::nil)),
        partner_id: uuid_of(body.get("partner_id"))
            .filter(|partner| !partner.is_nil())
            .map(AgentKey::from),
        about_text: text_of(body.get("sl_about_text")),
        fl_about_text: text_of(body.get("fl_about_text")),
        born_on: born_on(body.get("member_since")),
        profile_url: text_of(body.get("home_page")),
        charter_member: caption(body),
        flags,
        hide_age: body.get("hide_age").and_then(Llsd::as_bool),
        customer_type: body
            .get("customer_type")
            .and_then(Llsd::as_str)
            .map(str::to_owned),
        online,
    };
    let groups = body
        .get("groups")
        .and_then(Llsd::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|group| {
            Some(AvatarGroupMembership {
                group_id: GroupKey::from(uuid_of(group.get("id"))?),
                group_name: text_of(group.get("name")),
                // The capability names neither the title nor the powers.
                group_title: String::new(),
                group_powers: 0,
                accept_notices: false,
                group_insignia_id: TextureKey::from(
                    uuid_of(group.get("image_id")).unwrap_or_else(Uuid::nil),
                ),
            })
        })
        .collect();
    let picks = body
        .get("picks")
        .and_then(Llsd::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|pick| {
            Some(AvatarPick {
                pick_id: PickKey::from(uuid_of(pick.get("id"))?),
                name: text_of(pick.get("name")),
            })
        })
        .collect();
    let mut events = vec![Event::AvatarProperties(Box::new(properties))];
    // Second Life's answer lists no groups where `AvatarGroupsReply` lists
    // some; an answer without `online` is followed by the UDP read
    // ([`with_presence`]), which brings them, so it does not say "none" first.
    if online.is_some() {
        events.push(Event::AvatarGroups {
            avatar_id: AgentKey::from(avatar),
            groups,
            list_in_profile: true,
        });
    }
    events.extend([
        Event::AvatarPicks {
            target_id: avatar,
            picks,
        },
        Event::AvatarNotes {
            target_id: avatar,
            notes: text_of(body.get("notes")),
        },
    ]);
    Some(events)
}

/// The profile `AgentProfile` answered without `online`, given the online
/// state the UDP `AvatarPropertiesReply` read after it carries: the
/// reference marks such a status unknown and sends the UDP request for it
/// (Firestorm's `AVATAR_ONLINE_UNDEFINED`). Everything else stays the
/// capability's — the UDP texts are cut to 500 bytes.
pub(crate) const fn with_presence(
    mut profile: AvatarProperties,
    udp: &AvatarProperties,
) -> AvatarProperties {
    profile.flags = (profile.flags & !ONLINE) | (udp.flags & ONLINE);
    profile.online = Some(udp.flags & ONLINE != 0);
    profile
}

/// The event an `UploadAgentProfileImage` outcome
/// ([`profile_image_upload_outcome`]) brings; `None` where it names no
/// profile field.
pub(crate) fn profile_image_uploaded(body: &Llsd) -> Option<Event> {
    let image = ProfileImage::from_field(body.get(PROFILE_IMAGE_ASSET)?.as_str()?)?;
    Some(Event::ProfileImageUploaded {
        image,
        asset_id: uuid_of(body.get(NEW_ASSET)).map(TextureKey::from),
    })
}

/// The event a stamped `SearchStatRequest` answer
/// ([`stamp_search_stat_answer`]) brings; `None` where it is not one.
pub(crate) fn classified_click_stats(body: &Llsd) -> Option<Event> {
    let classified_id = ClassifiedKey::from(uuid_of(body.get(CLASSIFIED_ID))?);
    let count = |key: &str| body.get(key).and_then(Llsd::as_i32);
    Some(Event::ClassifiedClickStats(ClassifiedClickStats {
        classified_id,
        teleport: count("teleport_clicks")?,
        map: count("map_clicks")?,
        profile: count("profile_clicks")?,
        search: Some(ClassifiedSearchClicks {
            teleport: count("search_teleport_clicks")?,
            map: count("search_map_clicks")?,
            profile: count("search_profile_clicks")?,
        }),
    }))
}

/// The event a `classifiedclickthrough` generic message brings: its
/// parameters are the classified's id and its teleport, map and profile
/// counts, each a NUL-terminated string. `None` where they are not.
pub(crate) fn classified_click_through(params: &[Vec<u8>]) -> Option<Event> {
    let text = |index: usize| -> Option<String> {
        let bytes = params.get(index)?;
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        Some(String::from_utf8_lossy(bytes.get(..end)?).trim().to_owned())
    };
    let count = |index: usize| -> Option<i32> { text(index)?.parse().ok() };
    Some(Event::ClassifiedClickStats(ClassifiedClickStats {
        classified_id: ClassifiedKey::from(Uuid::parse_str(&text(0)?).ok()?),
        teleport: count(1)?,
        map: count(2)?,
        profile: count(3)?,
        search: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::{
        agent_profile_events, agent_profile_put_body, classified_click_stats,
        classified_click_through, profile_image_upload_outcome, profile_image_uploaded,
        set_display_name_body, stamp_search_stat_answer, with_presence,
    };
    use crate::types::{AvatarProperties, ProfileImage, ProfileUpdate};
    use crate::{Event, PickKey};
    use pretty_assertions::assert_eq;
    use sl_types::key::{AgentKey, ClassifiedKey, TextureKey};
    use sl_wire::{Llsd, parse_llsd_xml};
    use std::collections::HashMap;
    use uuid::Uuid;

    /// An LLSD map of `entries`.
    fn map(entries: Vec<(&str, Llsd)>) -> Llsd {
        Llsd::Map(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect::<HashMap<_, _>>(),
        )
    }

    /// Another avatar's profile as Second Life answers it (the shape the
    /// `profiles` census recorded on aditi): texts never written are `undef`,
    /// the partner is a string, `online` and `hide_age` are left out. Its
    /// groups are left to the UDP read that follows for the online state, and
    /// that read's state is merged into the capability's profile.
    #[test]
    fn another_avatars_profile_reads_as_the_udp_replies_would() -> Result<(), String> {
        let avatar = Uuid::from_u128(0xb0b);
        let pick = Uuid::from_u128(0x91c4);
        let group = Uuid::from_u128(0x6a0);
        let body = map(vec![
            ("id", Llsd::Uuid(avatar)),
            ("allow_publish", Llsd::Boolean(true)),
            ("charter_member", Llsd::Integer(1)),
            ("customer_type", Llsd::String("Base".to_owned())),
            ("fl_about_text", Llsd::Undef),
            ("fl_image_id", Llsd::Uuid(Uuid::nil())),
            (
                "groups",
                Llsd::Array(vec![map(vec![
                    ("id", Llsd::Uuid(group)),
                    ("name", Llsd::String("A group".to_owned())),
                    ("image_id", Llsd::Uuid(Uuid::nil())),
                ])]),
            ),
            ("home_page", Llsd::Undef),
            ("identified", Llsd::Boolean(false)),
            ("mature_profile", Llsd::Boolean(false)),
            (
                "member_since",
                Llsd::Date("2020-05-29T00:00:00Z".to_owned()),
            ),
            ("notes", Llsd::Undef),
            (
                "partner_id",
                Llsd::String("00000000-0000-0000-0000-000000000000".to_owned()),
            ),
            (
                "picks",
                Llsd::Array(vec![map(vec![
                    ("id", Llsd::Uuid(pick)),
                    ("name", Llsd::String("A pick".to_owned())),
                ])]),
            ),
            ("sl_about_text", Llsd::String("About me".to_owned())),
            ("sl_image_id", Llsd::Uuid(Uuid::nil())),
            ("transacted", Llsd::Boolean(false)),
        ]);
        let events = agent_profile_events(&body).ok_or("no events")?;
        let [
            Event::AvatarProperties(props),
            Event::AvatarPicks { target_id, picks },
            Event::AvatarNotes { notes, .. },
        ] = events.as_slice()
        else {
            return Err(format!("unexpected events: {events:?}"));
        };
        assert_eq!(props.avatar_id, AgentKey::from(avatar));
        assert_eq!(props.about_text, "About me");
        assert_eq!(props.fl_about_text, "");
        assert_eq!(props.partner_id, None);
        assert_eq!(props.born_on, "5/29/2020");
        assert_eq!(props.charter_member, "\u{1}");
        assert_eq!(props.flags, 0x1);
        assert_eq!(props.online, None);
        assert_eq!(props.hide_age, None);
        assert_eq!(props.customer_type.as_deref(), Some("Base"));
        assert_eq!(*target_id, avatar);
        assert_eq!(
            picks.first().map(|pick| pick.pick_id),
            Some(PickKey::from(pick))
        );
        assert_eq!(notes, "");

        let udp = AvatarProperties {
            about_text: "About".to_owned(),
            flags: 0x11,
            online: Some(true),
            ..(**props).clone()
        };
        let merged = with_presence((**props).clone(), &udp);
        assert_eq!(merged.flags, 0x11);
        assert_eq!(merged.online, Some(true));
        assert_eq!(merged.about_text, "About me", "the capability's text stays");
        Ok(())
    }

    /// The agent's own profile says it is online, which sets the flag the UDP
    /// reply would.
    #[test]
    fn an_online_profile_sets_the_online_flag() -> Result<(), String> {
        let body = map(vec![
            ("id", Llsd::Uuid(Uuid::from_u128(1))),
            ("online", Llsd::Boolean(true)),
            ("hide_age", Llsd::Boolean(true)),
        ]);
        let events = agent_profile_events(&body).ok_or("no events")?;
        let [
            Event::AvatarProperties(props),
            Event::AvatarGroups { .. },
            ..,
        ] = events.as_slice()
        else {
            return Err(format!("unexpected events: {events:?}"));
        };
        assert_eq!(props.flags, 0x10);
        assert_eq!(props.online, Some(true));
        assert_eq!(props.hide_age, Some(true));
        Ok(())
    }

    /// A PUT writes every field of the update in one map, and `hide_age`
    /// only where the update sets it.
    #[test]
    fn a_profile_put_writes_every_field() -> Result<(), String> {
        let update = ProfileUpdate {
            about_text: "About".to_owned(),
            allow_publish: true,
            ..ProfileUpdate::default()
        };
        let body = parse_llsd_xml(&agent_profile_put_body(&update)).map_err(|e| e.to_string())?;
        let mut keys: Vec<&str> = body
            .as_map()
            .ok_or("not a map")?
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "allow_publish",
                "fl_about_text",
                "fl_image_id",
                "sl_about_text",
                "sl_image_id"
            ]
        );
        let with_age = ProfileUpdate {
            hide_age: Some(true),
            ..update
        };
        let body = parse_llsd_xml(&agent_profile_put_body(&with_age)).map_err(|e| e.to_string())?;
        assert_eq!(body.get("hide_age").and_then(Llsd::as_bool), Some(true));
        Ok(())
    }

    /// An upload's outcome round-trips into the event, with and without an
    /// asset.
    #[test]
    fn an_upload_outcome_becomes_the_event() {
        let asset = Uuid::from_u128(0xa55e7);
        assert_eq!(
            profile_image_uploaded(&profile_image_upload_outcome(
                ProfileImage::FirstLife,
                Some(asset)
            )),
            Some(Event::ProfileImageUploaded {
                image: ProfileImage::FirstLife,
                asset_id: Some(TextureKey::from(asset)),
            })
        );
        assert_eq!(
            profile_image_uploaded(&profile_image_upload_outcome(
                ProfileImage::SecondLife,
                None
            )),
            Some(Event::ProfileImageUploaded {
                image: ProfileImage::SecondLife,
                asset_id: None,
            })
        );
    }

    /// The set body names the old name first.
    #[test]
    fn a_display_name_set_names_the_old_name_first() -> Result<(), String> {
        let body =
            parse_llsd_xml(&set_display_name_body("Old", "New")).map_err(|e| e.to_string())?;
        let names: Vec<&str> = body
            .get("display_name")
            .and_then(Llsd::as_array)
            .ok_or("no names")?
            .iter()
            .filter_map(Llsd::as_str)
            .collect();
        assert_eq!(names, vec!["Old", "New"]);
        Ok(())
    }

    /// Second Life's answer (all six counters) reads once stamped with its
    /// classified.
    #[test]
    fn a_search_stat_answer_reads_once_stamped() -> Result<(), String> {
        let classified = ClassifiedKey::from(Uuid::from_u128(0xc1a5));
        let answer = map(vec![
            ("map_clicks", Llsd::Integer(1)),
            ("profile_clicks", Llsd::Integer(2)),
            ("search_map_clicks", Llsd::Integer(3)),
            ("search_profile_clicks", Llsd::Integer(4)),
            ("search_teleport_clicks", Llsd::Integer(5)),
            ("teleport_clicks", Llsd::Integer(6)),
        ]);
        assert_eq!(classified_click_stats(&answer), None);
        let Some(Event::ClassifiedClickStats(stats)) =
            classified_click_stats(&stamp_search_stat_answer(answer, classified))
        else {
            return Err("no stats".to_owned());
        };
        assert_eq!(stats.classified_id, classified);
        assert_eq!((stats.teleport, stats.map, stats.profile), (6, 1, 2));
        assert_eq!(
            stats
                .search
                .map(|search| (search.teleport, search.map, search.profile)),
            Some((5, 3, 4))
        );
        Ok(())
    }

    /// What a client sends, a simulator reads back.
    #[test]
    fn the_server_reads_what_the_client_sends() -> Result<(), String> {
        use super::{
            parse_profile_image_upload_body, parse_search_stat_request_body,
            parse_set_display_name_body, profile_image_upload_body, search_stat_answer_body,
            search_stat_request_body,
        };
        assert_eq!(
            parse_profile_image_upload_body(&profile_image_upload_body(ProfileImage::FirstLife)),
            Some(ProfileImage::FirstLife)
        );
        assert_eq!(
            parse_set_display_name_body(&set_display_name_body("Old", "New")),
            Some(("Old".to_owned(), "New".to_owned()))
        );
        let classified = ClassifiedKey::from(Uuid::from_u128(0xc1a5));
        assert_eq!(
            parse_search_stat_request_body(&search_stat_request_body(classified)),
            Some(classified)
        );
        let stats = crate::ClassifiedClickStats {
            classified_id: classified,
            teleport: 1,
            map: 2,
            profile: 3,
            search: None,
        };
        let answer = parse_llsd_xml(&search_stat_answer_body(&stats)).map_err(|e| e.to_string())?;
        let Some(Event::ClassifiedClickStats(read)) =
            classified_click_stats(&stamp_search_stat_answer(answer, classified))
        else {
            return Err("the answer did not read".to_owned());
        };
        assert_eq!((read.teleport, read.map, read.profile), (1, 2, 3));
        Ok(())
    }

    /// The generic message Second Life sends with a classified's details: an
    /// id and three counts, NUL-terminated.
    #[test]
    fn a_click_through_message_reads() {
        let classified = Uuid::from_u128(0xc1a5);
        let params = vec![
            format!("{classified}\0").into_bytes(),
            b"0\0".to_vec(),
            b"1\0".to_vec(),
            b"2\0".to_vec(),
        ];
        assert_eq!(
            classified_click_through(&params),
            Some(Event::ClassifiedClickStats(crate::ClassifiedClickStats {
                classified_id: ClassifiedKey::from(classified),
                teleport: 0,
                map: 1,
                profile: 2,
                search: None,
            }))
        );
        assert_eq!(
            classified_click_through(params.get(..2).unwrap_or_default()),
            None
        );
    }
}
