---
id: gridspec-lsl-refreshed-table-probe-steps
title: Probe steps the refreshed library table unblocks
topic: gridspec
status: ready
origin: server-lsl-library-table-refresh (2026-10-26)
refs: [server-lsl-library-table-refresh, gridspec-lsl-experience,
  gridspec-lsl-http-url]
---

Context: [context/gridspec.md](../context/gridspec.md).

The probe corpus test compiles every probe against the library table, so
two probes left steps out until the table knew their functions. It does
now:

- `experience/no-experience`: `llSitOnLink` (from
  [[gridspec-lsl-experience]]). `llClearExperiencePermissions` and
  `llGetExperienceList` are still in no served document
  ([[server-lsl-library-unserved-functions]]).
- `http/outbound`: `HTTP_USER_AGENT`, `HTTP_ACCEPT` and
  `HTTP_EXTENDED_ERROR` (from [[gridspec-lsl-http-url]]) — the user agent
  and the `Accept` header set the documented way, and the extended error
  body asked for.

Run both on aditi and both OpenSim engines (OpenSim lacks the constants, so
the steps go in a probe of their own if the old probe should keep
compiling there), and add the rows to `book/src/gridspec/lsl.md`.
