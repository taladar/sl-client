//! Runtime for **Linden Scripting Language** (LSL) scripts on a test grid.
//!
//! The design record is the book chapter `simulator/lsl-engine.md`: this
//! crate will hold the value model, the lowering, a bytecode VM, the event
//! queue and the library, behind a `Host` trait the grid implements. Like
//! `sl-lsl` it is I/O-free, Bevy-free and synchronous.
//!
//! First, the **value model** — the value half of LSL's type
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
//! And the **compiler** — [`compile()`]: source to a [`bytecode::Program`],
//! a stack bytecode with every name resolved and every implicit conversion
//! explicit, or the [`CompileError`]s a grid would answer an upload with.
//!
//! And the **VM** — [`vm`]: an [`Engine`](vm::Engine) that runs a region's
//! script [`Instance`](vm::Instance)s round-robin in per-tick slices counted
//! in instructions, each suspendable between any two of them, with every
//! library call going through the [`Host`](vm::Host) the grid implements.
//!
//! Every rule is pinned by a table-driven test quoting its oracle: LSL
//! PyOptimizer's `lslbasefuncs.py` and its `unit_tests/expr.suite`
//! expectations, which were measured against Second Life.

pub mod bytecode;
pub mod cast;
pub mod compiler;
pub mod error;
pub mod format;
pub mod library;
mod num;
pub mod ops;
pub mod parse;
pub mod value;
pub mod vm;

pub use cast::cast;
pub use compiler::{CompileError, CompileErrorKind, compile};
pub use error::ValueError;
pub use ops::{binary, prefix};
pub use value::{Element, NULL_KEY, Value, ZERO_ROTATION, ZERO_VECTOR};
