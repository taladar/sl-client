---
id: viewer-asset-retry-counter-stuck
title: Asset fetch retry counter stuck at 1/6 — permanent failures retry forever
topic: viewer
status: bugs
origin: graphics-tab live verification (2026-08-12), local grid run logs
---

Context: [context/viewer.md](../context/viewer.md).

First live sighting of the failure-edge retry
(`viewer-asset-failure-edge-retry`) in the wild, and it misbehaves: a
texture that permanently fails (`fetch/decode failed: asset not found`,
id `e97cf410-…` on the local grid) logs `fetch failed; scheduling retry
1/6 in 0.5s` **~60 times in a single ~60 s session** — the attempt
counter never reaches `2/6`, `gave up after` never fires, and the asset
is re-fetched about once per second indefinitely.

Suspected cause: the per-asset retry state (attempt count) is reset on
each failure — e.g. the retry re-enqueues through a path that
re-registers the request fresh (new `asset_retry` entry) instead of
resuming the existing one, so the backoff never escalates and the
budget never exhausts.

## Task

Make the attempt counter actually persist across retries of the same
asset id: escalating backoff (`1/6` → `6/6`), then `gave up after`
and no further fetches for that id (until a genuine re-request, e.g.
cache eviction or a new consumer). Verify live by grepping a run's log
for the same id: expect exactly six escalating `scheduling retry N/6`
lines and one `gave up after`, not a steady once-per-second stream.

The retry *firing* live is now confirmed (this sighting) — the memory
note about "committed but unverified" can drop that caveat once this
bug is fixed.
