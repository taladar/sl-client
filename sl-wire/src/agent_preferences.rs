//! The **`AgentPreferences`** capability: get/set the agent's server-stored
//! preferences.
//!
//! A handful of agent preferences live on the grid rather than in the viewer's
//! local settings, because they affect how the agent appears to others or how
//! the simulator treats it: the avatar's *hover height*, the default permission
//! masks new objects are created with, the agent's maturity-access ceiling, and
//! the UI language (with a flag controlling whether it is public in the agent's
//! profile). The viewer reads and writes them through the `AgentPreferences`
//! capability — a single POST whose body carries the fields to change and whose
//! reply echoes the full, updated set. A POST with no recognised fields acts as
//! a pure "get".
//!
//! This module builds the request body and decodes the reply (client side), and
//! parses the request and builds the reply (server side). The body keys
//! (`hover_height`, `default_object_perm_masks` with `Group`/`Everyone`/
//! `NextOwner`, `access_prefs` with `max`, `language`, `language_is_public`,
//! `god_level`) are cross-checked against the Firestorm viewer's
//! `indra/newview/llagent.cpp` / `llfloaterperms.cpp` / `llagentlanguage.cpp`
//! and OpenSim's `AgentPreferencesModule.cs`.
//!
//! The capability is a single POST:
//!
//! - `AgentPreferences` — POST a partial `{ hover_height?, default_object_perm_masks?,
//!   access_prefs?, language?, language_is_public? }` → the full stored set echoed
//!   back (`{ access_prefs, default_object_perm_masks, hover_height, language,
//!   language_is_public, god_level }`).

use std::collections::HashMap;

use crate::WireError;
use crate::llsd::Llsd;

/// The default permission masks new objects are created with
/// (`default_object_perm_masks`), as raw `PERM_*` bit masks. The viewer sends
/// these from the "new object" defaults in its permissions preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ObjectPermMasks {
    /// The default permission mask granted to the object's group (`Group`).
    pub group: i32,
    /// The default permission mask granted to everyone (`Everyone`).
    pub everyone: i32,
    /// The default permission mask granted to the next owner (`NextOwner`).
    pub next_owner: i32,
}

/// The agent's server-stored preferences, as carried by the `AgentPreferences`
/// capability. Every field is [`Option`]: a request carries only the fields the
/// agent is changing (a fully-empty set is a pure "get"), and a reply fills in
/// the complete stored set the grid echoes back.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AgentPreferences {
    /// The avatar's hover height in metres (`hover_height`), the Z offset applied
    /// to the avatar's apparent ground position.
    pub hover_height: Option<f64>,
    /// The default permission masks for new objects (`default_object_perm_masks`).
    pub default_object_perm_masks: Option<ObjectPermMasks>,
    /// The agent's maturity-access ceiling (`access_prefs.max`): `"PG"`, `"M"`, or
    /// `"A"` — the highest content rating the agent opts in to seeing.
    pub max_access_pref: Option<String>,
    /// The agent's UI language code (`language`), e.g. `"en-us"`.
    pub language: Option<String>,
    /// Whether the agent's language is public in their profile
    /// (`language_is_public`).
    pub language_is_public: Option<bool>,
    /// The agent's god level (`god_level`) — reply-only; the grid reports the
    /// agent's administrative level (`0` for ordinary agents).
    pub god_level: Option<i32>,
}

/// Serialises an [`AgentPreferences`] to an LLSD map, emitting only the present
/// (`Some`) fields. Shared by the client request builder and the server reply
/// builder, which carry the identical key shape.
fn agent_preferences_to_llsd(prefs: &AgentPreferences) -> Llsd {
    let mut map: HashMap<String, Llsd> = HashMap::new();
    if let Some(hover_height) = prefs.hover_height {
        let _previous = map.insert("hover_height".to_owned(), Llsd::Real(hover_height));
    }
    if let Some(masks) = prefs.default_object_perm_masks {
        let _previous = map.insert(
            "default_object_perm_masks".to_owned(),
            Llsd::Map(HashMap::from([
                ("Group".to_owned(), Llsd::Integer(masks.group)),
                ("Everyone".to_owned(), Llsd::Integer(masks.everyone)),
                ("NextOwner".to_owned(), Llsd::Integer(masks.next_owner)),
            ])),
        );
    }
    if let Some(max) = &prefs.max_access_pref {
        let _previous = map.insert(
            "access_prefs".to_owned(),
            Llsd::Map(HashMap::from([(
                "max".to_owned(),
                Llsd::String(max.clone()),
            )])),
        );
    }
    if let Some(language) = &prefs.language {
        let _previous = map.insert("language".to_owned(), Llsd::String(language.clone()));
    }
    if let Some(is_public) = prefs.language_is_public {
        let _previous = map.insert("language_is_public".to_owned(), Llsd::Boolean(is_public));
    }
    if let Some(god_level) = prefs.god_level {
        let _previous = map.insert("god_level".to_owned(), Llsd::Integer(god_level));
    }
    Llsd::Map(map)
}

/// Decodes an `AgentPreferences` LLSD map into the present fields. A key that is
/// absent decodes to [`None`]; this parses both a request body (the partial set
/// the viewer sends) and a reply body (the full set the grid echoes), since the
/// two share the same key shape.
///
/// # Errors
/// Returns [`WireError::MalformedField`] if a decoded LLSD field is present but
/// of the wrong kind.
pub fn parse_agent_preferences(body: &Llsd) -> Result<AgentPreferences, WireError> {
    let default_object_perm_masks = match body.get("default_object_perm_masks") {
        None | Some(Llsd::Undef) => None,
        Some(masks @ Llsd::Map(_)) => Some(ObjectPermMasks {
            group: masks.field_i32("Group", "Group")?.unwrap_or(0),
            everyone: masks.field_i32("Everyone", "Everyone")?.unwrap_or(0),
            next_owner: masks.field_i32("NextOwner", "NextOwner")?.unwrap_or(0),
        }),
        Some(other) => {
            return Err(WireError::MalformedField {
                field: "default_object_perm_masks",
                value: other.kind().to_owned(),
            });
        }
    };
    let max_access_pref = match body.get("access_prefs") {
        None | Some(Llsd::Undef) => None,
        Some(prefs @ Llsd::Map(_)) => prefs.field_str("max", "max")?.map(str::to_owned),
        Some(other) => {
            return Err(WireError::MalformedField {
                field: "access_prefs",
                value: other.kind().to_owned(),
            });
        }
    };
    Ok(AgentPreferences {
        hover_height: body.field_f64("hover_height", "hover_height")?,
        default_object_perm_masks,
        max_access_pref,
        language: body.field_str("language", "language")?.map(str::to_owned),
        language_is_public: body.field_bool("language_is_public", "language_is_public")?,
        god_level: body.field_i32("god_level", "god_level")?,
    })
}

// ---------------------------------------------------------------------------
// Client side — the request builder and reply parser.
// ---------------------------------------------------------------------------

/// Builds the LLSD body for an `AgentPreferences` POST. Only the present
/// (`Some`) fields are sent — pass an all-[`None`] [`AgentPreferences::default`]
/// to perform a pure "get" (the grid replies with the current stored set).
/// Built on [`Llsd::to_llsd_xml`], so it round-trips through
/// [`parse_agent_preferences`].
#[must_use]
pub fn build_agent_preferences_request(prefs: &AgentPreferences) -> String {
    agent_preferences_to_llsd(prefs).to_llsd_xml()
}

// The reply is decoded with [`parse_agent_preferences`] above (request and reply
// share the same key shape).

// ---------------------------------------------------------------------------
// Server side — the inverse: the request parser and reply builder.
// ---------------------------------------------------------------------------

// The request is parsed with [`parse_agent_preferences`] above.

/// Builds an `AgentPreferences` reply from the full stored set — the inverse of
/// [`parse_agent_preferences`]. Emits every present (`Some`) field; a grid
/// echoing the complete stored set fills them all in. Built on
/// [`Llsd::to_llsd_xml`], so it round-trips through [`parse_agent_preferences`].
#[must_use]
pub fn build_agent_preferences_response(prefs: &AgentPreferences) -> String {
    agent_preferences_to_llsd(prefs).to_llsd_xml()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        AgentPreferences, ObjectPermMasks, build_agent_preferences_request,
        build_agent_preferences_response, parse_agent_preferences,
    };
    use crate::llsd::parse_llsd_xml;

    /// A partial request (only the hover height) emits just that key, and the
    /// server parser reads it back leaving the unset fields `None`.
    #[test]
    fn partial_request_round_trips() -> Result<(), String> {
        let request = AgentPreferences {
            hover_height: Some(0.35),
            ..AgentPreferences::default()
        };
        let body = build_agent_preferences_request(&request);
        let parsed =
            parse_agent_preferences(&parse_llsd_xml(&body).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            parsed.hover_height.map(f64::to_bits),
            Some(0.35_f64.to_bits())
        );
        assert_eq!(parsed.default_object_perm_masks, None);
        assert_eq!(parsed.language, None);
        Ok(())
    }

    /// An empty request body is a valid "get" — it carries no keys.
    #[test]
    fn empty_request_is_a_get() -> Result<(), String> {
        let body = build_agent_preferences_request(&AgentPreferences::default());
        let parsed =
            parse_agent_preferences(&parse_llsd_xml(&body).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, AgentPreferences::default());
        Ok(())
    }

    /// The full stored set the grid echoes round-trips through the server reply
    /// builder and the client parser, preserving the nested permission masks.
    #[test]
    fn full_response_round_trips() -> Result<(), String> {
        let prefs = AgentPreferences {
            hover_height: Some(0.5),
            default_object_perm_masks: Some(ObjectPermMasks {
                group: 0,
                everyone: 0,
                next_owner: 0x0008_2000,
            }),
            max_access_pref: Some("M".to_owned()),
            language: Some("en-us".to_owned()),
            language_is_public: Some(true),
            god_level: Some(0),
        };
        let xml = build_agent_preferences_response(&prefs);
        let parsed =
            parse_agent_preferences(&parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, prefs);
        Ok(())
    }
}
