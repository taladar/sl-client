//! Error type for parsing and building REPL lines.

/// An error produced while parsing a REPL line or building a
/// [`Command`](sl_proto::Command) from its arguments.
#[expect(
    clippy::module_name_repetitions,
    reason = "`ReplError` reads best as the crate's public error name"
)]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplError {
    /// The line named a command the [registry](crate::registry) does not know.
    #[error("unknown command: {0}")]
    UnknownCommand(String),
    /// A required argument was missing.
    #[error("command `{command}` is missing required argument `{field}`")]
    MissingArg {
        /// The command whose argument is missing.
        command: String,
        /// The name of the missing argument.
        field: String,
    },
    /// An argument was present but could not be parsed into the expected type.
    #[error("argument `{field}` = `{value}` is not a valid {expected}")]
    InvalidArg {
        /// The name of the offending argument.
        field: String,
        /// The raw (post-resolution) value that failed to parse.
        value: String,
        /// A human description of the type that was expected.
        expected: String,
    },
    /// A `$placeholder` token could not be resolved against the context.
    #[error("could not resolve placeholder `{0}`")]
    Unresolved(String),
    /// A quoted token was opened but never closed.
    #[error("unterminated quoted string")]
    UnterminatedQuote,
    /// A meta line (`#`, `sleep`, `set`, …) was malformed.
    #[error("invalid meta command: {0}")]
    BadMeta(String),
    /// The command is recognised but cannot be constructed from a text line.
    #[error("command `{0}` cannot be built from a REPL line: {1}")]
    NotSupported(&'static str, &'static str),
}
