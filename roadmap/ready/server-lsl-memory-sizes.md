---
id: server-lsl-memory-sizes
title: Measure Second Life's exact per-value script memory costs on aditi
topic: server
status: ready
origin: user review of server-lsl-value-model (2026-09-27)
points: 3
refs: [server-lsl-memory-and-limits, server-lsl-value-model]
---

Context: [context/lsl.md](../context/lsl.md).

Content is tuned to the memory the reference charges per value: a
notecard reader stops at `llGetFreeMemory() < 2000`, a cache trims its
list when it nears the limit. If the fake grid charges less than Second
Life, a script runs out of memory **later** than it would there; if it
charges more, it runs out **where Second Life would not**. Either way a
test on the fake grid passes or fails for the wrong reason. So the cost
of each value has to be the reference's exact number — not an
approximation — and it has to be measured, since no source on this
machine models it (PyOptimizer and tailslide do not; OpenSim's
`YEngine/XMRHeapTracker.cs` is OpenSim's own model, not Second Life's).

Wanted:

- **A measuring script** run on aditi (Mono) that reads
  `llGetUsedMemory()` before and after creating each kind of value and
  reports the deltas: per type (`integer`, `float`, `string` by length
  and by character class — ASCII against multi-byte — `key`, `vector`,
  `rotation`, `list`) and per **context** — a global, a local, a
  function argument, a list element, the list itself by length. Repeat
  each measurement enough to see the allocation granularity rather than
  one noisy delta.
- **The numbers recorded in the repo** with how they were measured (the
  script is committed alongside), so the tests that use them quote a
  source.
- **A per-value cost function beside `Value`** in `sl-lsl-runtime`
  (`value.rs`), table-driven and tested against the recorded numbers,
  for [[server-lsl-memory-and-limits]] to account with. Also record the
  empty-script baseline and `llGetMemoryLimit()` default.

Acceptance: every type × context the measuring script covers has a
recorded aditi value, and the cost function reproduces each one exactly.
