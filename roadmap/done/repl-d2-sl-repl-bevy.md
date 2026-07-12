---
id: repl-d2
title: sl-repl-bevy
topic: repl
status: done
origin: SL_REPL_ROAD_MAP.md — Phase D — binaries (parity)
---

Context: [context/repl.md](../context/repl.md).

**D2. sl-repl-bevy.** Same behaviour through
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
