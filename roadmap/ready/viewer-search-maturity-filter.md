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
(`DirFindFlags::{INC_PG, INC_MATURE, INC_ADULT}` in `types/directory.rs`).

**Audit 2026-10-15: half of the original finding was wrong or is outdated.**
`sl-viewer-search/src/search.rs` does set the rating bits: `maturity_flags`
reads the reference's per-category checkboxes (`SearchPlacesPG` / `…Mature` /
`…Adult` and their Groups, Land, Events and Classifieds twins; General and
Moderate on, Adult off by default) and `dispatch_query` unions them into every
`DirFindQuery`, `DirPlacesQuery`, `DirLandQuery` and `DirClassifiedQuery` it
sends, recomposed per
query. People carries none, as in the reference.

What remains is the **account** half. Nothing clamps those checkboxes by what
the account may see and has asked for: an account whose `PreferredMaturity`
is `PG` (or whose ceiling is below Adult) can still tick Moderate and Adult
and is sent those results. The reference disables the checkboxes the account
may not use and composes the flags from the preference too, recomputed when
the preference changes mid-session — and the General panel's maturity
conversation already raises that change.

Wanted:

- The checkboxes above the preference / entitlement disabled, and their bits
  left out of every query, following a mid-session preference change.
- A test. This is offline-testable end to end: the fake grid answers
  `DirFindQuery` and now has a maturity ceiling per account
  (`AccountConfig::maturity_ceiling` in `sl-fake-grid/src/accounts.rs`), so a
  case can set a preference, search, and assert the flags on the query the
  grid received — better than asserting the results, which would only prove
  the fixture.

The world map is the weaker sibling of the same gap and belongs here rather
than in an item of its own: `world_map.rs` renders a rating badge per region
(`worldmap-maturity-general` / `-moderate` / `-adult`) but never dims or hides a
region the account may not enter. Fix the badge's silence at the same time or
write down why not.

Acceptance: a search issued while the preference is `PG` sends a query whose
rating flags say `PG` only whatever the checkboxes say, the flags follow a
mid-session preference change, and a test pins the flags as the grid received
them.
