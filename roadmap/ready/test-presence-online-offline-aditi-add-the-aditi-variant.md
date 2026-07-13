---
id: test-presence-online-offline-aditi
title: Presence online/offline — [aditi] variant
topic: test
status: ready
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

Add the `[aditi]` variant of the `presence-online-offline` case
(`[[test-presence-online-offline]]`, already green `[opensim]`). The
multi-avatar blocker is gone: `credentials.aditi.toml` now carries the
`secondary` avatar alongside the `primary`, so the `2av` flow can run on Second
Life.

Flip the case's `grids()` to include `Grid::Aditi`, run it live on Aditi
(respect the per-avatar login cooldown — do not `--force`), confirm any
SL-specific routing the doc comment notes, and commit the record.
