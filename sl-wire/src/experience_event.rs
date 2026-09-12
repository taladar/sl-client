//! `ExperienceEvent`: the region telling the viewer, after the fact, what an
//! experience it is running under actually **did** to the agent.
//!
//! An experience the agent has joined runs its scripts without prompting for
//! each permission, so the in-the-moment `ScriptQuestion` the ordinary
//! permission surfaces are built on never appears. This message is what replaces
//! it: the simulator reports the exercised permission afterwards, and the
//! viewer's experience **log** keeps it so the user can see what an experience
//! has been doing with the trust they gave it.
//!
//! It is also the **only** signal in the protocol that says "an experience
//! attached something to you" ([`ExperienceEventPermission::Attach`]).
//!
//! The envelope is an ordinary `GenericMessage` (or `LargeGenericMessage`, which
//! the reference dispatches through the same `gGenericDispatcher`) with the
//! method [`EXPERIENCE_EVENT_METHOD`]; the experience's id rides in the
//! message's **invoice** rather than in the parameter list, exactly as an
//! experience environment push's does (see
//! [`parse_environment_push`](crate::parse_environment_push)). The three
//! parameters are:
//!
//! | # | Contents |
//! | --- | --- |
//! | 0 | a serialized LLSD map: `OwnerID`, `Permission`, `IsAttachment` |
//! | 1 | the name of the object whose script exercised the permission |
//! | 2 | the name of the parcel it happened on |
//!
//! Cross-checked against the Firestorm viewer's
//! `LLExperienceLogDispatchHandler` and `LLExperienceLog::handleExperienceMessage`
//! (`indra/newview/llexperiencelog.cpp`).

use sl_llsd::{LlsdEncoding, parse_llsd_serialized, to_llsd_serialized};
use sl_types::key::ExperienceKey;
use std::collections::HashMap;
use uuid::Uuid;

use crate::WireError;
use crate::llsd::{Llsd, LlsdError};

/// The `GenericMessage` method name an experience event travels under.
pub const EXPERIENCE_EVENT_METHOD: &str = "ExperienceEvent";

/// Which permission an experience exercised, as the `Permission` field names it.
///
/// The wire value is **not** the LSL `PERMISSION_*` bit: it is the *index* into
/// the reference's `SCRIPT_PERMISSIONS` table (`llscriptruntimeperms.h`), whose
/// entry *n* carries the bit `1 << (n + 1)`. So `4` is Attach
/// (`PERMISSION_ATTACH`, `1 << 5`) and not `PERMISSION_TRIGGER_ANIMATION`.
/// [`code`](Self::code) is that index.
///
/// Only the nine the reference has a description string for are named
/// (`ExperiencePermission1` … `ExperiencePermission17` in its `strings.xml`);
/// every other index — the ones an experience cannot exercise, like `DEBIT` —
/// is kept verbatim as [`Other`](Self::Other) rather than rejected, because the
/// simulator is free to report one and a log that dropped it would be lying
/// about what happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ExperienceEventPermission {
    /// Took over the agent's controls (`PERMISSION_TAKE_CONTROLS`).
    TakeControls,
    /// Triggered an animation on the agent (`PERMISSION_TRIGGER_ANIMATION`).
    TriggerAnimation,
    /// Attached an object to the agent (`PERMISSION_ATTACH`) — the one case
    /// nothing else in the protocol reports.
    Attach,
    /// Tracked the agent's camera (`PERMISSION_TRACK_CAMERA`).
    TrackCamera,
    /// Controlled the agent's camera (`PERMISSION_CONTROL_CAMERA`).
    ControlCamera,
    /// Teleported the agent (`PERMISSION_TELEPORT`).
    Teleport,
    /// Accepted experience permissions on the agent's behalf
    /// (`PERMISSION_EXPERIENCE`).
    JoinExperience,
    /// Forced the agent to sit (`PERMISSION_SILENT_ESTATE_MANAGEMENT`'s
    /// neighbour `ForceSitAvatar`).
    ForceSit,
    /// Changed the agent's environment settings (`ChangeEnvSettings`) — the
    /// permission behind an
    /// [`ExperienceEnvironmentPush`](crate::ExperienceEnvironmentPush).
    ChangeEnvironment,
    /// An index the reference has no description for, kept verbatim.
    Other(i32),
}

impl ExperienceEventPermission {
    /// The `Permission` index this case is spelled with on the wire.
    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            Self::TakeControls => 1,
            Self::TriggerAnimation => 3,
            Self::Attach => 4,
            Self::TrackCamera => 9,
            Self::ControlCamera => 10,
            Self::Teleport => 11,
            Self::JoinExperience => 12,
            Self::ForceSit => 16,
            Self::ChangeEnvironment => 17,
            Self::Other(code) => code,
        }
    }

    /// The case a wire `Permission` index names.
    #[must_use]
    pub const fn from_code(code: i32) -> Self {
        match code {
            1 => Self::TakeControls,
            3 => Self::TriggerAnimation,
            4 => Self::Attach,
            9 => Self::TrackCamera,
            10 => Self::ControlCamera,
            11 => Self::Teleport,
            12 => Self::JoinExperience,
            16 => Self::ForceSit,
            17 => Self::ChangeEnvironment,
            other => Self::Other(other),
        }
    }
}

/// One `ExperienceEvent` message: which experience acted, what it did, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ExperienceEvent {
    /// The experience that acted — carried in the message's **invoice**, not in
    /// the parameter list.
    pub experience_id: ExperienceKey,
    /// The owner of the object whose script acted (`OwnerID`). A bare [`Uuid`]
    /// because the wire says only "an owner": a deeded object's owner is its
    /// group, and nothing in the message says which kind this one is.
    pub owner_id: Uuid,
    /// The permission that was exercised, or `None` when the message carried no
    /// `Permission` field at all — which the reference tolerates
    /// (`message.has("Permission")`) and renders as an unknown operation.
    pub permission: Option<ExperienceEventPermission>,
    /// Whether the acting object was an **attachment** on the agent rather than
    /// something rezzed in the world. It picks which of the two notification
    /// templates the log raises.
    pub is_attachment: bool,
    /// The name of the object whose script acted (`ObjectName`).
    pub object_name: String,
    /// The name of the parcel it happened on (`ParcelName`).
    pub parcel_name: String,
}

/// Builds the three `GenericMessage` parameters carrying `event`.
///
/// The [`experience_id`](ExperienceEvent::experience_id) is **not** among them:
/// the reference reads it off the message's invoice
/// (`message["public_id"] = invoice`), so a sender has to put it there.
///
/// The parameter-0 map is written as notation LLSD behind its
/// `<? llsd/notation ?>` header — the reference reads it with
/// `LLSDSerialize::deserialize`, which accepts any of the three encodings and
/// guesses when there is no header, so naming the encoding is free and removes
/// the guess.
#[must_use]
pub fn build_experience_event_params(event: &ExperienceEvent) -> Vec<Vec<u8>> {
    let mut body = HashMap::from([
        ("OwnerID".to_owned(), Llsd::Uuid(event.owner_id)),
        (
            "IsAttachment".to_owned(),
            Llsd::Boolean(event.is_attachment),
        ),
    ]);
    if let Some(permission) = event.permission {
        drop(body.insert("Permission".to_owned(), Llsd::Integer(permission.code())));
    }
    vec![
        to_llsd_serialized(&Llsd::Map(body), LlsdEncoding::Notation),
        event.object_name.clone().into_bytes(),
        event.parcel_name.clone().into_bytes(),
    ]
}

/// Parses an `ExperienceEvent` parameter list — the inverse of
/// [`build_experience_event_params`]. `experience_id` is the message's invoice.
///
/// The two name parameters are optional, exactly as the reference's handler
/// treats them (it stops copying as soon as the parameter list runs out), and a
/// trailing NUL is stripped from each: the simulator sends its strings
/// NUL-terminated.
///
/// # Errors
///
/// Returns a [`WireError`] when the parameter list is empty or parameter 0 is
/// not a serialized LLSD map. Every *field* inside it is optional, because the
/// reference reads each with a `has`-guard and the log is expected to record
/// whatever arrived: a missing `Permission` becomes `None`, a missing `OwnerID`
/// the nil uuid, a missing `IsAttachment` false.
pub fn parse_experience_event(
    experience_id: ExperienceKey,
    params: &[Vec<u8>],
) -> Result<ExperienceEvent, WireError> {
    let body = params.first().ok_or(LlsdError::MissingField {
        field: "ExperienceEvent parameter 0",
    })?;
    let message = parse_llsd_serialized(body)?;
    Ok(ExperienceEvent {
        experience_id,
        owner_id: message
            .field_uuid("OwnerID", "OwnerID")?
            .unwrap_or_else(Uuid::nil),
        permission: message
            .get("Permission")
            .and_then(Llsd::as_i32)
            .map(ExperienceEventPermission::from_code),
        is_attachment: message
            .get("IsAttachment")
            .and_then(Llsd::as_bool)
            .unwrap_or(false),
        object_name: param_string(params.get(1)),
        parcel_name: param_string(params.get(2)),
    })
}

/// One string parameter, NUL-terminator and all trailing padding removed; the
/// empty string for a parameter the sender omitted.
fn param_string(param: Option<&Vec<u8>>) -> String {
    param.map_or_else(String::new, |bytes| {
        let end = bytes
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(bytes.len());
        String::from_utf8_lossy(bytes.get(..end).unwrap_or_default()).into_owned()
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::{
        ExperienceEvent, ExperienceEventPermission, ExperienceKey, Llsd, LlsdError, WireError,
        build_experience_event_params, parse_experience_event,
    };

    /// The experience every fixture below acts as.
    fn experience() -> ExperienceKey {
        ExperienceKey::from(Uuid::from_u128(0x0001_0002_0003_0004_0005_0006_0007_0008))
    }

    /// An event carrying `permission`, with the names and owner a real one has.
    fn event(
        permission: Option<ExperienceEventPermission>,
        is_attachment: bool,
    ) -> ExperienceEvent {
        ExperienceEvent {
            experience_id: experience(),
            owner_id: Uuid::from_u128(0x00aa),
            permission,
            is_attachment,
            object_name: "Ride Controller".to_owned(),
            parcel_name: "The Back Forty".to_owned(),
        }
    }

    /// Round-trips `original` through the parameter list and asserts it survives.
    fn round_trip(original: &ExperienceEvent) {
        let params = build_experience_event_params(original);
        assert_eq!(params.len(), 3, "an event is three parameters");
        assert_eq!(
            parse_experience_event(original.experience_id, &params).as_ref(),
            Ok(original)
        );
    }

    #[test]
    fn an_attach_event_round_trips() {
        round_trip(&event(Some(ExperienceEventPermission::Attach), true));
    }

    #[test]
    fn a_world_object_event_round_trips() {
        round_trip(&event(Some(ExperienceEventPermission::Teleport), false));
    }

    /// An index the reference names no string for survives verbatim rather than
    /// being dropped or collapsed onto a neighbour.
    #[test]
    fn an_unnamed_permission_index_round_trips_verbatim() {
        round_trip(&event(Some(ExperienceEventPermission::Other(0)), false));
        round_trip(&event(Some(ExperienceEventPermission::Other(18)), false));
    }

    /// Every named case's wire index is the one the reference's strings and its
    /// `SCRIPT_PERMISSIONS` table agree on, and the mapping is a bijection.
    #[test]
    fn the_named_codes_are_the_reference_indices() {
        for (permission, code) in [
            (ExperienceEventPermission::TakeControls, 1),
            (ExperienceEventPermission::TriggerAnimation, 3),
            (ExperienceEventPermission::Attach, 4),
            (ExperienceEventPermission::TrackCamera, 9),
            (ExperienceEventPermission::ControlCamera, 10),
            (ExperienceEventPermission::Teleport, 11),
            (ExperienceEventPermission::JoinExperience, 12),
            (ExperienceEventPermission::ForceSit, 16),
            (ExperienceEventPermission::ChangeEnvironment, 17),
        ] {
            assert_eq!(permission.code(), code);
            assert_eq!(ExperienceEventPermission::from_code(code), permission);
        }
        // An unnamed index is kept, not folded onto a named neighbour.
        assert_eq!(
            ExperienceEventPermission::from_code(2),
            ExperienceEventPermission::Other(2)
        );
    }

    /// A message with no `Permission` at all is a *record*, not a parse failure:
    /// the reference guards every read with `has` and logs whatever arrived. The
    /// experience environment push is exactly such a message.
    #[test]
    fn a_message_without_a_permission_decodes_as_none() -> Result<(), String> {
        let original = event(None, false);
        let params = build_experience_event_params(&original);
        let parsed =
            parse_experience_event(experience(), &params).map_err(|error| error.to_string())?;
        assert_eq!(parsed.permission, None);
        assert_eq!(parsed, original);
        Ok(())
    }

    /// An otherwise empty map still decodes — the fields default rather than
    /// reject, because a log that refused a sparse report would lose the event.
    #[test]
    fn an_empty_body_decodes_to_defaults() -> Result<(), String> {
        let body = Llsd::Map(HashMap::new());
        let parsed = parse_experience_event(experience(), &[body.to_llsd_notation()])
            .map_err(|error| error.to_string())?;
        assert_eq!(parsed.owner_id, Uuid::nil());
        assert_eq!(parsed.permission, None);
        assert!(!parsed.is_attachment);
        assert_eq!(parsed.object_name, "");
        assert_eq!(parsed.parcel_name, "");
        Ok(())
    }

    #[test]
    fn the_experience_id_is_the_invoice_and_never_a_parameter() -> Result<(), String> {
        let original = event(Some(ExperienceEventPermission::Attach), true);
        let params = build_experience_event_params(&original);
        let body = params.first().ok_or("no parameter 0")?;
        let text = String::from_utf8_lossy(body).into_owned();
        assert!(
            !text.contains(&original.experience_id.uuid().to_string()),
            "the experience id leaked into the parameter list: {text}"
        );
        let other = ExperienceKey::from(Uuid::from_u128(0x99));
        assert_eq!(
            parse_experience_event(other, &params)
                .map_err(|error| error.to_string())?
                .experience_id,
            other,
            "the invoice is what names the experience"
        );
        Ok(())
    }

    #[test]
    fn the_names_are_optional_and_nul_terminated_ones_are_trimmed() -> Result<(), String> {
        let original = event(Some(ExperienceEventPermission::Attach), true);
        let body = build_experience_event_params(&original)
            .into_iter()
            .next()
            .ok_or("no parameter 0")?;
        let bare = parse_experience_event(experience(), std::slice::from_ref(&body))
            .map_err(|error| error.to_string())?;
        assert_eq!(bare.object_name, "");
        assert_eq!(bare.parcel_name, "");
        let terminated = parse_experience_event(
            experience(),
            &[
                body,
                b"Ride Controller\0".to_vec(),
                b"The Back Forty\0".to_vec(),
            ],
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(terminated.object_name, "Ride Controller");
        assert_eq!(terminated.parcel_name, "The Back Forty");
        Ok(())
    }

    #[test]
    fn an_empty_parameter_list_is_rejected() {
        assert!(matches!(
            parse_experience_event(experience(), &[]),
            Err(WireError::Llsd(LlsdError::MissingField { .. }))
        ));
    }
}
