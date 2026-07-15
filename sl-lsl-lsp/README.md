# sl-lsl-lsp

A **Language Server Protocol** server for **Linden Scripting Language** (LSL),
built on the pure [`sl-lsl`](../sl-lsl) tooling (lexer, parser, semantic pass
and the grid-served symbol table).

Where `sl-lsl` is deliberately I/O-free — it turns a `&str` into a syntax tree
and nothing more — this crate adds the **stateful, message-driven** layer an
editor talks to: it holds the open buffers, keeps their parse trees current as
the user types, and answers LSP requests over a transport.

Speaking LSP means the grid's LSL intelligence reaches **any** editor — nvim, VS
Code, Helix, and anything else that speaks the protocol — not just a viewer's
built-in text box. And unlike every existing LSL editor plugin, which ships a
hardcoded function list scraped from the wiki that rots as Linden Lab adds
functions, this server takes its symbols from the *connected grid*, so it tracks
those additions and sees OpenSim's OSSL a static list cannot.

## What it does (so far)

This is the **server foundation**: document synchronisation and symbols.

- **Document synchronisation** — full-text `didOpen` / `didChange` / `didClose`,
  re-parsing the buffer (cheaply, error-tolerantly, with no grid round-trip) on
  every edit.
- **`textDocument/documentSymbol`** — the hierarchical outline of a buffer:
  globals and states, with each function's / event handler's parameters and
  locals nested beneath it.
- **`workspace/symbol`** — the query-filtered top-level symbols across every
  open document.
- **Position-encoding negotiation** — UTF-8, UTF-16 (the protocol default) or
  UTF-32, whichever the client offers, so byte spans map to the right column.

The language-intelligence half — diagnostics, go-to-definition, find-references,
rename, completion, hover and signature help — builds on this foundation in a
later task (`viewer-lsl-lsp-diagnostics-nav`).

## Embedded and standalone

The server is driven through an
[`lsp-server`](https://crates.io/crates/lsp-server) `Connection` it is *handed*,
and that crate builds one two ways:

- **standalone** — the `sl-lsl-lsp` binary runs over **stdio**, the process an
  external editor spawns;
- **embedded** — an in-memory channel pair lets the viewer host the *same*
  server code in-process and hold the other end.

The **same** `run` loop backs both. That is why `lsp-server` (rust-analyzer's:
synchronous, no forced async runtime, with an in-memory transport) was chosen
over `tower-lsp` (unmaintained) or `async-lsp`.

## Running the standalone server

```console
cargo run -p sl-lsl-lsp --release
```

It speaks LSP on stdin/stdout and logs to stderr (honouring `RUST_LOG`, default
`info`). Point your editor's LSL language-server setting at the binary. A
standalone process has no grid connection, so it starts with an empty library
table: document sync and the symbol outline work fully; grid-backed completion
and hover arrive only when the server is run embedded in the viewer, which
fetches the `LSLSyntax` capability and hands the table in.

## Layout

- `position` — converting `sl-lsl`'s byte spans into LSP `(line, character)`
  positions under the negotiated encoding.
- `document` — the in-memory document store: text, line index and parse tree.
- `symbols` — turning a parse tree into the `documentSymbol` outline and the
  `workspace/symbol` list.
- `server` — the `run` loop: the `initialize` handshake, the message loop and
  request dispatch.
