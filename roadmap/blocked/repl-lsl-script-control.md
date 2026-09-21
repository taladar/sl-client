---
id: repl-lsl-script-control
title: sl-repl verbs for compiling, running and watching scripts
topic: repl
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 3
blocked_by: [server-fake-grid-script-engine-wiring]
refs: [test-lsl-differential-opensim, server-lsl-runtime-errors]
---

Context: [context/lsl.md](../context/lsl.md).

`sl-repl` is already the tool for "what did the grid do when I did
this?" — it drives a live session by hand, and `--script` turns it into
a one-shot wire probe. Every workflow in the LSL programme wants the
same thing for scripts, on either grid: put this source in that prim,
start it, touch it, and show me what it said.

Wanted, as ordinary REPL verbs (they work against the local OpenSim and
aditi exactly as much as against the fake grid, which is the point):

- `script put <object> <name> <file.lsl>` — upload over
  `UpdateScriptTask`, print the compile result: `compiled`, and each
  `ScriptCompileError` rendered through `sl-lsl`'s existing
  `render_grid_error` (which already produces a caret under the source
  line);
- `script list <object>` / `script state <object> <item>` —
  the task listing filtered to scripts, and the run state from
  `GetScriptRunning`;
- `script run|stop|reset <object> <item>`;
- `script watch` — tail everything a script can say: chat on every
  channel the session can hear, `DEBUG_CHANNEL`
  ([[server-lsl-runtime-errors]]), dialogs, permission questions,
  `LoadURL`s, with the object and the channel on each line;
- `script check <file.lsl>` — no grid at all: lex, parse, analyse and
  render the diagnostics locally, which `sl-lsl` can already do and
  nothing currently exposes on a command line.

That last one is worth having on its own merits — the semantic pass is
held to a no-false-positive bar and there is no way to run it over a
file today without writing a test.

The `--script` mode makes each of these usable unattended, which is what
[[test-lsl-differential-opensim]] needs to plant and drive a script on
the local OpenSim.

Acceptance: a `.lsl` file goes from the filesystem into a prim on the
local grid and starts, in one `sl-repl --script` run; a syntax error
prints a readable caret diagnostic; `script watch` shows a
`DEBUG_CHANNEL` run-time error with its line.
