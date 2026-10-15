---
id: idiomatic-audit-lsl-scope-duplication
title: LSL scope resolution is implemented twice, with nothing pinning them equal
topic: idiomatic
status: ready
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`sl-lsl-lsp/src/navigation.rs` (`Resolver`, from line 171) and
`sl-lsl/src/semantics.rs` (`Analyzer`, from line 355) are two independent
implementations of LSL's scope rules, method for method: `collect_symbols`,
`run`, `param_scope`, `resolve_variable`, `walk_*` / `analyze_*`.
`navigation.rs`'s own doc says it "walks the tree exactly the way the semantic
pass does".

Any shadowing or scoping fix applied to one silently diverges from the other,
and there is **no test pinning them equal**.

Smaller, same cause: `state_name` is **byte-identical** between
`navigation.rs:588`, `semantics.rs:1035` and a third copy in
`sl-lsl-lsp/src/symbols.rs:350`, and `collect_labels` /
`collect_labels_into` / `collect_labels_stmt` are the same traversal written
twice (differing only `HashSet<String>` vs `HashMap<String, Range>`). Neither is
`pub` in `sl-lsl`, so the LSP had no choice.

Scope: make the scope pass public in `sl-lsl`, parameterised over what it
collects, and have the LSP consume it. `navigate.rs` vs `navigation.rs` is a
clean layering split (LSP-facing vs pure) and should stay.

Related: `sl-viewer-asset-editors` still does **not** depend on `sl-lsl`, and
`edit_script.rs` does no LSL analysis (a plain multiline field, the grid's
compile errors listed, a Save), while `sl-wire/src/lsl_syntax.rs` re-exports
the `sl-lsl` symbol table and `sl-lsl-lsp` can be embedded in-process
(`Connection::memory`, `sl-lsl-lsp/src/lib.rs`). That wiring is tracked by
[[viewer-lsl-editor-widget]] and [[viewer-lsl-editor-highlight]]; a public
scope pass is what the latter would consume.
