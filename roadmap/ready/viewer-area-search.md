---
id: viewer-area-search
title: Area search — find objects in the region
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-virtualized-list]
refs: [viewer-derender-blacklist, viewer-beacons-beam-render]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's area search: filter every object in view range by name /
description / owner / group / price / distance and list the matches — the
tool for "where is the thing named X on this parcel". The scene mirror
already holds every object; names/owners come from batched
`ObjectProperties` / `ObjectPropertiesFamily` requests (`protocol-36`,
request path done) issued lazily for objects whose metadata is not yet
known, exactly as the reference floods `RequestObjectPropertiesFamily`.

Scope: the filter form + virtualized result list (name, description, owner,
group, price, LI, distance), lazy property resolution with progress ("N of M
scanned"), row actions (beacon/track to it — [[viewer-beacons-beam-render]] —
touch, sit, pay, derender), and a refresh that re-walks the mirror.

Reference (Firestorm, read-only): `fsareasearch`,
`floater_fs_area_search.xml`.

Builds on: the scene mirror + `protocol-36` object properties.
