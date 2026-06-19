//! REPL meta commands: lines that control the REPL itself rather than the
//! session.
//!
//! A meta line is recognised by its leading token (or a `#` comment prefix) and
//! never reaches the [command registry](crate::registry). The variants here
//! cover the script-replay and variable controls used by the binaries:
//! comments, `sleep` delays, and the `set`/`unset`/`vars` variable commands.

use std::time::Duration;

use crate::error::ReplError;

/// A REPL control line that acts on the REPL session, not the grid.
#[expect(
    clippy::module_name_repetitions,
    reason = "`MetaCommand` reads best as the public meta-command type"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaCommand {
    /// A `#` comment (or blank-after-`#`); the text is preserved verbatim
    /// (without the leading `#`).
    Comment(String),
    /// A `sleep <seconds>` pause, used to pace script replay.
    Sleep(Duration),
    /// A `set <name> <value>` user-variable assignment; `value` is the rest of
    /// the line (surrounding double quotes stripped).
    Set {
        /// The variable name (without the `$`).
        name: String,
        /// The literal value to bind.
        value: String,
    },
    /// An `unset <name>` user-variable removal.
    Unset(String),
    /// A `vars` request to list the currently bound user variables.
    Vars,
}

impl MetaCommand {
    /// Try to parse a meta command from a line's leading token `head` and the
    /// remaining text `rest` (already trimmed of the separating space).
    ///
    /// Returns `Ok(None)` when `head` is not a meta keyword, so the caller can
    /// fall through to command parsing.
    pub(crate) fn try_parse(head: &str, rest: &str) -> Result<Option<Self>, ReplError> {
        let parsed = match head {
            "sleep" => {
                let seconds = rest.trim().parse::<f64>().ok().filter(|s| s.is_finite());
                let duration = seconds
                    .and_then(|s| Duration::try_from_secs_f64(s).ok())
                    .ok_or_else(|| ReplError::BadMeta(format!("sleep expects seconds: {rest}")))?;
                Self::Sleep(duration)
            }
            "set" => {
                let (name, value) =
                    rest.trim().split_once(char::is_whitespace).ok_or_else(|| {
                        ReplError::BadMeta(format!("set expects `<name> <value>`: {rest}"))
                    })?;
                Self::Set {
                    name: name.to_owned(),
                    value: strip_quotes(value.trim()).to_owned(),
                }
            }
            "unset" => {
                let name = rest.trim();
                if name.is_empty() || name.contains(char::is_whitespace) {
                    return Err(ReplError::BadMeta(format!(
                        "unset expects `<name>`: {rest}"
                    )));
                }
                Self::Unset(name.to_owned())
            }
            "vars" => {
                if !rest.trim().is_empty() {
                    return Err(ReplError::BadMeta(format!(
                        "vars takes no arguments: {rest}"
                    )));
                }
                Self::Vars
            }
            _ => return Ok(None),
        };
        Ok(Some(parsed))
    }
}

/// Strip a single pair of surrounding double quotes from `value`, if present.
fn strip_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(value)
}
