---
id: test-conformance-upload-after-yengine-start-dropped
title: Quick script uploads to one item on OpenSim take each other's answers, or fail as "operation timed out"
topic: test
status: bugs
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

## What explains half of it

YEngine posts a compile's errors into one slot per **item id**
(`XMREngine.cs`, `m_ScriptErrors[itemID]`), and the upload handler waits for
that slot and empties it (`GetScriptErrors`). Two compiles of one item close
together can therefore hand one answer the other's errors. Since
2026-11-01 the census gives every Luau leg on OpenSim a script of its own
(`Backend::fresh_each`), as it already did under XEngine, and no upload has
failed since.

## Still open

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
