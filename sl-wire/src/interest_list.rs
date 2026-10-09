//! The **`InterestList`** capability: which objects a simulator sends.
//!
//! A simulator does not send a viewer every object of its region. It keeps an
//! *interest list* per agent — what the camera's view takes in, out to the
//! draw distance — and sends what enters it and kills what leaves. The
//! capability switches that to **every object round the agent**, whichever
//! way the camera looks, which is what a 360° capture and an area search
//! need. Second Life grants it; OpenSim has no such capability.
//!
//! - POST `{ mode }`, `"default"` or `"360"` → `{ mode, previous_mode }`: the
//!   mode now in force and the one it replaced. A mode the simulator does not
//!   know is answered as `default` (measured on aditi, 2026-10-09).
//!
//! The reference viewer posts it from `LLViewerRegion::setInterestListMode`
//! (`indra/newview/llviewerregion.cpp`) and discards the reply.

use std::collections::HashMap;

use crate::WireError;
use crate::llsd::{Llsd, LlsdError};

/// Which objects a simulator sends an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum InterestListMode {
    /// What the camera's view takes in (`"default"`).
    #[default]
    Default,
    /// Every object round the agent, whichever way the camera looks
    /// (`"360"`).
    Full360,
}

impl InterestListMode {
    /// The mode's name on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Full360 => "360",
        }
    }

    /// The mode a wire name means. A name that is neither is
    /// [`Default`](Self::Default), which is how Second Life reads one.
    #[must_use]
    pub fn from_wire(name: &str) -> Self {
        if name == "360" {
            Self::Full360
        } else {
            Self::Default
        }
    }
}

/// What a simulator answers an `InterestList` POST with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InterestListReply {
    /// The mode now in force.
    pub mode: InterestListMode,
    /// The mode it replaced.
    pub previous_mode: InterestListMode,
}

/// The mode a field of an `InterestList` body names, [`None`] when the field
/// is absent.
fn mode_field(
    body: &Llsd,
    key: &str,
    field: &'static str,
) -> Result<Option<InterestListMode>, LlsdError> {
    Ok(body.field_str(key, field)?.map(InterestListMode::from_wire))
}

/// Builds the LLSD body of an `InterestList` POST asking for `mode`.
#[must_use]
pub fn build_interest_list_request(mode: InterestListMode) -> String {
    let mut map = HashMap::new();
    let _previous = map.insert("mode".to_owned(), Llsd::String(mode.as_str().to_owned()));
    Llsd::Map(map).to_llsd_xml()
}

/// Decodes an `InterestList` reply.
///
/// # Errors
/// Returns [`LlsdError::MissingField`] of a body that names no mode, and
/// [`LlsdError::MalformedField`] where a mode is not a string.
pub fn parse_interest_list_reply(body: &Llsd) -> Result<InterestListReply, WireError> {
    Ok(InterestListReply {
        mode: mode_field(body, "mode", "mode")?.ok_or(LlsdError::MissingField { field: "mode" })?,
        previous_mode: mode_field(body, "previous_mode", "previous_mode")?.unwrap_or_default(),
    })
}

/// Decodes the mode an `InterestList` POST asks for — the server's side of
/// [`build_interest_list_request`]. A body with no `mode` asks for
/// [`InterestListMode::Default`].
///
/// # Errors
/// Returns [`LlsdError::MalformedField`] where the mode is not a string.
pub fn parse_interest_list_request(body: &Llsd) -> Result<InterestListMode, WireError> {
    Ok(mode_field(body, "mode", "mode")?.unwrap_or_default())
}

/// Builds an `InterestList` reply — the server's side of
/// [`parse_interest_list_reply`].
#[must_use]
pub fn build_interest_list_reply(reply: &InterestListReply) -> String {
    let mut map = HashMap::new();
    let _mode = map.insert(
        "mode".to_owned(),
        Llsd::String(reply.mode.as_str().to_owned()),
    );
    let _previous = map.insert(
        "previous_mode".to_owned(),
        Llsd::String(reply.previous_mode.as_str().to_owned()),
    );
    Llsd::Map(map).to_llsd_xml()
}

#[cfg(test)]
mod tests {
    use super::{
        InterestListMode, InterestListReply, build_interest_list_reply,
        build_interest_list_request, parse_interest_list_reply, parse_interest_list_request,
    };
    use crate::llsd::parse_llsd_xml;
    use pretty_assertions::assert_eq;

    /// A request and a reply come back out of their own bodies.
    #[test]
    fn request_and_reply_round_trip() -> Result<(), String> {
        let request = parse_llsd_xml(&build_interest_list_request(InterestListMode::Full360))
            .map_err(|error| error.to_string())?;
        assert_eq!(
            parse_interest_list_request(&request).map_err(|error| error.to_string())?,
            InterestListMode::Full360
        );
        let reply = InterestListReply {
            mode: InterestListMode::Default,
            previous_mode: InterestListMode::Full360,
        };
        let body = parse_llsd_xml(&build_interest_list_reply(&reply))
            .map_err(|error| error.to_string())?;
        assert_eq!(
            parse_interest_list_reply(&body).map_err(|error| error.to_string())?,
            reply
        );
        Ok(())
    }

    /// The reply aditi gave a POST of `360`, and the one it gave a mode it did
    /// not know.
    #[test]
    fn the_measured_replies_decode() -> Result<(), String> {
        let body = parse_llsd_xml(
            "<llsd><map><key>mode</key><string>360</string><key>previous_mode</key>\
             <string>default</string></map></llsd>",
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(
            parse_interest_list_reply(&body).map_err(|error| error.to_string())?,
            InterestListReply {
                mode: InterestListMode::Full360,
                previous_mode: InterestListMode::Default,
            }
        );
        let empty = parse_llsd_xml("<llsd><map /></llsd>").map_err(|error| error.to_string())?;
        assert!(matches!(
            parse_interest_list_reply(&empty),
            Err(crate::WireError::Llsd(
                crate::llsd::LlsdError::MissingField { field: "mode" }
            ))
        ));
        assert_eq!(
            parse_interest_list_request(&empty).map_err(|error| error.to_string())?,
            InterestListMode::Default
        );
        assert_eq!(
            InterestListMode::from_wire("bogus"),
            InterestListMode::Default
        );
        Ok(())
    }
}
