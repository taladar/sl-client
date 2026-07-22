---
id: viewer-script-queue
title: Script queue — mass recompile / reset / run-state
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-object-selection-core, viewer-script-limits]
---

Context: [context/viewer.md](../context/viewer.md).

The batch script-operations floater: over a set of objects (the current
selection once [[viewer-object-selection-core]] lands; until then a picked
object), walk each task inventory, and for every script run one operation —
**reset**, **set running / not running**, or **recompile** (mono/LSO in the
reference; recompile re-saves the existing asset via the task-script caps) —
streaming per-script progress lines into the floater and a final
succeeded/failed count. The wire pieces all exist: task-inventory read
(`test-task-inventory`), `ScriptReset` / `SetScriptRunning`
(`test-script-running`), and the task-script update cap (`api-g9`).

Reference (Firestorm, read-only): `llfloaterscriptqueue`,
`floater_script_queue.xml`.

Builds on: task inventory + script-control protocol (`api-g9`).
