---
id: repl-d1
title: sl-repl-tokio
topic: repl
status: done
origin: SL_REPL_ROAD_MAP.md — Phase D — binaries (parity)
---

Context: [context/repl.md](../context/repl.md).

**D1. sl-repl-tokio.** clap CLI
(`--credentials/--avatar/--grid/--login-uri/--start/--channel/--version/
--script/--smoke/--log-file/--script-out`, manpage/completion subcommands).
**Always-on file logging**: `trace`-level `tracing_appender` file (the full
record: symbolized events + commands, binding lines, literal diagnostics +
wire trace) plus a prompt-safe terminal layer whose writer wraps rustyline's
`ExternalPrinter`; secrets never logged, login body not body-logged. Session
loop: `connect_with_mfa` via `auth`,
`client.run(event_tx,diag_tx,command_rx)`, `SessionContext` from events,
three-way `select!`, `--smoke` on handshake, `--script`/stdin replay with
`sleep`. DONE: new `sl-repl-tokio` crate (default invocation runs a session,
`generate-manpage`/`generate-shell-completion` subcommands). To seed
`$self/$session/$circuit` and symbolize `$cap:Name`, added read accessors
`session_id`/`circuit_code`/`seed_capability` on the tokio `Client`
(`session_id`/`circuit_code` on `sl-proto`'s `Session`) and a
`Client::set_caps_reporter(mpsc::Sender<HashMap>)` that streams the region cap
map (at login + each region change) with NO change to `run`'s signature;
mirrored in `sl-client-bevy` as an `SlCapabilities` event for runtime parity
(D2 consumes it). Input modes: interactive (rustyline on a `std::thread`,
external printer handed back via a `oneshot`), `--script`, and non-TTY stdin
(both replay with real `sleep` pacing); `--script-out` records interactive
lines. Live-verified against local OpenSim: login + full `--smoke` battery
decoded, events/commands symbolized (`$self`/`$parcel`/`$cap:*`), diagnostics
surfaced (`UnhandledMessage`), clean logout, zero password occurrences in the
log.
