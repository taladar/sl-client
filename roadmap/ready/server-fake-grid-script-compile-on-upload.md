---
id: server-fake-grid-script-compile-on-upload
title: A script upload is accepted and never compiled
topic: server
status: ready
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 3
blocked_by: [server-lsl-compiler-ir, gridspec-lsl-compile]
refs: [viewer-lsl-editor-save-compile, server-fake-grid-script-engine-wiring]
---

Context: [context/lsl.md](../context/lsl.md).

`SimCaps::dispatch_caps_upload` answers a completed script upload with
`compiled: is_script.then_some(true)` and this comment:

> A script upload reports the compile result; the sim server "compiles"
> cleanly (a real grid would run the compiler).

So `UpdateScriptTask` and `UpdateScriptAgent` always succeed, the
`errors` list is always empty, and the fake grid's `uploads.rs` simply
repoints the inventory item at the new asset. The viewer's editor has a
fully built error path — [[viewer-lsl-editor-save-compile]] renders
`ScriptCompileError`s as a listed report with line and column, which it
notes is strictly better than Firestorm's `sscanf` — and **nothing on
this grid can ever exercise it**.

Wanted, once the compiler exists:

- compile the uploaded source on the way through, with the language the
  item declares (`ScriptLanguage`: Mono, LSL2, and the Luau token
  `sl-proto` models and Firestorm does not);
- answer `{ compiled: false, errors: [...] }` on failure, with
  `ScriptCompileError` line/column/message from the semantic pass and
  the lowering ([[server-lsl-compiler-ir]] owns the mapping), and
  **leave the item pointing at the old asset** — a failed compile does
  not replace the script, which is the behaviour the Save button's
  "your script did not save" case depends on;
- on success, replace the asset, honour the upload's
  `is_script_running` flag, and restart the instance
  ([[server-fake-grid-script-engine-wiring]]) — a save *is* the
  recompile, and the in-world script resets;
- a fixture with a deliberate syntax error, so the error path is
  exercised by a test and by anyone eyeballing the viewer.

Note the fake grid may also want a knob for the *other* real-grid
behaviour: Second Life silently drops a `RezScript` task write, which is
what `script-upload`'s Phase Z investigation is about. That belongs with the
crate's grid-imitation knobs (`imitates.rs`), not here — but the two
must not be confused when a test fails.

Acceptance: uploading a script with a syntax error over
`UpdateScriptTask` answers `compiled: false` with at least one error
naming the right line; the item still points at the previous asset; the
Bevy viewer's editor shows the error list; and a clean upload restarts
the running script.

## From gridspec-lsl-compile (2026-10-25)

Every answer each grid gives is in `book/src/gridspec/lsl.md` § Compiling and
held by the `script-compile` census; once the fake grid compiles, add the case
to the offline list for both flavours (its tables are per grid, keyed by
backend: `mono_*`/`lsl2_*`/`luau_*` on Second Life, `yengine_*`/`xengine_*`/
`luau_*` on OpenSim). What a flavour splits on, beyond the messages:

- **The error strings.** Second Life: one error, `(L, C) : ERROR : <msg>\n`,
  zero-based, a space after the comma, the column in bytes, pointing at the
  token after the fault; names resolve before types, so of a type mismatch
  and a later undeclared name only the name is reported. OpenSim YEngine:
  every error, `(L,C) Error: <msg>`, zero-based characters. The fake grid
  carries one compiler — `sl-lsl-runtime`'s `CompileError` already renders
  Second Life's form; the OpenSim flavour wants YEngine's (its default
  engine), and when to reject must follow each (YEngine compiles a function
  without a return on every path; Second Life refuses it).
- **What a failed compile does**: the broken source is stored and the
  script stopped — on Second Life `GetScriptRunning` answers not running, on
  OpenSim it is not answered (no instance).
- **`new_asset`**: Second Life a fresh asset id; OpenSim the **item's own
  id** for a task upload, the stored asset for an agent upload.
- **Agent-inventory uploads**: Second Life compiles them like a task's;
  OpenSim compiles none (`compiled` true whatever the source).
- **The `luau` target**: Second Life runs a Luau compiler (`:L: <msg>`,
  one-based, no column) and marks the item Luau; OpenSim ignores the target.
- Second Life hides a script's asset id in task listings and inventory
  fetches even to its owner; OpenSim lists it.
- XEngine's never-answered upload over a running script is an OpenSim bug,
  not a behaviour to imitate.
