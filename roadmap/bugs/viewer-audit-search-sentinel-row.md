---
id: viewer-audit-search-sentinel-row
title: The directory 'there is more' sentinel row is rendered as a result
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-search/src/search.rs:642` — `Page::set_results` stores every row and
sets `self.filled = results.len() >= PAGE_SIZE_USIZE` (100).

The reference treats the `(mResultsPerPage)+1`th entry purely as a marker and
drops it (`llpaneldirbrowser.cpp:1170 showNextButton`, `rows -=
(mResultsReceived - mResultsPerPage)`), and also skips nil-id blocks
(`processDirPlacesReply:552`, `if (parcel_id.isNull()) continue;`). Neither
`sl-proto/src/session/methods.rs:4221-4300` nor `ingest_search_replies`
(`search.rs:2292`) filters nil ids.

Two visible consequences: a blank or garbage row at the bottom of every full
page, and a page of exactly 100 real results with no sentinel still enables
Next, giving one empty page.

`Page::set_results` is four lines and would be caught by a unit test: 101
results means 100 displayed and `filled == true`; exactly 100 means 100
displayed and `filled == false`; 0 means `filled == false`. Pair it with a
`strip_sentinel(&mut Vec<T>, is_nil)` helper shared by all six categories.
