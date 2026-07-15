//! A **Language Server Protocol** server for **Linden Scripting Language** (LSL),
//! built on the pure [`sl_lsl`] tooling (lexer, parser, semantic pass, grid
//! symbol table).
//!
//! Where `sl-lsl` is deliberately I/O-free — it turns a `&str` into a tree and
//! nothing more — this crate adds the **stateful, message-driven** layer an
//! editor talks to: it holds the open buffers, keeps their parse trees current
//! as the user types, and answers LSP requests over a transport. Speaking LSP
//! means the grid's LSL intelligence reaches **any** editor — nvim, VS Code,
//! Helix — not just a viewer's built-in text box, and unlike every existing LSL
//! editor plugin (which ships a hardcoded function list scraped from the wiki)
//! ours takes its symbols from the *connected grid*, so it tracks Linden Lab's
//! additions and sees OpenSim's OSSL a static list cannot.
//!
//! This crate is the **server foundation**: document synchronisation and
//! symbols. The language-intelligence half — diagnostics, go-to-definition,
//! find-references, rename, completion, hover and signature help — builds on it
//! in the `viewer-lsl-lsp-diagnostics-nav` task.
//!
//! ## Layout
//!
//! - [`position`] — converting `sl-lsl`'s byte spans into LSP `(line, character)`
//!   [`positions`](lsp_types::Position) under the negotiated position encoding
//!   ([`PositionEncoding`]).
//! - [`document`] — the in-memory [`Document`] store: text, line index and parse
//!   tree, kept in sync with the editor's `didOpen`/`didChange`/`didClose`.
//! - [`symbols`] — turning a parse tree into the LSP `documentSymbol` outline and
//!   `workspace/symbol` list.
//! - [`server`] — the [`run`] loop that negotiates `initialize`, drives the
//!   message loop and dispatches to the handlers.
//!
//! ## Embedded and standalone
//!
//! [`run`] drives an [`lsp_server::Connection`] it is handed, and `lsp-server`
//! builds one two ways: [`Connection::stdio`](lsp_server::Connection::stdio) for
//! the standalone `sl-lsl-lsp` binary an external editor spawns, and
//! [`Connection::memory`](lsp_server::Connection::memory) for an in-process pair
//! the viewer can hold the other end of. The **same** server code runs both
//! ways — the reason `lsp-server` (synchronous, no forced async runtime, with an
//! in-memory transport) was chosen over `tower-lsp` or `async-lsp`.

pub mod document;
pub mod position;
pub mod server;
pub mod symbols;

pub use document::Document;
pub use position::{LineIndex, PositionEncoding};
pub use server::{Server, ServerError, run};
pub use symbols::{document_symbols, workspace_symbols};
