---
id: test-e2e-sweep-relog
title: End-to-end tests for what must survive a relog
topic: test
status: ready
origin: test-e2e-live-verify-sweep (2026-09-30)
points: 5
refs: [test-e2e-live-verify-sweep, test-e2e-stage]
---

Context: [context/automation.md](../context/automation.md).

The pending live checks whose point is *after a relog*: a stage viewer logs
out and back in within one test, keeping its directories. The stage has no
relog yet; adding one (`Stage::relog(label)`: log out, start a new App or
process on the same `ViewerPaths` root, wait until it settles) is the first
part of this task, and each test below uses it.

- [[viewer-ui-floater-persist-geometry]]: move and resize the inventory,
  relog, and the geometry is restored.
- [[viewer-notification-persistence]]: a notice left unanswered comes back
  after a relog; answered, it is gone after the next.
- [[viewer-derender-blacklist]]: a derendered object stays derendered
  after a relog; a temporary entry clears on a teleport.
- [[viewer-contact-set-presence-extras]]: the settings window's checkboxes
  and reply editors persist, and the online-first order holds.
- [[viewer-preferences-alerts-tab]]: a suppressed confirmation auto-responds,
  the group-notice gate and inventory auto-accept hold across a relog.
