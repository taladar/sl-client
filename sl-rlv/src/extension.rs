//! The extension commands — the ones that are not in the behaviour dictionary
//! at all.
//!
//! `@getdebug_<setting>`, `@setdebug_<setting>` and `@setrot` never appear in
//! the reference's `m_String2InfoMap`. They reach the viewer as unknown
//! keywords and are picked up afterwards by a chain of registered handlers
//! (`RlvExtCommandHandler`), of which `RlvExtGetSet` — this module — is one
//! (`rlvextensions.cpp`). That is why [`RlvCommand::behaviour`] is
//! [`RlvBehaviour::Unknown`] for all three and why nothing here is reachable
//! through [`RlvState::apply`]: an unknown `=force` or `=<channel>` command
//! comes back as [`RlvOutcome::NotAStateChange`], and *this* is what the
//! consumer tries next before reporting the command unknown.
//!
//! Two families, one dispatch:
//!
//! - **the debug-setting window.** A short **allowlist** of settings a script
//!   may read, and a shorter one it may write ([`RLV_DEBUG_SETTINGS`]). It is
//!   an allowlist rather than the whole settings store for the obvious reason:
//!   a worn object that could write any debug setting could do anything the
//!   viewer can do. Two of the six are *pseudo* settings — computed facts with
//!   no stored value behind them at all;
//! - **`@setrot`**, which turns the avatar to face a heading.
//!
//! The split between this crate and its consumer is the same one the query
//! layer draws: the allowlist, the parsing, the formatting and every rule about
//! *what* may be read or written live here and are unit-tested to the letter;
//! the values themselves come from an [`RlvExtSource`] the consumer implements
//! over whatever its settings store actually is.
//!
//! ## The reference's own quirks, kept
//!
//! - **the pseudo `AvatarSex` write is a lie a script can tell itself.**
//!   `@setdebug_avatarsex:1=force` stores the *raw text* in a viewer-side slot
//!   that only `@getdebug_avatarsex` ever reads; it does not touch the avatar
//!   (`RlvExtGetSet::onSetPseudoDebug`). So the value that comes back is the
//!   spelling that went in — `t` reads back as `t`, not as `1`;
//! - **`@setrot` is not restricted to `=force`.** `RlvExtGetSet::processCommand`
//!   is registered for both the force and the reply dispatch and does not check
//!   which one it is called from, so `@setrot:1.5=2222` really does turn the
//!   avatar — and answers nothing, because the handler returns before any reply
//!   is built;
//! - **a `@getdebug_*` of a setting this viewer does not have still succeeds**,
//!   with an empty answer. The reference reports `RLV_RET_SUCCESS` whatever
//!   `onGetDebug` found, because a script that asked a question must not be
//!   left waiting.
//!
//! ## One deliberate divergence
//!
//! A number is read with C++'s `operator>>`, which stops at the first character
//! it cannot use: `2x` is `2`. [`parse_prefix`] reproduces that. What it does
//! *not* reproduce is `operator>>` accepting a **negative** number for an
//! unsigned setting and wrapping it — `-1` would be four billion. We refuse it,
//! because a script writing `-1` into an unsigned setting has made a mistake
//! and the wrap is a C++ accident, not an RLV rule.

use core::str::FromStr;

use uuid::Uuid;

use crate::behaviour::RlvBehaviour;
use crate::command::{RlvCommand, RlvParam, RlvParamKind};
use crate::query::{RlvReply, is_valid_reply_channel, truncate_chat};
use crate::state::{RlvOutcome, RlvState};

/// `@setrot` is off by 90° from the rest of Second Life, so the angle a script
/// names is subtracted from a quarter turn rather than used as-is
/// (`RLV_SETROT_OFFSET`, `rlvdefines.h:91`).
pub const SETROT_OFFSET: f32 = core::f32::consts::FRAC_PI_2;

/// One of the settings a script is allowed to reach through
/// `@getdebug_*` / `@setdebug_*`.
///
/// The roster is closed on purpose — see [`RLV_DEBUG_SETTINGS`] for what each
/// one is and why it is in the list. It is also, unusually for this crate, an
/// **exhaustive** enum: a consumer has to say what every row reads as, and a
/// row added later should break it into saying so rather than silently
/// answering nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RlvDebugSetting {
    /// `AvatarSex` — whether the avatar's shape is male. Pseudo: read from the
    /// avatar, but a write is remembered without changing anything.
    AvatarSex,
    /// `AspectRatio` — the 3D view's width divided by its height. Pseudo and
    /// read-only: it is a measurement, not a stored value.
    AspectRatio,
    /// `RenderResolutionDivisor` — render at `1/n` resolution. The classic
    /// cheap vision impairment: a collar sets it to blur what the wearer sees.
    RenderResolutionDivisor,
    /// `RestrainedLoveForbidGiveToRLV` — whether the user refuses folders
    /// offered into the shared `#RLV` folder. Read-only: it is the user's
    /// answer, so a script may look but not touch.
    ForbidGiveToRlv,
    /// `RestrainedLoveNoSetEnv` — whether the user has opted out of script
    /// control of the environment. Read-only for the same reason.
    NoSetEnv,
    /// `WindLightUseAtmosShaders` — whether the atmospheric sky shaders are in
    /// use, which is what tells a script whether `@setenv_*` can do anything.
    WindLightUseAtmosShaders,
}

/// What kind of value a debug setting holds, which is the whole of how it is
/// parsed and how it is written back to the script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvDebugKind {
    /// A boolean, answered as `0` or `1` (`llformat("%d", ...)`).
    Bool,
    /// An unsigned integer, answered in full (`llformat("%u", ...)`).
    U32,
    /// A float, answered to three decimals — which only [`AspectRatio`] is, and
    /// only because that is how the reference formats it
    /// (`RlvExtGetSet::onGetPseudoDebug`).
    ///
    /// [`AspectRatio`]: RlvDebugSetting::AspectRatio
    Float,
}

/// The value of one debug setting, in the type its row declares.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum RlvDebugValue {
    /// A [`RlvDebugKind::Bool`] setting's value.
    Bool(bool),
    /// A [`RlvDebugKind::U32`] setting's value.
    U32(u32),
    /// A [`RlvDebugKind::Float`] setting's value.
    Float(f32),
}

impl RlvDebugValue {
    /// The text a script is answered with — the reference's `llformat` for this
    /// value's type.
    #[must_use]
    pub fn to_text(self) -> String {
        match self {
            Self::Bool(value) => u8::from(value).to_string(),
            Self::U32(value) => value.to_string(),
            Self::Float(value) => format!("{value:.3}"),
        }
    }

    /// Which [`RlvDebugKind`] this value belongs to.
    #[must_use]
    pub const fn kind(self) -> RlvDebugKind {
        match self {
            Self::Bool(_) => RlvDebugKind::Bool,
            Self::U32(_) => RlvDebugKind::U32,
            Self::Float(_) => RlvDebugKind::Float,
        }
    }
}

/// One row of the debug-setting allowlist.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct RlvDebugSettingDef {
    /// Which setting this row is.
    pub setting: RlvDebugSetting,
    /// Its name in the reference's spelling, which is the name a script writes
    /// after `@getdebug_` / `@setdebug_`. Matched case-insensitively.
    pub name: &'static str,
    /// The type of its value.
    pub kind: RlvDebugKind,
    /// Whether `@getdebug_<name>` may read it (`DBG_READ`).
    pub readable: bool,
    /// Whether `@setdebug_<name>=force` may write it (`DBG_WRITE`).
    pub writable: bool,
    /// Whether it is a **pseudo** setting (`DBG_PSEUDO`): a computed fact with
    /// no stored value, so a write — where one is allowed at all — is
    /// remembered by the state machine and read back from there.
    pub pseudo: bool,
}

/// The allowlist, in the reference's own order
/// (`RlvExtGetSet::RlvExtGetSet`, `rlvextensions.cpp:37`).
///
/// The reference additionally caches a `DBG_PERSIST` flag per row, which it
/// uses to stop a script-written value from being saved to disk. There is no
/// row here that a script can write *and* that this viewer stores, so there is
/// nothing that flag could protect; when one appears, the rule it encodes —
/// **a value a script wrote never persists** — belongs at the write site in the
/// consumer, which is the only place that knows what persistence means.
pub const RLV_DEBUG_SETTINGS: &[RlvDebugSettingDef] = &[
    AVATAR_SEX,
    ASPECT_RATIO,
    RENDER_RESOLUTION_DIVISOR,
    FORBID_GIVE_TO_RLV,
    NO_SET_ENV,
    WINDLIGHT_USE_ATMOS_SHADERS,
];

/// The [`RlvDebugSetting::AvatarSex`] row.
const AVATAR_SEX: RlvDebugSettingDef = RlvDebugSettingDef {
    setting: RlvDebugSetting::AvatarSex,
    name: "AvatarSex",
    kind: RlvDebugKind::Bool,
    readable: true,
    writable: true,
    pseudo: true,
};

/// The [`RlvDebugSetting::AspectRatio`] row.
const ASPECT_RATIO: RlvDebugSettingDef = RlvDebugSettingDef {
    setting: RlvDebugSetting::AspectRatio,
    name: "AspectRatio",
    kind: RlvDebugKind::Float,
    readable: true,
    writable: false,
    pseudo: true,
};

/// The [`RlvDebugSetting::RenderResolutionDivisor`] row.
const RENDER_RESOLUTION_DIVISOR: RlvDebugSettingDef = RlvDebugSettingDef {
    setting: RlvDebugSetting::RenderResolutionDivisor,
    name: "RenderResolutionDivisor",
    kind: RlvDebugKind::U32,
    readable: true,
    writable: true,
    pseudo: false,
};

/// The [`RlvDebugSetting::ForbidGiveToRlv`] row.
const FORBID_GIVE_TO_RLV: RlvDebugSettingDef = RlvDebugSettingDef {
    setting: RlvDebugSetting::ForbidGiveToRlv,
    name: "RestrainedLoveForbidGiveToRLV",
    kind: RlvDebugKind::Bool,
    readable: true,
    writable: false,
    pseudo: false,
};

/// The [`RlvDebugSetting::NoSetEnv`] row.
const NO_SET_ENV: RlvDebugSettingDef = RlvDebugSettingDef {
    setting: RlvDebugSetting::NoSetEnv,
    name: "RestrainedLoveNoSetEnv",
    kind: RlvDebugKind::Bool,
    readable: true,
    writable: false,
    pseudo: false,
};

/// The [`RlvDebugSetting::WindLightUseAtmosShaders`] row.
const WINDLIGHT_USE_ATMOS_SHADERS: RlvDebugSettingDef = RlvDebugSettingDef {
    setting: RlvDebugSetting::WindLightUseAtmosShaders,
    name: "WindLightUseAtmosShaders",
    kind: RlvDebugKind::Bool,
    readable: true,
    writable: false,
    pseudo: false,
};

impl RlvDebugSetting {
    /// This setting's allowlist row.
    ///
    /// A `match` rather than a search of [`RLV_DEBUG_SETTINGS`] so the lookup
    /// is total without a fallback row to be wrong about; that the two agree is
    /// pinned by `every_setting_has_a_row`.
    #[must_use]
    pub const fn def(self) -> &'static RlvDebugSettingDef {
        match self {
            Self::AvatarSex => &AVATAR_SEX,
            Self::AspectRatio => &ASPECT_RATIO,
            Self::RenderResolutionDivisor => &RENDER_RESOLUTION_DIVISOR,
            Self::ForbidGiveToRlv => &FORBID_GIVE_TO_RLV,
            Self::NoSetEnv => &NO_SET_ENV,
            Self::WindLightUseAtmosShaders => &WINDLIGHT_USE_ATMOS_SHADERS,
        }
    }

    /// The setting a script named, matched case-insensitively as the reference
    /// matches it (`RlvExtGetSet::findDebugSetting`).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        RLV_DEBUG_SETTINGS
            .iter()
            .find(|row| row.name.eq_ignore_ascii_case(name))
            .map(|row| row.setting)
    }

    /// This setting's name in the reference's spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.def().name
    }
}

/// The names of every setting a script may **write**, which is the set the
/// `@setdebug=n` gate hides from the user's own settings editor
/// (`RlvBehaviourToggleHandler<RLV_BHVR_SETDEBUG>::onCommandToggle`,
/// `rlvhandler.cpp:2487`).
///
/// While one object holds `@setdebug` it owns these settings: the user may not
/// edit underneath it, and no other object may write them either.
pub fn writable_debug_setting_names() -> impl Iterator<Item = &'static str> {
    RLV_DEBUG_SETTINGS
        .iter()
        .filter(|row| row.writable)
        .map(|row| row.name)
}

/// Whether `name` is a setting the user may not edit right now because an
/// object holds `@setdebug`.
///
/// The comparison is case-insensitive because the name comes from a settings
/// store whose spelling is its own business.
#[must_use]
pub fn is_debug_setting_locked(state: &RlvState, name: &str) -> bool {
    state.has_behaviour(RlvBehaviour::Setdebug)
        && writable_debug_setting_names().any(|locked| locked.eq_ignore_ascii_case(name))
}

/// One of the three commands this module owns, decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RlvExtCommand {
    /// `@getdebug_<setting>=<channel>` — read an allowlisted setting.
    GetDebug {
        /// The setting name as the script spelled it, which is not necessarily
        /// one the allowlist knows.
        setting: String,
        /// The channel the answer is shouted on.
        channel: i32,
    },
    /// `@setdebug_<setting>:<value>=force` — write an allowlisted setting.
    SetDebug {
        /// The setting name as the script spelled it.
        setting: String,
        /// The value text, still unparsed — what it has to parse *as* depends
        /// on the row it lands on.
        value: String,
    },
    /// `@setrot:<radians>=force` — turn the avatar to face a heading.
    SetRot {
        /// The angle text, still unparsed.
        angle: String,
    },
}

impl RlvExtCommand {
    /// Decode `command` as an extension command, or `None` when it is not one.
    ///
    /// Only a command the dictionary did not claim can be one:
    /// [`RlvBehaviour::Unknown`] is the reference's whole dispatch condition
    /// (`RLV_BHVR_UNKNOWN` in the force and reply switches). The keyword is
    /// split on its **first** underscore, and the half before it must be
    /// exactly `getdebug` or `setdebug` — `@getcam_fov` splits the same way and
    /// lands on `getcam`, which is not this family.
    ///
    /// The param kind is part of the match, not a later check: `@getdebug_x`
    /// only means anything as a query and `@setdebug_x` only as an action, so
    /// `@getdebug_x=force` is not a malformed read — it is not a command at
    /// all, which is exactly how the reference reports it.
    ///
    /// ```
    /// # use sl_rlv::{RlvCommand, RlvExtCommand};
    /// let cmd = RlvCommand::parse_field("getdebug_avatarsex=2222")?;
    /// assert_eq!(
    ///     RlvExtCommand::classify(&cmd),
    ///     Some(RlvExtCommand::GetDebug { setting: "avatarsex".to_owned(), channel: 2222 })
    /// );
    /// # Ok::<(), sl_rlv::RlvParseError>(())
    /// ```
    #[must_use]
    pub fn classify(command: &RlvCommand) -> Option<Self> {
        if command.behaviour != RlvBehaviour::Unknown {
            return None;
        }
        let option = command.option.as_deref().unwrap_or("");
        let kind = command.param.kind();

        if command.keyword == "setrot" {
            // Registered for both dispatches and checking neither — see the
            // module documentation.
            return matches!(kind, RlvParamKind::Force | RlvParamKind::Reply).then(|| {
                Self::SetRot {
                    angle: option.to_owned(),
                }
            });
        }

        let (head, setting) = command.keyword.split_once('_')?;
        if setting.is_empty() {
            return None;
        }
        match (head, &command.param) {
            ("getdebug", &RlvParam::Reply { channel }) => Some(Self::GetDebug {
                setting: setting.to_owned(),
                channel,
            }),
            ("setdebug", &RlvParam::Force) => Some(Self::SetDebug {
                setting: setting.to_owned(),
                value: option.to_owned(),
            }),
            _ => None,
        }
    }
}

/// What the consumer supplies: the values behind the allowlist.
///
/// Everything else — which settings exist, who may read or write them, how a
/// value is parsed and how it is formatted — is this crate's. An
/// implementation only has to say what a setting is *now* and store what a
/// script writes.
pub trait RlvExtSource {
    /// The current value of `setting`, or `None` when this viewer has no such
    /// setting at all.
    ///
    /// A `None` is answered with an empty string, which is what the reference
    /// does when `gSavedSettings.getControl` finds nothing. The value's variant
    /// should match the row's [`RlvDebugSettingDef::kind`]; one that does not
    /// is still formatted faithfully, so a mismatch shows up in the answer
    /// rather than being silently coerced.
    fn debug_value(&self, setting: RlvDebugSetting) -> Option<RlvDebugValue>;

    /// Store a script-written value, answering whether it could be stored.
    ///
    /// Only called for a row that is [`writable`](RlvDebugSettingDef::writable)
    /// and not [`pseudo`](RlvDebugSettingDef::pseudo) — a pseudo write never
    /// leaves the state machine. `false` means this viewer has no such setting,
    /// which the script hears as a bad option.
    fn set_debug_value(&mut self, setting: RlvDebugSetting, value: RlvDebugValue) -> bool;
}

/// What running an extension command produced.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RlvExtResult {
    /// How it went, in the same vocabulary every other command reports in.
    pub outcome: RlvOutcome,
    /// The line to shout back, for a `@getdebug_*`. `None` for an action, and
    /// also for a read whose channel is not one a reply may go on.
    pub reply: Option<RlvReply>,
    /// The heading (radians, about the Second Life up axis) `@setrot` asks the
    /// avatar to face. `None` for everything else.
    pub rotate_to: Option<f32>,
}

impl RlvExtResult {
    /// A result that only reports an outcome.
    const fn outcome(outcome: RlvOutcome) -> Self {
        Self {
            outcome,
            reply: None,
            rotate_to: None,
        }
    }
}

/// Run one extension command — the body of [`RlvState::run_extension`], which
/// is where it is documented.
pub(crate) fn run(
    state: &mut RlvState,
    issuer: Uuid,
    command: &RlvCommand,
    source: &mut impl RlvExtSource,
) -> Option<RlvExtResult> {
    let decoded = RlvExtCommand::classify(command)?;
    Some(match decoded {
        RlvExtCommand::GetDebug {
            ref setting,
            channel,
        } => {
            let message = get_debug(state, setting, source);
            RlvExtResult {
                // A read always succeeds, however little it found: the script
                // must not be left waiting on an answer that never comes.
                outcome: RlvOutcome::Success,
                // The reply goes out through the ordinary chat path, which
                // refuses a channel a reply may not use — and unlike a
                // dictionary query this one is never a loopback, because the
                // reference reaches it through the plain `sendChatReply`.
                reply: is_valid_reply_channel(channel, false).then(|| RlvReply {
                    channel,
                    message: truncate_chat(&message).to_owned(),
                }),
                rotate_to: None,
            }
        }
        RlvExtCommand::SetDebug {
            ref setting,
            ref value,
        } => RlvExtResult::outcome(set_debug(state, issuer, setting, value, source)),
        RlvExtCommand::SetRot { ref angle } => match parse_prefix::<f32>(angle) {
            Some(radians) => RlvExtResult {
                outcome: RlvOutcome::Success,
                reply: None,
                rotate_to: Some(SETROT_OFFSET - radians),
            },
            None => RlvExtResult::outcome(RlvOutcome::FailedOption),
        },
    })
}

/// The text `@getdebug_<setting>` answers with (`RlvExtGetSet::onGetDebug`).
fn get_debug(state: &RlvState, setting: &str, source: &impl RlvExtSource) -> String {
    let Some(setting) = RlvDebugSetting::from_name(setting) else {
        return String::new();
    };
    let def = setting.def();
    if !def.readable {
        return String::new();
    }
    // A pseudo setting a script has written reads back as the text that was
    // written, not as the fact underneath it.
    if def.pseudo
        && let Some(stored) = state.pseudo_debug(setting)
    {
        return stored.to_owned();
    }
    source
        .debug_value(setting)
        .map(RlvDebugValue::to_text)
        .unwrap_or_default()
}

/// Apply `@setdebug_<setting>:<value>=force` (`RlvExtGetSet::onSetDebug`, plus
/// the `@setdebug` ownership gate `RlvExtGetSet::processCommand` checks first).
fn set_debug(
    state: &mut RlvState,
    issuer: Uuid,
    setting: &str,
    value: &str,
    source: &mut impl RlvExtSource,
) -> RlvOutcome {
    // `@setdebug=n` gives one object the debug settings. Another object writing
    // them is refused — the holder itself is not.
    if state.has_behaviour_except(RlvBehaviour::Setdebug, "", issuer) {
        return RlvOutcome::FailedLock;
    }
    // Not in the allowlist, or in it but read-only: either way the command
    // names nothing that may be written, which the reference reports as
    // `RLV_RET_FAILED_UNKNOWN` — the code this crate spells `FailedParam`, the
    // one it already gives every keyword it does not know.
    let Some(setting) = RlvDebugSetting::from_name(setting) else {
        return RlvOutcome::FailedParam;
    };
    let def = setting.def();
    if !def.writable {
        return RlvOutcome::FailedParam;
    }
    let Some(parsed) = parse_debug_value(def.kind, value) else {
        return RlvOutcome::FailedOption;
    };
    if def.pseudo {
        // Remembered verbatim, and read back verbatim — the reference stores
        // the text a script sent, not the value it parsed to.
        state.set_pseudo_debug(setting, value);
        return RlvOutcome::Success;
    }
    if source.set_debug_value(setting, parsed) {
        RlvOutcome::Success
    } else {
        RlvOutcome::FailedOption
    }
}

/// Parse a script-written value as the kind its row declares.
fn parse_debug_value(kind: RlvDebugKind, value: &str) -> Option<RlvDebugValue> {
    match kind {
        RlvDebugKind::Bool => parse_bool(value).map(RlvDebugValue::Bool),
        RlvDebugKind::U32 => parse_prefix::<u32>(value).map(RlvDebugValue::U32),
        RlvDebugKind::Float => parse_prefix::<f32>(value).map(RlvDebugValue::Float),
    }
}

/// The words the reference accepts for a boolean
/// (`LLStringUtil::convertToBOOL`, `llstring.h:1883`).
///
/// Its `TRUE` / `True` spellings are here for completeness only: an RLV command
/// line is lower-cased before it is decoded, so a script cannot send them.
#[must_use]
pub fn parse_bool(text: &str) -> Option<bool> {
    match text.trim() {
        "1" | "t" | "T" | "true" | "TRUE" | "True" => Some(true),
        "0" | "f" | "F" | "false" | "FALSE" | "False" => Some(false),
        _ => None,
    }
}

/// Parse the longest prefix of `text` that is a valid `T`, which is what C++'s
/// `operator>>` does — `2x` is `2`, `1.5rad` is `1.5`.
///
/// Leading and trailing whitespace is trimmed first, as the reference trims it.
#[must_use]
pub fn parse_prefix<T: FromStr>(text: &str) -> Option<T> {
    let text = text.trim();
    let mut best = None;
    for end in text
        .char_indices()
        .map(|(index, _)| index)
        .skip(1)
        .chain(core::iter::once(text.len()))
    {
        if let Some(prefix) = text.get(..end)
            && let Ok(value) = prefix.parse::<T>()
        {
            best = Some(value);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::{
        RLV_DEBUG_SETTINGS, RlvDebugKind, RlvDebugSetting, RlvDebugValue, RlvExtCommand,
        RlvExtSource, SETROT_OFFSET, is_debug_setting_locked, parse_bool, parse_prefix,
        writable_debug_setting_names,
    };
    use crate::command::RlvCommand;
    use crate::state::{RlvOutcome, RlvState};
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    /// A boxed error so tests can use `?` instead of the disallowed `unwrap`.
    type TestError = Box<dyn core::error::Error>;

    /// A source that has the two RLV settings and the shader flag, no
    /// resolution divisor (as this viewer has none), and whatever avatar facts
    /// a test hands it.
    #[derive(Default)]
    struct TestSource {
        stored: BTreeMap<RlvDebugSetting, RlvDebugValue>,
        missing: bool,
    }

    impl RlvExtSource for TestSource {
        fn debug_value(&self, setting: RlvDebugSetting) -> Option<RlvDebugValue> {
            if self.missing {
                return None;
            }
            self.stored.get(&setting).copied().or(match setting {
                RlvDebugSetting::AvatarSex => Some(RlvDebugValue::Bool(false)),
                RlvDebugSetting::AspectRatio => Some(RlvDebugValue::Float(1.777_777_8)),
                RlvDebugSetting::RenderResolutionDivisor => Some(RlvDebugValue::U32(1)),
                RlvDebugSetting::ForbidGiveToRlv | RlvDebugSetting::NoSetEnv => {
                    Some(RlvDebugValue::Bool(false))
                }
                RlvDebugSetting::WindLightUseAtmosShaders => Some(RlvDebugValue::Bool(true)),
            })
        }

        fn set_debug_value(&mut self, setting: RlvDebugSetting, value: RlvDebugValue) -> bool {
            if self.missing {
                return false;
            }
            let _previous = self.stored.insert(setting, value);
            true
        }
    }

    /// The object every test issues its commands from.
    fn collar() -> Uuid {
        Uuid::from_u128(0x0C01_1A12)
    }

    /// Run one command line's single command through the extension dispatch.
    fn run(
        state: &mut RlvState,
        issuer: Uuid,
        field: &str,
        source: &mut TestSource,
    ) -> Result<Option<super::RlvExtResult>, TestError> {
        let command = RlvCommand::parse_field(field)?;
        Ok(state.run_extension(issuer, &command, source))
    }

    /// Every enum variant has a roster row, so `def()` is a real lookup rather
    /// than a fallback.
    #[test]
    fn every_setting_has_a_row() {
        for setting in [
            RlvDebugSetting::AvatarSex,
            RlvDebugSetting::AspectRatio,
            RlvDebugSetting::RenderResolutionDivisor,
            RlvDebugSetting::ForbidGiveToRlv,
            RlvDebugSetting::NoSetEnv,
            RlvDebugSetting::WindLightUseAtmosShaders,
        ] {
            assert_eq!(setting.def().setting, setting);
            assert!(
                RLV_DEBUG_SETTINGS
                    .iter()
                    .any(|row| row.setting == setting && row.name == setting.name()),
                "{setting:?} is missing from the roster"
            );
        }
        assert_eq!(RLV_DEBUG_SETTINGS.len(), 6);
    }

    /// The roster's names are the reference's spellings and are matched however
    /// a script spells them.
    #[test]
    fn a_setting_is_found_whatever_its_case() {
        assert_eq!(
            RlvDebugSetting::from_name("restrainedloveforbidgivetorlv"),
            Some(RlvDebugSetting::ForbidGiveToRlv)
        );
        assert_eq!(
            RlvDebugSetting::ForbidGiveToRlv.name(),
            "RestrainedLoveForbidGiveToRLV"
        );
        assert_eq!(RlvDebugSetting::from_name("rendervolumelodfactor"), None);
    }

    /// The two dispatch conditions: the keyword's head, and the param kind. A
    /// read asked as an action is not a command at all.
    #[test]
    fn the_family_is_recognised_by_head_and_param_kind() -> Result<(), TestError> {
        let classify = |field: &str| -> Result<Option<RlvExtCommand>, TestError> {
            Ok(RlvExtCommand::classify(&RlvCommand::parse_field(field)?))
        };
        assert_eq!(
            classify("getdebug_avatarsex=2222")?,
            Some(RlvExtCommand::GetDebug {
                setting: "avatarsex".to_owned(),
                channel: 2222,
            })
        );
        assert_eq!(
            classify("setdebug_avatarsex:1=force")?,
            Some(RlvExtCommand::SetDebug {
                setting: "avatarsex".to_owned(),
                value: "1".to_owned(),
            })
        );
        assert_eq!(classify("getdebug_avatarsex=force")?, None);
        assert_eq!(classify("setdebug_avatarsex:1=2222")?, None);
        assert_eq!(classify("getdebug_=2222")?, None);
        assert_eq!(classify("getdebug=2222")?, None);
        // A dictionary command that happens to split the same way is not ours.
        assert_eq!(classify("getcam_fov=2222")?, None);
        Ok(())
    }

    /// The name is everything after the *first* underscore, so a setting whose
    /// own name has one still resolves.
    #[test]
    fn the_setting_name_is_everything_after_the_first_underscore() -> Result<(), TestError> {
        assert_eq!(
            RlvExtCommand::classify(&RlvCommand::parse_field("getdebug_render_thing=2222")?),
            Some(RlvExtCommand::GetDebug {
                setting: "render_thing".to_owned(),
                channel: 2222,
            })
        );
        Ok(())
    }

    /// A read is answered on its channel, formatted the way its row declares.
    #[test]
    fn a_read_answers_in_the_row_s_own_format() -> Result<(), TestError> {
        let mut state = RlvState::new();
        let mut source = TestSource::default();
        let result = run(
            &mut state,
            collar(),
            "getdebug_windlightuseatmosshaders=2222",
            &mut source,
        )?
        .ok_or("not an extension command")?;
        assert_eq!(result.outcome, RlvOutcome::Success);
        let reply = result.reply.ok_or("no reply")?;
        assert_eq!(reply.channel, 2222);
        assert_eq!(reply.message, "1");

        // A float is three decimals, an unsigned its plain digits.
        let ratio = run(
            &mut state,
            collar(),
            "getdebug_aspectratio=2222",
            &mut source,
        )?
        .ok_or("not an extension command")?;
        assert_eq!(
            ratio.reply.ok_or("no reply")?.message,
            "1.778",
            "the reference formats the aspect ratio to three decimals"
        );
        Ok(())
    }

    /// A setting the allowlist does not know, and one this viewer does not
    /// have, are both answered — with nothing. The script is never left
    /// waiting.
    #[test]
    fn an_unknown_read_still_succeeds_with_an_empty_answer() -> Result<(), TestError> {
        let mut state = RlvState::new();
        let mut source = TestSource::default();
        let result = run(
            &mut state,
            collar(),
            "getdebug_rendervolumelodfactor=2222",
            &mut source,
        )?
        .ok_or("not an extension command")?;
        assert_eq!(result.outcome, RlvOutcome::Success);
        assert_eq!(result.reply.ok_or("no reply")?.message, "");

        source.missing = true;
        let absent = run(&mut state, collar(), "getdebug_avatarsex=2222", &mut source)?
            .ok_or("not an extension command")?;
        assert_eq!(absent.outcome, RlvOutcome::Success);
        assert_eq!(absent.reply.ok_or("no reply")?.message, "");
        Ok(())
    }

    /// A channel a reply may not go on leaves no reply — but the command still
    /// counts as done, because there was nothing wrong with it.
    #[test]
    fn a_read_on_an_unusable_channel_is_silent() -> Result<(), TestError> {
        let mut state = RlvState::new();
        let mut source = TestSource::default();
        let result = run(&mut state, collar(), "getdebug_avatarsex=0", &mut source)?
            .ok_or("not an extension command")?;
        assert_eq!(result.outcome, RlvOutcome::Success);
        assert_eq!(result.reply, None);
        Ok(())
    }

    /// A read-only row refuses a write, and an unknown one refuses it the same
    /// way an unknown keyword is refused.
    #[test]
    fn only_a_writable_row_may_be_written() -> Result<(), TestError> {
        let mut state = RlvState::new();
        let mut source = TestSource::default();
        for field in [
            "setdebug_restrainedloveforbidgivetorlv:1=force",
            "setdebug_aspectratio:1.5=force",
            "setdebug_rendervolumelodfactor:1=force",
        ] {
            let result =
                run(&mut state, collar(), field, &mut source)?.ok_or("not an extension command")?;
            assert_eq!(result.outcome, RlvOutcome::FailedParam, "for {field}");
        }
        Ok(())
    }

    /// A writable row reaches the source, parsed into its declared type.
    #[test]
    fn a_write_reaches_the_source_typed() -> Result<(), TestError> {
        let mut state = RlvState::new();
        let mut source = TestSource::default();
        let result = run(
            &mut state,
            collar(),
            "setdebug_renderresolutiondivisor:4=force",
            &mut source,
        )?
        .ok_or("not an extension command")?;
        assert_eq!(result.outcome, RlvOutcome::Success);
        assert_eq!(
            source.debug_value(RlvDebugSetting::RenderResolutionDivisor),
            Some(RlvDebugValue::U32(4))
        );

        // A value that will not parse is a bad option, not a bad command.
        let bad = run(
            &mut state,
            collar(),
            "setdebug_renderresolutiondivisor:none=force",
            &mut source,
        )?
        .ok_or("not an extension command")?;
        assert_eq!(bad.outcome, RlvOutcome::FailedOption);

        // A viewer without the setting hears the write as a bad option too.
        source.missing = true;
        let absent = run(
            &mut state,
            collar(),
            "setdebug_renderresolutiondivisor:4=force",
            &mut source,
        )?
        .ok_or("not an extension command")?;
        assert_eq!(absent.outcome, RlvOutcome::FailedOption);
        Ok(())
    }

    /// The pseudo write never reaches the source, and reads back as the text
    /// that was written rather than as the fact underneath it.
    #[test]
    fn a_pseudo_write_is_remembered_verbatim() -> Result<(), TestError> {
        let mut state = RlvState::new();
        let mut source = TestSource::default();
        let before = run(&mut state, collar(), "getdebug_avatarsex=2222", &mut source)?
            .ok_or("not an extension command")?;
        assert_eq!(before.reply.ok_or("no reply")?.message, "0");

        let write = run(
            &mut state,
            collar(),
            "setdebug_avatarsex:t=force",
            &mut source,
        )?
        .ok_or("not an extension command")?;
        assert_eq!(write.outcome, RlvOutcome::Success);
        assert_eq!(
            source.stored.get(&RlvDebugSetting::AvatarSex),
            None,
            "a pseudo write must not reach the settings store"
        );

        let after = run(&mut state, collar(), "getdebug_avatarsex=2222", &mut source)?
            .ok_or("not an extension command")?;
        assert_eq!(after.reply.ok_or("no reply")?.message, "t");
        Ok(())
    }

    /// `@setdebug=n` gives one object the writable settings: another object is
    /// refused, the holder is not.
    #[test]
    fn the_setdebug_holder_owns_the_writable_settings() -> Result<(), TestError> {
        let mut state = RlvState::new();
        let mut source = TestSource::default();
        let cuffs = Uuid::from_u128(2);
        let _held = state.apply(collar(), &RlvCommand::parse_field("setdebug=n")?);

        let refused = run(
            &mut state,
            cuffs,
            "setdebug_renderresolutiondivisor:4=force",
            &mut source,
        )?
        .ok_or("not an extension command")?;
        assert_eq!(refused.outcome, RlvOutcome::FailedLock);

        let allowed = run(
            &mut state,
            collar(),
            "setdebug_renderresolutiondivisor:4=force",
            &mut source,
        )?
        .ok_or("not an extension command")?;
        assert_eq!(allowed.outcome, RlvOutcome::Success);

        // The same gate is what hides those settings from the user's editor.
        assert!(is_debug_setting_locked(&state, "RenderResolutionDivisor"));
        assert!(is_debug_setting_locked(&state, "renderresolutiondivisor"));
        assert!(!is_debug_setting_locked(
            &state,
            "RestrainedLoveForbidGiveToRLV"
        ));
        Ok(())
    }

    /// Nothing is locked while no object holds `@setdebug`, and only the
    /// writable rows are ever locked.
    #[test]
    fn nothing_is_locked_without_the_restriction() {
        let state = RlvState::new();
        assert!(!is_debug_setting_locked(&state, "RenderResolutionDivisor"));
        let names: Vec<&str> = writable_debug_setting_names().collect();
        assert_eq!(names, vec!["AvatarSex", "RenderResolutionDivisor"]);
    }

    /// `@setrot` turns the avatar to a heading a quarter turn away from the
    /// angle it named, and refuses an angle that is not a number.
    #[test]
    fn setrot_is_offset_by_a_quarter_turn() -> Result<(), TestError> {
        let mut state = RlvState::new();
        let mut source = TestSource::default();
        let result = run(&mut state, collar(), "setrot:1.5=force", &mut source)?
            .ok_or("not an extension command")?;
        assert_eq!(result.outcome, RlvOutcome::Success);
        assert_eq!(result.rotate_to, Some(SETROT_OFFSET - 1.5));

        let missing = run(&mut state, collar(), "setrot=force", &mut source)?
            .ok_or("not an extension command")?;
        assert_eq!(missing.outcome, RlvOutcome::FailedOption);
        assert_eq!(missing.rotate_to, None);

        // Registered for the reply dispatch too, and turning the avatar there
        // as well — see the module documentation.
        let as_query = run(&mut state, collar(), "setrot:0=2222", &mut source)?
            .ok_or("not an extension command")?;
        assert_eq!(as_query.rotate_to, Some(SETROT_OFFSET));
        assert_eq!(as_query.reply, None);
        Ok(())
    }

    /// A command from no family at all is not ours, so the consumer can go on
    /// to report it unknown.
    #[test]
    fn a_command_from_no_family_is_not_ours() -> Result<(), TestError> {
        let mut state = RlvState::new();
        let mut source = TestSource::default();
        assert_eq!(
            run(&mut state, collar(), "frobnicate=force", &mut source)?,
            None
        );
        assert_eq!(run(&mut state, collar(), "detach=n", &mut source)?, None);
        Ok(())
    }

    /// The reference's number reading stops at the first character it cannot
    /// use, and its boolean vocabulary is a closed list of words.
    #[test]
    fn numbers_are_read_as_a_prefix_and_booleans_as_words() {
        assert_eq!(parse_prefix::<u32>(" 2x "), Some(2));
        assert_eq!(parse_prefix::<f32>("1.5rad"), Some(1.5));
        assert_eq!(parse_prefix::<f32>("1e2"), Some(100.0));
        assert_eq!(parse_prefix::<u32>("x2"), None);
        assert_eq!(parse_prefix::<u32>(""), None);
        assert_eq!(
            parse_prefix::<u32>("-1"),
            None,
            "an unsigned setting refuses a negative rather than wrapping it"
        );
        assert_eq!(parse_bool("t"), Some(true));
        assert_eq!(parse_bool(" false "), Some(false));
        assert_eq!(parse_bool("yes"), None);
    }

    /// Every value formats the way its kind declares.
    #[test]
    fn a_value_formats_by_its_kind() {
        assert_eq!(RlvDebugValue::Bool(true).to_text(), "1");
        assert_eq!(RlvDebugValue::Bool(false).to_text(), "0");
        assert_eq!(RlvDebugValue::U32(1024).to_text(), "1024");
        assert_eq!(RlvDebugValue::Float(1.5).to_text(), "1.500");
        assert_eq!(RlvDebugValue::Bool(true).kind(), RlvDebugKind::Bool);
    }
}
