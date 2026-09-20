---
id: server-fake-grid-script-compile-on-upload
title: A script upload is accepted and never compiled
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 3
blocked_by: [server-lsl-compiler-ir]
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
