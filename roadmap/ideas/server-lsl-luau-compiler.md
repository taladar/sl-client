---
id: server-lsl-luau-compiler
title: Compile the Luau target as Second Life does
topic: server
status: ideas
origin: server-fake-grid-script-compile-on-upload (2026-10-30)
refs: [server-fake-grid-script-compile-on-upload, gridspec-lsl-compile]
---

Context: [context/lsl.md](../context/lsl.md).

Second Life compiles an upload with the `luau` target with its Luau compiler
and marks the item Luau (`book/src/gridspec/lsl.md` § Running, re-uploading,
sizes, targets): a syntax error is `:2: Expected identifier when parsing
expression, got '='`, a one-based line and no column; LSL source under the
target compiles, since `default { state_entry() … }` parses as Lua, and fails
as it starts.

The fake grid has no Luau compiler. Its Second Life flavour marks the item
Luau and answers every such upload `compiled` false with
`:1: the fake grid has no Luau compiler; the source was stored, not compiled`
(`sl_fake_grid::script_compile::compile_source`), so a viewer's Luau path
is exercised but never sees a clean Luau compile. The OpenSim flavour
ignores the target, as OpenSim does.

What it would take: a Luau parser (the `luau` crate wraps the reference
implementation, C++ behind FFI; a pure-Rust front end such as `full_moon`
parses Luau syntax) and Second Life's error wording, measured beyond the
census's three legs. Running Luau is a separate and much larger thing.
