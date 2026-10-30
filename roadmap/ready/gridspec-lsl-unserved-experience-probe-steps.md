---
id: gridspec-lsl-unserved-experience-probe-steps
title: Probe llClearExperiencePermissions and llGetExperienceList outside an experience
topic: gridspec
status: ready
origin: server-lsl-library-unserved-functions (2026-10-30)
refs: [server-lsl-library-unserved-functions, gridspec-lsl-experience,
  gridspec-lsl-refreshed-table-probe-steps, server-lsl-lib-experience]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Second Life compiles both under Mono and LSL2 (`script-compile`, 2026-10-30),
though no `LSLSyntax` document it serves lists them; the library table carries
them since [[server-lsl-library-unserved-functions]]. The probe corpus test
compiles every probe against that table, which is why
[[gridspec-lsl-experience]] and [[gridspec-lsl-refreshed-table-probe-steps]]
left them out of `experience/no-experience`.

## Discover

What a script compiled into no experience gets from
`llClearExperiencePermissions` (for its owner, another agent, `NULL_KEY`)
and `llGetExperienceList` (the same agents, before and after the owner
granted the experience of a stranger's object, if one is at hand) — the
return value, any `DEBUG_CHANNEL` line, any event. In a probe of its own if
OpenSim compiles neither, as `experience/sit-on-link` did.

## Document

`book/src/gridspec/lsl.md` § Experiences and the key-value store.

## Fake grid

What [[server-lsl-lib-experience]] implements for the two.
