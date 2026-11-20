---
id: test-conformance-upload-after-yengine-start-dropped
title: Quick script uploads to one item on OpenSim seemed to take each other's answers, or fail as "operation timed out"
topic: test
status: done
origin: server-lsl-luau-compiler (2026-11-01)
refs: [server-lsl-luau-compiler, gridspec-lsl-compile,
  test-conformance-script-compile-xengine-cold-start]
---

Context: [context/test.md](../context/test.md).

## Observation

The `script-compile` census's `luau` backend used to upload all its legs to
one script on the local OpenSim (YEngine), each upload about a second after
the last answer. In five runs on 2026-11-01, one upload of each run failed
on the uploader POST after about 0.5 s:

```text
upload request failed: error sending request for url
(http://127.0.0.1:9000/<id>): operation timed out
```

(the cause chain only shows since `sl_client_common::error_chain`). OpenSim
**did** take that upload: YEngine logged its `reduction errors` at the same
second and the item lists the new asset. The failing leg was not tied to its
source (a 6 KB one, a one-line one, a probe moved in front), and the one
time the order changed, **the next upload's answer carried the failed
upload's error** — `lsl_source`, valid LSL, came back `compiled` false with
`(1,6) Error: looking for open paren …`, the recursion source's error.

## Resolution (2026-11-20)

Neither half was OpenSim's. Both were the census's own misattribution, which
`fec83f3b` (2026-11-04, three days after these runs) had already fixed
without this item being closed: the census stopped waiting for an upload
after 30 s while the client gives each HTTP step 60 s
(`UPLOAD_STEP_TIMEOUT`), and the timeout then arrives as an
`AssetUploadFailed` that names no upload. At the time the census ran YEngine,
XEngine and then Luau; XEngine never answers `v3_again` (an upload over a
running script), so its timeout landed in the middle of the Luau legs, a
fixed time after the backend started, whatever the source in that slot. The
leg awaited then took it as its own refusal, its real answer came a moment
later, and the next leg took that one: the "next upload's answer carried the
failed upload's error". The "0.5 s" was measured from the wrong upload; the
reqwest timeout that raised it ran its full 60 s.

Reproduced on the local OpenSim with `wait_out_unended` disabled and the Luau
legs back on one item: XEngine's `v3_again` was sent at 18:43:20.863, given up
at 18:43:50.863, and the "refusal" of `luau_register_limit` logged at
18:44:20.863 — 60.000 s after `v3_again`, 0.41 s after `luau_register_limit`
— naming an uploader URL no Luau leg was given; `luau_recursion_limit` was
then "answered" in 0.509 s, where every real compile took 1.005 s.

With the wait in place, five runs uploading all 23 Luau legs to **one** item
a second apart answered every one, in 1.005 s each, with its own answer
(`lsl_source` compiled, the rest refused). YEngine does keep a compile's
errors in one slot per item (`XMREngine.cs`, `m_ScriptErrors`, emptied by
`GetScriptErrors`), so two saves of one item in flight at once could cross,
but saves that wait for each other's answers — all a viewer's editor sends —
do not. So the census's `Backend::fresh_each` for Luau on OpenSim, added for
this theory, is gone: the Luau legs share one script on every grid, and the
OpenSim table's `luau_*_fresh_item` rows say `false`. `book/src/gridspec/lsl.md`
§ Compiling says what a viewer sees.

## The theory this item started from (superseded)

YEngine posts a compile's errors into one slot per **item id**
(`XMREngine.cs`, `m_ScriptErrors[itemID]`), and the upload handler waits for
that slot and empties it (`GetScriptErrors`). Two compiles of one item close
together can therefore hand one answer the other's errors. Since
2026-11-01 the census gives every Luau leg on OpenSim a script of its own
(`Backend::fresh_each`), as it already did under XEngine, and no upload has
failed since.

## What was open (answered above)

- "operation timed out" is reqwest's own `TimedOut`, raised in 0.5 s by a
  request whose timeout is 60 s (`UPLOAD_STEP_TIMEOUT`), with no other
  timeout configured on the client. Where does it come from?
- A viewer that saves one script twice in quick succession on OpenSim may
  see one save fail and the other report the wrong errors. Does Firestorm?
  If so, it is OpenSim's, and belongs in `book/src/gridspec/lsl.md`
  § Compiling.

## Done when

The timeout's origin is known and either fixed, or noted here and in the
gridspec as OpenSim's behaviour, with what a viewer sees.
