---
id: protocol-experience-search-paging
title: Carry the experience search's paging URLs through to the viewer
topic: protocol
status: done
origin: split out of [[viewer-experiences-floater]] when the search tab was
  built (2026-09-11)
refs: [viewer-experiences-floater, protocol-27]
---

Context: [context/protocol.md](../context/protocol.md).

`FindExperienceByName` answers a page of results **and** whether there is
another one: the reply carries `next_page_url` / `previous_page_url`, which is
what the reference's `LLPanelExperiencePicker::processResponse` enables its
`right_btn` / `left_btn` from.

Our decoder keeps only the `experience_keys` array
(`Event::ExperienceSearchResults(Vec<ExperienceInfo>)`), so the two markers are
dropped on the floor. The Experiences floater's search tab therefore guesses:
it offers **Next** while the page came back full (`SEARCH_PAGE_SIZE` rows) and
**Previous** while the page number is above one. The guess is wrong in exactly
one place — a result set whose size is an exact multiple of the page size
offers one Next that comes back empty — and it cannot be right, because "was
that the last page" is not derivable from the page.

What this needs:

- `sl-wire`: keep the two URLs (or, better, the two booleans — the viewer never
  fetches the URL, it re-queries by page number) when parsing the reply.
- `sl-proto`: widen `Event::ExperienceSearchResults` to carry them. That is the
  whole event-shape checklist — the `sl-client-bevy` / `sl-client-tokio`
  re-export blocks, the exhaustive matches in both runtimes' examples,
  `sl-repl`'s formatter and `sl-survey`.
- `sl-fake-grid` / `SimSession`: serve them, so the paging can be exercised
  offline with a fixture of more than one page.
- The floater: replace `page_is_full` with the real flag and delete the
  heuristic paragraph from its module docs.

Reference (Firestorm, read-only): `llpanelexperiencepicker.cpp`
(`processResponse`, `onPage`), `llexperiencecache.cpp`
(`findExperienceByName`).
