---
id: test-experience-permissions
title: request / set experience permission
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 18 — Experiences `[aditi] 1av`
---

Context: [context/test.md](../context/test.md).

`experience-permissions` — request / set experience permission. `1av`.

Implemented as `sl-conformance/src/cases/experience_permissions.rs`. Exercises
both directions of the experience-preference caps: the **request** half issues
`RequestExperiencePermissions` (a `GetExperiences` GET) and asserts the read
capability answers with a `{ allowed, blocked }` pair (either list may be
empty); the **set** half round-trips one experience's preference over
`ExperiencePreferences` (`SetExperiencePermission`) — record the fixture's
current preference (`allowed`/`blocked`/`neither`), flip it to a distinct one,
assert the write reply's full lists moved the id into the target list, then
restore the original preference and assert the lists returned to their starting
classification. Non-destructive by construction (it puts the avatar back), and
mutation runs only against a stable `experience` fixture — never a discovered
one.

Both live records are `partial`: stock OpenSim ships no experience module (no
fixture → nothing to read or set, recorded partial up front), and on aditi the
read half ran green (`GetExperiences` answered in ~0.33 s with empty lists — the
test avatar relates to no experience) but the set/restore round-trip was skipped
because no `experience` fixture is configured. To capture a green set
round-trip, add `experience = "<aditi-experience-id>"` to `fixtures.aditi.toml`
— same fixture hook the `experience-info` case uses.
