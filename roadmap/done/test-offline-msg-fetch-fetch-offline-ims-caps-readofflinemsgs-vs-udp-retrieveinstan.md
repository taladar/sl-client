---
id: test-offline-msg-fetch
title: fetch offline IMs (CAPS ReadOfflineMsgs vs UDP RetrieveInstantMessages
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 3 — Instant messaging & chat sessions `[both]`
---

Context: [context/test.md](../context/test.md).

`offline-msg-fetch` — fetch offline IMs (CAPS `ReadOfflineMsgs` vs UDP
`RetrieveInstantMessages`). `2av` (OpenSim now; Aditi deferred → Phase Z).
The store-and-forward counterpart of `im-1to1`: an IM sent while the recipient
is *offline* is stored by the grid and replayed as an offline message when the
recipient returns and fetches it. The flow needs the recipient absent at send
time, so the case drives a mid-run logout/login on the primary via new harness
support (`Session::disconnect` tears the run loop down but keeps the identity;
`Session::relogin` logs the same avatar back in, inheriting the OpenSim
"already logged in" retry that evicts the stale presence the disconnect
leaves, and *waiting out* the aditi login cooldown rather than bypassing it so
the same flow is safe on aditi). Sequence: the primary (recipient)
disconnects; the secondary (sender) IMs the now-offline primary; the grid
cannot deliver it, so it stores the message and replies to the sender with a
"… Message saved" system IM (from the recipient's id) — the synchronisation
point proving the message reached offline storage; the primary relogs in and
issues `Command::RetrieveInstantMessages` (the client never auto-requests
offline IMs), observing the marker replayed as an
`Event::InstantMessageReceived` with `offline == true`. The replayed IM is
matched on the sender's id and exact text, and `offline` distinguishes a
replayed stored message from a live one. Requires the "Offline Message Module
V2" on the test grid (SQLite has no offline-IM data provider, so its storage
points at the throwaway `os_groups` MariaDB on `:3307`); OpenSim has no
`ReadOfflineMsgs` capability, so this is the UDP path. `Session::relogin` is
now cooldown-aware, so the remaining Aditi work is only branching the fetch to
the CAPS `ReadOfflineMsgs` path and a second Aditi avatar (Phase Z). Green on
OpenSim; store-confirm ≈ 96 ms, fetch RTT ≈ 35 ms loopback. `[opensim]` only.
