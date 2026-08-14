---
id: test-kick-user
title: observe KickUser handling
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 19 — Error handling & recovery `[both]`
---

Context: [context/test.md](../context/test.md).

`kick-user` — observe `KickUser` handling. `1av`. **Green on OpenSim
(as the estate-owner avatar), partial on aditi.** Implemented 2026-08-12.

The one kick a single avatar can deterministically provoke is the **estate
self-kick**: `EstateOwnerMessage`/`kickestate` naming the sender itself as
prey (`Command::KickEstateUser`). OpenSim's
`HandleEstateTeleportOneUserHomeRequest` (kick variant) answers with a
`KickUser` and then closes the agent. The case asserts the session surfaces
[`Event::Kicked`] with a non-empty reason and the kicked agent equal to
self, followed by [`Event::Disconnected`] with `DisconnectReason::Kicked`
(the predicate resolves on the disconnect event itself so an unexpected
reason is reported, not swallowed). Records the reason and the
request-to-kick latency.

Grid findings:

- **`kickestate` needs estate-manager/owner rights and is refused
  silently** (`CanIssueEstateCommand` fails → `return`, no reply, no log).
  On the local grid the estate owner of all four regions is the dedicated
  estate-owner account — not the primary test avatar — so like the other
  estate cases the OpenSim run must use `--avatar estate-owner`. A run as
  the primary avatar times out (kept as an honest fail in the local record
  history).
- **OpenSim's kick reason is "You have been kicked out"** (the
  `kickestate` path), observed live; the kick and the disconnect arrive
  ~50 ms after the request.
- **On Second Life the test avatar holds no estate power**, so no 1av
  action can provoke a `KickUser` — the aditi run records that honestly as
  partial. SL's *other* kick path (same account logging in elsewhere sends
  the old session a `KickUser`) needs a second concurrent login of one
  account, which the harness deliberately does not do.

**Decode is covered in-process**: the client ↔ `SimSession` round-trip
`kick_user_reaches_client_and_disconnects`
(`sl-proto/tests/sim_session.rs`) already drives a `KickUser` through the
server-side encoder and asserts the typed [`Event::Kicked`] carrier and
the terminal disconnect transition.

**New client code:** none — `Command::KickEstateUser`, `Event::Kicked`,
and the kicked-close transition all pre-existed; the case only consumes
them.
