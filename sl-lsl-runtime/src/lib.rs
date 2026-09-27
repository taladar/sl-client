//! Runtime for **Linden Scripting Language** (LSL) scripts on a test grid.
//!
//! The design record is the book chapter `simulator/lsl-engine.md`: this
//! crate will hold the value model, the lowering, a bytecode VM, the event
//! queue and the library, behind a `Host` trait the grid implements. Like
//! `sl-lsl` it is I/O-free, Bevy-free and synchronous.
//!
//! What exists so far is the **value model** — the value half of LSL's type
//! rules, whose compile-time half is [`sl_lsl::types`]:
//!
//! - [`value`] — [`Value`] and the flat list [`Element`], with boolean
//!   context ([`Value::is_true`]);
//! - [`cast`](mod@cast) — the cast matrix on values ([`cast()`](fn@cast));
//! - [`ops`] — the operators ([`binary`], [`prefix`]), including the
//!   run-time `Math Error`;
//! - [`parse`] and [`format`](mod@format) — the lenient string parsers behind a cast out
//!   of a string, and Mono's float/vector printing behind a cast into one.
//!
//! And the **library table** — [`library`]: every `ll*` function, constant
//! and event, generated at build time from Linden Lab's own `LSLSyntax`
//! document, with the typed dispatch that holds each implementation to it
//! and the coverage count of what is implemented.
//!
//! Every rule is pinned by a table-driven test quoting its oracle: LSL
//! PyOptimizer's `lslbasefuncs.py` and its `unit_tests/expr.suite`
//! expectations, which were measured against Second Life.

pub mod cast;
pub mod error;
pub mod format;
pub mod library;
mod num;
pub mod ops;
pub mod parse;
pub mod value;

pub use cast::cast;
pub use error::ValueError;
pub use ops::{binary, prefix};
pub use value::{Element, NULL_KEY, Value, ZERO_ROTATION, ZERO_VECTOR};
