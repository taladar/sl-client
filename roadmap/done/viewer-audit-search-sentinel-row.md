---
id: viewer-audit-search-sentinel-row
title: The directory 'there is more' sentinel row is rendered as a result
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 2
refs: [viewer-search-multi-packet-page]
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

## Resolved (2026-09-12)

`strip_sentinel` is that helper, and `Page::set_results` is the one place every
category goes through. A local `DirRow` trait answers "is this block padding?"
once per result type — nil `agent_id` / `group_id` / `parcel_id` (Places and
Land) / `owner_id` / `classified_id` — so the six categories share one rule
instead of six copies of it.

The trim is **positional and happens first**, then the padding is dropped, which
is the order the reference uses (it trims the raw block count in
`showNextButton` and skips nil ids inside the row loop). So a full page can show
fewer than 100 rows, and `filled` — now "the reply reached *past* the page" —
answers the reference's `mResultsReceived > mResultsPerPage`, not the old
`>=`. That `>=` was the second half of the bug: exactly 100 real results lit
**Next** and the next page came back empty.

Places needed the order stated: its dwell sort used to run before the page was
stored, and the sentinel is the *last entry of the reply*, not the
least-visited parcel — sorting first can move it inland and truncation then
drops a real result instead. `set_results_sorted_by` strips, stores, and only
then sorts.

Tests: the sentinel is not a result (99 / 100 / 101 / 0 entries, checking both
the count and which row is last); a nil id is padding in each of the six
categories; padding *inside* the page is dropped too (98 + 2 padding + sentinel
→ 98 rows, Next on); Places drops the sentinel before sorting (the sentinel
carries the highest dwell in the reply, so a sort-first implementation keeps
it); and the dwell sort still orders the page.

Not fixed here, and now its own task: a page the grid splits over several
packets still shows only the first packet —
[[viewer-search-multi-packet-page]].
