//! The resolution context a registry build function consults to turn
//! `$placeholder` tokens into literal argument values.
//!
//! This module defines only the *interface* (the [`ReplContext`] trait) plus a
//! trivial [`NoContext`] that resolves nothing. The session-aware
//! implementation (`SessionContext`, the forward placeholder table, the reverse
//! symbolizer, and the `info!` binding lines) lands with phase C2; the build
//! functions in [`registry`](crate::registry) are written against the trait so
//! they need no change when the real context arrives.

/// Resolves the `$placeholder` argument tokens a REPL line may use.
///
/// A token of the form `$name` is handed to [`ReplContext::resolve_placeholder`]
/// with the text after the `$` (for example `self`, `session`, `cap:GetTexture`,
/// or a user variable). Returning `None` makes the argument fail to parse with
/// [`ReplError::Unresolved`](crate::ReplError::Unresolved).
#[expect(
    clippy::module_name_repetitions,
    reason = "`ReplContext` reads best as the crate's public trait name"
)]
pub trait ReplContext {
    /// Resolve a placeholder name (the text after the leading `$`) to the
    /// literal string it stands for, or `None` if it is unknown.
    fn resolve_placeholder(&self, name: &str) -> Option<String>;
}

/// A [`ReplContext`] that resolves no placeholders.
///
/// Useful for parsing fully-literal lines (every argument spelled out) and for
/// unit tests that do not need session state.
#[expect(
    clippy::module_name_repetitions,
    reason = "`NoContext` names the empty `ReplContext` clearly"
)]
#[derive(Debug, Clone, Copy, Default)]
pub struct NoContext;

impl ReplContext for NoContext {
    fn resolve_placeholder(&self, _name: &str) -> Option<String> {
        None
    }
}
