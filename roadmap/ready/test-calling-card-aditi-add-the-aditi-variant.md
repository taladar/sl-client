---
id: test-calling-card-aditi
title: Calling card — [aditi] variant
topic: test
status: ready
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

Add the `[aditi]` variant of the `calling-card` case (`[[test-calling-card]]`,
already green `[opensim]`). The multi-avatar blocker is gone:
`credentials.aditi.toml` now carries the `secondary` avatar alongside the
`primary`, so the `2av` flow can run on Second Life.

Flip the case's `grids()` to include `Grid::Aditi`, run it live on Aditi
(respect the per-avatar login cooldown — do not `--force`), confirm any
SL-specific routing the doc comment notes, and commit the record.
