---
id: gridspec-lsl-refreshed-table-probe-steps
title: Probe steps the refreshed library table unblocks
topic: gridspec
status: done
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

## Outcome (2026-10-27)

Both went into probes of their own, since OpenSim compiles neither:
`experience/sit-on-link` and `http/options` (whose user agent with a space
is tried by two scripts of its own, in case it stopped the script — it
does not). Run on aditi and on YEngine and XEngine; the rows are in
`book/src/gridspec/lsl.md` § *Experiences and the key-value store* and §
*The user agent, `Accept` and the extended error*, and what the fake grid
should do with them in [[server-lsl-lib-experience]] and
[[server-lsl-lib-http-url]].

- **Second Life.** `llSitOnLink` answers −1 (`SIT_NOT_EXPERIENCE`) to
  every agent and link, before checking either. `HTTP_USER_AGENT` appends
  to the default user agent; `HTTP_ACCEPT` takes one type and turns an
  answer of another into a 415; `HTTP_EXTENDED_ERROR` turns a refusal into
  a key and an RFC 7807 `http_response`.
- **OpenSim.** Neither probe compiles on either engine.
- **Not measured.** `llSitOnLink` inside an experience
  ([[gridspec-lsl-in-experience]]); the user agent's length limit (somewhere
  under 306 characters).
