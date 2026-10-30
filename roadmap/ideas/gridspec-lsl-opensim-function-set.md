---
id: gridspec-lsl-opensim-function-set
title: Measure which library functions OpenSim's engines compile
topic: gridspec
status: ideas
origin: server-fake-grid-script-compile-on-upload (2026-10-30)
refs: [server-fake-grid-script-compile-on-upload, gridspec-lsl-compile,
  server-lsl-library-unserved-functions]
---

Context: [context/gridspec.md](../context/gridspec.md).

The fake grid's OpenSim flavour compiles against Second Life's library table
less the two functions the `script-compile` census saw OpenSim refuse
(`llClearExperiencePermissions`, `llGetExperienceList`:
`sl_lsl_runtime::Dialect::OpenSim`). Which of the rest OpenSim lacks is not
measured. The local OpenSim's `bin/ScriptSyntax.xml` lists 425 `ll*`
functions against the 524 the table carries; the 99 missing include the
experience, pathfinding, linkset-data, key-value, combat and newer
environment and material functions — but a syntax document is not the
engine, and that file may be older than OpenSim 0.9.2.1's `LSL_Api`.

A census in the shape of `script-compile`'s `UNSERVED` legs — one upload per
function, a call in a `touch_start` nobody raises — on YEngine and XEngine
would settle it, and give the OpenSim dialect a measured table
(`Dialect::has_builtin`). The constants want the same treatment.
