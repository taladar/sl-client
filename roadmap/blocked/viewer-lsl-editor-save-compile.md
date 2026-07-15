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
