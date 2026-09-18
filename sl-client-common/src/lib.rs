//! The runtime-independent half of a Second Life / OpenSim client.
//!
//! [`sl_client_tokio`](https://docs.rs/sl-client-tokio) and
//! [`sl_client_bevy`](https://docs.rs/sl-client-bevy) are two runtimes over the
//! same sans-IO [`sl_proto`] core, and the workspace requires them to stay at
//! feature parity. Parity used to be maintained by **copying**: three of their
//! modules were byte-identical in both crates and a fourth differed only in a
//! doc comment, so every fix and every test had to be applied twice and one
//! missed paste would have let them diverge silently.
//!
//! Nothing in those modules was runtime-specific — they are synchronous
//! `fs` plus pure logic — so they live here instead, and both runtimes depend
//! on this crate:
//!
//! - [`chat_log`] — the file-I/O shell over `sl_proto`'s sans-IO chat-log core
//!   (the local wall clock, the transcript append/seek, and the name/path
//!   caches the Firestorm format needs).
//! - [`inventory_cache`] — the gzipped on-disk inventory skeleton cache, loaded
//!   into a [`Session`](sl_proto::Session) at login and saved back on a timer.
//! - [`lsl_syntax_cache`] — the gzipped on-disk cache of the grid's
//!   `LSLSyntax` capability document, keyed by the id the grid advertises.
//! - [`retry`] — the transient-HTTP-error retry policy (which statuses are
//!   worth retrying, and the backoff before each attempt). It yields a
//!   [`Duration`](std::time::Duration); *how* to wait it out is the caller's
//!   runtime's business — the tokio side awaits its timer, the bevy side blocks
//!   its task-pool thread.
//!
//! What stayed behind in each runtime is genuinely runtime-specific: the HTTP
//! proxy shells differ by more than half their lines, and everything that owns
//! a task, a channel or a timer cannot be shared at all.

pub mod chat_log;
pub mod inventory_cache;
pub mod lsl_syntax_cache;
pub mod retry;
