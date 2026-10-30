---
id: server-fake-grid-script-compile-on-upload
title: A script upload is accepted and never compiled
topic: server
status: done
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

## From server-lsl-library-unserved-functions (2026-10-30)

The library table now carries `llPointAt`, `llStopPointAt`,
`llRemoteLoadScript`, `llClearExperiencePermissions` and
`llGetExperienceList`, which Second Life compiles and serves no document
for. Both OpenSim engines refuse the two experience functions as undefined
(`book/src/gridspec/lsl.md` § Running, re-uploading, sizes, targets), so
`FakeOpensim` should compile against a table without them.

## Implemented (2026-10-30)

The task's first notes predate the census: a failed compile does **not** keep
the old asset on any grid — every grid stores the broken source and stops
the script — so the fake grid does that.

- **`sl-proto`**: step 2 of a script upload stores the source, then hands
  it to the driver as `CapsDispatch::ScriptUpload(UploadedScript)` (the
  request as `UploadedScriptItem::Task`/`Agent`, the source, the stored
  asset); the driver answers with `SimCaps::script_upload_response`
  (`ScriptUploadAnswer`: `compiled`, the errors, `new_asset`, the item, the
  flags). `UploadedScript::compiled_unchecked` is the answer of a driver
  with no compiler.
- **`sl-lsl-runtime`**: `compile_for(source, Dialect)`. `Dialect::OpenSim`
  is YEngine's: it compiles a function that can end without a `return`, and
  has no `llClearExperiencePermissions` or `llGetExperienceList`.
- **`sl-fake-grid`**: `script_compile` and
  `ImitatedGrid::script_compile_policy`. Second Life: one error (a syntax
  error, else the first name not defined or declared twice, else the first),
  `(L, C) : ERROR : <msg>\n`, at the token Linden's parser stood on, the
  column in bytes and a tab two; stopped after a failed compile; the stored
  asset as `new_asset`; agent uploads compiled. OpenSim: YEngine's every
  error, `(L,C) Error: <msg>`, at the fault (an unknown label or state at
  its `jump`/`state`), characters; `fetched source empty asset://<id>` for
  an empty source; no instance after a failed compile (`TaskInventory::
  uncompiled`, `GetScriptRunning` unanswered); the item's id as a task
  upload's `new_asset` and no item; agent uploads not compiled. A clean
  compile sets the script running as the upload asked. The messages are the
  compiler's own.
- **The Luau target** on the Second Life flavour marks the item Luau and is
  answered `compiled` false with an error saying the fake grid has no Luau
  compiler ([[server-lsl-luau-compiler]]); the OpenSim flavour ignores it.
- **Fixture**: "Broken Script" in the stock scripted box
  (`stock_broken_script_item`, `STOCK_BROKEN_SCRIPT_BODY`).
- **Tests**: the compiler's dialect rules; the fake grid's positions for
  every census leg; an end-to-end upload on both flavours (broken: the
  error at the grid's place, the run state after; clean: compiled and
  running); and in `sl-conformance`'s `script-compile` tests, the fake
  grid's compiler run over every source the census uploads and held to the
  measured `compiled`, error count and first-error position, with ten
  `KNOWN_DEVIATIONS` named and explained (YEngine's follow-on syntax errors
  and unterminated string, LSL2's 16 KB, one column of Second Life's
  unterminated string); and a viewer e2e stage on both fake flavours
  (`e2e_objects`'
  `a_failed_compile_is_listed_in_the_editor_on_each_fake_flavour`):
  a prim's new script saved from the editor with a missing expression shows
  "Compilation failed" and the error at line 1, column 39, and saved fixed
  shows "Saved" with no error listed.

Not done, and where it went:

- **Restarting the instance on a save** — the fake grid runs no script;
  [[server-fake-grid-script-engine-wiring]] has the note, with running the
  census offline (it listens to the scripts it uploads).
- **Hiding a script's asset id** on the Second Life flavour:
  [[server-fake-grid-hide-script-asset-ids]].
- **Which library functions OpenSim lacks** beyond the two measured:
  [[gridspec-lsl-opensim-function-set]].
- **The compile target reaching the instance** (LSO): noted on
  [[server-lsl-lso-memory]].
