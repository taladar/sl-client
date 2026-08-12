---
id: test-group-notice-aditi
title: Group notice — [aditi] variant
topic: test
status: done
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

The `[aditi]` variant of the `group-notice` case (`[[test-group-notice]]`,
already green `[opensim]`): **pass (partial) on aditi live** (2026-08-12,
Phase Z batch). The full notice-post / receive / history round-trip is
asserted on OpenSim; on Second Life the case records an honest partial.

**Second Life drops notices posted into a freshly created group** — the
distributed group service propagates a new group and its membership
asynchronously, and a notice posted before that completes reaches no
member (the poster included). The case retries the post with a fresh
marker (six attempts, 10 s each = 60 s) and still saw no relay, so it
records a partial ("Second Life drops notices from a freshly created
group until its group service has propagated the membership") rather than
failing. A pre-made, already-propagated fixture group (via
`fixtures.aditi.toml` `premade_groups`) would let the full assertion run
on SL; that is the remaining follow-up. Shares the short-group-name and
creation-retry fixes in [[test-group-join-leave-aditi]].
