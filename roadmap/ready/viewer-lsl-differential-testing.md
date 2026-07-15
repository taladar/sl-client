---
id: viewer-lsl-differential-testing
title: LSL differential testing — a tailslide diagnostics oracle
topic: viewer
status: ready
origin: user request (2026-07); split from viewer-lsl-parser
blocked_by: [viewer-lsl-semantic-pass]
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
