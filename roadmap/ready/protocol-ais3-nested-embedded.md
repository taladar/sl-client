---
id: protocol-ais3-nested-embedded
title: An AIS3 depth fetch reads only its first level
topic: protocol
status: ready
origin: measured doing object-asset-format on aditi (2026-09-06)
points: 3
refs: [protocol-ais3-library-cap]
---

`GET /category/<id>/children?depth=<n>` asks the AIS service for a subtree, and
the real service answers by nesting `_embedded` once per level. This
workspace's parser (`ais_inventory_update_from_llsd`) gathers only the **top**
level of that document, which its own serializer documents in passing:

> The real AIS service nests `_embedded` recursively per depth level; our
> client parser gathers only the top-level `_embedded` maps, so the whole
> subtree is served **flattened** into them — a deliberate, documented
> divergence that is information-equivalent.

That is true of the fake grid, which flattens to compensate. Against Second
Life it is not: a `depth=50` fetch of the inventory root returns the root's
subfolders and **no items at all**, which is what `object-asset-format`
recorded (`items_seen = 0`) before it was changed to walk one level at a time.

One level per request is correct but costs a round trip per folder, so a real
account's tree is only ever sampled. Reading the nested form would let a single
request bring back a subtree, which is what the depth parameter is for.

Wanted: `ais_inventory_update_from_llsd` recursing into nested `_embedded`
maps, with the fake grid's flattened form still accepted (it is what every
offline test produces), and a test pinning both shapes.

Acceptance: a `depth > 1` fetch against a nested document yields every folder
and item in it, and the offline cases still pass unchanged.
