---
id: viewer-lsl-parser-recursion-stack-overflow
title: LSL parser overflows the native stack on deeply-nested input (no recursion-depth guard)
topic: viewer
status: bugs
origin: found by the differential-testing oracle (2026-07-15)
refs: [viewer-lsl-parser-tree, viewer-lsl-differential-testing]
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
