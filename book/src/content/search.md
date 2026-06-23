# Search & Directory

The viewer's *Search* floater and its land / land-holdings panels are driven by
a small family of query/reply message pairs. A query carries a client-chosen
`query_id` (a fresh UUID) that the simulator echoes back in the reply, so a
client can correlate an answer with the request that produced it. Results are
paged: most queries take a `query_start` index and the simulator returns a
batch from there.

This chapter covers the directory searches (`Dir*Query`), the avatar-name
autocomplete (`AvatarPickerRequest`), and the land-holdings lookup
(`PlacesQuery`).

## Query flags

`DirFindFlags` is the `DFQ_*` bitfield shared by every directory query (and
`PlacesQuery`). It does three jobs:

- **selects what a `DirFindQuery` searches** — `DirFindFlags::PEOPLE`,
  `DirFindFlags::GROUPS` or `DirFindFlags::EVENTS`;
- **filters results** — e.g. `DirFindFlags::ONLINE` (people online now),
  `DirFindFlags::FOR_SALE` / `DirFindFlags::AGENT_OWNED` (land), and the
  maturity-inclusion bits `DirFindFlags::INC_PG` / `INC_MATURE` / `INC_ADULT`;
- **sorts results** — `DirFindFlags::NAME_SORT`, `DirFindFlags::PRICE_SORT`,
  `DirFindFlags::AREA_SORT`, `DirFindFlags::DWELL_SORT`, and
  `DirFindFlags::SORT_ASC` for ascending order.

Combine flags with `DirFindFlags::union`; query the set with `contains`. The
land query additionally takes a `LandSearchType` (`ST_*`) selecting which sale
categories to include (`AUCTION`, `NEWBIE`, `MAINLAND`, `ESTATE`, or `ALL`,
which is the viewer's default).

## The unified find query

`DirFindQuery` is one message used for three searches; the flags pick which, and
the simulator answers with the matching reply:

| Flag                  | Command           | Event                       |
| --------------------- | ----------------- | --------------------------- |
| `DirFindFlags::PEOPLE`| `DirFindQuery`    | `Event::DirPeopleReply`     |
| `DirFindFlags::GROUPS`| `DirFindQuery`    | `Event::DirGroupsReply`     |
| `DirFindFlags::EVENTS`| `DirFindQuery`    | `Event::DirEventsReply`     |

```rust,ignore
session.dir_find_query(
    query_id,
    "alice",
    DirFindFlags::PEOPLE.union(DirFindFlags::ONLINE),
    0, // query_start
    now,
)?;
// later: Event::DirPeopleReply { query_id, results }
```

Each result type carries the fields the viewer shows in its result list:
`DirPeopleResult` (agent id, legacy name, online), `DirGroupResult` (group id,
name, member count, ranking) and `DirEventResult` (owner, name, event id, date
string, Unix time, event flags). The events reply also carries a `status` word
(`STATUS_SEARCH_EVENTS_*`; `0` on success).

## Places, land and classifieds

Three dedicated queries cover the remaining directory tabs:

- **`DirPlacesQuery`** → `Event::DirPlacesReply` — named parcels, optionally
  filtered by `ParcelCategory` and region name. Results (`DirPlaceResult`) give
  the parcel id, name, for-sale/auction flags and dwell.
- **`DirLandQuery`** → `Event::DirLandReply` — land for sale or auction,
  filtered by `LandSearchType`, price and area. Results (`DirLandResult`) give
  the parcel id, name, auction/for-sale flags, sale price and area.
- **`DirClassifiedQuery`** → `Event::DirClassifiedReply` — classified ads,
  filtered by a classified category. Results (`DirClassifiedResult`) give the
  classified id, name, flags, creation/expiration dates and weekly listing
  price. Fetch the full ad with `ClassifiedInfoRequest` (see
  [Profiles, Picks & Classifieds](profiles.md)).

The places and classified replies also carry a `status` word
(`STATUS_SEARCH_PLACES_*` / `STATUS_SEARCH_CLASSIFIEDS_*`).

## Events directory

A `DirEventResult` (or the events directory floater) only carries an event's
summary; its `event_id` keys the rest. `EventInfoRequest` fetches the full
listing, which arrives as `Event::EventInfoReply` carrying an `EventInfo`: the
running avatar (`creator`), name, category, description, the human-readable
`date` plus a Unix `date_utc`, the `duration` in minutes, the cover charge
(`cover` non-zero, with the L$ `amount` as `Some(LindenAmount)` — `None` when no
cover charge applies), the region name, the global position and the
`EVENT_FLAG_*` maturity flags.

```rust,ignore
session.event_info_request(event_id, now)?;
// later: Event::EventInfoReply { info }
```

Two fire-and-forget requests manage event reminders (there is no direct reply):
`EventNotificationAddRequest` subscribes to a reminder as the event approaches,
and `EventNotificationRemoveRequest` cancels it.

```rust,ignore
session.event_notification_add_request(event_id, now)?;
session.event_notification_remove_request(event_id, now)?;
```

The `EventLocationRequest` / `EventLocationReply` pair is *not* wrapped: those
are trusted simulator↔dataserver messages the viewer never sends or receives
(the event's location already arrives in `EventInfoReply`'s global position).

## Avatar-name autocomplete

`AvatarPickerRequest` is the lookup behind the avatar picker: send a partial
name, receive a short list of matches in `Event::AvatarPickerReply` (each an
`AvatarPickerResult` of avatar id and legacy first/last name).

```rust,ignore
session.avatar_picker_request(query_id, "bob", now)?;
// later: Event::AvatarPickerReply { query_id, results }
```

## Land holdings

`PlacesQuery` is distinct from the directory: it lists an agent's or a group's
land *holdings* (the land and group-land panels), not the public search index.
It echoes both a `query_id` and a `transaction_id`, and answers with
`Event::PlacesReply` carrying `PlacesResult` entries (owner, name, description,
actual/billable area, flags, global position, region name, snapshot, dwell and
price).

## Server side

The simulator side mirrors every query and reply. Each inbound query surfaces as
a `ServerEvent` (`ServerEvent::DirFindQuery`, `DirPlacesQuery`, `DirLandQuery`,
`DirClassifiedQuery`, `AvatarPickerRequest`, `PlacesQuery`, `EventInfoRequest`,
`EventNotificationAddRequest`, `EventNotificationRemoveRequest`) and
`SimSession` gains a matching reply encoder (`send_dir_people_reply`,
`send_dir_groups_reply`, `send_dir_events_reply`, `send_dir_classified_reply`,
`send_dir_places_reply`, `send_dir_land_reply`, `send_avatar_picker_reply`,
`send_places_reply`, `send_event_info_reply`), so the whole surface round-trips
through the real wire path.

## REPL

The REPL exposes one command per query: `dir_find_query`, `dir_places_query`,
`dir_land_query`, `dir_classified_query`, `avatar_picker_request`,
`places_query`, `event_info_request`, `event_notification_add_request` and
`event_notification_remove_request`. Flags are passed as raw `u32` bit values
(the `DFQ_*` / `ST_*` numbers), so for a people search you might run:

```text
dir_find_query <query_id> alice 3 0
```

where `3` is `DFQ_PEOPLE | DFQ_ONLINE`.
