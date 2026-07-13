---
id: test-offline-msg-fetch-aditi
title: Offline message fetch — [aditi] variant
topic: test
status: ready
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

Add the `[aditi]` variant of the `offline-msg-fetch` case
(`[[test-offline-msg-fetch]]`, already green `[opensim]`). The multi-avatar
blocker is gone: `credentials.aditi.toml` now carries the `secondary` avatar
alongside the `primary`, so the `2av` flow can run on Second Life.

The Aditi variant fetches over the CAPS `ReadOfflineMsgs` path (SL delivers
stored offline IMs there rather than the UDP path).

Flip the case's `grids()` to include `Grid::Aditi`, run it live on Aditi
(respect the per-avatar login cooldown — do not `--force`), confirm any
SL-specific routing the doc comment notes, and commit the record.
