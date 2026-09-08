---
id: viewer-search-maturity-filter
title: Search asks for every rating whatever the account is allowed to see
topic: viewer
status: ready
origin: noticed auditing maturity enforcement while doing test-fake-grid-imitates-economy (2026-09-08)
points: 2
refs: [protocol-account-benefits-package, viewer-region-entry-maturity-gate]
---

Context: [context/viewer.md](../context/viewer.md).

`DirFindQuery` carries three include-flags — `DFQ_INC_PG`, `DFQ_INC_MATURE`,
`DFQ_INC_ADULT` — and `sl-proto` types all three
(`DirFindFlags::{IncPg, IncMature, IncAdult}` in `types/directory.rs`).
`sl-viewer-search` imports `DirFindFlags` for the *category* bits and never sets
any of the three rating bits.

So every search this viewer sends asks for whatever the grid's default is,
rather than for what the account is entitled to and has asked for. An account
whose `PreferredMaturity` is `PG` can be shown Moderate and Adult results, and
the preference the General panel spent a whole retry conversation keeping in
sync with the server does not reach the one query family it most obviously
governs.

The reference viewer composes the flags from the preference each time it builds
a query, not once at login, because the preference can change mid-session — and
this viewer already has the change event to hang that off.

Wanted:

- The rating bits set from `PreferredMaturity` on every `DirFindQuery` the
  search panel builds, recomposed when the preference changes rather than
  captured at startup.
- The same for the other directory queries that take rating flags, not just
  people/groups: places, land, classifieds and events all filter by rating on a
  live grid.
- A test. This is offline-testable end to end: the fake grid answers
  `DirFindQuery`, so a case can set a preference, search, and assert the flags
  on the query the grid received — better than asserting the results, which
  would only prove the fixture.

The world map is the weaker sibling of the same gap and belongs here rather
than in an item of its own: `world_map.rs` renders a rating badge per region
(`worldmap-maturity-general` / `-moderate` / `-adult`) but never dims or hides a
region the account may not enter. Fix the badge's silence at the same time or
write down why not.

Acceptance: a search issued while the preference is `PG` sends a query whose
rating flags say `PG` only, the flags follow a mid-session preference change,
and a conformance case pins the flags as the grid received them.
