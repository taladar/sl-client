---
id: protocol-audit-inventory-fetch-deadlock
title: Lost folder replies permanently deadlock the background inventory crawl
topic: protocol
status: done
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/protocol.md](../context/protocol.md).

`FolderState::Fetching` is written at `sl-proto/src/session/inventory.rs:611`
(the background batch) and `:624` (an on-demand request). The only transitions
back out are `:418` (`invalidate_folder`, gated on `FolderState::Loaded`) and
`:724` (the login-time cache reconcile). **Nothing resets `Fetching` to
`Unknown` on failure.**

`next_fetch_batch` (`:580`) computes
`slots = max_in_flight.saturating_sub(self.fetching_count())` and returns early
when `slots == 0`. So once `INVENTORY_FETCH_MAX_IN_FLIGHT` replies are lost or
fail, `slots` is pinned at zero for the rest of the session, those folders are
never re-fetched, and `fully_loaded` (`:635`) never returns true.

Scope: a per-entry deadline that returns a stalled folder to `Unknown` and
allows a bounded number of retries. This is pure state-machine logic with no
I/O; the three tests that touch `Fetching` (`:950`, `:951`, `:982`) only assert
the happy flip. A test that hands out `max_in_flight` folders and never replies
catches it immediately.

## Fixed (2026-08-27)

Every folder flipped `Fetching` now carries a **deadline** and a failure count,
on the same `FolderEntry` as its state and child index (no side map to desync).
`Inventory::expire_stalled_fetches(now)`, run from `Session::handle_timeout`
alongside the other timed prunes, returns any folder still unanswered after
`INVENTORY_FETCH_TIMEOUT` to `Unknown` — the next sweep re-issues it — and the
earliest armed deadline is merged into `Session::poll_timeout` so an otherwise
idle shell still wakes for it.

The two constants mirror the reference viewer, which has both halves of this:
`INVENTORY_FETCH_TIMEOUT` = 30 s is `LLViewerInventoryCategory`'s non-AIS
`FETCH_TIMER_EXPIRY`, past which `getFetching()` reports the category fetchable
again; `INVENTORY_FETCH_MAX_ATTEMPTS` = 10 is
`LLInventoryModelBackgroundFetch`'s `MAX_FETCH_RETRIES`, past which a category
is dropped from the fetch queue rather than re-queued.

The give-up needed a state of its own — leaving the folder `Unknown` would have
the crawl re-issue it forever, and leaving it `Fetching` is the deadlock. So
`FolderState` gains `Failed`: it holds no in-flight slot, the scheduler never
picks it, and `inventory_fully_loaded` keeps reporting the tree incomplete
(honestly — the folder is not loaded). It is terminal for the *scheduler* only.
`mark_folder_fetching` — the caller-driven path — grants a `Failed` folder a
fresh budget, and both runtime shells now issue an on-demand fetch for a
`Failed` folder a consumer queries exactly as they do for an `Unknown` one, so
opening the folder in the UI retries it. Every other state keeps its running
count, which is what stops the UDP library-fetch path (`next_fetch_batch` then
`request_folder_contents`, which re-marks an already-`Fetching` folder) from
resetting the budget on itself.

`next_fetch_batch` and `mark_folder_fetching` (and their `Session` wrappers)
therefore take `now`; both shells already had one at the call site.

The CAPS fetch was the practical source of the stall: `fetch_inventory` in both
shells is `let Ok(response) = … else { return }` at every step, so a POST that
fails drops the request silently with the folder left `Fetching`.

Three tests: the model-level one the task asks for (hand out the whole budget,
lose every reply, watch the slots come back and the sweep re-issue), a
retry-budget one that walks a never-answered folder to `Failed` and then proves
an explicit request restarts its budget, and a session-level one driving it
through `handle_timeout`.
