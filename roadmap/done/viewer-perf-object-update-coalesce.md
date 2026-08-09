---
id: viewer-perf-object-update-coalesce
title: Coalesce repeated object updates in the pending queue
topic: viewer
status: done
origin: unbounded-frame-work survey (2026-08-09, performance branch)
refs: []
---

Context: [context/viewer.md](../context/viewer.md).

Every `ObjectAdded` / `ObjectUpdated` event carries a **full merged
snapshot** — `sl-proto`'s `upsert_object` keeps a per-object cache and
re-emits the whole object, preserving previously merged properties — so
building geometry from anything but the newest queued snapshot is pure
waste. The `PendingObjectEvents` backlog was strict FIFO with no per-object
coalescing: under a rez-burst backlog, N updates for one object meant N
`apply_object` calls (and up to N geometry builds).

Now the queue holds per-object upsert **markers** with the snapshots
out-of-line: an update arriving for an object whose newest queued event is
a still-undrained upsert replaces that queued snapshot in place — one build
from the newest data, at the original queue position, so linkset
root-before-child and upsert-before-remove ordering are untouched. An
upsert queued behind a remove for the same id never merges across it
(remove → re-add replays in order; a per-id queued-removes count guards
the merge). Zero added latency — unlike a time debounce, this only merges
work that was already deferred. Unit-tested
(`pending_object_events_coalesce_repeated_upserts`).

Possible follow-up (measure first): the inline no-backlog path still
rebuilds per event, so an already-built object receiving rapid scripted
shape updates rebuilds each time; a per-object rebuild rate cap would
bound that.
