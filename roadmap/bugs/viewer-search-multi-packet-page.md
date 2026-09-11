---
id: viewer-search-multi-packet-page
title: A search page split over several packets shows only the first packet
topic: viewer
status: bugs
origin: found fixing [[viewer-audit-search-sentinel-row]] (2026-09-12)
points: 2
refs: [viewer-audit-search-sentinel-row, viewer-search-floater]
---

Context: [context/viewer.md](../context/viewer.md).

A `Dir*Reply` carries up to 255 variable blocks, and a page is 100 results —
but the grid is free to answer one query with **several** replies, and the
reference is written for exactly that: `mResultsReceived` accumulates across
the packets of one page (`llpaneldirbrowser.cpp`, every
`process*Reply`), the list is cleared only on the *first* one (the
`!list->getCanSelect()` test — the "Searching…" placeholder row is
unselectable), and `showNextButton` compares the **running total** against the
page size.

`ingest_search_replies` (`sl-viewer-search/src/search.rs`) treats one reply as
one page:

```text
state.people.pending = None;
state.people.set_results(results.clone());
```

`pending` is cleared by the first reply, so `pending_matches` rejects every
later packet of the same query, and `set_results` replaces rather than appends.
A page the grid sends as 60 + 41 therefore shows 60 results and no **Next** —
the remaining 41 are dropped, and the page looks like the end of the results.

## The fix

Accumulate per query instead of per packet: keep the `QueryId` pending until the
page is complete or the query is replaced, append each reply's rows, and let
`strip_sentinel` see the running total (which is what its `> PAGE_SIZE` test is
about). "Complete" has no marker on the wire — the reference never decides it
either; it simply keeps appending, and a new query resets the list. Clearing
`pending` on the *next* query (or on a paging step) rather than on the first
reply is the shape to aim for.

Watch the interaction with the sentinel: the trim must apply to the accumulated
page, not to each packet, or a 60 + 41 page keeps all 101 rows.

## How to verify

A unit test on the accumulation — two replies with the same query id append and
the page's `filled` reflects their sum — plus a live check on Second Life
(aditi) with a query broad enough to fill a page: the count read-out must say
`Showing 1–100` and **Next** must reach a second page.
