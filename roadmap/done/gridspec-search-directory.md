---
id: gridspec-search-directory
title: Directory search and the avatar picker on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-classified-query-wrong-flag-space,
  viewer-search-maturity-filter]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim answers only people / groups; SL's UDP picker returns a nil sentinel;
web search URL sources differ; classified search is broken by our flag bug.

## Discover

The directory cases on aditi (after the classified flag fix) and OpenSim;
paging, maturity filtering.

## Document

`book/src/gridspec/search.md`.

## Fake grid

Medium — [[server-fake-grid-directory-search]].

## Viewer

Sentinel row, empty tabs on OpenSim.

## Capabilities done in this task

[[protocol-cap-product-info]]: land-type names over `ProductInfoRequest`, with
the `ProductSKU` the LLSD forms of the land replies carry.

## Already known (from [[gridspec-parcel-info-dwell]], 2026-10-05)

Aditi answers `DirLandQuery` over the **event queue**: a `DirLandReply` event
mirroring the UDP blocks, each row with a `ProductSKU` (`023`, `024` seen). The
session decodes it into `Event::DirLandReply` (without the SKU) since that
task; before it the event was dropped, which is why `dir-places-land-classified`
recorded no land answer on Second Life. Still to measure here: paging, the
other searches' transports, and whether `PlacesReply` comes the same way.

## Done (2026-10-16)

**Discover**: `search-directory` (`sl-conformance/src/cases/
search_directory.rs`, 2av, live grids): every search the reference's Search
floater sends — people (own name, the other avatar with `DFQ_ONLINE`, empty,
two letters, a common word on two pages), groups (two pages), events
(`u|0|`), places (the Linden browse, a common word on two pages, `C_NONE`
against `C_ANY`, General only, two letters), land (two pages, General only,
the SKUs), classifieds (both bit spaces) — the avatar picker over UDP and over
`AvatarPickerSearch`, the land holdings and `ProductInfoRequest`. The circuit
probe records each reply's road. Results are described against the page of
100, never by row. Four runs per live grid (two that shaped the metrics, two
held), then a verification run.

Findings beyond the plan: **`PlacesReply` comes over the event queue** on
Second Life and was dropped as an unknown event; our viewer's places "Any"
sent `C_NONE` (0), which Second Life answers with other rows than the
reference's `C_ANY` (−1); Second Life never says a person is online, and
answers an empty or two-letter people search with nothing at all; aditi lists
no events or classifieds, so the classified flag fix could not be shown by
rows. Paging and the 101-row page are as the content chapter said; page two
repeats page one's marker row.

**Document**: `book/src/gridspec/search.md`, linked from `content/search.md`,
`gridspec/land.md`, `gridspec/capabilities.md` and `comms/caps-reference.md`.

**Fake grid**: `ProductInfoRequest` per flavour (Second Life's 26 products,
OpenSim withholds it); the directory index itself is
[[server-fake-grid-directory-search]] (now ready, with the measurements).

**Viewer**: the search floater's status line tells searching, nothing found,
the grid's status, and "did not answer" (after 20 s — OpenSim's places, land,
events and classifieds tabs) apart, where it said "No results" for all of
them; a too-short text is refused as the reference refuses it; "Any" places
send `C_ANY`; classifieds send their own rating bits; the land type column
names the SKU's product. The nil sentinel rows were already dropped
(`DirRow::is_placeholder`). Unit tests cover each; the e2e
`the_search_window_says_why_it_shows_nothing` (fake grid) holds the status
line, and the live-grid stage
`the_search_window_on_a_live_grid_names_land_types`
passed on aditi (land rows named by product, "Any" places a full page) and on
OpenSim (both tabs "did not answer").

**Capabilities**: [[protocol-cap-product-info]] done here, and the bug
[[protocol-classified-query-wrong-flag-space]] fixed.

### What could not be measured

- The web search URL: the login response's `search` field is measured (none
  on either grid); OpenSim's `OpenSimExtras` search URL was already measured
  in the simulator-features chapter.
- Maturity filtering of events and classifieds: aditi lists none of either
  (people carry no rating); places and land are measured.
