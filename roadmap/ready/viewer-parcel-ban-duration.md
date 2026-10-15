---
id: viewer-parcel-ban-duration
title: Parcel ban duration picker
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-parcel-options-access-media, test-parcel-access-list]
---

Context: [context/viewer.md](../context/viewer.md).

When adding an avatar to the About Land ban list, the reference viewer
pops a small Ban Duration floater (`floater_ban_duration.xml`,
`llfloaterbanduration.cpp`, invoked from `llfloaterland.cpp`) offering
"always" versus an hours-limited ban; a limited ban fills the entry's
expiry time field in the `ParcelAccessListUpdate` message so the sim
lifts the ban automatically.

Our About Land Access tab ([[viewer-parcel-options-access-media]], done;
now `sl-viewer-places/src/about_land.rs`) adds ban entries with no
duration choice — the add path pushes a `ParcelAccessEntry` with
`time: 0`, so every ban is permanent; the only hours field we have is
pass-hours. The allow / ban tables already have an expiry column
(`expiry_text`: "Always" for `0`, else the date), so a timed ban made
elsewhere already shows when it ends. What remains is a small duration
dialog (or inline choice) on the Add-to-ban flow that sets the
access-list entry's time field (and, if wanted, the reference's
remaining-hours wording instead of a date). The wire side is already
exercised by [[test-parcel-access-list]].

Reference (Firestorm, read-only):
`indra/newview/llfloaterbanduration.cpp`,
`indra/newview/llfloaterland.cpp`,
`indra/newview/skins/default/xui/en/floater_ban_duration.xml`.
