---
id: viewer-block-list
title: Block / mute list UI
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-social-people-panel, viewer-ui-virtualized-list]
refs: [viewer-derender-blacklist]
---

Context: [context/viewer.md](../context/viewer.md).

The block-list surface over the fully-implemented mute protocol
(`protocol-9`): list every muted resident / object with type icons, unblock,
and the per-mute flag toggles (text / voice / particles / object sounds —
`MuteFlags`). The Vintage skin presents this as a **"Blocked Residents &
Objects" tab inside the People floater**, which is where ours goes; the
avatar and object context menus' Block / Unblock entries stay the quick path
in.

Includes the reference's **block-object-by-name** dialog (mute by name for
spammy objects you cannot click) and the mute-list-full error surface.
Distinct from render-side derendering ([[viewer-derender-blacklist]]) — this
is the server-side mute list.

Reference (Firestorm, read-only): `llpanelblockedlist`, `llmutelist`,
`floater_fs_blocklist.xml`, `floater_mute_object.xml`; Vintage
`panel_people.xml` (the added Blocked tab).

Builds on: `protocol-9` mute list and the People panel.
