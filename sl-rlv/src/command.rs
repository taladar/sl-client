//! The RLV command grammar: an `@behaviour[:option]=param` field and how it
//! decodes into a typed [`RlvCommand`].

use crate::behaviour::RlvBehaviour;

/// The RLV command prefix character (`RLV_CMD_PREFIX` in the reference).
///
/// An owner-say chat line is an RLV command line when its first character is
/// this `@`.
pub const RLV_PREFIX: char = '@';

/// The classified kind of a command's `param` (the text after `=`), mirroring
/// `ERlvParamType`.
///
/// The kind is what tells restriction state from an action from a query: `n` /
/// `add` and `y` / `rem` toggle a restriction, `force` runs an action now, a
/// number is a query whose answer is chatted back on that channel, and `@clear`
/// is its own thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RlvParam {
    /// `=n` or `=add`: add / turn on the restriction (`RLV_TYPE_ADD`).
    Add,
    /// `=y` or `=rem`: lift / turn off the restriction (`RLV_TYPE_REMOVE`).
    Remove,
    /// `=force`: perform the action immediately (`RLV_TYPE_FORCE`).
    Force,
    /// `=<number>`: a query — the answer is chatted back on `channel`
    /// (`RLV_TYPE_REPLY`). `@version=2222` and `@getoutfit=1234` are of this
    /// kind.
    Reply {
        /// The channel the reply is expected on (`convertToS32` of the param).
        channel: i32,
    },
    /// `@clear[=<filter>]`: drop this object's restrictions, optionally only
    /// those whose text contains `filter` (`RLV_TYPE_CLEAR`).
    Clear {
        /// The substring filter, or `None` for a bare `@clear`.
        filter: Option<String>,
    },
}

/// A single decoded RLV command — one `behaviour[:option]=param` field of an
/// owner-say chat line.
///
/// The raw [`keyword`](RlvCommand::keyword) is always kept so an unrecognised
/// behaviour ([`RlvBehaviour::Unknown`]) does not lose its spelling, and so a
/// consumer can echo the command back (`@getcommand`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RlvCommand {
    /// The raw behaviour keyword as it appeared, lower-cased and with any
    /// strict `_sec` suffix still attached (`recvim_sec`).
    pub keyword: String,
    /// The classified behaviour.
    pub behaviour: RlvBehaviour,
    /// Whether the keyword carried the strict `_sec` suffix *and* the behaviour
    /// supports it (`@recvim_sec=n`). A `_sec` on a behaviour that does not
    /// support strict mode leaves `behaviour` as [`RlvBehaviour::Unknown`] and
    /// this `false`, matching the reference.
    pub strict: bool,
    /// The `:option` between behaviour and `=`, lower-cased, if present and
    /// non-empty. Its meaning (a UUID, an exception, a modifier, a folder path)
    /// is behaviour-specific and left to the consumer to interpret.
    pub option: Option<String>,
    /// The classified param (the text after `=`).
    pub param: RlvParam,
}

/// What went wrong decoding a single command field.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RlvParseError {
    /// The behaviour was empty (the field began with `:` or `=`).
    #[error("empty behaviour keyword")]
    EmptyBehaviour,
    /// The field had no `=<param>` and the behaviour was not `clear` (which is
    /// the only behaviour valid without a param).
    #[error("missing `=<param>`")]
    MissingParam,
    /// The param was none of `n` / `add`, `y` / `rem`, `force`, or a reply
    /// channel number.
    #[error("unrecognised param `{0}`")]
    UnknownParam(String),
}

impl RlvCommand {
    /// Decode one `behaviour[:option]=param` command field.
    ///
    /// This is the per-field decoder (the reference's `RlvCommand` constructor
    /// plus `parseCommand`). The field is the text of a single comma-separated
    /// command; a leading `@` is tolerated and stripped. The whole field is
    /// lower-cased first, exactly as the reference lower-cases the message
    /// before tokenising, so option and param text are matched case-insensitively.
    ///
    /// # Errors
    ///
    /// Returns [`RlvParseError`] for a structurally malformed field (empty
    /// behaviour, missing param, or an unrecognised param kind).
    pub fn parse_field(field: &str) -> Result<Self, RlvParseError> {
        let field = field.strip_prefix(RLV_PREFIX).unwrap_or(field);
        let lowered = field.to_ascii_lowercase();

        // Format: <behaviour>[:<option>]=<param>. The option is only recognised
        // when a `=` is present (matching parseCommand, which only looks for
        // `:` when `=` was found).
        let (keyword, option, param_str, param_missing) = match lowered.split_once('=') {
            None => (lowered.as_str(), None, "", true),
            Some((before_eq, param_str)) => {
                let (keyword, option) = match before_eq.split_once(':') {
                    Some((keyword, opt)) => {
                        let option = (!opt.is_empty()).then(|| opt.to_owned());
                        (keyword, option)
                    }
                    None => (before_eq, None),
                };
                (keyword, option, param_str, param_str.is_empty())
            }
        };

        if keyword.is_empty() {
            return Err(RlvParseError::EmptyBehaviour);
        }

        // `clear` is the only behaviour that may appear without a param.
        if param_missing && keyword != "clear" {
            return Err(RlvParseError::MissingParam);
        }

        // Classify the param. The precedence mirrors the reference exactly:
        // `n`/`add` and `y`/`rem` win even for `@clear=n`, then `clear`, then
        // `force`, then a reply channel number.
        let param = if param_str == "n" || param_str == "add" {
            RlvParam::Add
        } else if param_str == "y" || param_str == "rem" {
            RlvParam::Remove
        } else if keyword == "clear" {
            RlvParam::Clear {
                filter: (!param_str.is_empty()).then(|| param_str.to_owned()),
            }
        } else if param_str == "force" {
            RlvParam::Force
        } else if let Ok(channel) = param_str.parse::<i32>() {
            RlvParam::Reply { channel }
        } else {
            return Err(RlvParseError::UnknownParam(param_str.to_owned()));
        };

        let (behaviour, strict) = resolve_behaviour(keyword);

        Ok(Self {
            keyword: keyword.to_owned(),
            behaviour,
            strict,
            option,
            param,
        })
    }
}

/// Resolve a raw keyword into a behaviour plus its strict flag.
///
/// A trailing `_sec` selects the strict variant, but only if the base behaviour
/// actually supports it ([`RlvBehaviour::has_strict`]); otherwise the whole
/// keyword is treated as unknown, matching `getBehaviourInfo`.
fn resolve_behaviour(keyword: &str) -> (RlvBehaviour, bool) {
    match keyword.strip_suffix("_sec") {
        Some(base) => match RlvBehaviour::from_keyword(base) {
            Some(behaviour) if behaviour.has_strict() => (behaviour, true),
            _ => (RlvBehaviour::Unknown, false),
        },
        None => (
            RlvBehaviour::from_keyword(keyword).unwrap_or(RlvBehaviour::Unknown),
            false,
        ),
    }
}
