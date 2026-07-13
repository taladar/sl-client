---
id: test-chat-invite-accept-decline-aditi
title: Chat invite accept/decline — [aditi] variant
topic: test
status: ready
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

Add the `[aditi]` variant of the `chat-invite-accept-decline` case
(`[[test-chat-invite-accept-decline]]`, already green `[opensim]`). The
multi-avatar blocker is gone: `credentials.aditi.toml` now carries the
`secondary` avatar alongside the `primary`, so the `2av` flow can run on Second
Life.

The Aditi variant exercises the CAPS `ChatSessionRequest` POST and its reply
roster (SL routes ad-hoc chat over CAPS, not the UDP path OpenSim uses).

Flip the case's `grids()` to include `Grid::Aditi`, run it live on Aditi
(respect the per-avatar login cooldown — do not `--force`), confirm any
SL-specific routing the doc comment notes, and commit the record.
