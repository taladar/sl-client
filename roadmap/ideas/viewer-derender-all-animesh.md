---
id: viewer-derender-all-animesh
title: One-click derender of all animated objects
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-derender-blacklist, viewer-avatar-complexity-limit]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm ships a toolbar command (`derender_animated_objects`,
handler `Tools.DerenderAnimatedObjects`) that derenders every animated
object (animesh / control avatar) currently in the scene in one click
— a blunt crowd-event lag lever, distinct from per-object derender and
from avatar complexity limiting ([[viewer-avatar-complexity-limit]]
jellydolls avatars but leaves animesh meshes rendering).

Our derender core is per-object (`sl-client-bevy-viewer/src/
derender.rs` + `asset_blacklist.rs`, [[viewer-derender-blacklist]]
done); the new piece is a "sweep all current animesh into the session
derender list" action — plus deciding whether it also blacklists
persistently, as Firestorm optionally does via fsassetblacklist. Value
is situational (crowded events), hence ideas.

Reference (Firestorm, read-only):
`indra/newview/app_settings/commands.xml` (command
`derender_animated_objects`), `indra/newview/llviewermenu.cpp`
(handler), `indra/newview/fsassetblacklist.cpp`.
