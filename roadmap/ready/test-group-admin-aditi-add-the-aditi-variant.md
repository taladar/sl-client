---
id: test-group-admin-aditi
title: Group admin — [aditi] variant
topic: test
status: ready
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

Add the `[aditi]` variant of the `group-admin` case (`[[test-group-admin]]`,
already green `[opensim]`). The multi-avatar blocker is gone:
`credentials.aditi.toml` now carries the `secondary`/`tertiary` avatars
alongside the `primary`, so the `2av` flow can run on Second Life.

Needs a pre-made Aditi group the primary owns; the full role/roster assertion
wants the `tertiary` avatar as a second member (`3av`).

Flip the case's `grids()` to include `Grid::Aditi`, run it live on Aditi
(respect the per-avatar login cooldown — do not `--force`), confirm any
SL-specific routing the doc comment notes, and commit the record.
