---
id: protocol-audit-inventory-fetch-deadlock
title: Lost folder replies permanently deadlock the background inventory crawl
topic: protocol
status: bugs
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
