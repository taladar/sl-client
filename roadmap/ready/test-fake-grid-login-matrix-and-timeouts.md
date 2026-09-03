---
id: test-fake-grid-login-matrix-and-timeouts
title: The login refusals and handover timeouts nothing tests
topic: test
status: ready
origin: doing test-audit-fake-grid-conformance-grid (2026-09-03)
points: 3
refs: [test-audit-fake-grid-conformance-grid, viewer-login-tos]
---

Context: [context/testing.md](../context/testing.md).

Three behaviours the fake grid has served since it was written, that nothing
has ever exercised. [[test-audit-fake-grid-conformance-grid]] listed them and
left them; now that `Grid::Fake` exists there is somewhere obvious to put them.

- **The login refusal matrix.** `FakeGridBuilder::gates` (`runtime.rs`) and
  `AccountConfig::mfa` (`accounts.rs`) between them can refuse a login for
  every reason a real grid does: a pending ToS acceptance, a critical message,
  an already-logged-in presence, and an MFA challenge. The client decodes each
  into a distinct `LoginRejectKind` / `Error::MfaChallenge`, and
  `sl-conformance`'s own `connect_and_spawn` has a retry branch keyed on one of
  them — which nothing proves. This wants its own harness rather than a
  `GridTest`, because each case needs a *differently built* grid and the
  conformance context assumes a login that succeeded: a set of `#[tokio::test]`s
  beside `tests/offline.rs` driving `sl_client_tokio::Client::connect` directly.
  It is also the offline half of [[viewer-login-tos]], which has to render what
  these produce.
- **The teleport arrival timeout.** `teleport.rs` gives a client a budget to
  send its `CompleteAgentMovement` into a destination and, when it elapses,
  retires the destination and tells the client `timeout_tport`. That branch is
  what the teleport-progress watchdog exists for
  ([[viewer-teleport-flow-progress]]), and no test has ever reached it. The grid
  can be built with a `handover_timeout` short enough to make it deterministic.
  `CROSSING_ARRIVAL_TIMEOUT` is the same branch on the crossing path.
- **A capability fetch with a `Range` header.** The asset caps honour one (it
  is how a viewer pulls a single mesh LOD out of an asset without transferring
  the rest), and no test sends one. `Command::FetchMesh { byte_range }` is the
  client end.

Acceptance: a login is refused for each reason and the client names the reason;
a teleport whose client never completes its movement leaves the agent where it
was with the destination torn down; a ranged cap fetch returns the range and
not the asset.
