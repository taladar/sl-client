//! REPL meta commands: lines that control the REPL itself rather than the
//! session.
//!
//! A meta line is recognised by its leading token (or a `#` comment prefix) and
//! never reaches the [command registry](crate::registry) as a command. The
//! variants here cover the script-replay and variable controls used by the
//! binaries: comments, `sleep` delays, the `set`/`unset`/`vars` variable
//! commands, and the `help` / `?` command list — which *asks* the registry for
//! the [usage hints](crate::registry::CommandSpec::usage) it carries.

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
    /// A `help` / `?` request for command usage: `Some(name)` asks for one
    /// command's usage line, `None` lists every registered command.
    Help(Option<String>),
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
            "help" | "?" => {
                let name = rest.trim();
                if name.contains(char::is_whitespace) {
                    return Err(ReplError::BadMeta(format!(
                        "help expects at most one command name: {rest}"
                    )));
                }
                Self::Help((!name.is_empty()).then(|| name.to_owned()))
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pretty_assertions::assert_eq;

    use super::{MetaCommand, strip_quotes};
    use crate::error::ReplError;

    /// Parse a meta line already split into its head token and the rest.
    fn parse(head: &str, rest: &str) -> Result<Option<MetaCommand>, ReplError> {
        MetaCommand::try_parse(head, rest)
    }

    #[test]
    fn a_non_keyword_head_falls_through_to_command_parsing() {
        assert_eq!(parse("chat", "hello"), Ok(None));
    }

    #[test]
    fn sleep_takes_fractional_seconds() {
        assert_eq!(
            parse("sleep", "1.5"),
            Ok(Some(MetaCommand::Sleep(Duration::from_millis(1500))))
        );
    }

    #[test]
    fn sleep_rejects_a_duration_no_clock_can_wait() {
        for rest in ["nope", "-1", "inf", "NaN"] {
            assert!(
                matches!(parse("sleep", rest), Err(ReplError::BadMeta(_))),
                "`sleep {rest}` should be rejected, not turned into a duration"
            );
        }
    }

    #[test]
    fn set_keeps_the_rest_of_the_line_as_one_value() {
        assert_eq!(
            parse("set", r#"region "Da Boom""#),
            Ok(Some(MetaCommand::Set {
                name: "region".to_owned(),
                value: "Da Boom".to_owned(),
            }))
        );
    }

    #[test]
    fn set_needs_both_a_name_and_a_value() {
        assert!(matches!(parse("set", "lonely"), Err(ReplError::BadMeta(_))));
    }

    #[test]
    fn unset_and_vars_reject_the_arguments_they_do_not_take() {
        assert!(matches!(parse("unset", ""), Err(ReplError::BadMeta(_))));
        assert!(matches!(parse("unset", "a b"), Err(ReplError::BadMeta(_))));
        assert!(matches!(parse("vars", "extra"), Err(ReplError::BadMeta(_))));
        assert_eq!(parse("vars", ""), Ok(Some(MetaCommand::Vars)));
    }

    #[test]
    fn help_lists_everything_or_one_command() {
        assert_eq!(parse("help", ""), Ok(Some(MetaCommand::Help(None))));
        assert_eq!(parse("?", "  "), Ok(Some(MetaCommand::Help(None))));
        assert_eq!(
            parse("help", "chat"),
            Ok(Some(MetaCommand::Help(Some("chat".to_owned()))))
        );
        assert_eq!(
            parse("?", "chat"),
            Ok(Some(MetaCommand::Help(Some("chat".to_owned()))))
        );
    }

    #[test]
    fn help_takes_at_most_one_command_name() {
        assert!(matches!(
            parse("help", "chat im"),
            Err(ReplError::BadMeta(_))
        ));
    }

    #[test]
    fn quotes_are_stripped_only_as_a_matched_pair() {
        assert_eq!(strip_quotes(r#""quoted""#), "quoted");
        assert_eq!(strip_quotes(r#""unbalanced"#), r#""unbalanced"#);
        assert_eq!(strip_quotes("bare"), "bare");
    }
}
