//! The **`UserInfo`** capability: get/set the agent's account contact and
//! directory preferences.
//!
//! The modern equivalent of the legacy `UserInfoRequest` / `UpdateUserInfo` /
//! `UserInfoReply` UDP messages: a GET returns the account's stored contact
//! settings (whether offline IMs are forwarded to email, the directory/search
//! visibility, and the account email address), and a POST of the writable pair
//! updates them. The key names are cross-checked against the Firestorm
//! viewer's `indra/newview/llagent.cpp` (`requestAgentUserInfoCoro` /
//! `updateAgentUserInfoCoro`): the GET reply carries `success` (with an
//! optional `message` on failure) plus `im_via_email`, `email`, and
//! `directory_visibility`; the POST body carries `dir_visibility` and —
//! meaningful on OpenSim only, Second Life manages email forwarding on the
//! account website — `im_via_email`. The POST reply carries only
//! `success`/`message` (the reference viewer does not read any echoed fields
//! from it).
//!
//! This module builds the update body and decodes the reply (client side), and
//! parses the update body and builds the reply (server side).

use std::collections::HashMap;

use crate::WireError;
use crate::llsd::Llsd;

/// A `UserInfo` capability reply — either a GET's full stored set or a POST's
/// bare acknowledgement. Field keys: `success`, `message`, `im_via_email`,
/// `email`, `directory_visibility`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UserInfoCapReply {
    /// Whether the request succeeded (`success`). Absent is treated as
    /// success, matching the reference viewer's tolerant read.
    pub success: bool,
    /// The failure detail the grid attached (`message`), if any.
    pub message: Option<String>,
    /// Whether offline instant messages are forwarded to the account email
    /// (`im_via_email`). Absent on a bare POST acknowledgement (and on Second
    /// Life, which manages the forwarding preference on the account website).
    pub im_via_email: Option<bool>,
    /// The account's directory/search visibility token
    /// (`directory_visibility`), `"default"` or `"hidden"`. Absent on a bare
    /// POST acknowledgement.
    pub directory_visibility: Option<String>,
    /// The account email address on file (`email`). Absent on a bare POST
    /// acknowledgement.
    pub email: Option<String>,
}

/// The writable pair a `UserInfo` POST carries (`dir_visibility`,
/// `im_via_email`). The email address itself is not settable through the
/// capability.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UserInfoUpdate {
    /// Whether offline instant messages should be forwarded to the account
    /// email (`im_via_email`). [`None`] omits the key, as the reference viewer
    /// does on Second Life (the grid manages the preference itself there);
    /// OpenSim expects it present.
    pub im_via_email: Option<bool>,
    /// The directory/search visibility token to store (`dir_visibility`),
    /// `"default"` or `"hidden"`.
    pub dir_visibility: String,
}

// ---------------------------------------------------------------------------
// Client side — the update-body builder and reply parser.
// ---------------------------------------------------------------------------

/// Serialises a [`UserInfoUpdate`] to the `UserInfo` POST body's LLSD map.
fn user_info_update_to_llsd(update: &UserInfoUpdate) -> Llsd {
    let mut map: HashMap<String, Llsd> = HashMap::new();
    let _previous = map.insert(
        "dir_visibility".to_owned(),
        Llsd::String(update.dir_visibility.clone()),
    );
    if let Some(im_via_email) = update.im_via_email {
        let _previous = map.insert("im_via_email".to_owned(), Llsd::Boolean(im_via_email));
    }
    Llsd::Map(map)
}

/// Builds the LLSD-XML body for a `UserInfo` POST. Built on
/// [`Llsd::to_llsd_xml`], so it round-trips through
/// [`parse_user_info_update`].
#[must_use]
pub fn build_user_info_update(update: &UserInfoUpdate) -> String {
    user_info_update_to_llsd(update).to_llsd_xml()
}

/// Decodes a `UserInfo` capability reply — a GET's stored set or a POST's bare
/// acknowledgement (whose contact fields decode to [`None`]). An absent
/// `success` key counts as success, matching the reference viewer's read of
/// grids that omit it.
///
/// # Errors
/// Returns [`WireError`] if a present field is of the wrong LLSD kind.
pub fn parse_user_info_reply(body: &Llsd) -> Result<UserInfoCapReply, WireError> {
    Ok(UserInfoCapReply {
        success: body.field_bool("success", "success")?.unwrap_or(true),
        message: body.field_str("message", "message")?.map(str::to_owned),
        im_via_email: body.field_bool("im_via_email", "im_via_email")?,
        directory_visibility: body
            .field_str("directory_visibility", "directory_visibility")?
            .map(str::to_owned),
        email: body.field_str("email", "email")?.map(str::to_owned),
    })
}

// ---------------------------------------------------------------------------
// Server side — the inverse: the update-body parser and reply builders.
// ---------------------------------------------------------------------------

/// Parses a `UserInfo` POST body into the writable pair — the inverse of
/// [`build_user_info_update`].
///
/// # Errors
/// Returns [`WireError`] if `dir_visibility` is missing or a present field is
/// of the wrong LLSD kind.
pub fn parse_user_info_update(body: &Llsd) -> Result<UserInfoUpdate, WireError> {
    Ok(UserInfoUpdate {
        im_via_email: body.field_bool("im_via_email", "im_via_email")?,
        dir_visibility: body
            .require_str("dir_visibility", "dir_visibility")?
            .to_owned(),
    })
}

/// Builds a `UserInfo` GET reply carrying the stored set (`success` true plus
/// the three contact fields) — the shape [`parse_user_info_reply`] decodes.
#[must_use]
pub fn build_user_info_reply(
    im_via_email: bool,
    directory_visibility: &str,
    email: &str,
) -> String {
    Llsd::Map(HashMap::from([
        ("success".to_owned(), Llsd::Boolean(true)),
        ("im_via_email".to_owned(), Llsd::Boolean(im_via_email)),
        (
            "directory_visibility".to_owned(),
            Llsd::String(directory_visibility.to_owned()),
        ),
        ("email".to_owned(), Llsd::String(email.to_owned())),
    ]))
    .to_llsd_xml()
}

/// Builds a `UserInfo` POST acknowledgement: `success`, with `message`
/// attached on failure — the bare-acknowledgement shape
/// [`parse_user_info_reply`] decodes.
#[must_use]
pub fn build_user_info_ack(success: bool, message: Option<&str>) -> String {
    let mut map: HashMap<String, Llsd> =
        HashMap::from([("success".to_owned(), Llsd::Boolean(success))]);
    if let Some(message) = message {
        let _previous = map.insert("message".to_owned(), Llsd::String(message.to_owned()));
    }
    Llsd::Map(map).to_llsd_xml()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        UserInfoCapReply, UserInfoUpdate, build_user_info_ack, build_user_info_reply,
        build_user_info_update, parse_user_info_reply, parse_user_info_update,
    };
    use crate::llsd::parse_llsd_xml;

    /// An update body round-trips through the server-side parser, both with
    /// the OpenSim-meaningful `im_via_email` present and with it omitted (the
    /// Second Life shape).
    #[test]
    fn update_body_round_trips() -> Result<(), String> {
        for update in [
            UserInfoUpdate {
                im_via_email: Some(true),
                dir_visibility: "hidden".to_owned(),
            },
            UserInfoUpdate {
                im_via_email: None,
                dir_visibility: "default".to_owned(),
            },
        ] {
            let body = build_user_info_update(&update);
            let parsed = parse_user_info_update(
                &parse_llsd_xml(&body).map_err(|error| format!("{error:?}"))?,
            )
            .map_err(|error| format!("{error:?}"))?;
            assert_eq!(parsed, update);
        }
        Ok(())
    }

    /// A GET reply carrying the stored set decodes into the full
    /// [`UserInfoCapReply`], and a bare POST acknowledgement decodes with the
    /// contact fields absent (success defaulting from the present key).
    #[test]
    fn replies_round_trip() -> Result<(), String> {
        let xml = build_user_info_reply(true, "hidden", "someone@example.com");
        let parsed =
            parse_user_info_reply(&parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            parsed,
            UserInfoCapReply {
                success: true,
                message: None,
                im_via_email: Some(true),
                directory_visibility: Some("hidden".to_owned()),
                email: Some("someone@example.com".to_owned()),
            }
        );

        let ack = build_user_info_ack(false, Some("no such agent"));
        let parsed =
            parse_user_info_reply(&parse_llsd_xml(&ack).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert!(!parsed.success);
        assert_eq!(parsed.message.as_deref(), Some("no such agent"));
        assert_eq!(parsed.directory_visibility, None);
        Ok(())
    }
}
