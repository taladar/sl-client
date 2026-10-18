//! The **`UntrustedSimulatorMessage`** capability: a template message sent as
//! LLSD over HTTP instead of as a UDP datagram.
//!
//! The reference viewer's `message.xml` names the messages whose flavour is
//! `llsd`; `LLMessageSystem::sendMessage` posts those to this capability as
//!
//! ```text
//! { message: "<name>", body: { <block>: [{ <field>: … }] } }
//! ```
//!
//! In practice the one such message a viewer sends is **`GodKickUser`** — the
//! god kick, freeze and unfreeze (`LLAvatarActions::handleKick`,
//! `handleFreeze`, `handleUnfreeze`). Its `KickFlags` is a `U32`, which the
//! reference's LLSD builder writes as four big-endian bytes (`ll_sd_from_U32`),
//! and OpenSim's `GodsModule.HandleUntrustedSimulatorMessage` reads with
//! `AsUInteger`. Both grids grant the capability (`seed-capabilities`,
//! 2026-10-04); what each does with a kick is in
//! `book/src/gridspec/estate.md` § God tools.
//!
//! This module builds the body (client side) and decodes it (server side).

use std::collections::HashMap;

use sl_types::key::AgentKey;
use uuid::Uuid;

use crate::WireError;
use crate::llsd::Llsd;

/// The `message` name of a god kick.
pub const GOD_KICK_USER_MESSAGE: &str = "GodKickUser";

/// The agent id a god kick names to act on every agent in the region
/// (`LL_UUID_ALL_AGENTS` in the reference, `ALL_AGENTS` in OpenSim's
/// `GodsModule`).
pub const ALL_AGENTS: Uuid = Uuid::from_u128(0x44e8_7126_e794_4ded_05b3_7c42_da3d_5cdb);

/// One `GodKickUser`, as either transport carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GodKickUserRequest {
    /// The god acting (`GodID`).
    pub god: AgentKey,
    /// The god's session (`GodSessionID`), which OpenSim checks against the
    /// god's presence.
    pub god_session: Uuid,
    /// The agent acted on (`AgentID`); [`ALL_AGENTS`] for everybody.
    pub target: Uuid,
    /// What to do (`KickFlags`): `0` kick, `1` freeze, `2` unfreeze.
    pub kick_flags: u32,
    /// The text the target is shown (`Reason`).
    pub reason: String,
}

/// A message posted to `UntrustedSimulatorMessage`, decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UntrustedMessage {
    /// A god kick, freeze or unfreeze.
    GodKickUser(GodKickUserRequest),
    /// A message this module does not decode, by its name.
    Other(String),
}

/// A malformed-field error for `field`.
fn malformed(field: &'static str, value: &Llsd) -> WireError {
    WireError::from(sl_llsd::LlsdError::MalformedField {
        field,
        value: format!("{value:?}"),
    })
}

/// A `U32` field, in either of the forms an LLSD writer chooses: four
/// big-endian bytes (the reference) or an integer.
fn u32_field(block: &Llsd, field: &'static str) -> Result<u32, WireError> {
    match block.get(field) {
        None | Some(Llsd::Undef) => Ok(0),
        Some(Llsd::Integer(value)) => Ok(value.cast_unsigned()),
        Some(Llsd::Binary(bytes)) => match <[u8; 4]>::try_from(bytes.as_slice()) {
            Ok(word) => Ok(crate::endian::u32_from_be(word)),
            Err(_wrong_length) => Err(malformed(field, &Llsd::Binary(bytes.clone()))),
        },
        Some(other) => Err(malformed(field, other)),
    }
}

/// Builds the body a viewer posts to `UntrustedSimulatorMessage` for a god
/// kick, as the reference's LLSD message builder writes it.
#[must_use]
pub fn build_god_kick_user_untrusted(request: &GodKickUserRequest) -> String {
    let user_info = Llsd::Map(HashMap::from([
        ("GodID".to_owned(), Llsd::Uuid(request.god.uuid())),
        ("GodSessionID".to_owned(), Llsd::Uuid(request.god_session)),
        ("AgentID".to_owned(), Llsd::Uuid(request.target)),
        (
            "KickFlags".to_owned(),
            Llsd::Binary(crate::endian::u32_to_be(request.kick_flags).to_vec()),
        ),
        ("Reason".to_owned(), Llsd::String(request.reason.clone())),
    ]));
    Llsd::Map(HashMap::from([
        (
            "message".to_owned(),
            Llsd::String(GOD_KICK_USER_MESSAGE.to_owned()),
        ),
        (
            "body".to_owned(),
            Llsd::Map(HashMap::from([(
                "UserInfo".to_owned(),
                Llsd::Array(vec![user_info]),
            )])),
        ),
    ]))
    .to_llsd_xml()
}

/// Decodes a body posted to `UntrustedSimulatorMessage` — the inverse of
/// [`build_god_kick_user_untrusted`] for a god kick.
///
/// # Errors
/// Returns [`WireError`] if the body has no `message` name, or a god kick
/// lacks its `UserInfo` block or carries a field of the wrong kind.
pub fn parse_untrusted_message(body: &Llsd) -> Result<UntrustedMessage, WireError> {
    let name = body.require_str("message", "message")?;
    if name != GOD_KICK_USER_MESSAGE {
        return Ok(UntrustedMessage::Other(name.to_owned()));
    }
    let blocks = body
        .require_map("body", "body")?
        .get("UserInfo")
        .and_then(Llsd::as_array)
        .ok_or_else(|| malformed("UserInfo", &Llsd::Undef))?;
    let block = blocks
        .first()
        .ok_or_else(|| malformed("UserInfo", &Llsd::Array(Vec::new())))?;
    Ok(UntrustedMessage::GodKickUser(GodKickUserRequest {
        god: AgentKey::from(block.require_uuid("GodID", "GodID")?),
        god_session: block.require_uuid("GodSessionID", "GodSessionID")?,
        target: block.require_uuid("AgentID", "AgentID")?,
        kick_flags: u32_field(block, "KickFlags")?,
        reason: block
            .field_str("Reason", "Reason")?
            .unwrap_or_default()
            .to_owned(),
    }))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::key::AgentKey;
    use uuid::Uuid;

    use super::{
        ALL_AGENTS, GodKickUserRequest, UntrustedMessage, build_god_kick_user_untrusted,
        parse_untrusted_message,
    };
    use crate::llsd::parse_llsd_xml;

    /// A freeze round-trips, its flags as the reference's four bytes.
    #[test]
    fn god_kick_round_trips() -> Result<(), String> {
        let request = GodKickUserRequest {
            god: AgentKey::from(Uuid::from_u128(1)),
            god_session: Uuid::from_u128(2),
            target: ALL_AGENTS,
            kick_flags: 1,
            reason: "frozen".to_owned(),
        };
        let body = build_god_kick_user_untrusted(&request);
        assert!(body.contains("<binary"), "{body}");
        let parsed =
            parse_untrusted_message(&parse_llsd_xml(&body).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, UntrustedMessage::GodKickUser(request));
        Ok(())
    }

    /// An integer `KickFlags` reads too, and another message by its name.
    #[test]
    fn integer_flags_and_other_messages_read() -> Result<(), String> {
        let body = "<llsd><map><key>message</key><string>GodKickUser</string>\
                    <key>body</key><map><key>UserInfo</key><array><map>\
                    <key>GodID</key><uuid>00000000-0000-0000-0000-000000000001</uuid>\
                    <key>GodSessionID</key><uuid>00000000-0000-0000-0000-000000000002</uuid>\
                    <key>AgentID</key><uuid>00000000-0000-0000-0000-000000000003</uuid>\
                    <key>KickFlags</key><integer>2</integer>\
                    </map></array></map></map></llsd>";
        let parsed =
            parse_untrusted_message(&parse_llsd_xml(body).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            parsed,
            UntrustedMessage::GodKickUser(GodKickUserRequest {
                god: AgentKey::from(Uuid::from_u128(1)),
                god_session: Uuid::from_u128(2),
                target: Uuid::from_u128(3),
                kick_flags: 2,
                reason: String::new(),
            })
        );
        let other = "<llsd><map><key>message</key><string>Other</string></map></llsd>";
        let parsed =
            parse_untrusted_message(&parse_llsd_xml(other).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, UntrustedMessage::Other("Other".to_owned()));
        Ok(())
    }
}
