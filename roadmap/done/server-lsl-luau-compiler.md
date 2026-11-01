---
id: server-lsl-luau-compiler
title: Compile the Luau target as Second Life does
topic: server
status: done
origin: server-fake-grid-script-compile-on-upload (2026-10-30)
refs: [server-fake-grid-script-compile-on-upload, gridspec-lsl-compile,
  test-conformance-upload-after-yengine-start-dropped]
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

## Done (2026-11-01)

- **The reference compiler, not a second parser.** Second Life's Luau errors
  are `luau_compile`'s own output — a zero byte, then `:<line>: <message>`
  for the first parse error or a compile error alike — so the fake grid runs
  that compiler through `mlua` (feature `luau`, vendored Luau 0.709, safe
  API; the workspace forbids `unsafe`) and answers `":" + message`
  (`script_compile::luau_errors`). `full_moon` would have parsed Luau but
  never said what Second Life says.
- **Measured beyond the three legs**: the census's `luau` backend now uploads
  twenty-three sources (`luau_legs`): Luau's own syntax and type annotations, an
  unclosed block, malformed string and number, `goto`, `<const>`,
  `break`/`continue`/`...` out of place, a stray `end`, an unfinished comment,
  two errors, CR LF, multi-byte text, `@native`, the empty source, 201 locals
  and 1100 nested parentheses, and records the item's language after each. Aditi
  answered **every one in upstream Luau's words**, never type-checks
  (`local x: number = "text"` compiles), and marks the item Luau whether it
  compiled or not; OpenSim reads each as LSL. Both tables hold the answers, and
  `the_fake_grid_compiles_the_luau_legs_as_measured` holds the fake grid to both
  grids' — no known deviation.
- **Beyond the plan, on the OpenSim side**: the new legs showed two places
  the fake grid's YEngine imitation placed an error elsewhere than YEngine.
  YEngine tokenizes before it parses and answers every unknown character
  (`` ` ``, `\`, `$`, …) alone (`yengine_unknown_characters`); and it faults a
  statement keyword outside every body on the word itself
  (`is_yengine_statement_keyword`). Both hold for LSL sources too.
- **Beyond the plan, the clients**: an upload failure now carries reqwest's
  cause chain in both runtimes (`sl_client_common::error_chain`), which is
  how the next item was told apart from a stale connection.
- **Found, not solved**: on OpenSim, quick uploads to one script took each
  other's answers or failed as `operation timed out` (YEngine keeps a
  compile's errors in one slot per item). The census now gives each Luau leg
  on OpenSim a script of its own; the timeout's origin is
  [[test-conformance-upload-after-yengine-start-dropped]].
- **Still out of scope**: running Luau — the LSL source under the target
  compiles and Second Life fails it as it starts (`lua_script:3: attempt to
  call a nil value`); the fake grid runs nothing it compiled.
