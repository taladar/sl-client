---
id: repl-c4
title: Smoke battery + script recorder
topic: repl
status: done
origin: SL_REPL_ROAD_MAP.md — Phase C — shared library `sl-repl`
---

Context: [context/repl.md](../context/repl.md).

**C4. Smoke battery + script recorder.** `smoke.rs`
(`smoke_battery(self)` read-only requests). `record.rs` (`ScriptRecorder`:
replayable `.repl` of interactive lines, `sleep` deltas, placeholders
preserved verbatim, parse-fails as `# ERROR` comments, flush per line).
