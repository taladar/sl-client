---
id: viewer-build-selection-filters
title: Build-tool selection filters
topic: viewer
status: blocked
origin: main-menu survey (2026-07-23)
blocked_by: [viewer-object-selection-core]
refs: [viewer-transform-gizmos]
---

Context: [context/viewer.md](../context/viewer.md).

Build ▸ Options predicates that constrain what click/rubber-band
selection picks up, so mass edits don't grab the wrong things:

- Select Only My Objects / Movable / Locked / Copyable objects
- Select Invisible Objects; Select Reflection Probes
- Include Group-Owned Objects
- Select By Surrounding (rectangle must fully contain vs. touch)

Scope: the filter toggles as settings + Build ▸ Options menu entries,
applied as predicates in the selection code path (ownership, movable,
locked, copy-permission, invisibility, probe-volume, group-owned), plus
the inclusive/contained rectangle-select mode.

Reference (Firestorm, read-only): `menu_viewer.xml` Build ▸ Options
(~L2097-2167), `LLSelectMgr` filter globals (`gAllowSelectAvatar`,
select-owned/movable flags) in `llselectmgr.cpp`.

Builds on: the selection-set core (blocked task) — these are predicates
inside its pick/marquee path.
