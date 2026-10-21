//! The **`AvatarRenderInfo`** capability: how complex other viewers find each
//! avatar in the region, and how many of them would not render ours.
//!
//! One capability, two halves (Firestorm `llavatarrenderinfoaccountant.cpp`):
//!
//! - **GET** — the region's statement: `{ agents: { <id>: { weight } },
//!   reportinglimit, overlimit }`, each avatar's complexity as the region
//!   reckons it from the viewers' reports, and how many of those viewers report
//!   on the reader (`reportinglimit`) and render it as a jellydoll
//!   (`overlimit`).
//! - **POST** — this viewer's report: `{ agents: { <id>: { weight, tooComplex }
//!   } }`, the complexity it reckons for each avatar it sees and whether that is
//!   past its own limit. Second Life answers every POST `200 {}`, malformed or
//!   not (`book/src/gridspec/avatars.md` § Render info).
//!
//! This module builds the report and decodes the statement (client side), and
//! parses the report and builds the statement (server side).

use std::collections::HashMap;

use sl_types::key::AgentKey;
use uuid::Uuid;

use crate::WireError;
use crate::llsd::{Llsd, LlsdError};

/// The two counts a statement carries about the reader itself: how many
/// viewers report on it, and how many of them find it too complex to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RenderLimitCounts {
    /// How many reports of the reader the region holds (`reportinglimit`).
    /// Second Life counts reports, not reporters: one viewer that reported
    /// twice counts twice, and a viewer's report of its own avatar counts too
    /// (`avatar-render-info`, 2026-10-21).
    pub reporting: u32,
    /// How many of those reports found the reader too complex (`overlimit`) —
    /// the viewers rendering it as a jellydoll.
    pub over_limit: u32,
}

/// A GET answer of `AvatarRenderInfo`: the region's statement.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AvatarRenderInfo {
    /// Each avatar's complexity as the region states it (`agents`), sorted by
    /// agent id. An entry without a `weight` is left out, as the reference
    /// viewer ignores it.
    pub weights: Vec<(AgentKey, u32)>,
    /// The reader's own counts, when the statement carries both
    /// (`reportinglimit` and `overlimit`); the reference viewer reads neither
    /// unless both are there.
    pub limits: Option<RenderLimitCounts>,
}

/// One avatar in a POST report of `AvatarRenderInfo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AvatarRenderReport {
    /// The avatar reported on.
    pub agent: AgentKey,
    /// The complexity this viewer reckons for it (`weight`).
    pub weight: u32,
    /// Whether that is past this viewer's limit, so it renders the avatar as a
    /// jellydoll (`tooComplex`).
    pub too_complex: bool,
}

/// The LLSD key of the per-avatar map, in both halves.
const KEY_AGENTS: &str = "agents";
/// The LLSD key of an avatar's complexity, in both halves.
const KEY_WEIGHT: &str = "weight";
/// The LLSD key of a report's jellydoll flag.
const KEY_TOO_COMPLEX: &str = "tooComplex";
/// The LLSD key of the count of reports of the reader.
const KEY_REPORTING: &str = "reportinglimit";
/// The LLSD key of the count of reports finding the reader too complex.
const KEY_OVER_LIMIT: &str = "overlimit";

/// A complexity as LLSD carries it: a signed integer, so a weight past
/// `i32::MAX` is sent as `i32::MAX`, as the reference viewer does.
fn weight_to_llsd(weight: u32) -> Llsd {
    Llsd::Integer(i32::try_from(weight).unwrap_or(i32::MAX))
}

/// A complexity read back from LLSD; a negative one is malformed.
fn weight_from_llsd(weight: i32) -> Result<u32, WireError> {
    u32::try_from(weight).map_err(|_negative| {
        LlsdError::MalformedField {
            field: KEY_WEIGHT,
            value: weight.to_string(),
        }
        .into()
    })
}

/// A count read back from LLSD; a negative one is malformed.
fn count_from_llsd(count: i32, field: &'static str) -> Result<u32, WireError> {
    u32::try_from(count).map_err(|_negative| {
        LlsdError::MalformedField {
            field,
            value: count.to_string(),
        }
        .into()
    })
}

/// The per-avatar entries of either half's `agents` map, keyed by the avatar's
/// id; a key that is not an id is skipped. An absent map has no entries.
fn agent_entries(body: &Llsd) -> Result<Vec<(AgentKey, &Llsd)>, WireError> {
    let Some(agents) = body.field_map(KEY_AGENTS, KEY_AGENTS)? else {
        return Ok(Vec::new());
    };
    let mut entries: Vec<(AgentKey, &Llsd)> = agents
        .iter()
        .filter_map(|(key, value)| {
            Uuid::parse_str(key)
                .ok()
                .map(|id| (AgentKey::from(id), value))
        })
        .collect();
    entries.sort_by_key(|(agent, _value)| agent.uuid());
    Ok(entries)
}

/// Decodes a GET answer of `AvatarRenderInfo`.
///
/// # Errors
/// Returns [`LlsdError::MalformedField`] if `agents`, an avatar's entry, its
/// `weight` or one of the counts is of the wrong kind, or a weight or count is
/// negative.
pub fn parse_avatar_render_info(body: &Llsd) -> Result<AvatarRenderInfo, WireError> {
    let mut weights = Vec::new();
    for (agent, entry) in agent_entries(body)? {
        if entry.as_map().is_none() {
            return Err(LlsdError::MalformedField {
                field: KEY_AGENTS,
                value: entry.kind().to_owned(),
            }
            .into());
        }
        if let Some(weight) = entry.field_i32(KEY_WEIGHT, KEY_WEIGHT)? {
            weights.push((agent, weight_from_llsd(weight)?));
        }
    }
    let limits = match (
        body.field_i32(KEY_REPORTING, KEY_REPORTING)?,
        body.field_i32(KEY_OVER_LIMIT, KEY_OVER_LIMIT)?,
    ) {
        (Some(reporting), Some(over_limit)) => Some(RenderLimitCounts {
            reporting: count_from_llsd(reporting, KEY_REPORTING)?,
            over_limit: count_from_llsd(over_limit, KEY_OVER_LIMIT)?,
        }),
        _ => None,
    };
    Ok(AvatarRenderInfo { weights, limits })
}

/// Builds a GET answer of `AvatarRenderInfo` (server side) — the inverse of
/// [`parse_avatar_render_info`]. A statement without counts carries neither.
#[must_use]
pub fn build_avatar_render_info_response(info: &AvatarRenderInfo) -> String {
    let agents = info
        .weights
        .iter()
        .map(|(agent, weight)| {
            (
                agent.uuid().to_string(),
                Llsd::Map(HashMap::from([(
                    KEY_WEIGHT.to_owned(),
                    weight_to_llsd(*weight),
                )])),
            )
        })
        .collect();
    let mut body = HashMap::from([(KEY_AGENTS.to_owned(), Llsd::Map(agents))]);
    if let Some(limits) = info.limits {
        for (key, count) in [
            (KEY_REPORTING, limits.reporting),
            (KEY_OVER_LIMIT, limits.over_limit),
        ] {
            body.insert(
                key.to_owned(),
                Llsd::Integer(i32::try_from(count).unwrap_or(i32::MAX)),
            );
        }
    }
    Llsd::Map(body).to_llsd_xml()
}

/// Builds the body of a POST report of `AvatarRenderInfo`.
#[must_use]
pub fn build_avatar_render_report_request(reports: &[AvatarRenderReport]) -> String {
    let agents = reports
        .iter()
        .map(|report| {
            (
                report.agent.uuid().to_string(),
                Llsd::Map(HashMap::from([
                    (KEY_WEIGHT.to_owned(), weight_to_llsd(report.weight)),
                    (
                        KEY_TOO_COMPLEX.to_owned(),
                        Llsd::Boolean(report.too_complex),
                    ),
                ])),
            )
        })
        .collect();
    Llsd::Map(HashMap::from([(KEY_AGENTS.to_owned(), Llsd::Map(agents))])).to_llsd_xml()
}

/// Decodes a POST report of `AvatarRenderInfo` (server side) — the inverse of
/// [`build_avatar_render_report_request`]. An entry without a `weight` reports
/// nothing and is left out; one without `tooComplex` is not too complex.
///
/// # Errors
/// Returns [`LlsdError::MalformedField`] if `agents`, an avatar's entry or one
/// of its fields is of the wrong kind, or a weight is negative.
pub fn parse_avatar_render_report_request(
    body: &Llsd,
) -> Result<Vec<AvatarRenderReport>, WireError> {
    let mut reports = Vec::new();
    for (agent, entry) in agent_entries(body)? {
        if entry.as_map().is_none() {
            return Err(LlsdError::MalformedField {
                field: KEY_AGENTS,
                value: entry.kind().to_owned(),
            }
            .into());
        }
        if let Some(weight) = entry.field_i32(KEY_WEIGHT, KEY_WEIGHT)? {
            reports.push(AvatarRenderReport {
                agent,
                weight: weight_from_llsd(weight)?,
                too_complex: entry
                    .field_bool(KEY_TOO_COMPLEX, KEY_TOO_COMPLEX)?
                    .unwrap_or(false),
            });
        }
    }
    Ok(reports)
}

/// Builds the answer to a POST report of `AvatarRenderInfo` (server side):
/// Second Life's empty map, whatever the report held.
#[must_use]
pub fn build_avatar_render_report_response() -> String {
    Llsd::Map(HashMap::new()).to_llsd_xml()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::key::AgentKey;
    use uuid::Uuid;

    use super::{
        AvatarRenderInfo, AvatarRenderReport, RenderLimitCounts, build_avatar_render_info_response,
        build_avatar_render_report_request, build_avatar_render_report_response,
        parse_avatar_render_info, parse_avatar_render_report_request,
    };
    use crate::WireError;
    use crate::llsd::{Llsd, parse_llsd_xml};

    /// Parses an LLSD XML document for a test.
    fn llsd(xml: &str) -> Result<Llsd, String> {
        parse_llsd_xml(xml).map_err(|error| format!("{error:?}"))
    }

    /// A statement round-trips through the server builder and the client
    /// decoder, sorted by agent id.
    #[test]
    fn a_statement_round_trips() -> Result<(), String> {
        let info = AvatarRenderInfo {
            weights: vec![
                (AgentKey::from(Uuid::from_u128(1)), 123_457),
                (AgentKey::from(Uuid::from_u128(2)), 25_054),
            ],
            limits: Some(RenderLimitCounts {
                reporting: 3,
                over_limit: 1,
            }),
        };
        let parsed = parse_avatar_render_info(&llsd(&build_avatar_render_info_response(&info))?)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, info);
        Ok(())
    }

    /// Second Life's statement before any report: no avatars, both counts
    /// zero (`avatar-render-info`, 2026-10-21).
    #[test]
    fn the_first_statement_is_empty_with_counts() -> Result<(), String> {
        let body = llsd(concat!(
            "<llsd><map><key>agents</key><map/>",
            "<key>overlimit</key><integer>0</integer>",
            "<key>reportinglimit</key><integer>0</integer></map></llsd>"
        ))?;
        assert_eq!(
            parse_avatar_render_info(&body).map_err(|error| format!("{error:?}"))?,
            AvatarRenderInfo {
                weights: Vec::new(),
                limits: Some(RenderLimitCounts::default()),
            }
        );
        Ok(())
    }

    /// Without both counts the statement carries none, as the reference viewer
    /// reads neither unless both are there.
    #[test]
    fn one_count_alone_is_no_counts() -> Result<(), String> {
        let body = llsd("<llsd><map><key>reportinglimit</key><integer>2</integer></map></llsd>")?;
        assert_eq!(
            parse_avatar_render_info(&body)
                .map_err(|error| format!("{error:?}"))?
                .limits,
            None
        );
        Ok(())
    }

    /// An avatar's entry that is not a map is malformed, not skipped.
    #[test]
    fn an_entry_that_is_no_map_is_malformed() -> Result<(), String> {
        let body = llsd(concat!(
            "<llsd><map><key>agents</key><map>",
            "<key>00000000-0000-0000-0000-000000000001</key><integer>5</integer>",
            "</map></map></llsd>"
        ))?;
        match parse_avatar_render_info(&body) {
            Err(WireError::Llsd(crate::LlsdError::MalformedField {
                field: "agents", ..
            })) => Ok(()),
            other => Err(format!("expected a malformed agents entry, got {other:?}")),
        }
    }

    /// A report round-trips through the client builder and the server decoder.
    #[test]
    fn a_report_round_trips() -> Result<(), String> {
        let reports = vec![
            AvatarRenderReport {
                agent: AgentKey::from(Uuid::from_u128(1)),
                weight: 345_679,
                too_complex: false,
            },
            AvatarRenderReport {
                agent: AgentKey::from(Uuid::from_u128(2)),
                weight: 234_571,
                too_complex: true,
            },
        ];
        let parsed = parse_avatar_render_report_request(&llsd(
            &build_avatar_render_report_request(&reports),
        )?)
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, reports);
        Ok(())
    }

    /// A weight past what LLSD can carry is sent as `i32::MAX`.
    #[test]
    fn a_huge_weight_is_sent_as_the_largest_integer() -> Result<(), String> {
        let reports = [AvatarRenderReport {
            agent: AgentKey::from(Uuid::from_u128(1)),
            weight: u32::MAX,
            too_complex: true,
        }];
        let parsed = parse_avatar_render_report_request(&llsd(
            &build_avatar_render_report_request(&reports),
        )?)
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            parsed.first().map(|report| report.weight),
            Some(i32::MAX.unsigned_abs())
        );
        Ok(())
    }

    /// The answer to a report is the empty map.
    #[test]
    fn the_answer_to_a_report_is_an_empty_map() -> Result<(), String> {
        assert_eq!(
            llsd(&build_avatar_render_report_response())?,
            Llsd::Map(std::collections::HashMap::new())
        );
        Ok(())
    }
}
