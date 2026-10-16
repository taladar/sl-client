# Search and the directory

The reference viewer's *Search* floater sends one query per tab:
`DirFindQuery` for people, groups and events (`DFQ_PEOPLE`, `DFQ_GROUPS`,
`DFQ_DATE_EVENTS`), `DirPlacesQuery`, `DirLandQuery` and
`DirClassifiedQuery`, each answered by its own `Dir*Reply`. The avatar
picker sends `AvatarPickerRequest` or, where granted, a GET of
`AvatarPickerSearch`; the land holdings floater sends `PlacesQuery`.
Second Life names a land row's type through `ProductInfoRequest`.

Measured on 2026-10-16 by the conformance case `search-directory` (`2av`:
avatar **A** searches, avatar **B** is the other avatar it looks for). Two
runs on aditi (Mauve) and two on the local OpenSim (`BasicSearchModule`, no
search service configured) with the final metrics, after two earlier runs
each that shaped them, and a verification run per grid with the table in
place (**held**). Every reply's road was read off the circuit probe: a
reply that came as datagrams came over UDP, one that did not over the event
queue. The fake grid answers no directory search yet
(`server-fake-grid-directory-search`), so the case runs on the live grids
only; it does serve `ProductInfoRequest` per flavour.

A public search's results change from day to day, so no row is named: a
reply is described by how many rows came against the reference's page of
100, where its nil rows stand, and how two answers' rows relate. "A page and
one more row" is 101 rows — a full page and the marker that there is more.

## The roads and the page

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| people, groups, events, places, classifieds, picker (**held**) | over **UDP** | over UDP, where answered |
| land for sale (**held**) | over the **event queue** (`DirLandReply`; `UDPDeprecated` in the template) | **nothing** |
| land holdings (**held**) | over the **event queue** (`PlacesReply`, also `UDPDeprecated`) | **nothing**: no module answers `PlacesQuery` |
| a full page (**held**) | **101 rows**, in several messages (how many turns on the rows' names) | at most 101 (`BasicSearchModule` cuts there) |
| the second page (`QueryStart` 100) of people and groups (**held**) | another 101; **one row in common** with the first — the first page's marker row is the second page's first | — (no common word has more than a page here) |
| the second page of places (**held** as any) | another 101; whether it shares a row with the first turns on the parcels' traffic between the two queries (dwell sort) | — |
| the second page of land (**held**) | **one nil row**: aditi has fewer than a page of land for sale | nothing |
| the login response's `search` URL (**held**) | none | none |

## People

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| A's own name (a Resident's first name) (**held**) | one row: A, **offline** | the accounts matching A's legacy name, A among them, offline |
| B's name with `DFQ_ONLINE`, B logged in (**held**) | one row: B — and **offline**: Second Life never says online | B, offline (`BasicSearchModule` sets `online` false on every row and ignores the flag) |
| every row's `Group` and `Reputation` (**held**) | empty and 0 | empty and 0 |
| an empty text (**held**) | **nothing** | **two** empty replies: one for the empty text, then the account search's (which finds nobody for it) |
| two letters (**held**) | **nothing** | an empty reply |
| a common word, `linden` (**held**) | a page and one more row | an empty reply (no such account) |

The reference refuses a people text of two characters or fewer itself
("search text too short") and reads a `.` as a space; it never reads the
`Online` field (commented out in `fsfloatersearch.cpp`).

## Groups and events

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| groups for a common word, every rating (**held**) | a page and one more row | an empty reply (no group matches) |
| events, the reference's upcoming query `u\|0\|` (**held**) | one **nil row**, status `0x4` (`FOUNDNONE`): aditi lists no events | **nothing**: events are unimplemented |

## Places

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| the Linden-location browse (category Linden, empty text) (**held**) | a page and one more row, status 0 | **nothing**: no module answers `DirPlacesQuery` |
| a common word, category `C_ANY` (−1), every rating (**held**) | a page and one more row, status 0 | nothing |
| the same, category `C_NONE` (0) (**held**) | a page and one more row, **several of the same rows and others**: `C_NONE` is not "any" | nothing |
| the same, General only (**held**) | a page and one more row; several of the same rows and others | nothing |
| two letters (**held**) | **one nil row, status `0x2`** (`SHORTSTRING`) | nothing |

Our viewer sent `C_NONE` for its "Any" category until this census; the
reference sends `C_ANY` (`ParcelCategory::Any` since).

## Land

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| every parcel for sale, every rating, dearest first (**held**) | fewer than a page, **every row with a `ProductSKU`**, each a product the list below names | nothing |
| the same, General only (**held**) | fewer than a page, **some of the same rows**: the rating filter narrows | nothing |
| land holdings of an avatar with none, the nil query id (**held**) | **one nil row** | nothing |

## Classifieds

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| every rating, in the classifieds' bits (`CLASSIFIED_QUERY_INC_*`, `0x4C`) (**held**) | an empty reply, status `0x4` (`FOUNDNONE`) | **nothing**: no module answers `DirClassifiedQuery` |
| every rating, in the `DFQ_INC_*` bits our client sent until this census (**held**) | the same empty reply | nothing |

Aditi lists no classifieds, so the two bit spaces could not be told apart
by their rows; the reference packs the classifieds' own bits
(`pack_classified_flags_request`), and so does our client since
(`ClassifiedQueryFlags`).

## The avatar picker

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| `AvatarPickerRequest` for A's name (**held**) | **one nil row**: A is not found | the matching accounts, A among them |
| … for a common word (**held**) | one nil row | an empty reply |
| `AvatarPickerSearch` for A's name (**held**) | `200`, one agent: A, with `display_name_next_update` | `200`, the matching agents, no `display_name_next_update` |
| the client's own picker command (**held**) | finds A (it takes the capability) | finds A (it takes the capability) |

## Land products

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `ProductInfoRequest` (**held**) | `200`, an array of **26** `{description, name, sku}` | no capability | Second Life's flavour serves aditi's 26; OpenSim's withholds it |
| the SKUs (**held**) | `023`, `024`, `027`, `029`, `121`, `123`, `125`, `127`, `129`, `131`, `132`, `133`, `141`, `221`, `224`, `229`, `241`, `324`, `329`, `424`, `524`, `624`, `733`, `833`, `841`, `844` | — | the same |
| what the client reads (**held**) | 26 products, each with a description (`Mainland / Full Region`, `Estate / Homestead`, …) | nothing | — |

## What the client does

- `Command::DirClassifiedQuery` takes `ClassifiedQueryFlags`, the
  classifieds' own bit space; `ClassifiedQueryFlags::for_ratings` packs the
  rating checkboxes as the reference does (General without Moderate also
  sets the mature filter).
- `DirLandResult` and `PlacesResult` carry the `product_sku` of an
  event-queue row (`None` over UDP, which has no field for it). The
  event-queue `PlacesReply` is decoded (`Event::PlacesReply`); it used to be
  dropped as an unknown event, so Second Life's land holdings never arrived.
- `Command::RequestLandProducts` reads `ProductInfoRequest` into
  `Event::LandProducts`; a region without it answers nothing, and the runtime
  logs that.
- `ParcelCategory::Any` is `C_ANY`, the category a places search of every
  category sends.

## What the viewer does

The search floater:

- sends "Any" places as `C_ANY`, and the classifieds' ratings in their own
  bits;
- refuses a people text of two characters or fewer and strips the words of
  two characters or fewer from the other tabs' text, as the reference does,
  and says "The search text is too short" rather than waiting on a grid that
  answers nothing (Second Life) or an empty reply (OpenSim);
- tells every state of a search apart on its status line: "Searching…",
  "Nothing found" for an answer with no real row (a nil row is never shown),
  the grid's status (too short, a refused word, search disabled), and "The
  grid did not answer this search" after 20 s — what a places, land, events
  or classifieds search on OpenSim comes to;
- asks for the land products once, before the first land search, and names a
  land row's type by its SKU ("Unknown" for a row without one, as the
  reference shows it).
