---
id: test-conformance-script-compile-xengine-cold-start
title: script-compile fails its XEngine legs on a freshly started OpenSim
topic: test
status: done
origin: server-lsl-library-unserved-functions (2026-10-30)
refs: [gridspec-lsl-compile, test-unbounded-waits-in-test-harnesses]
---

Context: [context/test.md](../context/test.md).

Fixed (2026-11-04). Two causes, both the census's own:

- **The start line was heard in a fixed 5 s window.** XEngine answers the
  upload and compiles afterwards, and its first compile after a start takes
  longer. The census now waits for the valid script's `start` line after
  `v1`, `v3_running` and `v3_again` (up to 90 s, the probe runner's compile
  bound), then listens 5 s more. Every grid measured says that line, so a
  warm run loses nothing.
- **An unanswered upload's late end was taken for a later one's answer.**
  The census stops waiting for an upload after 30 s, but the client gives
  each HTTP step 60 s and then reports `AssetUploadFailed` ("error sending
  request … operation timed out"), which names no upload. Whatever upload
  was awaited at that moment took it as its own refusal, while OpenSim had
  in fact compiled it (its log shows the compile). In the first cold run
  after the window fix, that hit the first Luau leg, `luau_break_outside`,
  exactly 60 s after XEngine's unanswered `v3_again`. The census now waits
  out an unanswered upload's end (65 s after it was sent) before the next
  upload. `multibyte_column`'s refusal below was the same timeout, and so
  were the refusals the book had put down to OpenSim running out of HTTP
  handlers; the gridspec row is corrected. A script of its own per XEngine
  leg stays, since XEngine answers no upload to a running script.

The census passed on OpenSim restarted seconds before the run (2026-11-03,
23:58), and on aditi. The fake flavours run it in `tests/offline.rs`.

## Observation

The first `sl-conformance run --grid opensim script-compile` after
`systemctl --user start opensim.service` (2026-10-30, 14:24) failed; the
same binary passed nine minutes later on the warm grid. Every mismatch was
an XEngine leg:

- `xengine_v1_heard` was `nothing` and `xengine_syntax_after_heard` was
  `v1 start`: v1's `state_entry` line arrived after the 5 s `LISTEN`
  window, so it was booked to the next leg;
- after that `syntax_after_running` read `true` (measured: `no answer`)
  and v1 answered the pings it never answers on a warm grid;
- `multibyte_column`'s upload was refused (`error sending request`), the
  HTTP-handler exhaustion the census works around with a fresh script per
  leg.

The likely cause is XEngine's first C# compile after start-up taking longer
than the census allows, which then shifts every later answer.

## Done when

The census passes on a freshly started OpenSim — by waiting for v1's start
line instead of a fixed window, or by warming XEngine first — or the cause
is shown to be something else and noted here.

## From server-lsl-opensim-shared-lists (2026-11-01)

XEngine's compile is slow on a warm grid too, not only after a start:
OpenSim's log (`[LOGHTTP] Slow handling of … POST`) timed the
`persistence/lists` upload at 33, 39 and 34 seconds in three runs in a row —
two on a grid that had been up and running probes for hours, the third right
after a restart of `opensim.service` — and the same log shows uploads of 32 s
on 2026-10-27. The
`lsl-probes` runner waited 30 s and failed all three; it now waits up to 90 s
for a compile (`COMPILE_TIMEOUT`). The `script-compile` census still answers
within its own fixed windows, which this length overruns.
