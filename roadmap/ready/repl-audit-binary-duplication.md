---
id: repl-audit-binary-duplication
title: The two REPL binaries share ~400 near-verbatim lines and have already drifted
topic: repl
status: ready
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/repl.md](../context/repl.md).

`sl-repl-tokio/src/bin/sl-repl-tokio.rs` (769 lines) and
`sl-repl-bevy/src/bin/sl-repl-bevy.rs` (853) share roughly **400 lines** with a
twin, about 256 of them near-verbatim (at most two differing lines per block):

- **fully identical** (61 lines, zero differences): `grid_login_uri`,
  `resolve_login_uri`, `input_mode`, `replay_lines`;
- near-identical: the `PrinterWriter` / `MakeWriter` / `TerminalSink` stack
  (76 lines, 1 difference), `Subcommand` + `Options` + `InputMode` (48/2),
  `build_filters` + `init_logging` (56/2).

The only real divergence is `tokio::sync::mpsc` vs `crossbeam_channel` and
`mpsc::Sender<Command>` vs `MessageWriter<SlCommand>` — one trait's worth.

It has already cost something: `--http-proxy` / `SL_REPL_HTTP_PROXY` exists in
the tokio REPL and in `sl-survey` (`sl-survey.rs:149`) but **not** in
`sl-repl-bevy`, even though `sl_client_bevy::http_proxy::set_proxy` is public —
so the bevy REPL cannot be proxied.

Scope: a small runtime trait (send a command, receive events, receive
diagnostics) plus one shared binary skeleton. `ReplContext`
(`sl-repl/src/context.rs:35`) abstracts placeholder resolution, not the runtime,
so nothing covers this today.

All three binaries (`sl-repl-tokio`, `sl-repl-bevy`, `sl-survey`) have **zero
tests**, which is why the verbatim-duplicated `resolve_login_uri` ladder and
`sl-survey`'s `merge_bitmap` bit-walk (`sl-survey.rs:358`) are uncovered.

Two `sl-survey` defects to fold in: `:588`, `:603`, `:1090` — a `MapBlock` with
no name stores `""` via `unwrap_or_default()`, which `:603` then treats as a
valid re-login target (`warn!("... re-logging in at ")` and
`StartLocation::region("")`); and `:1077` —
`let (diag_tx, _diag_rx) = mpsc::channel(16);` drops the diagnostics receiver at
construction, so every protocol `Diagnostic` is discarded.
