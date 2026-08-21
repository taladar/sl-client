---
id: viewer-radar-avatar-marks
title: Radar avatar marks — colour-tag nearby avatars
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-avatar-radar, viewer-contact-sets, viewer-radar-multi-select]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's radar lets you **Mark…** any nearby avatar Red / Green /
Blue / Purple / Yellow (plus clear mark / clear all marks); the mark
tints the radar row so individuals stay trackable in a crowd during
events. Marks are session-scoped and purely client-side.

**Half of this landed** with [[viewer-radar-multi-select]]: both radar
row menus now carry the reference's *Mark…* submenu (the five colours,
Clear Mark, Clear All Marks), writing the same session-scoped
`MinimapMarks` the minimap's own Mark submenu writes — one mark model,
two surfaces, as in the reference.

What is left is the **row tint**: a marked avatar currently shows their
colour only on the minimap dot (which is all the reference does too —
`fsradar` never reads the mark back), so the radar list itself gives no
sign. Tinting the row is cheap on the existing table-row styling and is
the part that makes a mark useful during a crowded event, which is why
this stays open as our own addition rather than parity work. Distinct
from contact sets ([[viewer-contact-sets]]), which are persistent named
groups — marks are throwaway per-session tags.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_fs_radar.xml`,
`menu_fs_radar_multiselect.xml`, `indra/newview/fsradar.cpp` (marks),
`indra/newview/fspanelradar.cpp`.
