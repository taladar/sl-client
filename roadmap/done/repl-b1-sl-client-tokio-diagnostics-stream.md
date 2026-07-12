---
id: repl-b1
title: sl-client-tokio diagnostics stream
topic: repl
status: done
origin: SL_REPL_ROAD_MAP.md — Phase B — runtime wiring (keep tokio & bevy at parity)
---

Context: [context/repl.md](../context/repl.md).

**B1. sl-client-tokio diagnostics stream.** Added
`diagnostics: mpsc::Sender<Diagnostic>` to `Client::run` and a per-iteration
`poll_diagnostic` drain. The flag is a `Client::set_diagnostics` option (kept
off the protocol-input `LoginParams`). The generic CAPS http helpers
(`get`/`put`/`patch`/`delete_caps_llsd`, `post_voice_cap`) now report a failed
request over the events channel with a reserved `\0caps-failure\0` sentinel
(`caps::report_caps_failure`); the run loop logs it (`tracing::warn!`) and,
when diagnostics are enabled, surfaces `ExpectedReplyMissing` instead of
swallowing into `Option`. Callers updated: `sl-survey` and the tokio examples
add a drained diagnostics channel.
