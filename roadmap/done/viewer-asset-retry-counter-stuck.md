---
id: viewer-asset-retry-counter-stuck
title: Asset fetch retry counter stuck at 1/6 — permanent failures retry forever
topic: viewer
status: done
origin: graphics-tab live verification (2026-08-12), local grid run logs
---

Context: [context/viewer.md](../context/viewer.md).

First live sighting of the failure-edge retry
(`viewer-asset-failure-edge-retry`) in the wild, and it misbehaved: a texture
that permanently fails (`fetch/decode failed: asset not found`, id
`e97cf410-…` on the local grid) logged `fetch failed; scheduling retry 1/6 in
0.5s` **~60 times in a single ~60 s session** — the attempt counter never
reached `2/6`, `gave up after` never fired, and the asset was re-fetched about
once per second indefinitely.

Re-sighted unchanged on 2026-08-26, on the run that live-verified
[viewer-ecs-idiom-audit](viewer-ecs-idiom-audit.md). That audit's `F3` change
(the asset stores now publish their pipeline figures instead of being read
directly) made the permanently-deferred texture easy to *see* in the panel's
`def` column — it did not cause it.

## The cause

The suspicion in the original report was right, and the mechanism is precise.

Both asset stores re-issue a due retry like this:

```text
poll_textures / poll_meshes         request_from / request
─────────────────────────────       ─────────────────────────────────
state.issued()   ← parks the        …
   accumulated attempt count        self.retry.remove(&id);   ← drops it
manager.request_from(…)  ─────────▶ "a fresh explicit request
                                     supersedes any pending retry"
```

`RetryState::issued` exists precisely to carry the attempt count across a
re-issue, and its doc comment describes this exact failure mode. But the
re-issue then goes through the *same* request entry point a fresh consumer
request uses, and that entry point's first act is to clear the id's retry
bookkeeping. The count it was just handed is discarded before the fetch is even
spawned. The next failure calls `RetryState::after_failure(previous: None)`,
which yields attempt 1 — forever.

The entry point could not tell the two callers apart, and guessing from the
state was not safe either: a parked entry can legitimately outlive its re-issue
when `request_from` returns early into `pending_default` (the `GetTexture` cap
is not up yet), so "a parked entry means this is the re-issue" would wrongly
preserve bookkeeping for a later unrelated request and inflate the deferred
count for good.

**Why the existing tests did not catch it.** `asset_retry`'s unit tests all
passed while this was live, including one named
`issued_preserves_count_so_reissue_escalates_and_gives_up`. They test
`RetryState` in isolation, and `RetryState` was correct throughout — the store
around it threw the state away. A policy type cannot test the caller that
ignores it.

## The fix

The caller says which kind of request it is, rather than the callee guessing:
`RetryDisposition::{Supersede, Keep}` in `sl-viewer-platform::asset_retry`,
threaded through `TextureManager::request_from` and a new private
`MeshManager::request_with` (the public `MeshManager::request` keeps its
signature and means `Supersede`).

- **`Supersede`** — the four public consumer entry points (`request_boosted`,
  `request_face`'s two arms, `request_server_bake`). A fresh consumer asking for
  an id starts its attempt budget over, which is the behaviour the old comment
  described and intended.
- **`Keep`** — the two paths where the store re-issues a request it already
  owns: the backoff loop's own re-issue, and `retry_pending_default` /
  `retry_pending` releasing a request that was held back for a capability that
  has now arrived. Neither is a new consumer, so neither should reset the
  budget.

The type is what does the work: every call site must now decide, so a new caller
cannot silently inherit the wrong answer.
`a_store_reissue_loop_escalates_to_exhaustion` walks the failure → park →
re-issue → failure loop the stores run and asserts the counts come out `1..6`
and then give up, which is the level the old tests were missing.

## Live verification

Re-run against the local grid on 2026-08-26 with the same permanently-failing
texture, grepping the run log for its id:

```text
13:07:50.919  fetch failed; scheduling retry 1/6 in 0.5s
13:07:50.998  fetch failed; scheduling retry 1/6 in 0.5s
13:07:51.623  fetch failed; scheduling retry 2/6 in 1.0s
13:07:52.750  fetch failed; scheduling retry 3/6 in 2.0s
13:07:54.871  fetch failed; scheduling retry 4/6 in 4.0s
13:08:00.758  fetch failed; scheduling retry 5/6 in 8.0s
13:08:34.732  fetch failed; gave up after 6 attempts
```

Six `scheduling retry` lines with doubling backoff, then the give-up — against
roughly sixty lines all reading `1/6` before. **Not one further mention of the
id in the remaining 2 m 23 s of the run**, where previously it was re-fetched
about once a second for as long as the session lasted.

The two `1/6` lines are correct, not a residual of the bug: a second consumer
(another face wanting the same texture) issued a fresh request in the window
between the first failure and its re-issue, and a fresh consumer request is
`Supersede` by design — it restarts the budget. The sequence escalates cleanly
from there. A request arriving while the fetch is actually in flight cannot do
even this, because `request_from` returns early on `inflight` before reaching
the bookkeeping at all.

The memory note about the failure-edge retry being "committed but unverified"
can now drop that caveat: the retry fires, escalates, and terminates.
