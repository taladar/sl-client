---
id: test-phase-z-deferred-01
title: Provision a 2nd Aditi avatar; add it to credentials.aditi.toml
topic: test
status: done
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

Done: `credentials.aditi.toml` now carries a `secondary` (and `tertiary`) Aditi
avatar alongside the `primary`. The first case to exercise a `2av` Aditi flow
against them is `money-transfer` (Phase 15), which passes green on Aditi with
the two accounts. The remaining Aditi `2av`/`3av` variants are collected in
[deferred-03](test-phase-z-deferred-03-add-aditi-variants-of-the-deferred-cases-as-the-avatars-land.md),
now unblocked.

Collects every multi-avatar case that needs additional **Aditi** avatars, so it
did not block Phases 1-19. Each item is the Aditi variant of a case already
listed in its functional-area phase.

Provisioning (now complete):

- A **2nd Aditi avatar** unblocks all Aditi `2av` cases: chat (Phase 2),
  IM/sessions (3), friends/presence (4), give-inventory (5), group join/leave
  /notice/proposal (6), teleport offer (12), money transfer (15). Existing
  follow-up: see memory `sl-conformance-harness` and `SL_REPL_ROAD_MAP.md` E3.
- A **3rd Aditi avatar** is needed ONLY for the `3av` cases:
  `conference-roster` (Phase 3) and the multi-member `group-admin` roster
  assertion (Phase 6).

OpenSim `2av`/`3av` equivalents do NOT wait on Aditi — the local secondary
`avatar2` already exists, extra console avatars are cheap, and the Phase 0
tertiary-avatar harness support is the only prerequisite for OpenSim `3av`.

Provision a 2nd Aditi avatar; add it to `credentials.aditi.toml`.
