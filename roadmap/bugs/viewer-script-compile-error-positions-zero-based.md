---
id: viewer-script-compile-error-positions-zero-based
title: Grid compile-error positions are zero-based and shown as one-based
topic: viewer
status: bugs
origin: server-lsl-compiler-ir (2026-09-27)
refs: [server-lsl-compiler-ir, viewer-lsl-editor-save-compile]
---

Context: [context/viewer.md](../context/viewer.md).

Second Life's compiler counts the `(line, column)` of an error **from zero** —
measured on aditi (2026-09-27): a function missing a return, whose closing brace
is on line 5, column 1, answers
`(4, 0): ERROR : Not all code paths return a value`. The reference agrees: the
reference viewer hands them straight to `LLTextEditor::setCursor`
(`LLScriptEdCore::onErrorList`, `llpreviewscript.cpp`), which is zero-based, and
OpenSim subtracts one from its own positions before sending them for the same
reason (`Compiler.cs`: "The Second Life viewer's script editor begins counting
lines and columns at 0, so we subtract 1").

`sl-proto`'s `ScriptCompileError` documents `line` as "the 1-based source
line", and the script editor (`sl-viewer-asset-editors/src/edit_script.rs`,
`error_line`) shows the numbers as they arrive — so every grid error names the
line *above* the one at fault, and anything that turns the position into a
span (`sl_lsl::render_grid_error` takes a one-based line) points one line and
one column early.

Fix at the parse: `ScriptCompileError::parse` should add one to both numbers
(saturating) so the fields mean what their docs say, with a test quoting a
real Second Life error string; the editor then shows the right line. Verify
live on aditi with a deliberate error on a known line. `sl-lsl-runtime`'s
`CompileError` already renders the zero-based wire form, so the fake grid
will send what Second Life sends.
