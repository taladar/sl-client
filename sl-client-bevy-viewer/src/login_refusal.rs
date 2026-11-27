//! A login refusal said in the user's language
//! (`viewer-login-refusal-localised-text`).
//!
//! Second Life sends every refusal with a localization key, `message_id`
//! (`LoginFailedAuthenticationFailed` for a wrong password), the arguments the
//! string takes, `message_args`, and an incident id, `Linden_Error_Code` —
//! besides its English `message`. The reference (`llstartup.cpp`, the
//! `LoginFailedHeader` block) looks the key up in its own strings with the
//! arguments substituted, and shows the grid's `message` only when it has no
//! such string: a grid may add refusals a viewer does not know yet. A
//! second-factor challenge is worded the same way (`lllogininstance.cpp`).
//! OpenSim sends no key, so its text is always shown as sent.
//!
//! [`GridText`] is what a refusal or a challenge brings; [`GridText::localise`]
//! is the lookup. The key table, [`REFUSAL_STRINGS`], is the reference's
//! `LoginFailed*` / `LogoutFailed*` strings (`strings.xml`), each under a
//! `login-refusal-` key in the Fluent catalogue.

use std::collections::BTreeMap;

use sl_l10n::{CivilDateTime, DateTimeLength, DateTimeStyle};

use crate::i18n::{TransArgs, Translator};

/// The reference's refusal strings, by the `message_id` a grid names them
/// with, and the catalogue key each is kept under here.
///
/// `LoginFailedAcountSuspended` is misspelt on purpose: it is the reference's
/// name for the string, and so the key Second Life sends.
pub(crate) const REFUSAL_STRINGS: &[(&str, &str)] = &[
    ("LoginFailed", "login-refusal-failed"),
    (
        "LoginFailedViewerNotPermitted",
        "login-refusal-viewer-not-permitted",
    ),
    ("LoginFailedRequiredUpdate", "login-refusal-required-update"),
    (
        "LoginFailedAlreadyLoggedIn",
        "login-refusal-already-logged-in",
    ),
    (
        "LoginFailedAuthenticationFailed",
        "login-refusal-authentication-failed",
    ),
    (
        "LoginFailedPasswordChanged",
        "login-refusal-password-changed",
    ),
    ("LoginFailedPasswordReset", "login-refusal-password-reset"),
    ("LoginFailedEmployeesOnly", "login-refusal-employees-only"),
    ("LoginFailedPremiumOnly", "login-refusal-premium-only"),
    (
        "LoginFailedComputerProhibited",
        "login-refusal-computer-prohibited",
    ),
    (
        "LoginFailedAcountSuspended",
        "login-refusal-account-suspended",
    ),
    (
        "LoginFailedAccountDisabled",
        "login-refusal-account-disabled",
    ),
    ("LoginFailedTransformError", "login-refusal-transform-error"),
    (
        "LoginFailedAccountMaintenance",
        "login-refusal-account-maintenance",
    ),
    (
        "LoginFailedPendingLogoutFault",
        "login-refusal-pending-logout-fault",
    ),
    ("LoginFailedPendingLogout", "login-refusal-pending-logout"),
    (
        "LoginFailedUnableToCreateSession",
        "login-refusal-unable-to-create-session",
    ),
    (
        "LoginFailedUnableToConnectToSimulator",
        "login-refusal-unable-to-connect-to-simulator",
    ),
    (
        "LoginFailedRestrictedHours",
        "login-refusal-restricted-hours",
    ),
    (
        "LoginFailedIncorrectParameters",
        "login-refusal-incorrect-parameters",
    ),
    (
        "LoginFailedFirstNameNotAlphanumeric",
        "login-refusal-first-name-not-alphanumeric",
    ),
    (
        "LoginFailedLastNameNotAlphanumeric",
        "login-refusal-last-name-not-alphanumeric",
    ),
    (
        "LoginFailedLoggingOutSession",
        "login-refusal-logging-out-session",
    ),
    (
        "LoginFailedAuthenticationMFARequired",
        "login-refusal-mfa-required",
    ),
    (
        "LogoutFailedRegionGoingOffline",
        "login-refusal-region-going-offline",
    ),
    (
        "LogoutFailedAgentNotInRegion",
        "login-refusal-agent-not-in-region",
    ),
    ("LogoutFailedPendingLogin", "login-refusal-pending-login"),
    ("LogoutFailedLoggingOut", "login-refusal-logging-out"),
    (
        "LogoutFailedStillLoggingOut",
        "login-refusal-still-logging-out",
    ),
    ("LogoutSucceeded", "login-refusal-logout-succeeded"),
    ("LogoutFailedLogoutBegun", "login-refusal-logout-begun"),
];

/// The arguments a refusal string takes, by the name the grid sends each
/// under and the Fluent variable it fills here. An argument the grid sends
/// under any other name fills nothing: no string here names it.
const REFUSAL_ARGS: &[(&str, &str)] = &[
    ("TIME", "time"),
    ("VERSION", "version"),
    ("START", "start"),
    ("END", "end"),
];

/// The refusals whose `TIME` is an instant the reference turns into the
/// user's local time (`llstartup.cpp`), rather than text it inserts as sent.
const LOCAL_TIME_REFUSALS: &[&str] = &[
    "LoginFailedAcountSuspended",
    "LoginFailedAccountMaintenance",
];

/// The catalogue key of the refusal string a grid names `message_id`, or
/// `None` for one this viewer does not know.
#[must_use]
pub(crate) fn refusal_key(message_id: &str) -> Option<&'static str> {
    REFUSAL_STRINGS
        .iter()
        .find(|(id, _key)| *id == message_id)
        .map(|(_id, key)| *key)
}

/// When a suspension or a maintenance ends, as the reference words it: the
/// user's own local time when the grid sent an instant, else the grid's text
/// followed by "Pacific Time".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RefusalTime {
    /// The instant, on the user's wall clock.
    Local(CivilDateTime),
    /// Text that is not an instant, as the grid sent it.
    Pacific(String),
}

/// Read a refusal's `TIME` argument: an RFC 3339 instant (what the
/// reference's `LLDate::fromString` reads) becomes the wall-clock time in
/// `zone`, anything else is kept as text.
#[must_use]
pub(crate) fn refusal_time(raw: &str, zone: &jiff::tz::TimeZone) -> RefusalTime {
    let Ok(instant) = raw.trim().parse::<jiff::Timestamp>() else {
        return RefusalTime::Pacific(raw.to_owned());
    };
    civil(&instant.to_zoned_datetime(zone.clone()))
        .map_or_else(|| RefusalTime::Pacific(raw.to_owned()), RefusalTime::Local)
}

/// The wall-clock components of `local`, or `None` for one out of range
/// (jiff's are signed; none is negative for a valid date-time).
fn civil(local: &jiff::ZonedDateTime) -> Option<CivilDateTime> {
    Some(CivilDateTime {
        year: i32::from(local.year()),
        month: u8::try_from(local.month()).ok()?,
        day: u8::try_from(local.day()).ok()?,
        hour: u8::try_from(local.hour()).ok()?,
        minute: u8::try_from(local.minute()).ok()?,
        second: u8::try_from(local.second()).ok()?,
    })
}

/// What a grid said when it refused a login or asked for a second factor:
/// its own words, and the localization key and arguments it named them with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GridText {
    /// The grid's own (English) words.
    pub(crate) message: String,
    /// The localization key, when the grid sent one (Second Life).
    pub(crate) message_id: Option<String>,
    /// The key's arguments, by the grid's names for them.
    pub(crate) message_args: BTreeMap<String, String>,
}

impl GridText {
    /// Text with no localization key: the grid's words are all there is.
    #[must_use]
    pub(crate) const fn plain(message: String) -> Self {
        Self {
            message,
            message_id: None,
            message_args: BTreeMap::new(),
        }
    }

    /// The catalogue string this text is said with and its arguments, or
    /// `None` when there is none — no `message_id`, or one this viewer does
    /// not know — and the grid's own words are shown. A suspension's or a
    /// maintenance's `TIME` is read as an instant in `zone`, the user's own
    /// time zone, as the reference reads it.
    #[must_use]
    pub(crate) fn resolve(&self, zone: &jiff::tz::TimeZone) -> Option<ResolvedText> {
        let message_id = self.message_id.as_deref()?;
        let key = refusal_key(message_id)?;
        let local_time = LOCAL_TIME_REFUSALS.contains(&message_id);
        let args = REFUSAL_ARGS
            .iter()
            .filter_map(|(wire, variable)| {
                let value = self.message_args.get(*wire)?;
                let arg = if local_time && *wire == "TIME" {
                    RefusalArg::Time(refusal_time(value, zone))
                } else {
                    RefusalArg::Text(value.clone())
                };
                Some((*variable, arg))
            })
            .collect();
        Some(ResolvedText { key, args })
    }

    /// The text in the user's language: the catalogue string the key names,
    /// with the arguments substituted ([`resolve`](Self::resolve)), or the
    /// grid's words when there is no such string.
    #[must_use]
    pub(crate) fn localise(&self, translator: &Translator, zone: &jiff::tz::TimeZone) -> String {
        let Some(resolved) = self.resolve(zone) else {
            return self.message.clone();
        };
        let args = resolved
            .args
            .iter()
            .fold(TransArgs::new(), |args, (variable, arg)| {
                let value = match arg {
                    RefusalArg::Text(text) => text.clone(),
                    RefusalArg::Time(RefusalTime::Local(when)) => {
                        translator.datetime(*when, DateTimeStyle::DateTime, DateTimeLength::Medium)
                    }
                    RefusalArg::Time(RefusalTime::Pacific(text)) => translator.format(
                        "login-refusal-pacific-time",
                        &TransArgs::new().text("time", text),
                    ),
                };
                args.text(variable, &value)
            });
        translator.format(resolved.key, &args)
    }
}

/// A refusal string and the arguments it is formatted with: what
/// [`GridText::resolve`] finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedText {
    /// The catalogue key.
    pub(crate) key: &'static str,
    /// The Fluent variables, each with its value.
    pub(crate) args: Vec<(&'static str, RefusalArg)>,
}

/// The value of one refusal argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RefusalArg {
    /// Text inserted as the grid sent it.
    Text(String),
    /// When a suspension or a maintenance ends.
    Time(RefusalTime),
}

impl From<sl_client_bevy::LoginFailure> for GridText {
    fn from(failure: sl_client_bevy::LoginFailure) -> Self {
        Self {
            message: failure.message,
            message_id: failure.message_id,
            message_args: failure.message_args,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};

    use bevy::ecs::system::RunSystemOnce as _;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_l10n::CivilDateTime;

    use super::{
        GridText, REFUSAL_STRINGS, RefusalArg, RefusalTime, ResolvedText, refusal_key, refusal_time,
    };
    use crate::i18n::Translator;
    use crate::i18n_keys::ENGLISH_BUNDLE;

    /// A boxed error so tests use `?` rather than disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// Every refusal string the table names has English words, so a known
    /// refusal is never drawn as its key. (Most span several lines, which the
    /// harnesses' line-wise table leaves out, so this reads the bundle for the
    /// key's definition; `tests/locale_bundles.rs` formats some of them.)
    #[test]
    fn every_refusal_key_has_an_english_string() {
        let missing: Vec<&str> = REFUSAL_STRINGS
            .iter()
            .map(|(_id, key)| *key)
            .chain(["login-refusal-pacific-time"])
            .filter(|key| {
                !ENGLISH_BUNDLE
                    .lines()
                    .any(|line| line.split_once(" =").is_some_and(|(name, _)| name == *key))
            })
            .collect();
        assert!(missing.is_empty(), "no English string for {missing:?}");
    }

    /// The table names each `message_id` and each key once.
    #[test]
    fn the_refusal_table_has_no_duplicates() {
        let ids: HashSet<&str> = REFUSAL_STRINGS.iter().map(|(id, _key)| *id).collect();
        let keys: HashSet<&str> = REFUSAL_STRINGS.iter().map(|(_id, key)| *key).collect();
        assert_eq!(ids.len(), REFUSAL_STRINGS.len());
        assert_eq!(keys.len(), REFUSAL_STRINGS.len());
    }

    /// Second Life's key for a wrong password is known, the reference's
    /// misspelt suspension key is the one looked up, and an unknown key is
    /// not.
    #[test]
    fn a_message_id_names_its_catalogue_key() {
        assert_eq!(
            refusal_key("LoginFailedAuthenticationFailed"),
            Some("login-refusal-authentication-failed")
        );
        assert_eq!(
            refusal_key("LoginFailedAcountSuspended"),
            Some("login-refusal-account-suspended")
        );
        assert_eq!(refusal_key("LoginFailedSomethingNew"), None);
    }

    /// An instant becomes the user's wall-clock time; text that is not one
    /// is kept as sent.
    #[test]
    fn a_suspension_time_is_read_as_an_instant_or_kept_as_text() -> Result<(), TestError> {
        let zone = jiff::tz::TimeZone::get("Europe/Berlin")?;
        assert_eq!(
            refusal_time("2026-11-27T22:30:00Z", &zone),
            RefusalTime::Local(CivilDateTime {
                year: 2026,
                month: 11,
                day: 27,
                hour: 23,
                minute: 30,
                second: 0,
            })
        );
        assert_eq!(
            refusal_time("next Tuesday", &zone),
            RefusalTime::Pacific("next Tuesday".to_owned())
        );
        Ok(())
    }

    /// Say `text` through a [`Translator`] whose every key answers with
    /// itself, so the line is the key it was built from.
    fn localise(text: GridText) -> Result<String, TestError> {
        let mut app = App::new();
        crate::i18n::install_untranslated(&mut app);
        Ok(app
            .world_mut()
            .run_system_once(move |translator: Translator| {
                text.localise(&translator, &jiff::tz::TimeZone::UTC)
            })
            .map_err(|error| error.to_string())?)
    }

    /// A known key is said from the catalogue, not in the grid's words.
    #[test]
    fn a_known_refusal_is_said_from_the_catalogue() -> Result<(), TestError> {
        let text = GridText {
            message: "the grid's own words".to_owned(),
            message_id: Some("LoginFailedAuthenticationFailed".to_owned()),
            message_args: BTreeMap::new(),
        };
        assert_eq!(localise(text)?, "login-refusal-authentication-failed");
        Ok(())
    }

    /// An unknown key, and no key at all (OpenSim), show the grid's words.
    #[test]
    fn an_unknown_or_missing_key_shows_the_grids_words() -> Result<(), TestError> {
        let unknown = GridText {
            message: "a refusal added after this viewer".to_owned(),
            message_id: Some("LoginFailedSomethingNew".to_owned()),
            message_args: BTreeMap::new(),
        };
        assert_eq!(localise(unknown)?, "a refusal added after this viewer");
        let opensim = GridText::plain("Could not authenticate your avatar.".to_owned());
        assert_eq!(localise(opensim)?, "Could not authenticate your avatar.");
        Ok(())
    }

    /// The grid's arguments fill the string's variables by their names here:
    /// a required update's `VERSION` is `$version`, and an argument no string
    /// takes fills nothing.
    #[test]
    fn a_refusal_takes_its_arguments() {
        let text = GridText {
            message: String::new(),
            message_id: Some("LoginFailedRequiredUpdate".to_owned()),
            message_args: BTreeMap::from([
                ("VERSION".to_owned(), "7.2.1".to_owned()),
                ("UNHEARD_OF".to_owned(), "x".to_owned()),
            ]),
        };
        assert_eq!(
            text.resolve(&jiff::tz::TimeZone::UTC),
            Some(ResolvedText {
                key: "login-refusal-required-update",
                args: vec![("version", RefusalArg::Text("7.2.1".to_owned()))],
            })
        );
    }

    /// A suspension's end is read as an instant on the user's clock, or kept
    /// as text to be said in Pacific Time; the same argument on any other
    /// refusal is inserted as sent, as the reference inserts it.
    #[test]
    fn a_suspension_end_is_put_on_the_users_clock() -> Result<(), TestError> {
        let zone = jiff::tz::TimeZone::get("Europe/Berlin")?;
        let suspended = |time: &str| GridText {
            message: String::new(),
            message_id: Some("LoginFailedAcountSuspended".to_owned()),
            message_args: BTreeMap::from([("TIME".to_owned(), time.to_owned())]),
        };
        assert_eq!(
            suspended("2026-11-27T22:30:00Z").resolve(&zone),
            Some(ResolvedText {
                key: "login-refusal-account-suspended",
                args: vec![(
                    "time",
                    RefusalArg::Time(RefusalTime::Local(CivilDateTime {
                        year: 2026,
                        month: 11,
                        day: 27,
                        hour: 23,
                        minute: 30,
                        second: 0,
                    }))
                )],
            })
        );
        assert_eq!(
            suspended("Monday noon").resolve(&zone),
            Some(ResolvedText {
                key: "login-refusal-account-suspended",
                args: vec![(
                    "time",
                    RefusalArg::Time(RefusalTime::Pacific("Monday noon".to_owned()))
                )],
            })
        );
        let other = GridText {
            message_id: Some("LoginFailedRequiredUpdate".to_owned()),
            ..suspended("2026-11-27T22:30:00Z")
        };
        assert_eq!(
            other.resolve(&zone),
            Some(ResolvedText {
                key: "login-refusal-required-update",
                args: vec![("time", RefusalArg::Text("2026-11-27T22:30:00Z".to_owned()))],
            })
        );
        Ok(())
    }
}
