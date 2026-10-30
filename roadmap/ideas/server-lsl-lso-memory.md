---
id: server-lsl-lso-memory
title: Run an LSO-targeted script in LSO's 16 KB, as Second Life does
topic: server
status: ideas
origin: server-lsl-memory-and-limits (2026-10-30)
points: 5
refs: [server-lsl-memory-and-limits, server-fake-grid-script-compile-on-upload]
---

Context: [context/lsl.md](../context/lsl.md).

`server-lsl-memory-and-limits` accounts every script as Mono's (or as the
OpenSim engine's), because the fake grid compiles every script from source
and keeps no target. Second Life still compiles a script uploaded with the
LSO target (`target: lsl2` in a probe header, the viewer's "Mono" checkbox
unticked) to LSO, and such a script reads its memory differently
(`book/src/gridspec/lsl.md` § Memory and limits): `llGetUsedMemory` is
always 16 384, `llGetMemoryLimit` 16 384, `llSetMemoryLimit` takes only
16 384, `llGetFreeMemory` is a low-water mark of a 16 KB budget, and it runs
out sooner — a doubled string at 8 192 characters, a one-integer recursion
at depth 763, every run.

Two things before it can be built:

- **The target has to reach the instance.** The upload's target is on the
  wire (`UpdateScriptTask`/`UpdateScriptAgent`'s `target`); the compile on
  upload ([[server-fake-grid-script-compile-on-upload]]) would keep it with
  the program, and the instance would account by it.
- **LSO's per-value costs are not measured exactly.** The memory probes saw
  LSO only through its low-water mark, which a list's growth moved unevenly
  ("about 30" an integer, the copy an append makes included; strings within
  an earlier mark). A `memory/sizes-lso` probe, reading `llGetFreeMemory`
  after each value from a freshly reset script, would give the exact costs
  the accounting needs; until then an LSO model would be a guess, which the
  memory task's rule ("exact, not approximated") forbids.
