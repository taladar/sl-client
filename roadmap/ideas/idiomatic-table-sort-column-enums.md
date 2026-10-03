---
id: idiomatic-table-sort-column-enums
title: Table sort columns as enums instead of string tokens
topic: idiomatic
status: ideas
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

## What

`TableColumn { token: "…" }` against comparators `match token { …, _ => }`
with a catch-all that sorts by name or `Equal`: top objects, settings list,
group profile tables, blocked list, contact sets, asset blacklist, avatar
render floater, radar model, experience search / floater. Sort keys are
`(&'static str, bool)` where the bool means ascending; persisted sort strings
parse `dir == "a"`. A renamed token silently sorts by the fallback; exactly one
catch-all column per table today.

## How

Make the table widget generic over a column enum (`order_by_sort_keys` is
already generic over `Column`); `SortDirection { Ascending, Descending }`;
strings only for the persisted form.
