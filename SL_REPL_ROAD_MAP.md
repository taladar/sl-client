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

- [x] **A1. sl-wire enablers.** Generate
  `message_name(MessageId) -> Option<&'static str>` in `build.rs`; add a
  `Reader::position()` accessor; make `WireError: Clone`. No behaviour change.
  (Foundation for A2.)
- [x] **A2. `Diagnostic` type + decode/CAPS surfacing.** New
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
- [x] **A3. Expected-reply-missing diagnostics.** Emit
  `Diagnostic::ExpectedReplyMissing` when a reliable packet exhausts
  `MAX_RESEND_ATTEMPTS` (root *and* child circuits), and for the logout/sit
  timeouts (teleport stays `Event::TeleportFailed`). A sit timeout had no
  mechanism yet, so added a `Timers::sit` timer (`SIT_TIMEOUT` 15s) armed in
  `sit_on`, disarmed on `AvatarSitResponse`; it surfaces the diagnostic without
  closing the session. `UnackedPacket` now carries the message name to label the
  diagnostic; `process_resends` returns the exhausted `(sequence, name)` pairs
  and drops them so each is reported once. `tracing::warn!` at every site.

## Phase B — runtime wiring (keep tokio & bevy at parity)

- [x] **B1. sl-client-tokio diagnostics stream.** Added
  `diagnostics: mpsc::Sender<Diagnostic>` to `Client::run` and a per-iteration
  `poll_diagnostic` drain. The flag is a `Client::set_diagnostics` option (kept
  off the protocol-input `LoginParams`). The generic CAPS http helpers
  (`get`/`put`/`patch`/`delete_caps_llsd`, `post_voice_cap`) now report a failed
  request over the events channel with a reserved `\0caps-failure\0` sentinel
  (`caps::report_caps_failure`); the run loop logs it (`tracing::warn!`) and,
  when diagnostics are enabled, surfaces `ExpectedReplyMissing` instead of
  swallowing into `Option`. Callers updated: `sl-survey` and the tokio examples
  add a drained diagnostics channel.
- [x] **B2. sl-client-bevy diagnostics stream.** Registered
  `SlDiagnostic(pub Diagnostic)` next to `SlEvent`, written from the running
  system (drains `poll_diagnostic` plus the CAPS-failure sentinel). Same flag
  via `SlClientPlugin::diagnostics` → `set_diagnostics`, and the mirrored
  CAPS-failure surfacing in the blocking helpers. (Mirrors B1; parity.)

## Phase C — shared library `sl-repl`

- [x] **C1. Crate scaffold + parser + registry (full command coverage).** New
  member `sl-repl` (dep `sl-proto`). `parse.rs`
  (`ReplAction { Meta | Command(PendingCommand) }`, meta incl.
  `Sleep/Comment/Set/Unset/Vars`), `registry.rs` (one entry per `Command`
  variant, `build: fn(&Args,&dyn ReplContext)->Result<Command,_>` called at
  dispatch), `args.rs`, `meta.rs`. Unit tests round-trip a line per arg-type.
  Added `context.rs` with the minimal `ReplContext` trait + a `NoContext`
  stub (the session-aware `SessionContext`, forward resolution, and reverse
  symbolizer land with C2; build fns are written against the trait so they need
  no change). All **191** `Command` variants are registered; **190** build a
  command, and **`send`** (an arbitrary `Box<AnyMessage>`) returns
  `ReplError::NotSupported` — it can't be expressed as a text line and points
  the user at the specific commands instead. Grammar: positional + `key=value`
  tokens, double-quoted strings, `<x,y,z>`/`<x,y,z,s>` vectors/rotations, hex
  byte blobs, comma lists with `:`-separated records; every enum accepts a
  lowercase name (underscores optional) and/or its numeric wire code.
  Multi-field-`Vec` commands (`update_group_roles`, `modify_material_params`,
  `set_object_media`, `send_voice_signaling`) accept a single element via
  keyword fields; multi-element forms are deferred. 39 unit tests; clippy-clean
  under the restriction lints.
- [x] **C2. Context + placeholders.** `context.rs`: `ReplContext` +
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
