---
id: test-offline-msg-fetch-aditi
title: Offline message fetch — [aditi] variant
topic: test
status: ready
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

Add the `[aditi]` variant of the `offline-msg-fetch` case
(`[[test-offline-msg-fetch]]`, green `[opensim]`). The case's `grids()`
now includes `Grid::Aditi` and per-grid routing was added, but the live
aditi run **does not yet pass** — it needs one more iteration.

Groundwork already committed:

- The stored-reply wait accepts both grids' wording (OpenSim
  "Message saved.", SL "User not online - message will be stored and
  delivered later.").
- The fetch branches per grid: OpenSim uses the legacy UDP
  `RetrieveInstantMessages`; SL uses the `ReadOfflineMsgs` capability
  ([`Command::RequestOfflineMessages`], waited for after the relogin).

**Open problem (2026-08-12 live):** the store-confirm half passes (SL
stores the IM), but after the recipient relogs in the fetch finds nothing
— the `ReadOfflineMsgs` GET returns an **empty** reply and no
`ImprovedInstantMessage` arrives in the post-handshake drain window. The
likely cause is timing: Second Life **auto-delivers** stored offline IMs
as ordinary UDP messages shortly after login (and that drains the store),
but they arrive *after* `RegionHandshakeComplete`, so the current
handshake-drain misses them and the subsequent cap GET sees an already-
empty store. Next step: after the relogin, drain for a longer bounded
window that captures a late auto-delivered offline IM (offline flag set,
`ImDialog::Message`) *and* the cap reply, resolving on whichever carries
the marker; verify against the `ReadOfflineMsgs` LLSD record field names
(cross-checked against Firestorm `llimprocessing.cpp`
`requestOfflineMessagesCoro`).
