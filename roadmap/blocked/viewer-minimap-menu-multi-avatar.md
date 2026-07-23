---
id: viewer-minimap-menu-multi-avatar
title: Minimap context menu — multi-avatar entries (dynamic labels)
topic: viewer
status: blocked
origin: split from viewer-minimap-interactions (2026-07-23)
blocked_by: [viewer-contact-sets]
refs: [viewer-minimap-interactions, viewer-minimap-avatar-dots]
---

Context: [context/viewer.md](../context/viewer.md).

When several avatar dots sit within the minimap's pick radius, the
reference context menu grows multi-avatar variants: a **View Profiles**
submenu with one entry *per avatar under the cursor* (labelled by
resolved display name, filled asynchronously as names arrive) and
**Add to Set Multiple** ([[viewer-contact-sets]]). The mark actions
already apply to every avatar in the pick radius.

Blocker in our stack: `MenuDef` / `MenuCommand` labels are
`&'static str` — a menu is a compile-time static. Dynamic per-avatar
entries need a menu-widget extension (runtime-labelled entries or a
dynamic submenu builder), which should be designed once for every
consumer (the minimap here, later the world map and radar), not
special-cased.

Deps: [[viewer-contact-sets]] for the set actions; the dynamic-label
widget work has no task yet and belongs to this one.
