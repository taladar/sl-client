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
(`sl-client-bevy-viewer/src/experiences_floater.rs`). The remaining
scope of this task is the experience **profile panel**, **search**,
the **contributor / owned lists**, and the **events-log tab**.
