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
  keyword fields; multi-element forms are intentionally **not** offered (decided
  2026-06-26) because each per-element payload embeds free text / JSON / URLs /
  SDP strings containing the `,` and `:` separators the unescaped `vec_records`
  parser splits on, so a record syntax would break on realistic input — the
  full `Vec` stays reachable through the typed `Session` API and the tokio/bevy
  drivers. (Contrast the `set_object_extra_params` `render_material` faces+ids
  lists, which tokenize cleanly and *are* exposed.) 39 unit tests; clippy-clean
  under the restriction lints.
- [x] **C2. Context + placeholders.** `context.rs`: `ReplContext` +
  `SessionContext` (`apply_event`), forward resolution
  (`$self/$session/$circuit/$region/$parcel/$lastobj/$cap:Name/$var` → literal
  at **dispatch time**), reverse **symbolizer**, and `info!` **binding lines**
  on change. `PendingCommand::resolve(&ctx)`. Tests: resolve from stub ctx;
  unresolvable errors.
- [x] **C3. Formatters.** `format.rs`:
  `format_event(&Event,&dyn ReplContext)` and
  `format_command(&Command,&dyn ReplContext)` render `<name><fields>` where the
  name is the snake-case event name / the registry command spelling (`im`,
  not `InstantMessage`) and the fields are the variant's `Debug` form with every
  binding-backed literal symbolized through the context — a token scanner offers
  each bare `[0-9A-Za-z-]` run (UUIDs, integers, names) and each whole quoted
  string (cap URLs, var values) to `ReplContext::symbolize`, so `$self` /
  `$region` / `$cap:Name` replace the volatile ids for clean cross-run diffs.
  `format_diagnostic(&Diagnostic)` renders **literally** (one header line per
  variant + a marked hexdump for `DecodeFailed`); `hexdump(bytes, mark)` is a
  classic offset/hex/ASCII dump that brackets the byte at `failed_offset`
  (`[ab]` vs ` ab `, equal-width so rows stay aligned) and notes an
  at/past-end mark on a trailing line. Compile-time exhaustiveness: the two enum
  renderers match every variant via `event_name`/`command_name` (no `_` arm —
  191 commands + 113 events), so a new variant fails to compile until named. 6
  unit tests; clippy-clean under the restriction lints.
- [x] **C4. Smoke battery + script recorder.** `smoke.rs`
  (`smoke_battery(self)` read-only requests). `record.rs` (`ScriptRecorder`:
  replayable `.repl` of interactive lines, `sleep` deltas, placeholders
  preserved verbatim, parse-fails as `# ERROR` comments, flush per line).
- [x] **C5. Auth + secrets.** `auth.rs`: TOML credentials (multi-avatar,
  optional `mfa_command`, `mfa_window_guard_secs`), `Secret` redacting newtype,
  and `acquire_mfa_token` with the **wall-clock-aligned 30s-window wait**
  (`remaining = 30-(unix%30)`; if `< guard` sleep to next boundary, then run
  command). Tests: window math + `Secret` redaction.

## Phase D — binaries (parity)

- [x] **D1. sl-repl-tokio.** clap CLI
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
- [x] **D2. sl-repl-bevy.** Same behaviour through
  `SlClientPlugin`/`SlEvent`/`SlDiagnostic`/`SlCommand`, rustyline on a
  `std::thread` → crossbeam channel, `SessionContext` resource, identical
  always-on file logging + auth/MFA. DONE: new `sl-repl-tokio`-mirroring
  `sl-repl-bevy` crate (default invocation runs a headless Bevy session via
  `MinimalPlugins`+`ScheduleRunnerPlugin`; `generate-manpage`/
  `generate-shell-completion` subcommands). The `repl_driver` `Update` system
  folds `SlIdentity`/`SlCapabilities`/`SlEvent`/`SlDiagnostic` into the
  `SessionContext` + log, fires `--smoke` on `RegionHandshakeComplete`, drains
  the crossbeam line channel into dispatched `SlCommand`s, and writes `AppExit`
  on `LoggedOut`/`Disconnected`. To seed `$self/$session/$circuit/$cap:Seed`
  (none of which are in the `Event` stream), added an additive `SlIdentity`
  event to the plugin (mirrors the tokio D1 identity accessors), emitted once at
  the `Running` transition. **MFA risk RESOLVED (full parity, not the
  fallback):** the plugin's Startup login stays one-shot and surfaces the
  existing `SlMfaChallenge`; the binary re-issues login with the acquired token
  by re-running the Bevy `App` once per challenge (persistent line editor + log
  survive the restart, recorder handed back via `remove_non_send_resource`).
  The recorder is a `NonSend` resource (`ScriptRecorder` is `!Sync`). Input
  modes: interactive (rustyline on a `std::thread`, `ExternalPrinter` handed
  back over a crossbeam channel), `--script`, and non-TTY stdin (both replay
  with real `std::thread::sleep` pacing via a feeder thread). Live-verified
  against local OpenSim: login + full `--smoke` battery decoded
  (region/money/economy/parcel/avatar/wearables), `$self`/`$parcel`/`$cap:*`
  symbolized, diagnostics surfaced (`UnknownCapsEvent`/`UnhandledMessage`),
  interactive `chat` round-tripped, clean logout, zero password occurrences in
  the log. **NEXT = Phase E1** (mdbook docs).

## Phase E — docs & live verification

- [x] **E1. Update the mdbook (`book/`).** Added a new **Tools** section to
  `book/src/SUMMARY.md` with `book/src/tools/sl-repl.md`, documenting the three
  new crates, how to run them, the command grammar/placeholders/meta commands,
  smoke mode, the wire-diagnostics surface, always-on logging/recording, and the
  credential TOML + window-aligned MFA timing (`In this codebase` note maps it
  all to modules). Updated the protocol pages for the diagnostics surface:
  `comms/sessions.md` gained a **Diagnostics** section (the `Diagnostic` type vs
  `Event`, the five variants, `set_diagnostics`/`poll_diagnostic`, the changed
  `Client::run` signature, `SlDiagnostic`); `comms/lludp-transport.md`
  (`Reader::position` → `DecodeFailed` offset, `ExpectedReplyMissing` on resend
  exhaustion); `comms/messages.md` (a *When decoding goes wrong* section +
  generated `message_name`); `comms/caps.md`
  (`UnknownCapsEvent`/`CapsDecodeFailed`, the `report_caps_failure` sentinel →
  `ExpectedReplyMissing`); `content/login.md` (TOML credentials + the
  wall-clock-aligned MFA wait). Also listed the REPL crates in
  `architecture.md`. `mdbook build` clean, `rumdl`/`typos` clean, all links
  resolve.
- [x] **E2. Local OpenSim verification (both runtimes).** Verified both
  `sl-repl-tokio` and `sl-repl-bevy` against local `opensim.service`: smoke
  battery
  decoded (region/money/economy/parcel/avatar/wearables); interactive `chat` +
  `im $self` dispatched with `$self` resolved at dispatch and symbolized in the
  echoes; record/replay (`record-interactive.py` pty driver →
  `--script-out`; `$self` preserved verbatim in the `.repl`; replay re-logs in
  with a fresh `$circuit`/`$session` and honours `sleep` pacing) on both
  runtimes; diagnostics surfaced (`UnhandledMessage`, `UnknownCapsEvent`);
  secret-leak grep of every log + transcript = **zero** matches; two-run
  symbolized-log clean-diff (differences only in genuinely volatile server data
  — random terrain/wind, the appearance `serial`, per-session neighbor seed-cap
  UUIDs, the avatar's per-session `local_id` — never in symbolized ids). The
  forced-bad-input `DecodeFailed`-with-marked-hexdump path can't be driven over
  a live socket (no way to spoof the sim's source port), so it is covered by
  tests: `sl-proto`'s `unknown_message_id_surfaces_decode_failed_with_offset` (a
  `Session` produces the diagnostic from a malformed datagram) plus a NEW
  end-to-end `sl-repl` test `decode_failed_renders_header_and_marked_hexdump`
  (the formatter renders the header + bracketed `[aa]` hexdump for the exact
  bytes a live `Session` captures). Closing the latter's coverage gap required
  re-exporting `MessageId` from `sl-proto` (a `Diagnostic::DecodeFailed` public
  field whose type a consumer previously couldn't name). Reusable inputs saved
  under `sl-repl/testdata/` (`credentials.example.toml`,
  `interactive-probe.repl`, `record-interactive.py`, `README.md`);
  `credentials.toml` + the default log
  files added to `.gitignore`. Known cosmetic (pre-existing, out of scope): the
  C3 token-scanner over-matches bare integer runs (`dilation: $parcel.0` for
  `1.0`).
- [ ] **E3. Live aditi run.** TOML `mfa_command`, window-aligned wait; capture
  every `DecodeFailed`/`UnhandledMessage`/`ExpectedReplyMissing`/`Disconnected`
  as the follow-up fix list for `sl-proto`/the runtimes (append those as new
  roadmap items).
