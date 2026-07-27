---
id: viewer-lsl-editor-save-compile
title: LSL editor save — upload/compile round-trip and error list
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-lsl-script-editor
blocked_by: [viewer-lsl-editor-widget]
refs: [viewer-lsl-diagnostics, viewer-prim-inventory-editing]
---

Context: [context/viewer.md](../context/viewer.md).

Wire the editor widget ([[viewer-lsl-editor-widget]]) to the grid: save →
upload/compile, surface the result, and support the object-contents workflow
(open a script from a prim's inventory, edit, save back).

Almost all the protocol is already done: upload (`UpdateScriptAgent` /
`UpdateScriptTask`), target selection (we support `Lsl2` / `Mono` / **`Luau`** —
Firestorm only has two), run / reset / query, and **`ScriptCompileError`,
already parsed into line, column and message** — which is strictly better than
Firestorm, whose error navigation is a `sscanf` that assumes a `(line, col)`
prefix. So a **clickable error list that jumps the caret** (go-to-line into the
widget) is nearly free; render each entry through the shared diagnostic span
machinery ([[viewer-lsl-diagnostics]]).

**The one hard fact: upload *is* the compile — there is no dry run.** SL has no
compile-without-save, so a save stores the asset *and* resets the in-world
script's state; a live vendor or attachment misbehaves while it recompiles. The
fast, side-effect-free feedback lives in the local checker
([[viewer-lsl-semantic-pass]]); this task owns the *authoritative* grid
round-trip, which the user triggers deliberately. Carry `is_script_running`
through the upload so a save does not silently start or stop the script.

Opening a script from a prim's contents needs the task-inventory surface from
[[viewer-prim-inventory-editing]]; agent-inventory scripts need only the
inventory already present.

**Skip the Firestorm preprocessor in v1.** It is boost::wave (a full C
preprocessor) plus custom sugar, it changes what is actually stored in-world,
and it forces a source-map so compile errors still point at your real lines. If
it ever happens, it must ship the line map and the round-trip encoding.

Reference (Firestorm, read-only): `llpreviewscript`,
`llfloaterscriptdebug`, `fslslpreproc`.

## Implemented so far (2026-07-27)

The save/compile round-trip is built on the **plain** multi-line text field
(the same `EditableText` the notecard editor uses), the way
[[viewer-notecard-editor]]'s non-rich half was — so the editor exists and works
today, while the *rich* affordances still wait on the widget. This task stays
`blocked/` because the remaining items each need
[[viewer-lsl-editor-widget]].

Done:

- **`sl-client-bevy-viewer/src/edit_script.rs`** — a dedicated floater
  (`EditScriptPlugin`, id `script-editor`) opened by the inventory **Open**
  action (routed from `inventory_properties`, the `InventoryType::Script` arm,
  now `previewable`) and by **double-clicking a script in the Object Contents
  floater** (`edit_contents`'s `openItem`). It fetches the source
  (`AssetType::ScriptText`), decodes it as UTF-8 (lossy on a corrupt asset), and
  shows it **read-only** — a note, a monospace non-editable block, no Save —
  when it is not modifiable, or **editable** (the field) otherwise. A
  `ScriptSource` carries agent-vs-task provenance so Save writes back to the
  right capability, exactly as the notecard editor does.
- **Save = compile.** Save uploads over `UpdateScriptAgent` (agent inventory) or
  `UpdateScriptTask` (a script inside a prim) via one `Command::UploadScript`;
  the simulator compiles and the result (`ScriptUploaded`: `compiled` + parsed
  `ScriptCompileError`s) is surfaced as a status line plus a **listed compile
  diagnostics report** (`Line L, column C: message`, via a Fluent arg). The
  compile backend follows the item's language flag (`ScriptLanguage` → Luau vs
  Mono; OpenSim ignores the token).
- **Run state preserved.** A task script's **Running** checkbox is seeded from a
  `RequestScriptRunning` query on open and its value is carried through the
  upload's `is_script_running`, so a save does not silently start or stop the
  script. The checkbox is save-coupled (it does not send a separate
  `SetScriptRunning` on toggle — a save *is* the recompile that applies it).
- **Permissions.** Agent editability is the item's own `MODIFY` bit; task
  editability is the two-level rule (object modify **and** item modify; a
  redacted nil task `asset_id` can't be opened). Four-locale Fluent keys
  (`script-*`) and a `script-editor` gallery specimen swept by `ui_test`.

Still to do (each needs [[viewer-lsl-editor-widget]] or its own task):

- **Clickable error list that jumps the caret** — needs the widget's
  go-to-line; today the diagnostics are listed, not clickable.
- **Syntax colour, gutter/line numbers, brace match, a monospace *editing*
  font** — the editable field is the plain Sans widget with one whole-buffer
  style; colour is [[viewer-lsl-editor-highlight]] and the rest is the widget
  itself. (The read-only block is already monospace.)
- **No experience is set on a task upload** (`experience: None`), and the
  Firestorm preprocessor stays out of v1 as planned.
