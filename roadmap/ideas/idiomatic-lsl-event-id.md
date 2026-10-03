---
id: idiomatic-lsl-event-id
title: Generated EventId for LSL events in the runtime
topic: idiomatic
status: ideas
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

## What

`sl-lsl-runtime` identifies events by name strings: `lifecycle("changed")` /
`("on_rez")` / `("timer")` falling back to a `MISSING` placeholder that
matches no handler; `DETECTION_EVENTS`, `COALESCING`, `NON_STACKING` as
hand-kept string lists; `State::handler(&str)` and literal compares on
`"state_entry"` / `"state_exit"`. A misspelling silently never delivers the
event; refreshing the vendored `keywords_lsl_default.xml` with new events that
use `llDetected*` (the damage events) would make posting them fail until the
list is edited by hand. Builtins already have a generated `BuiltinId`.

## How

Generate `EventId` from the table with per-event flags (detected, coalescing,
non-stacking, transition) as generated columns; `Handler::event` and
`lifecycle` take it. `Engine::changed(id, change: i32)` takes a
`ChangedFlags` bitflags at the host API.
