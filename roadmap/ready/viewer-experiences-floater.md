---
id: viewer-experiences-floater
title: Experiences floater — lists, profile, search
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-virtualized-list]
refs: [viewer-experience-permission-dialog]
---

Context: [context/viewer.md](../context/viewer.md).

The experiences UI over the fully-implemented experience protocol
(`protocol-27` + the caps pairing `protocol-62`):

- **My experiences**: allowed / blocked lists with per-row revoke ("forget"
  / unblock), and the experiences the agent contributes to / owns.
- **Experience profile**: name, description, maturity, owner/group, slurl,
  the permission actions (allow / block / forget), and — for owned ones —
  the editable fields the caps expose.
- **Search**: find experiences by name (the experience-search cap), rows
  opening the profile.
- The events log tab (recent experience permission events) as the reference
  ships.

The in-the-moment grant dialog is separate
([[viewer-experience-permission-dialog]]); this floater is the management
surface it links out to.

Reference (Firestorm, read-only): `llfloaterexperiences`,
`llfloaterexperienceprofile`, `llpanelexperiences`,
`floater_experience_search.xml`.

Builds on: `protocol-27` / `protocol-62` experience surface.

## Parity-audit addendum (2026-08-19)

Parity-audit status update: the allowed/blocked lists, forget action,
name resolution, and the top-menu entry are ALREADY IMPLEMENTED
(`sl-viewer-notices/src/experiences_floater.rs`). The remaining
scope of this task is the experience **profile panel**, **search**,
and the **contributor / owned lists**.

## Events log: done, and what it left behind (2026-09-11)

The **events-log tab** is done — [[viewer-experience-event-stream]] landed the
`ExperienceEvent` ingest, the per-account log, its notifications, and a Recent
events section in this floater.

It also inherited this floater's list mechanism, which is the one the
build-once-update-in-place rule argues against: all three
columns (allowed, blocked, events) despawn their rows and respawn them when a
revision moves, rather than binding a pooled `ui_table` / `VirtualList` in
place. Three despawn-rebuilding columns in one window is more churn than the two
that were here before, and the events one is the one that grows without an upper
bound on rows. Converting **all three** together — one mechanism per window, not
two — belongs with whichever of the panels above is built first, since the
contributor / owned lists want the same widget.
