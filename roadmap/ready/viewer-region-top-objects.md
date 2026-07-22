---
id: viewer-region-top-objects
title: Top objects — top scripts / top colliders
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-virtualized-list]
refs: [viewer-region-options-debug]
---

Context: [context/viewer.md](../context/viewer.md).

The estate "Top Objects" tool: request the region's top script-time / top
collider object list (`LandStatRequest` → `LandStatReply`, estate-manager
gated — verify the pair landed in the god/estate protocol batches, add the
decode if not), show the sortable list (score, name, owner, location, time),
filter by name/owner, and the actions: beacon to an entry, return it,
disable its scripts. Reached from the Region/Estate floater's Debug tab
([[viewer-region-options-debug]]) but usable standalone.

Reference (Firestorm, read-only): `llfloatertopobjects`,
`floater_top_objects.xml`.

Builds on: the estate protocol (`protocol-14`) and the god/estate batches.
