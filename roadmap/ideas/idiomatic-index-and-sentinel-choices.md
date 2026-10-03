---
id: idiomatic-index-and-sentinel-choices
title: Enums for combo/radio/tab indices and sentinel values
topic: idiomatic
status: ideas
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

## What

Indices standing in for the choice they select, and magic sentinels:
quick preferences' `ENV_GROUP_KEYS[5]` / `ENV_GROUP_CUSTOM = 4` /
`fixed_for` matching `1|2|3`; debug settings `change.active == 1` meaning
Account; experience maturity combo index → 13/21/42; search
`events_category: usize` into `EVENT_CATEGORIES` (with hardcoded English
labels) and a radio `== 0` for `EventsMode`, tab order / label / category
arrays kept parallel; the legacy alpha and media-controls combo indices that
*are* the wire values; `LAND_ACTIONS` / `LAND_ACTION_KEYS` parallel arrays;
preferences tabs addressed by string id and `usize` index; `0` = "no limit"
(`land_price_limit`), rig index `0` = the default probe, the nil uuid meaning
"all" (`UnDerender { id }` — a nil-id row would clear every temporary entry —,
experience id, the RLV fallback agent defeating an `!= agent` filter);
cache kind strings (`"texturecache"`, …) in ~8 crates that `PURGE_KIND_DIRS`
and the replay bundle must each repeat, or the kind escapes Clear Cache.

## How

Enums with `ALL` and `label_key()` (a generic `TypedCombo<T>` beside
`ComboBindingValues`), `Option<NonZeroU32>`, `enum UnDerender { One(Uuid),
AllTemporary }`, `ProbeRig { Default, Local(NonZeroUsize) }`, `enum
CacheKind` with `dir_name()` and `ALL` (purge list derived from it).
