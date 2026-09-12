//! `PushExpEnvironment`: the environment an **experience** pushes at the
//! viewers inside it (`llSetEnvironment`).
//!
//! This is the one live environment change in the protocol. An estate that
//! edits its sky changes what the `ExtEnvironment` capability *answers* and
//! relies on a `RegionInfo` to make viewers ask again; an experience instead
//! sends each viewer a settings layer that sits **above** the region's and is
//! taken away again when the experience releases them — so releasing it
//! restores the region's own sky with no refetch.
//!
//! The envelope is an ordinary `GenericMessage` (or `LargeGenericMessage`,
//! which the reference dispatches through the same handler) with the method
//! [`PUSH_EXP_ENVIRONMENT_METHOD`]; the experience's id rides in the message's
//! **invoice** rather than in the parameter list. The three parameters are:
//!
//! | # | Contents |
//! | --- | --- |
//! | 0 | a serialized LLSD map: `action`, `action_data`, and the log fields |
//! | 1 | the name of the object whose script pushed it |
//! | 2 | the name of the parcel it was pushed from |
//!
//! Cross-checked against the Firestorm viewer's
//! `LLEnvironmentPushDispatchHandler` and `LLEnvironment::handleEnvironmentPush`
//! (`indra/newview/llenvironment.cpp`), plus `LLExperienceLog
//! ::handleExperienceMessage`, which reads the same map for the experience log.

use sl_llsd::{LlsdEncoding, parse_llsd_serialized, to_llsd_serialized};
use sl_types::key::ExperienceKey;
use std::collections::HashMap;
use uuid::Uuid;

use crate::WireError;
use crate::llsd::{Llsd, LlsdError};

/// The `GenericMessage` method name an experience environment push travels
/// under (`MESSAGE_PUSHENVIRONMENT`).
pub const PUSH_EXP_ENVIRONMENT_METHOD: &str = "PushExpEnvironment";

/// The `action` value asking the viewer to drop this experience's layer
/// (`ACTION_CLEARENVIRONMENT`).
const ACTION_CLEAR: &str = "ClearEnvironment";

/// The `action` value pushing a whole settings asset (`ACTION_PUSHFULLENVIRONMENT`).
const ACTION_PUSH_FULL: &str = "PushFullEnvironment";

/// The `action` value pushing a sky / water fragment (`ACTION_PUSHPARTIALENVIRONMENT`).
const ACTION_PUSH_PARTIAL: &str = "PushPartialEnvironment";

/// What an experience is asking the viewer's environment to do.
///
/// The three cases are `LLEnvironment::handleEnvironmentPush`'s own dispatch,
/// and they differ in more than degree: a **full** push names a settings asset
/// the viewer has to fetch, a **partial** one carries the changed sky / water
/// values inline, and a **clear** carries nothing at all.
///
/// Deliberately **not** `#[non_exhaustive]`: these three are the whole of
/// `handleEnvironmentPush`'s dispatch, and [`parse_environment_push`] rejects
/// an `action` naming anything else rather than surfacing it. A fourth would be
/// a protocol change that ought to break each consumer's match rather than fall
/// into a wildcard that quietly renders the old sky.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EnvironmentPushAction {
    /// Release the viewer: drop whatever this experience had injected and go
    /// back to the environment underneath.
    Clear,
    /// Install a whole settings asset — a day cycle, a sky, or a water frame,
    /// whichever the asset turns out to be — which the viewer fetches by id.
    Full {
        /// The `AT_SETTINGS` asset to install.
        asset_id: Uuid,
    },
    /// Merge a fragment of sky and/or water settings over whatever is in force.
    ///
    /// The maps are **not** whole frames: the reference overlays each key it
    /// finds onto the settings underneath (`LLSettingsInjected
    /// ::injectExperienceValues`, one `injectSetting` per key), so a push that
    /// names only `cloud_shadow` changes only the clouds. Carried as raw
    /// [`Llsd`] for exactly that reason — a decoded frame would have invented
    /// values for every key the experience did not send.
    Partial {
        /// The sky keys to overlay, if the push carries any.
        sky: Option<Llsd>,
        /// The water keys to overlay, if the push carries any.
        water: Option<Llsd>,
    },
}

impl EnvironmentPushAction {
    /// The `action` string this case is spelled with on the wire.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match *self {
            Self::Clear => ACTION_CLEAR,
            Self::Full { .. } => ACTION_PUSH_FULL,
            Self::Partial { .. } => ACTION_PUSH_PARTIAL,
        }
    }
}

/// One `PushExpEnvironment` message: which experience is pushing, what it wants
/// done, how long the change should take, and the two names the experience log
/// records it under.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ExperienceEnvironmentPush {
    /// The experience doing the pushing — carried in the message's **invoice**,
    /// not in the parameter list, and the key every injected value is filed
    /// under so releasing one experience leaves another's alone.
    pub experience_id: ExperienceKey,
    /// What is being pushed (or taken away).
    pub action: EnvironmentPushAction,
    /// How long the viewer should take to reach the new environment, in
    /// seconds. Zero is an instant cut; the reference treats anything up to
    /// `0.1` as one.
    pub transition_time: f32,
    /// The owner of the object whose script pushed this (`OwnerID`) — read by
    /// the experience log, which groups repeat pushes by it. A bare `Uuid`
    /// because the wire says only "an owner": a deeded object's owner is its
    /// group, and nothing in the message says which kind this one is.
    pub owner_id: Uuid,
    /// The name of the object whose script pushed this (`ObjectName`).
    pub object_name: String,
    /// The name of the parcel it was pushed from (`ParcelName`).
    pub parcel_name: String,
}

/// Builds the three `GenericMessage` parameters carrying `push`.
///
/// The [`experience_id`](ExperienceEnvironmentPush::experience_id) is **not**
/// among them: the reference reads it off the message's invoice
/// (`message[KEY_EXPERIENCEID] = invoice`), so a sender has to put it there.
///
/// The parameter-0 map is written as notation LLSD behind its
/// `<? llsd/notation ?>` header. The reference reads it with
/// `LLSDSerialize::deserialize`, which accepts any of the three encodings and
/// guesses when there is no header — so naming the encoding is free and
/// removes the guess.
#[must_use]
pub fn build_environment_push_params(push: &ExperienceEnvironmentPush) -> Vec<Vec<u8>> {
    let mut action_data = HashMap::from([(
        "transition_time".to_owned(),
        Llsd::Real(push.transition_time.into()),
    )]);
    match &push.action {
        EnvironmentPushAction::Clear => {}
        EnvironmentPushAction::Full { asset_id } => {
            drop(action_data.insert("asset_id".to_owned(), Llsd::Uuid(*asset_id)));
        }
        EnvironmentPushAction::Partial { sky, water } => {
            let mut settings = HashMap::new();
            if let Some(sky) = sky {
                drop(settings.insert("sky".to_owned(), sky.clone()));
            }
            if let Some(water) = water {
                drop(settings.insert("water".to_owned(), water.clone()));
            }
            drop(action_data.insert("settings".to_owned(), Llsd::Map(settings)));
        }
    }
    let message = Llsd::Map(HashMap::from([
        (
            "action".to_owned(),
            Llsd::String(push.action.name().to_owned()),
        ),
        ("action_data".to_owned(), Llsd::Map(action_data)),
        ("OwnerID".to_owned(), Llsd::Uuid(push.owner_id)),
    ]));
    vec![
        to_llsd_serialized(&message, LlsdEncoding::Notation),
        push.object_name.clone().into_bytes(),
        push.parcel_name.clone().into_bytes(),
    ]
}

/// Parses a `PushExpEnvironment` parameter list — the inverse of
/// [`build_environment_push_params`]. `experience_id` is the message's invoice.
///
/// The two name parameters are optional, exactly as the reference's handler
/// treats them (it stops copying as soon as the parameter list runs out), and a
/// trailing NUL is stripped from each: the simulator sends its strings
/// NUL-terminated.
///
/// # Errors
///
/// Returns a [`WireError`] when the parameter list is empty, when parameter 0
/// is not a serialized LLSD map, or when its `action` is absent or names none
/// of the three cases. An unknown action is a *rejection* rather than a
/// tolerated no-op: the reference logs it and does nothing, which leaves a
/// viewer showing an environment nobody can account for.
pub fn parse_environment_push(
    experience_id: ExperienceKey,
    params: &[Vec<u8>],
) -> Result<ExperienceEnvironmentPush, WireError> {
    let body = params.first().ok_or(LlsdError::MissingField {
        field: "PushExpEnvironment parameter 0",
    })?;
    let message = parse_llsd_serialized(body)?;
    let action_name = message.require_str("action", "action")?;
    let action_data = message.get("action_data");
    let transition_time = action_data
        .and_then(|data| data.get("transition_time"))
        .and_then(Llsd::as_f32)
        .unwrap_or(0.0);
    let action = match action_name {
        ACTION_CLEAR => EnvironmentPushAction::Clear,
        ACTION_PUSH_FULL => EnvironmentPushAction::Full {
            asset_id: action_data
                .ok_or(LlsdError::MissingField {
                    field: "action_data",
                })?
                .require_uuid("asset_id", "asset_id")?,
        },
        ACTION_PUSH_PARTIAL => {
            let settings = action_data.and_then(|data| data.get("settings"));
            EnvironmentPushAction::Partial {
                sky: settings.and_then(|map| map.get("sky")).cloned(),
                water: settings.and_then(|map| map.get("water")).cloned(),
            }
        }
        unknown => {
            return Err(WireError::from(LlsdError::MalformedField {
                field: "action",
                value: unknown.to_owned(),
            }));
        }
    };
    Ok(ExperienceEnvironmentPush {
        experience_id,
        action,
        transition_time,
        owner_id: message
            .field_uuid("OwnerID", "OwnerID")?
            .unwrap_or_else(Uuid::nil),
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
        EnvironmentPushAction, ExperienceEnvironmentPush, ExperienceKey, Llsd, LlsdError,
        WireError, build_environment_push_params, parse_environment_push,
    };

    /// The experience every fixture below pushes as.
    fn experience() -> ExperienceKey {
        ExperienceKey::from(Uuid::from_u128(0x0001_0002_0003_0004_0005_0006_0007_0008))
    }

    /// A push carrying `action`, with the names and owner a real one has.
    fn push(action: EnvironmentPushAction, transition_time: f32) -> ExperienceEnvironmentPush {
        ExperienceEnvironmentPush {
            experience_id: experience(),
            action,
            transition_time,
            owner_id: Uuid::from_u128(0x00aa),
            object_name: "Weather Machine".to_owned(),
            parcel_name: "The Back Forty".to_owned(),
        }
    }

    /// Round-trips `original` through the parameter list and asserts it survives.
    fn round_trip(original: &ExperienceEnvironmentPush) {
        let params = build_environment_push_params(original);
        assert_eq!(params.len(), 3, "a push is three parameters");
        assert_eq!(
            parse_environment_push(original.experience_id, &params).as_ref(),
            Ok(original)
        );
    }

    #[test]
    fn a_clear_round_trips() {
        round_trip(&push(EnvironmentPushAction::Clear, 0.0));
    }

    #[test]
    fn a_full_push_round_trips_with_its_asset_and_transition() {
        round_trip(&push(
            EnvironmentPushAction::Full {
                asset_id: Uuid::from_u128(0xfeed),
            },
            4.5,
        ));
    }

    #[test]
    fn a_partial_push_round_trips_only_the_keys_it_carries() {
        round_trip(&push(
            EnvironmentPushAction::Partial {
                sky: Some(Llsd::Map(HashMap::from([(
                    "cloud_shadow".to_owned(),
                    Llsd::Real(0.75),
                )]))),
                water: None,
            },
            1.0,
        ));
    }

    #[test]
    fn the_experience_id_is_the_invoice_and_never_a_parameter() -> Result<(), String> {
        let original = push(EnvironmentPushAction::Clear, 0.0);
        let params = build_environment_push_params(&original);
        let body = params.first().ok_or("no parameter 0")?;
        let text = String::from_utf8_lossy(body).into_owned();
        assert!(
            !text.contains(&original.experience_id.uuid().to_string()),
            "the experience id leaked into the parameter list: {text}"
        );
        let other = ExperienceKey::from(Uuid::from_u128(0x99));
        assert_eq!(
            parse_environment_push(other, &params)
                .map_err(|error| error.to_string())?
                .experience_id,
            other,
            "the invoice is what names the experience"
        );
        Ok(())
    }

    #[test]
    fn the_names_are_optional_and_nul_terminated_ones_are_trimmed() -> Result<(), String> {
        let original = push(EnvironmentPushAction::Clear, 0.0);
        let body = build_environment_push_params(&original)
            .into_iter()
            .next()
            .ok_or("no parameter 0")?;
        let parsed = parse_environment_push(experience(), std::slice::from_ref(&body))
            .map_err(|error| error.to_string())?;
        assert_eq!(parsed.object_name, "");
        assert_eq!(parsed.parcel_name, "");
        let terminated = parse_environment_push(
            experience(),
            &[
                body,
                b"Weather Machine\0".to_vec(),
                b"The Back Forty\0".to_vec(),
            ],
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(parsed.action, terminated.action);
        assert_eq!(terminated.object_name, "Weather Machine");
        assert_eq!(terminated.parcel_name, "The Back Forty");
        Ok(())
    }

    #[test]
    fn an_unknown_action_is_rejected_rather_than_ignored() {
        let body = Llsd::Map(HashMap::from([(
            "action".to_owned(),
            Llsd::String("MakeItRain".to_owned()),
        )]));
        assert!(
            matches!(
                parse_environment_push(experience(), &[body.to_llsd_notation()]),
                Err(WireError::Llsd(LlsdError::MalformedField {
                    field: "action",
                    ..
                }))
            ),
            "an action nothing implements must not decode as a no-op"
        );
    }

    #[test]
    fn a_full_push_without_an_asset_is_rejected() {
        let body = Llsd::Map(HashMap::from([
            (
                "action".to_owned(),
                Llsd::String("PushFullEnvironment".to_owned()),
            ),
            ("action_data".to_owned(), Llsd::Map(HashMap::new())),
        ]));
        assert!(matches!(
            parse_environment_push(experience(), &[body.to_llsd_notation()]),
            Err(WireError::Llsd(LlsdError::MissingField {
                field: "asset_id"
            }))
        ));
    }

    #[test]
    fn an_empty_parameter_list_is_rejected() {
        assert!(matches!(
            parse_environment_push(experience(), &[]),
            Err(WireError::Llsd(LlsdError::MissingField { .. }))
        ));
    }
}
