//! The top-level line parser: text in, a [`ReplAction`] out.
//!
//! [`parse_line`] classifies a single input line as either a
//! [meta command](MetaCommand) (a comment, `sleep`, `set`/`unset`/`vars`) or a
//! [`PendingCommand`] — the named command plus its parsed [`Args`], with
//! `$placeholder` tokens left unresolved until the registry builds it against a
//! [`ReplContext`](crate::context::ReplContext) at dispatch time.

use std::sync::LazyLock;

use sl_proto::Command;

use crate::args::Args;
use crate::context::ReplContext;
use crate::error::ReplError;
use crate::meta::MetaCommand;
use crate::registry::Registry;

/// The shared default [`Registry`], built once, that [`PendingCommand::resolve`]
/// dispatches through. A binary that already holds a [`Registry`] should call
/// [`Registry::build`] directly instead.
static DEFAULT_REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::new);

/// A grid command named on a REPL line, with its arguments parsed but not yet
/// resolved or type-checked. The [registry](crate::registry) turns this into a
/// [`Command`] at dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCommand {
    /// The command name (the line's first token).
    pub name: String,
    /// The parsed positional and keyword arguments.
    pub args: Args,
}

impl PendingCommand {
    /// Resolve and build this pending command into a [`Command`], resolving its
    /// `$placeholder` arguments against `ctx` at dispatch time.
    ///
    /// This is a convenience over the shared default [`Registry`]; a binary that
    /// keeps its own [`Registry`] should call [`Registry::build`] directly.
    ///
    /// # Errors
    ///
    /// Returns [`ReplError::UnknownCommand`] if the name is not registered, or
    /// whatever the command's build function returns when an argument is
    /// missing, malformed, or an unresolvable placeholder.
    pub fn resolve(&self, ctx: &dyn ReplContext) -> Result<Command, ReplError> {
        DEFAULT_REGISTRY.build(self, ctx)
    }
}

/// The outcome of parsing one REPL line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplAction {
    /// A REPL control line (comment, sleep, variable command).
    Meta(MetaCommand),
    /// A grid command to dispatch to the session.
    Command(PendingCommand),
}

/// Parse a single REPL input line.
///
/// Returns `Ok(None)` for a blank line (after trimming). A `#`-prefixed line is
/// a [`MetaCommand::Comment`]; a line whose first token is a meta keyword is the
/// matching [`MetaCommand`]; anything else is a [`PendingCommand`].
///
/// # Errors
///
/// Returns a [`ReplError`] if a meta command is malformed
/// ([`ReplError::BadMeta`]) or the argument tokens cannot be tokenized
/// (e.g. [`ReplError::UnterminatedQuote`]).
#[expect(
    clippy::module_name_repetitions,
    reason = "`parse_line` reads best as the crate's public entry point"
)]
pub fn parse_line(line: &str) -> Result<Option<ReplAction>, ReplError> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if let Some(comment) = trimmed.strip_prefix('#') {
        return Ok(Some(ReplAction::Meta(MetaCommand::Comment(
            comment.trim_start().to_owned(),
        ))));
    }
    let (head, rest) = match trimmed.split_once(char::is_whitespace) {
        Some((head, rest)) => (head, rest.trim_start()),
        None => (trimmed, ""),
    };
    if let Some(meta) = MetaCommand::try_parse(head, rest)? {
        return Ok(Some(ReplAction::Meta(meta)));
    }
    Ok(Some(ReplAction::Command(PendingCommand {
        name: head.to_owned(),
        args: Args::parse(rest)?,
    })))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pretty_assertions::assert_eq;
    use sl_proto::{Command, Uuid};

    use super::{PendingCommand, ReplAction, parse_line};
    use crate::context::{NoContext, SessionContext};
    use crate::error::ReplError;
    use crate::meta::MetaCommand;

    /// Parse a line known to be a command, returning its [`PendingCommand`].
    fn command(line: &str) -> Option<PendingCommand> {
        match parse_line(line) {
            Ok(Some(ReplAction::Command(pending))) => Some(pending),
            _ => None,
        }
    }

    #[test]
    fn blank_and_whitespace_lines_are_none() {
        assert_eq!(parse_line(""), Ok(None));
        assert_eq!(parse_line("   \t "), Ok(None));
    }

    #[test]
    fn comment_lines_become_meta_comments() {
        assert_eq!(
            parse_line("# hello there"),
            Ok(Some(ReplAction::Meta(MetaCommand::Comment(
                "hello there".to_owned()
            ))))
        );
    }

    #[test]
    fn sleep_meta_parses_seconds() {
        assert_eq!(
            parse_line("sleep 1.5"),
            Ok(Some(ReplAction::Meta(MetaCommand::Sleep(
                Duration::from_millis(1500)
            ))))
        );
        assert_eq!(
            parse_line("sleep nope"),
            Err(ReplError::BadMeta("sleep expects seconds: nope".to_owned()))
        );
    }

    #[test]
    fn set_unset_vars_meta() {
        assert_eq!(
            parse_line("set region \"Da Boom\""),
            Ok(Some(ReplAction::Meta(MetaCommand::Set {
                name: "region".to_owned(),
                value: "Da Boom".to_owned(),
            })))
        );
        assert_eq!(
            parse_line("unset region"),
            Ok(Some(ReplAction::Meta(MetaCommand::Unset(
                "region".to_owned()
            ))))
        );
        assert_eq!(
            parse_line("vars"),
            Ok(Some(ReplAction::Meta(MetaCommand::Vars)))
        );
    }

    #[test]
    fn command_line_splits_name_and_args() {
        let pending = command("im 11111111-1111-1111-1111-111111111111 hi");
        assert_eq!(
            pending.as_ref().map(|p| p.name.clone()),
            Some("im".to_owned())
        );
        assert_eq!(
            pending.and_then(|p| p.args.req_str(&crate::context::NoContext, "x", 1).ok()),
            Some("hi".to_owned())
        );
    }

    #[test]
    fn unterminated_quote_is_an_error() {
        assert_eq!(
            parse_line(r#"chat "oops"#),
            Err(ReplError::UnterminatedQuote)
        );
    }

    #[test]
    fn resolve_builds_command_from_stub_context() {
        let built = command("chat hello").map(|pending| pending.resolve(&NoContext));
        assert!(
            matches!(built, Some(Ok(Command::Chat { .. }))),
            "a literal command builds against the stub context"
        );
    }

    #[test]
    fn resolve_expands_session_placeholders() {
        let mut ctx = SessionContext::new();
        ctx.set_identity(
            Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap_or_else(|_| Uuid::nil()),
            Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap_or_else(|_| Uuid::nil()),
            7,
        );
        let built = command("im $self hi").map(|pending| pending.resolve(&ctx));
        assert!(
            matches!(built, Some(Ok(Command::InstantMessage { .. }))),
            "$self resolves through the session context at dispatch time"
        );
    }

    #[test]
    fn resolve_unresolvable_placeholder_errors() {
        let built = command("im $self hi").map(|pending| pending.resolve(&NoContext));
        assert!(
            matches!(built, Some(Err(ReplError::Unresolved(ref name))) if name == "$self"),
            "an unresolvable placeholder fails to build"
        );
    }
}
