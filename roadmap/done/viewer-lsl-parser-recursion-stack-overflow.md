---
id: viewer-lsl-parser-recursion-stack-overflow
title: LSL parser overflows the native stack on deeply-nested input (no recursion-depth guard)
topic: viewer
status: done
origin: found by the differential-testing oracle (2026-07-15)
refs: [viewer-lsl-parser-tree, viewer-lsl-differential-testing, viewer-lsl-semantic-false-positives]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-lsl`'s recursive-descent parser ([[viewer-lsl-parser-tree]]) has **no
recursion-depth guard**, so a pathologically deep expression nesting recurses
until the native thread stack is exhausted and the process aborts:

```text
thread 'differential_oracle_matches_tailslide' has overflowed its stack
fatal runtime error: stack overflow, aborting
```

This contradicts the parser's own contract ("Total and error-tolerant: it
always returns a tree, never panics"): a stack overflow is an **abort**, not a
recovered error, and it takes the whole process down.

Found by [[viewer-lsl-differential-testing]]: pointing the oracle at
tailslide's own `tests/scripts/` corpus (`SL_LSL_DIFFTEST_CORPUS`) crashed on
`parserstackdepth2.lsl` / `parserstackdepth3.lsl`, which nest ~10 000 `(` deep.
The committed corpus is shallow and unaffected, so the differential test itself
stays green.

**Grid-truthful target behaviour** — tailslide does not crash; it emits a clean
diagnostic and mirrors what the real grid does:

```text
ERROR:: ( 2,10002): [E10024] Parser stack depth exceeded; SL will throw a
                              syntax error here.
```

So the fix is a bounded recursion depth in the Pratt/expression parser (and the
statement/block nesting) that, on exceeding the limit, records a recovered
`ParseError` ("expression nesting too deep") at that span and stops descending —
never a native overflow. Pick a limit at or below the grid's own so we never
accept what SL would reject; cross-check the exact threshold against
`tailslide`'s guard and (if reachable) a live upload.

## Done (2026-07-15)

`Parser` now carries a shared `depth` counter and a `MAX_DEPTH = 128` ceiling.
The three recursion spines — `parse_statement`, `parse_expr_bp` and
`parse_prefix` — each check the counter on entry and, past the ceiling, record a
recovered `ParseError` ("statement/expression nests too deeply") and return an
error node instead of recursing. `parse_expr_bp` needs its own guard because a
long right-associative chain (`a = b = c = … = z`) recurses through *it*, not
`parse_prefix`. A companion `MAX_ERRORS = 200` cap keeps a pathological input
(thousands of unbalanced brackets) from producing an error per token; parsing
still runs to completion, guaranteed to terminate by the existing anti-stall
progress checks in the block/list/arg loops.

Ceiling rationale — a debug build on a 1 MiB stack survives ~340 levels
(measured), a 2 MiB test thread ~680, and release far more; 128 leaves ~2.6–5×
margin while sitting far above any nesting a real LSL script reaches (it runs in
64 KiB). Regression tests in `tests/parse.rs`: five 5 000-deep inputs (parens,
blocks, right-assoc, casts, unary) terminate with a bounded error list, and a
30-deep nesting still parses cleanly (no false positive).

**Accepted limitation vs tailslide:** tailslide's bison parser uses a *heap*
stack and only trips at ~10 000 depth (its `parserstackdepth2.lsl`, ~9 994 `(`,
compiles cleanly there; `parserstackdepth3.lsl` at ~10 004 fails). A
recursive-descent parser on a native stack cannot match ~10 000 without
overflowing, so between 128 and ~10 000 `sl-lsl` reports an error where
tailslide does not. This is a deliberate, safe divergence on input no 64 KiB
script contains; matching tailslide exactly would require an iterative
(explicit-stack) parser rewrite, which is not warranted. The scale differential
run over tailslide's corpus therefore lists `parserstackdepth2.lsl` as a "false
positive" by design.

The same scale run (now that it no longer crashes) surfaced a separate batch of
genuine semantic-pass false positives, tracked as
[[viewer-lsl-semantic-false-positives]].
