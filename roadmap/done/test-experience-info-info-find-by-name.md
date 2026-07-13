---
id: test-experience-info
title: info / find by name
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 18 — Experiences `[aditi] 1av`
---

Context: [context/test.md](../context/test.md).

`experience-info` — info / find by name. `1av`.

Implemented as `sl-conformance/src/cases/experience_info.rs`: settles on an
*anchor* experience (a configured `experience` fixture, or one discovered from
the agent's owned / administered / created / admitted relationships), resolves
it over `GetExperienceInfo` (asserting a real, name-bearing record whose
`public_id` matches), then searches for that name over `FindExperienceByName`
and confirms the search capability answers.

Both live records are `partial`: stock OpenSim ships no experience module, and
the aditi test avatar relates to no experience with no `experience` fixture
configured (the discovery queries fired and came back empty). To capture a green
info round-trip, add `experience = "<aditi-experience-id>"` to
`fixtures.aditi.toml` (a stable experience the avatar can resolve) — the fixture
hook was added for exactly this.
