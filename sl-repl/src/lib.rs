//! Shared library for an interactive Second Life / OpenSim REPL test client.
//!
//! `sl-repl` is the sans-I/O core of the `sl-repl-tokio` and `sl-repl-bevy`
//! binaries: it parses a line of text into a [`ReplAction`] — either a
//! [`MetaCommand`] that drives the REPL itself, or a [`PendingCommand`] naming a
//! grid [`Command`](sl_proto::Command) — and the [`Registry`] turns the latter
//! into a real `Command` against a [`ReplContext`] at dispatch time.
//!
//! The pieces:
//!
//! - [`parse`] — the line classifier ([`parse_line`]).
//! - [`args`] — the tokenizer and typed argument accessors ([`Args`]).
//! - [`meta`] — REPL control lines ([`MetaCommand`]).
//! - [`registry`] — one build entry per `Command` variant ([`Registry`]).
//! - [`context`] — the [`ReplContext`] placeholder-resolution interface.
//! - [`format`](mod@format) — symbolized renderers for events, commands, and
//!   diagnostics ([`format_event`], [`format_command`], [`format_diagnostic`],
//!   [`hexdump`]).

pub mod args;
pub mod context;
pub mod error;
pub mod format;
pub mod meta;
pub mod parse;
pub mod registry;

pub use args::Args;
pub use context::{NoContext, ReplContext, SessionContext};
pub use error::ReplError;
pub use format::{format_command, format_diagnostic, format_event, hexdump};
pub use meta::MetaCommand;
pub use parse::{PendingCommand, ReplAction, parse_line};
pub use registry::{CommandSpec, Registry};
