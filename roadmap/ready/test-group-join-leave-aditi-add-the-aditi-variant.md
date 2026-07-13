---
id: test-group-join-leave-aditi
title: Group join/leave — [aditi] variant
topic: test
status: ready
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

Add the `[aditi]` variant of the `group-join-leave` case
(`[[test-group-join-leave]]`, already green `[opensim]`). The multi-avatar
blocker is gone: `credentials.aditi.toml` now carries the `secondary` avatar
alongside the `primary`, so the `2av` flow can run on Second Life.

Needs a pre-made, open-enrolment Aditi group to join and leave.

Flip the case's `grids()` to include `Grid::Aditi`, run it live on Aditi
(respect the per-avatar login cooldown — do not `--force`), confirm any
SL-specific routing the doc comment notes, and commit the record.
