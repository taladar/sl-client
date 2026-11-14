---
id: repl-lsl-script-control
title: sl-repl verbs for compiling, running and watching scripts
topic: repl
status: done
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

## From gridspec-lsl-live-differential-runner (2026-10-24)

The `lsl-probes` case (`sl-conformance/src/cases/lsl_probes.rs`) has the
live sequence these verbs need: `RezScript` then a listing poll until the
item shows under its name, `UploadScript` into the task item and the
`ScriptUploaded` compile result, and a watch that keeps owner-say and
`DEBUG_CHANNEL` lines from given prims. The live verbs need nothing from
the fake grid; only their fake-grid acceptance does.

## Done (2026-11-14)

- `sl-repl`'s `script_verbs.rs`: a `script` line parses to a `ScriptRequest`
  (`ReplAction::Script`) and resolves to a `ScriptVerb` at dispatch; the
  sans-I/O `ScriptDriver` carries it out, fed every event and a tick by both
  binaries, which hold further input lines (and the closing logout) while it
  is busy, so a `--script` run is sequential.
- `script put <object> <name> <file.lsl> [target=] [running=]` lists the prim,
  re-uses a script of that name or makes one (`RezScript`) and polls the
  listing until it shows, uploads (`UploadScript`) and prints `compiled` with
  each error through `ScriptCompileError::render` (the `render_grid_error`
  caret). `script list`, `script state|run|stop|reset` (an `<item>` by UUID or
  by name), `script watch [seconds|off]` (object chat by type, dialogs,
  permission questions, `LoadURL`, map teleports, object IMs) and
  `script check <file.lsl> [syntax=<LSLSyntax.xml>]`, also a
  `check-script` subcommand of both binaries that exits non-zero on errors.
  `help script` lists the verbs.
- Live: one `sl-repl-tokio --script` run on the local OpenSim put a file into
  an owned prim, which compiled and ran; listed, queried, watched its
  `DEBUG_CHANNEL` run-time error, put a broken file (two caret diagnostics),
  stopped and reset it. The same probe on the fake grid (`scripts` scene, a
  cube rezzed by the avatar) with both `sl-repl-tokio` and `sl-repl-bevy`.
- Not as the acceptance put it: "a `DEBUG_CHANNEL` run-time error with its
  line". No grid states a line in a run-time error — OpenSim's YEngine says
  `Division by Zero (script: … event: timer primID: … at: <133,133,21>)`, and
  Second Life (and the fake grid, imitating it) `Script run-time error` then
  `Math Error` — so the watch shows the error as the grid says it.
- A grid makes a script only in a prim the agent may edit and says nothing
  when it refuses (the fake grid too: `task_contents.rs`), so a put into
  someone else's prim ends in a timeout that names that cause.
- Unit tests in `script_verbs.rs` drive the put (make, poll, upload, render),
  a run by name, a missing name, a locate timeout, a timed watch and the
  local check. The book's REPL chapter has a Script verbs section.
