//! The **`EstateAccess`** capability: the estate's four access lists in one
//! read.
//!
//! The UDP `getinfo` trails its `estateupdateinfo` reply with one `setaccess`
//! message per list, which carries ids only. The capability answers a GET with
//! all four at once and, for a ban, who set it and when:
//!
//! ```text
//! { AllowedAgents: [{ id }], AllowedGroups: [{ id }],
//!   BannedAgents: [{ id, banning_id, ban_date, last_login_date }],
//!   Managers: [{ agent_id }] }
//! ```
//!
//! The reference viewer (`LLPanelEstateAccess::requestEstateGetAccessCoro`)
//! reads its Access tab from here and re-reads it after every edit; edits stay
//! on the UDP `estateaccessdelta`. Measured on 2026-10-17 (the `estate`
//! census, `book/src/gridspec/estate.md`): Second Life answers a resident
//! without estate rights `200` with every list `undef`; OpenSim answers it
//! `401`, and an estate manager every list as an array, empty or not, a ban's
//! `last_login_date` as `na` and an unknown `ban_date` as zeros.
//!
//! This module decodes the answer (client side) and builds it (server side).

use std::collections::HashMap;

use sl_types::key::{AgentKey, GroupKey};
use uuid::Uuid;

use crate::WireError;
use crate::llsd::Llsd;

/// The estate's four access lists, as the `EstateAccess` capability answers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EstateAccessLists {
    /// The residents let in when the estate is closed (`AllowedAgents`).
    pub allowed_agents: Vec<AgentKey>,
    /// The groups whose members are let in (`AllowedGroups`).
    pub allowed_groups: Vec<GroupKey>,
    /// The residents refused entry, with who banned them and when
    /// (`BannedAgents`).
    pub banned_agents: Vec<EstateBan>,
    /// The estate's managers (`Managers`).
    pub managers: Vec<AgentKey>,
}

/// One entry of an estate's ban list.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EstateBan {
    /// The banned resident (`id`).
    pub agent: AgentKey,
    /// Who banned them (`banning_id`); [`None`] for the nil id, which the
    /// reference shows as "n/a".
    pub banning_agent: Option<AgentKey>,
    /// When, as the grid writes it (`ban_date`, `yyyy-mm-dd hh:mm` on OpenSim);
    /// [`None`] for the all-zero date a grid sends when it does not know,
    /// which the reference shows as "n/a".
    pub ban_date: Option<String>,
    /// When the banned resident last logged in (`last_login_date`); [`None`]
    /// where the grid does not know — OpenSim always sends `na`.
    pub last_login_date: Option<String>,
}

/// Whether a date field says "unknown": empty, `na`, or a date whose every
/// digit is zero (`0000-00-00 00:00`).
fn unknown_date(text: &str) -> bool {
    let text = text.trim();
    text.is_empty()
        || text.eq_ignore_ascii_case("na")
        || text
            .chars()
            .all(|character| !character.is_ascii_digit() || character == '0')
}

/// A date field's value, [`None`] when [`unknown_date`].
fn date_field(entry: &Llsd, key: &'static str) -> Result<Option<String>, WireError> {
    let text = match entry.get(key) {
        None | Some(Llsd::Undef) => return Ok(None),
        Some(Llsd::String(text) | Llsd::Date(text)) => text.clone(),
        Some(other) => {
            return Err(WireError::from(sl_llsd::LlsdError::MalformedField {
                field: key,
                value: format!("{other:?}"),
            }));
        }
    };
    Ok((!unknown_date(&text)).then_some(text))
}

/// The id under `key` in one list entry; the nil id reads as [`None`].
fn id_field(entry: &Llsd, key: &'static str) -> Result<Option<Uuid>, WireError> {
    Ok(entry.field_uuid(key, key)?.filter(|id| !id.is_nil()))
}

/// The entries of the list `key`: an absent list or `undef` is empty (Second
/// Life's answer to a resident without rights).
fn entries<'llsd>(body: &'llsd Llsd, key: &'static str) -> Result<&'llsd [Llsd], WireError> {
    match body.get(key) {
        None | Some(Llsd::Undef) => Ok(&[]),
        Some(Llsd::Array(items)) => Ok(items),
        Some(other) => Err(WireError::from(sl_llsd::LlsdError::MalformedField {
            field: key,
            value: format!("{other:?}"),
        })),
    }
}

/// The ids under `id_key` of every entry of the list `key`, skipping the nil
/// id, as the reference's name lists would show none for it.
fn ids(body: &Llsd, key: &'static str, id_key: &'static str) -> Result<Vec<Uuid>, WireError> {
    let mut out = Vec::new();
    for entry in entries(body, key)? {
        if let Some(id) = id_field(entry, id_key)? {
            out.push(id);
        }
    }
    Ok(out)
}

/// Decodes an `EstateAccess` answer.
///
/// # Errors
/// Returns [`WireError`] if a list or a field is of the wrong LLSD kind.
pub fn parse_estate_access(body: &Llsd) -> Result<EstateAccessLists, WireError> {
    let mut banned_agents = Vec::new();
    for entry in entries(body, "BannedAgents")? {
        let Some(agent) = id_field(entry, "id")? else {
            continue;
        };
        banned_agents.push(EstateBan {
            agent: AgentKey::from(agent),
            banning_agent: id_field(entry, "banning_id")?.map(AgentKey::from),
            ban_date: date_field(entry, "ban_date")?,
            last_login_date: date_field(entry, "last_login_date")?,
        });
    }
    Ok(EstateAccessLists {
        allowed_agents: ids(body, "AllowedAgents", "id")?
            .into_iter()
            .map(AgentKey::from)
            .collect(),
        allowed_groups: ids(body, "AllowedGroups", "id")?
            .into_iter()
            .map(GroupKey::from)
            .collect(),
        banned_agents,
        managers: ids(body, "Managers", "agent_id")?
            .into_iter()
            .map(AgentKey::from)
            .collect(),
    })
}

/// How a grid writes an `EstateAccess` answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstateAccessStyle {
    /// Every list as an array, empty or not; a ban's unknown dates as `na` and
    /// zeros (OpenSim's `EstateAccess.cs`).
    Arrays,
    /// Every list `undef`, whatever the estate holds: Second Life's answer to
    /// a resident without estate rights.
    Undefined,
}

/// One list entry: a map of `key` to `id`.
fn entry(key: &str, id: Uuid) -> Llsd {
    Llsd::Map(HashMap::from([(key.to_owned(), Llsd::Uuid(id))]))
}

/// Builds an `EstateAccess` answer in `style` — the inverse of
/// [`parse_estate_access`].
#[must_use]
pub fn build_estate_access_response(lists: &EstateAccessLists, style: EstateAccessStyle) -> String {
    let list = |items: Vec<Llsd>| match style {
        EstateAccessStyle::Arrays => Llsd::Array(items),
        EstateAccessStyle::Undefined => Llsd::Undef,
    };
    let bans = lists
        .banned_agents
        .iter()
        .map(|ban| {
            Llsd::Map(HashMap::from([
                ("id".to_owned(), Llsd::Uuid(ban.agent.uuid())),
                (
                    "banning_id".to_owned(),
                    Llsd::Uuid(
                        ban.banning_agent
                            .map_or_else(Uuid::nil, |agent| agent.uuid()),
                    ),
                ),
                (
                    "ban_date".to_owned(),
                    Llsd::String(
                        ban.ban_date
                            .clone()
                            .unwrap_or_else(|| "0000-00-00 00:00".to_owned()),
                    ),
                ),
                (
                    "last_login_date".to_owned(),
                    Llsd::String(
                        ban.last_login_date
                            .clone()
                            .unwrap_or_else(|| "na".to_owned()),
                    ),
                ),
            ]))
        })
        .collect();
    Llsd::Map(HashMap::from([
        (
            "AllowedAgents".to_owned(),
            list(
                lists
                    .allowed_agents
                    .iter()
                    .map(|agent| entry("id", agent.uuid()))
                    .collect(),
            ),
        ),
        (
            "AllowedGroups".to_owned(),
            list(
                lists
                    .allowed_groups
                    .iter()
                    .map(|group| entry("id", group.uuid()))
                    .collect(),
            ),
        ),
        ("BannedAgents".to_owned(), list(bans)),
        (
            "Managers".to_owned(),
            list(
                lists
                    .managers
                    .iter()
                    .map(|agent| entry("agent_id", agent.uuid()))
                    .collect(),
            ),
        ),
    ]))
    .to_llsd_xml()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::key::{AgentKey, GroupKey};
    use uuid::Uuid;

    use super::{
        EstateAccessLists, EstateAccessStyle, EstateBan, build_estate_access_response,
        parse_estate_access,
    };
    use crate::llsd::parse_llsd_xml;

    /// A full set of lists round-trips through OpenSim's array form.
    #[test]
    fn lists_round_trip() -> Result<(), String> {
        let lists = EstateAccessLists {
            allowed_agents: vec![AgentKey::from(Uuid::from_u128(1))],
            allowed_groups: vec![GroupKey::from(Uuid::from_u128(2))],
            banned_agents: vec![EstateBan {
                agent: AgentKey::from(Uuid::from_u128(3)),
                banning_agent: Some(AgentKey::from(Uuid::from_u128(4))),
                ban_date: Some("2026-10-17 16:05".to_owned()),
                last_login_date: None,
            }],
            managers: vec![AgentKey::from(Uuid::from_u128(5))],
        };
        let body = build_estate_access_response(&lists, EstateAccessStyle::Arrays);
        let parsed =
            parse_estate_access(&parse_llsd_xml(&body).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, lists);
        Ok(())
    }

    /// Second Life's answer to a resident without rights — every list `undef`
    /// — reads as four empty lists, whatever the estate holds.
    #[test]
    fn undefined_lists_read_empty() -> Result<(), String> {
        let lists = EstateAccessLists {
            managers: vec![AgentKey::from(Uuid::from_u128(5))],
            ..EstateAccessLists::default()
        };
        let body = build_estate_access_response(&lists, EstateAccessStyle::Undefined);
        let parsed =
            parse_estate_access(&parse_llsd_xml(&body).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, EstateAccessLists::default());
        Ok(())
    }

    /// OpenSim's ban of an unknown time and an unknown banner: the zero date,
    /// `na` and the nil id all read as unknown.
    #[test]
    fn unknown_ban_details_read_as_none() -> Result<(), String> {
        let body = "<llsd><map><key>BannedAgents</key><array><map>\
                    <key>id</key><uuid>00000000-0000-0000-0000-000000000003</uuid>\
                    <key>banning_id</key><uuid>00000000-0000-0000-0000-000000000000</uuid>\
                    <key>last_login_date</key><string>na</string>\
                    <key>ban_date</key><string>0000-00-00 00:00</string>\
                    </map></array></map></llsd>";
        let parsed =
            parse_estate_access(&parse_llsd_xml(body).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            parsed.banned_agents,
            vec![EstateBan {
                agent: AgentKey::from(Uuid::from_u128(3)),
                banning_agent: None,
                ban_date: None,
                last_login_date: None,
            }]
        );
        Ok(())
    }
}
