# sl-repl road map

An interactive REPL test client (`sl-repl` lib + `sl-repl-tokio` /
`sl-repl-bevy` bins) for shaking the SL client out against a live grid
(aditi), plus the wire-level diagnostics it needs. Work these top-to-bottom;
tick a box only when the step builds, is clippy-clean (restriction lints), and
`cargo test` passes. Add sub-tasks as you discover them. Detailed design lives
in the planning doc this was generated from and is re-derivable from the code.

Scope reminders:

- Commit on the current branch only (never auto-create a feature branch).
- Keep `sl-client-tokio` and `sl-client-bevy` at feature parity (land mirrored
  steps together).
- Never push client-only protocol types into the shared `sl-types` crate.
- Secrets (password, MFA token) must never reach any log, transcript, or HTTP
  body that gets logged.

## Phase A — core diagnostics (sl-wire / sl-proto), no REPL yet

- [ ] **A1. sl-wire enablers.** Generate
  `message_name(MessageId) -> Option<&'static str>` in `build.rs`; add a
  `Reader::position()` accessor; make `WireError: Clone`. No behaviour change.
  (Foundation for A2.)
- [ ] **A2. `Diagnostic` type + decode/CAPS surfacing.** New
  `sl-proto/src/types/diagnostic.rs` enum
  (`DecodeFailed{id,name,error,raw,failed_offset}`, `UnhandledMessage`,
  `UnknownCapsEvent`, `CapsDecodeFailed`, `ExpectedReplyMissing`) — **separate
  from `Event`** (a match on `Event` must never see diagnostics). Add
  `set_diagnostics(bool)` (default off), a diagnostic `VecDeque`, and
  `poll_diagnostic()`. At the silent sites (`handle_datagram` drop `:725`,
  `dispatch` catch-all arms `:~775`, `handle_caps_event`
  unknown/`from_llsd`-None `:~279`,`:389`) emit the matching diagnostic (capture
  raw bytes + `failed_offset` from `Reader::position()`) and add `tracing`
  (`trace!` per
  inbound message, `warn!` on failures). Gate raw-byte capture on the flag. Unit
  test: malformed/short datagram + unknown id → `DecodeFailed` with offset; flag
  off → nothing emitted, no clone.
- [ ] **A3. Expected-reply-missing diagnostics.** Emit
  `Diagnostic::ExpectedReplyMissing` when a reliable packet exhausts
  `MAX_RESEND_ATTEMPTS`, and for the logout/sit timeouts (teleport stays
  `Event::TeleportFailed`).

## Phase B — runtime wiring (keep tokio & bevy at parity)

- [ ] **B1. sl-client-tokio diagnostics stream.** Add
  `diagnostics_tx: mpsc::Sender<Diagnostic>` to `Client::run`; drain
  `poll_diagnostic`. Plumb the `diagnostics` flag through `LoginParams`/the
  `Client` option. Make CAPS http helpers log failures + feed
  `ExpectedReplyMissing` instead of swallowing into `Option`. Update callers:
  `sl-survey` and the tokio/bevy examples.
- [ ] **B2. sl-client-bevy diagnostics stream.** Register
  `SlDiagnostic(pub Diagnostic)` next to `SlEvent`; write it from the running
  system. Same flag plumbing + CAPS-failure surfacing. (Mirrors B1; parity.)

## Phase C — shared library `sl-repl`

- [ ] **C1. Crate scaffold + parser + registry (full command coverage).** New
  member `sl-repl` (dep `sl-proto`). `parse.rs`
  (`ReplAction { Meta | Command(PendingCommand) }`, meta incl.
  `Sleep/Comment/Set/Unset/Vars`), `registry.rs` (one entry per `Command`
  variant, `build: fn(&Args,&dyn ReplContext)->Result<Command,_>` called at
  dispatch), `args.rs`, `meta.rs`. Unit tests round-trip a line per arg-type.
- [ ] **C2. Context + placeholders.** `context.rs`: `ReplContext` +
  `SessionContext` (`apply_event`), forward resolution
  (`$self/$session/$circuit/$region/$parcel/$lastobj/$cap:Name/$var` → literal
  at **dispatch time**), reverse **symbolizer**, and `info!` **binding lines**
  on change. `PendingCommand::resolve(&ctx)`. Tests: resolve from stub ctx;
  unresolvable errors.
- [ ] **C3. Formatters.** `format.rs`:
  `format_event(&Event,&dyn ReplContext)` and
  `format_command(&Command,&dyn ReplContext)` (symbolized for clean cross-run
  diffs); `format_diagnostic(&Diagnostic)` (literal); `hexdump(bytes, mark)`
  marking `failed_offset`. Compile-time exhaustiveness (no `_` arm).
- [ ] **C4. Smoke battery + script recorder.** `smoke.rs`
  (`smoke_battery(self)` read-only requests). `record.rs` (`ScriptRecorder`:
  replayable `.repl` of interactive lines, `sleep` deltas, placeholders
  preserved verbatim, parse-fails as `# ERROR` comments, flush per line).
- [ ] **C5. Auth + secrets.** `auth.rs`: TOML credentials (multi-avatar,
  optional `mfa_command`, `mfa_window_guard_secs`), `Secret` redacting newtype,
  and `acquire_mfa_token` with the **wall-clock-aligned 30s-window wait**
  (`remaining = 30-(unix%30)`; if `< guard` sleep to next boundary, then run
  command). Tests: window math + `Secret` redaction.

## Phase D — binaries (parity)

- [ ] **D1. sl-repl-tokio.** clap CLI
  (`--credentials/--avatar/--grid/--login-uri/--start/--channel/--version/
  --script/--smoke/--log-file/--script-out`, manpage/completion subcommands).
  **Always-on file logging**: `trace`-level `tracing_appender` file (the full
  record: symbolized events + commands, binding lines, literal diagnostics +
  wire trace) plus a prompt-safe terminal layer whose writer wraps rustyline's
  `ExternalPrinter`; secrets never logged, login body not body-logged. Session
  loop: `connect_with_mfa` via `auth`,
  `client.run(event_tx,diag_tx,command_rx)`, `SessionContext` from events,
  three-way `select!`, `--smoke` on handshake, `--script`/stdin replay with
  `sleep`.
- [ ] **D2. sl-repl-bevy.** Same behaviour through
  `SlClientPlugin`/`SlEvent`/`SlDiagnostic`/`SlCommand`, rustyline on a
  `std::thread` → crossbeam channel, `SessionContext` resource, identical
  always-on file logging + auth/MFA. **MFA risk**: confirm/extend the plugin to
  re-issue login with an MFA token (Startup login is currently one-shot); if not
  feasible, document tokio as the MFA path and keep bevy for OpenSim — surface
  explicitly.

## Phase E — docs & live verification

- [ ] **E1. Update the mdbook (`book/`).** Add a new page (e.g.
  `book/src/tools/sl-repl.md`, linked from `book/src/SUMMARY.md`) documenting
  the three new crates, how to run them, the command grammar/placeholders,
  credential TOML + MFA timing, and the always-on logging/recording. Update the
  protocol
  pages for the new diagnostics surface: `comms/lludp-transport.md` and
  `comms/messages.md` (decode-failure/`UnhandledMessage` diagnostics,
  `message_name`, `Reader::position`), `comms/caps.md`
  (unknown/`CapsDecodeFailed` events), and `comms/sessions.md` (the `Diagnostic`
  type vs `Event`, `poll_diagnostic`, `set_diagnostics`, changed `Client::run`
  signature, `SlDiagnostic`). Touch `content/login.md` for the TOML/MFA flow.
  Keep diagrams consistent.
- [ ] **E2. Local OpenSim verification (both runtimes).** smoke + interactive +
  record/replay (with placeholders, re-login) + diagnostics check
  (`UnhandledMessage`/`UnknownCapsEvent` appear; forced bad input →
  `DecodeFailed` w/ marked hexdump) + secret-leak grep of log & transcript
  (zero matches) +
  two-run symbolized-log clean-diff.
- [ ] **E3. Live aditi run.** TOML `mfa_command`, window-aligned wait; capture
  every `DecodeFailed`/`UnhandledMessage`/`ExpectedReplyMissing`/`Disconnected`
  as the follow-up fix list for `sl-proto`/the runtimes (append those as new
  roadmap items).
