---
id: viewer-minimap-avatar-dot-color
title: Minimap other-avatar dots are red, not green like the reference
topic: viewer
status: bugs
origin: user report (2026-08-07)
refs: [viewer-minimap-avatar-dots, viewer-minimap]
---

Context: [context/viewer.md](../context/viewer.md).

Other avatars render on the minimap as **red** dots. In the reference viewer
they are **green** (`MapAvatarColor`); red is reserved for the tracking beacon
(`MapTrackColor`), so today the two collide — a tracked location and a nearby
avatar are indistinguishable.

The wrong constant is in `minimap_math.rs`:

- `COLOR_AVATAR` (`MapAvatarColor`) is `[255, 0, 0, 255]` (red) — should be the
  reference green.
- For comparison the tracking beacon `COLOR_TRACK` (`MapTrackColor`) is also
  `[255, 0, 0, 255]` (red), which is correct — so the avatar dot must move off
  red to stop the collision.
- `COLOR_AVATAR_FRIEND` (`MapAvatarFriendColor`) is already green
  `[0, 255, 0, 255]`. If the base other-avatar colour also becomes plain green,
  friends and non-friends would look identical, so pick the reference's actual
  defaults for **both** (the reference distinguishes them — e.g. a
  yellower/olive green for non-friends vs. a brighter green for friends) rather
  than making them the same green.

Fix: take the real default RGB values from the reference `colors.xml`
(`MapAvatarColor`, `MapAvatarFriendColor`, and while there confirm
`MapAvatarSelfColor` / `MapAvatarLindenColor`) and correct the constants, with a
test pinning that the other-avatar dot is not red (so it can never collide with
the track beacon again).

Reference (read-only): `indra/newview/skins/default/colors.xml`
(`MapAvatarColor` and the `MapAvatar*Color` family), `llnetmap.cpp` dot
rendering.
