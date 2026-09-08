---
id: test-fake-grid-login-matrix-and-timeouts
title: The login refusals and handover timeouts nothing tests
topic: test
status: done
origin: doing test-audit-fake-grid-conformance-grid (2026-09-03)
points: 3
refs: [test-audit-fake-grid-conformance-grid, viewer-login-tos]
---

Done (2026-09-08). Three behaviours the fake grid had served since it was
written that nothing had ever exercised — the login refusals, the handover
arrival timeouts, and a ranged capability fetch.

**The login refusal matrix** is `sl-conformance/tests/login_refusals.rs`, ten
tests, one grid built per case. It uses no registry: a conformance case starts
from a login that *succeeded* (a `TestContext` is assembled out of live
sessions), so no case can be the one that asserts a login was declined. What is
asserted is not that `sl_wire::LoginServer` decides correctly — its own tests
pin that — but that each decision survives the XML-RPC round trip and reaches a
**client** as a reason it can act on:

- a wrong password and an unknown account, asserted **indistinguishable**;
- `tos` and `critical`, refused with the text to display and cleared by the
  same login re-sent with `agree_to_tos` / `read_critical`;
- `presence`, classified retryable from the message rather than the code;
- an MFA challenge, answered by the one-time code *and* by echoing the
  `mfa_hash` alone ("remember this device"), and not raised for a wrong
  password;
- a redirect, followed to the grid that answers it, and abandoned at the hop
  bound when it loops.

One thing the matrix turned up for [[viewer-login-tos]]: `LoginRequest::new`
leaves `agree_to_tos` and `read_critical` **set**, so the stock request sails
through both gates and this client has never seen either. A viewer that has to
*show* the terms sends its first attempt with them cleared, which is what the
tests model.

`FakeGridBuilder::stale_presence` is new, and is what made the last bullet of
the original task testable: it refuses **one** login as already-logged-in and
the refusal itself clears it — OpenSim's login service evicts the ghost on its
way to reporting it, which is the whole reason a driver may retry such a
rejection at all. It is checked in `login_endpoint::respond` after
`LoginServer::rejection` passes, so a wrong password does not spend the
eviction. With it, `sl-conformance`'s own retry branch in `connect_and_spawn` —
the only production code in this workspace that reacts to an `AlreadyLoggedIn`
— is exercised end to end, addressed as `Grid::Opensim` because that is what
the branch is keyed on.

**The teleport arrival timeout** was already covered, by two tests written
after this task was — the `a_teleport_that_never_arrives_*` pair in
`client_end_to_end.rs` assert both cleanup shapes: the destination this
teleport opened is taken back down, a borrowed neighbour's is not, and the
agent stays root in the region it never left. What was
missing was the same branch on the **crossing** path, and
`a_crossing_that_never_arrives_leaves_the_neighbour_alone` now reaches
`Error::CrossingTimedOut`.

What none of the three can assert is the client *receiving* `timeout_tport`:
holding a handover open deterministically means stopping the client, and a
stopped client cannot report what it was told. Every candidate for keeping it
running — a tiny event channel, a one-millisecond budget — is a race dressed as
a test. Reaching it would need the grid to be able to swallow one
`CompleteAgentMovement` on request; worth doing when
[[viewer-teleport-flow-progress]] needs to see the watchdog fire, not before.

**A ranged capability fetch** is two tests in `client_end_to_end.rs`: a span
that is neither at the start of the asset nor the whole of it comes back the
right length *and* the right bytes over both asset surfaces (`GetMesh2` and
`ViewerAsset`), and a range starting past the end is a `416` the client reports
as a failed transfer rather than an empty asset — the distinction a progressive
fetcher walks a mesh with.
