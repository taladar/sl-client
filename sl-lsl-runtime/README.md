# sl-lsl-runtime

The runtime half of running **Linden Scripting Language** (LSL) scripts on
`sl-fake-grid`: values, lowering, a bytecode VM, the event queue and the
library, behind a `Host` trait the grid implements. The design record is the
book chapter `book/src/simulator/lsl-engine.md` — read it before adding to
this crate.

Like `sl-lsl`, which it builds on, the crate is **I/O-free, Bevy-free and
synchronous**. It depends on neither `sl-wire` nor `sl-proto`: turning a
script's `llSay` into a `ChatFromSimulator` is the grid's job.

## What is here so far

The **value model** — LSL's seven types and the rules that make them
observable from a script:

- `Value` and its flat list element, `Element` (a list cannot hold a list, so
  the type cannot express one);
- the **cast matrix** on actual values, including the lenient string parsers
  (`(integer)"0x1A"`, `(integer)"  12abc"`, `(vector)"<1,2,3"`);
- the **operators** — 32-bit wrapping integers, `f32` floats, vector dot and
  cross products, Linden-order rotation composition, list `+`, and the run-time
  `Math Error` on division by zero;
- **comparison** (a list compares by length, `!=` of two lists is the length
  difference) and **boolean context** (what a condition treats as false);
- **formatting**: Mono's seven-significant-digit float printing and the five-
  versus six-decimal vector forms.

The *compile-time* half — which combinations are legal and what type each
produces — is `sl_lsl::types`, and a test here runs every combination through
both and checks they agree.

And the **library table** (`library`): every `ll*` function (428), constant
(709) and event (38), generated at build time, with a typed dispatch that makes
an implementation at the wrong arity or argument types a compile error, and a
coverage test that prints `implemented / stubbed / missing` and fails only if a
function falls back from the committed baseline (`src/library/coverage.txt`).

And the **compiler** (`compile`, modules `compiler` and `bytecode`): source to a
stack bytecode `Program` — names resolved to slots and indices, implicit
conversions spelled out as casts, a source map on every instruction — or the
compile errors a grid would send — in the same situations, with messages that
name what is wrong. It is held to tailslide as an oracle by
`tests/compile_corpus.rs`: set `SL_LSL_TAILSLIDE_BIN` to a built `tailslide`,
and `SL_LSL_DIFFTEST_CORPUS` to its `tests/scripts/` for the scale run.

And the **VM** (`vm`): a script `Instance` runs in slices of at most a budget of
instructions and can stop between any two — for a sleep, an exhausted budget, a
state change or a reset — and resume there on a later tick; an `Engine` serves a
region's instances round-robin per tick under a per-script and a region-wide
instruction budget. Time is counted in ticks and budgets in instructions, never
read from a clock. A run-time error stops the one script and never panics the
host. Library calls reach the world only through the `Host` trait.

## Vendored library definition

`keywords_lsl_default.xml` is Linden Lab's own `LSLSyntax` document — the file
the viewer ships as `indra/newview/app_settings/keywords_lsl_default.xml` —
copied byte-for-byte from the Firestorm tree at commit `684bc1d1` (a merge of
`secondlife/viewer`, 2024-06-21; the file's last upstream change is
`secondlife/viewer#1744`, "Fix missing LSL constant INVENTORY_SETTING").
SHA-256 `d665088d59981453e986306508bd8ed0d2886f57be0e1537eea905c7a4280cd8`.
It is part of the viewer source, licensed LGPL-2.1 like this workspace.

It was chosen over the two other local candidates because it is the only one
that carries everything the table needs: tailslide's `builtins.txt` has no
energy, sleep or descriptions, and OpenSim's `bin/ScriptSyntax.xml` has no
energy or sleep either and adds OpenSim's `os*` functions. It is also already
the format the grid serves, so the table and the served document cannot
disagree about what a field means.

Two of its string constants are spelled in a notation rather than literally,
and `build.rs` decodes them: `EOF` (served escaped twice, `\\n\\n\\n`, meaning
three newlines) and the `JSON_*` markers (served as `U+FDD0` … `U+FDD8`). The
override for `EOF` records the raw text it expects, so an updated document that
changes it stops the build rather than being misread. To update the file,
replace it and let the tests say what moved: a test decodes the same file
through `sl-wire`'s independent `LSLSyntax` decoder and compares every
function, event and constant with the generated table.

The oracles are LSL PyOptimizer's `lslopt/lslbasefuncs.py` and its
`unit_tests/expr.suite` expected outputs, which were measured against Second
Life; each test names the one it quotes.
