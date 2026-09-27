---
id: test-e2e-live-verify-sweep
title: Turn pending live-verify checks into end-to-end tests
topic: test
status: ideas
origin: viewer automation design (2026-09-28)
points: 5
blocked_by: [test-e2e-pilot-suite]
---

Context: [context/automation.md](../context/automation.md).

Several finished features are unit-verified but still carry a "check it
live" step that only a person can do today — interactive checks in the
viewer, UI states after a real login, behaviour between two avatars. Once
the pilot suite shows the mechanism holds, sweep those pending checks: each
becomes an end-to-end test (fake grid where it can, live grid where it
must), and the "pending live verify" note on the feature is retired.

Scope is the list itself; promote to ready once it has been gathered and
sized.
