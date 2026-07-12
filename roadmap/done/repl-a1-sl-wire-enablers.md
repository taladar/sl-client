---
id: repl-a1
title: sl-wire enablers
topic: repl
status: done
origin: SL_REPL_ROAD_MAP.md — Phase A — core diagnostics (sl-wire / sl-proto), no REPL yet
---

Context: [context/repl.md](../context/repl.md).

**A1. sl-wire enablers.** Generate
`message_name(MessageId) -> Option<&'static str>` in `build.rs`; add a
`Reader::position()` accessor; make `WireError: Clone`. No behaviour change.
(Foundation for A2.)
