---
id: test-p0-01
title: cases/common.rs (or a support module) with:
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 0 — Test utilities & helpers (do first)
---

Context: [context/test.md](../context/test.md).

Pure code, no new avatars. Build the shared scaffolding so later cases stay
short and consistent.

`cases/common.rs` (or a `support` module) with: standard timeout
constants, a `send-then-await-matching-event` combinator, a grid-gating
helper, and metric-name helpers. (`sl-conformance/src/support.rs`:
`REGION_TIMEOUT`/`REPLY_TIMEOUT`/`LONG_TIMEOUT`, `send_then_wait`,
`is_opensim`/`is_aditi`, `secs_metric`/`count_metric`.)
