---
id: viewer-search-floater
title: Search floater — directory search UI
topic: viewer
status: done
origin: user request (2026-07-22), while shipping viewer-social-profiles
blocked_by: [viewer-ui-widget-scaffold]
refs: [api-g4, viewer-social-profiles, viewer-media-prim-browser]
---

**Done (2026-07-28).** `src/search.rs` reproduces Firestorm's
`fsfloatersearch` layout: a tab strip of result lists on the left plus a
**shared details pane** on the right. Tabs: **Web** (an embedded
`browser_widget` view to the grid search site; a placeholder without CEF)
then **People / Groups / Places / Land / Events / Classifieds** over the four
`Dir*Query` calls. Each directory tab is a `ui_table` results table
(single-select) with the reference columns, its own `query_start` paging
(Prev / Next, page 100) and a count. Shared query field (Enter / Search).
Filters: People online-only; Places / Classifieds category combos; Land
sale-type + sort combos (default **Price**, descending); **Events** date-mode
radio (Upcoming / By date) + day stepper + category combo, encoded as the
reference's `"<day>|<category>|<text>"` QueryText with `DFQ_DATE_EVENTS`.
**Places** sets `DFQ_DWELL_SORT` and lists traffic-descending by default,
with a Traffic column.

Maturity is **per-tab** (each of Groups / Places / Land / Events /
Classifieds carries its own General / Moderate / Adult checkboxes over
per-category settings; People has none), matching the reference. **Land**
also has the ascending toggle, price / area limit fields (`LIMIT_BY_PRICE` /
`LIMIT_BY_AREA`), and the L$/m2 + Type columns.

The **shared details pane** fills from the selected row and fires the
secondary detail request per category — `RequestAvatarProperties` (People:
Born / Partner / About + profile image + **Send Message** / **Add Friend**),
`RequestGroupProfile` (Groups: members / enrollment / charter + insignia +
**Join Chat** / **Join Group**), `RequestParcelInfo` → `ParcelDetails`
(Places / Land, the teleport position + snapshot), `EventInfoRequest`,
`RequestClassifiedInfo` (+ snapshot). It renders the **snapshot image**
(profile pic / insignia / parcel / classified snapshot) via the
`TextureManager` request-then-poll path. Actions: Open Profile, Send Message
/ Add Friend (`OfferFriendship`), Join Chat / Join Group (`OpenConversation`
/ `JoinGroup`), Teleport / Show on Map, Remind me. Opened from the Content
menu (`Search…`, Ctrl+F) and the bottom-toolbar Search button.

Row selection was added to the **table widget** (`TableSelectionMode`
`None` / `Single` / `Multi` — Ctrl-toggle + Shift-range for Multi, awaiting
the friends-list conference picker as its first Multi consumer). Directory
types + `EventInfo` / `AvatarProperties` / `GroupProfile` re-exports were
added to `sl-client-bevy` (the viewer's `sl-proto` dep is dev-only). The
**Web** tab uses the OpenSim `search-server-url` from `SimulatorFeatures`
when present, else the SL search site. Tests: 6 in `search` + 4 selection
tests in `ui_table`.

**Notes / gaps:** the reference's leading 20 px **icon column** (skin
maturity / type / online-status icon graphics) is not reproduced — it needs
the skin icon set wired into table Custom cells, a separable follow-up. A
standalone event floater + the reminder-that-arrives stay with
[[viewer-event-details]] (the protocol has no `EventNotification` event). The
full SL templated web URL (login `search_token`, per-grid `search.[GRID]`
host) is a follow-up on the base URL.

Context: [context/viewer.md](../context/viewer.md).

The **Search floater**: the in-viewer directory search the Vintage skin keeps
(Firestorm's legacy search window) — a query field plus category tabs over
the **directory protocol**, which is fully implemented (`api-g4`):

- **People** (`DirFindQuery` people flag → `DirPeopleReply`) — result rows
  open the profile floater ([[viewer-social-profiles]]).
- **Groups** (`DirFindQuery` groups flag → `DirGroupsReply`) — rows open the
  group profile ([[viewer-social-group-profile]]).
- **Events** (`DirFindQuery` events flags → `DirEventsReply`), with the
  date / category filters the reference offers — result rows open the event
  detail floater ([[viewer-event-details]]).
- **Places** (`DirPlacesQuery` → `DirPlacesReply`) and **Land** sales
  (`DirLandQuery` → `DirLandReply`, price / area sort flags).
- **Classifieds** (`DirClassifiedQuery` → `DirClassifiedReply`) — rows show
  the classified detail (the profile floater's detail panel is the model).

Maturity checkboxes (General / Moderate / Adult → the `DFQ_*` maturity
flags), paging via the query-start offsets, and per-category result counts.

The reference's **web search** (the search *website* in an embedded browser)
is a separate concern blocked on CEF ([[viewer-media-prim-browser]]); this
task is the protocol-backed legacy directory UI, which OpenSim's search
module can exercise locally.

Reference (Firestorm, read-only): `fsfloatersearch.cpp` (legacy search),
`lldirectory*`, Vintage `floater_fs_search.xml`.

Builds on: `api-g4` (directory queries / replies, all decoded).
