---
id: viewer-search-multi-packet-page
title: A search page split over several packets shows only the first packet
topic: viewer
status: done
origin: found fixing [[viewer-audit-search-sentinel-row]] (2026-09-12)
points: 2
refs: [viewer-audit-search-sentinel-row, viewer-search-floater,
  viewer-table-scrollbar-overlays-last-column]
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

`Page` now accumulates per query instead of per packet. `begin_query` arms it
when the query goes out — recording the `QueryId` and resetting `received` to
`None` — and `append_reply` folds each matching reply in: the first reply of a
query clears the list, every later one appends, and `pending` is cleared by
nothing but the next `begin_query`. `received` is the reference's
`mResultsReceived`: raw blocks as sent, padding and sentinel included.

Both the page trim and the **Next** test read that running total, not the
packet. `append_reply` truncates the incoming packet to `PAGE_SIZE - received`
before it drops the nil-id padding, so the positional trim lands where the
reference's `rows -= (mResultsReceived - mResultsPerPage)` lands, and a packet
that starts past the page contributes nothing. `filled` is
`received > PAGE_SIZE` over the whole page. `append_reply_sorted_by` (Places,
dwell-desc) sorts the accumulated page rather than the packet — otherwise the
second packet's parcels would all sort below the first packet's.

Tests: 60 + 41 accumulates to 100 rows with **Next** on while 60 + 40 leaves it
off; the trim runs over the accumulated total (95 + 10 keeps five and drops the
rest, and a further packet keeps nothing); and a fresh `begin_query` makes the
next reply replace rather than append. The pre-existing sentinel / padding /
dwell-sort tests were re-pointed at the new pair.

The book's search chapter said "a page is 100 results" without saying a page is
not a packet; it now states the accumulation rule and that the trim is
positional and comes first.

## How to verify

A unit test on the accumulation — two replies with the same query id append and
the page's `filled` reflects their sum — plus a live check on Second Life
(aditi) with a query broad enough to fill a page: the count read-out must say
`Showing 1–100` and **Next** must reach a second page.

Live-verified on aditi: a broad People query reads `Showing 1–100` and
**Next** reaches the second page. Places could not settle the dwell ordering
there — every aditi parcel reports 0 traffic — but the run turned up a
separate defect in the table widget, now its own task:
[[viewer-table-scrollbar-overlays-last-column]].
