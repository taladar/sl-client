---
id: viewer-radar-avatar-marks
title: Radar avatar marks — colour-tag nearby avatars
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-avatar-radar, viewer-contact-sets]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's radar lets you **Mark…** any nearby avatar Red / Green /
Blue / Purple / Yellow (plus clear mark / clear all marks); the mark
tints the radar row so individuals stay trackable in a crowd during
events. Marks are session-scoped and purely client-side.

Our radar ([[viewer-avatar-radar]] done;
`sl-client-bevy-viewer/src/radar.rs`, `radar_model.rs`) has no marking.
The feature is cheap on the existing table-row styling: a per-avatar
mark colour in the session radar model, a Mark submenu on the row menu,
and a row tint. Distinct from contact sets ([[viewer-contact-sets]]),
which are persistent named groups — marks are throwaway per-session
tags.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_fs_radar.xml`,
`menu_fs_radar_multiselect.xml`, `indra/newview/fsradar.cpp` (marks),
`indra/newview/fspanelradar.cpp`.
