---
id: idiomatic-p3-02
title: always_run:
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 3 — Intent enums replacing bool / magic-int params (low-medium)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`always_run: bool` → `MovementMode { Walk, AlwaysRun }`
    (`command.rs:1450`, `sim_session.rs:340`). New public `MovementMode` enum
    (`Walk`/`AlwaysRun`, `is_always_run`/`from_always_run_flag`) in
    `sl-proto/src/types/session.rs` (next to `Reliability`) replaces the
    `always_run: bool` on both `Command::SetAlwaysRun` and
    `ServerEvent::SetAlwaysRun` (field renamed `always_run` → `mode`).
    `Session::set_always_run` takes `MovementMode`; the codec wraps at the
    boundary (`mode.is_always_run()` on encode,
    `MovementMode::from_always_run_flag(..)` on decode) so the `SetAlwaysRun`
    wire byte is byte-identical. Re-exported through
    `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (both runtimes updated at
    parity). REPL gains `parse_movement_mode` (accepts `run`/`walk` plus the
    legacy `true`/`false` boolean spelling); `set_always_run` usage is now
    `<mode:run|walk>`. Book `content/appearance.md` updated. +1 unit test
    (mode↔always-run-flag mapping + round-trip) and the lifecycle +
    `sim_session` round-trip suites updated. NO sl-types touched (a client
    wire-protocol concept).
