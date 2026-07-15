---
id: viewer-lsl-differential-testing
title: LSL differential testing — a tailslide diagnostics oracle
topic: viewer
status: done
origin: user request (2026-07); split from viewer-lsl-parser
blocked_by: [viewer-lsl-semantic-pass]
refs: [viewer-lsl-parser-recursion-stack-overflow]
---

Context: [context/viewer.md](../context/viewer.md).

The semantic pass ([[viewer-lsl-semantic-pass]]) is held to a real standard: a
false error on code the grid would happily compile is worse than no error at
all. What makes that bar reachable is that **Linden Lab's own front-end is
public** — `tailslide` reproduces the legacy bytecode **byte-for-byte**, so its
lexing, typing and implicit-conversion quirks are the real ones.

Use it as a **differential-testing oracle**: run tailslide and `sl-lsl` over a
corpus and diff the diagnostics, rather than hoping we matched by reading. A
local OpenSim serves the same role for grid-side truth. This is how the
no-false-positive bar is *proven* rather than asserted, and it becomes a
regression guard as the semantic rules grow.

Do not bind to tailslide as a library — a C++ FFI would only pay off if we
wanted to generate bytecode in the viewer, which the protocol does not need (the
simulator compiles). It is a test oracle, invoked out of process over the
corpus, not a runtime dependency of `sl-lsl`.

Prior art worth reading: **`secondlife/tailslide`** — Linden Lab's own
**MIT-licensed**, actively maintained LSL parser / AST / compiler (descended
from the community's `lslint`). It is the authority on real LSL semantics and
the edge cases a wiki grammar will miss.

## Done (2026-07-15)

`sl-lsl/tests/differential.rs` drives the oracle: it builds an `LslSyntax` table
from **tailslide's own `builtins.txt`** (so both sides share identical library
definitions — a diagnostic difference is a real semantic-rule difference, not a
library-version artefact), runs `tailslide --lint` out of process per script,
parses its findings, and diffs them against `sl-lsl`'s parse + `analyze` output.
The gate is the no-false-positive bar made concrete: **on any script tailslide
compiles cleanly, `sl-lsl` must report zero error-severity diagnostics** (a
violation fails the test); a *miss* on a script tailslide rejects is tolerated
(the pass is deliberately conservative). Warnings are never gated.

- Committed corpus: `sl-lsl/tests/corpus/{valid,error}/` (14 scripts). Result:
  6 clean-agree, 8 error-agree, **0 false positives, 0 misses**.
- The oracle **skips (passing)** unless `SL_LSL_TAILSLIDE_BIN` points at a built
  `tailslide`, so CI without the C++ toolchain stays green. `builtins.txt` is
  found via `SL_LSL_TAILSLIDE_BUILTINS` or derived from the binary path.
- `SL_LSL_DIFFTEST_CORPUS` points the harness at an arbitrary tree (e.g.
  tailslide's own `tests/scripts/`) to exercise the bar at scale.
- The pure helpers (builtins parser, lint-output parser, byte→line map) are
  unit-tested and run with no tailslide present.

The scale run against tailslide's 187-script corpus surfaced one pre-existing
parser bug (out of scope here, filed separately):
[[viewer-lsl-parser-recursion-stack-overflow]].
