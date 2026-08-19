---
id: viewer-render-friends-only
title: Show friends only (hide non-friend avatars)
topic: viewer
status: ready
origin: user request (2026-08-19), alongside viewer-derender-blacklist
refs: [viewer-derender-blacklist, viewer-social-people-panel,
  viewer-avatar-complexity-limit]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's **Show Friends only** (`World ▸ Show Friends only`,
`FSRenderFriendsOnly`): a per-avatar toggle that stops rendering every avatar
who is not on your friends list — the blunt instrument for a laggy or hostile
crowd, next to the per-target derender ([[viewer-derender-blacklist]]) and the
automatic complexity cap ([[viewer-avatar-complexity-limit]]).

Scope:

- A persisted per-account setting, toggled from the World menu (a checked
  entry) — and worth a Quick Preferences line, since it is reached for exactly
  when a region is already unusable.
- Suppression through the **same path derendering uses**: the ingest gate in
  `update_avatar_objects` / `apply_coarse` plus the scoped suppression index in
  `derender.rs`, so a hidden avatar's attachments go with it for free. The
  predicate is the only new part: `pcode == AVATAR && !is_friend(id) && id !=
  own`, over `FriendsModel`
  ([[viewer-social-people-panel]]). The reference
  exempts control ("animesh") avatars — ours must too, since an animesh object
  is content, not a resident.
- Turning it **on** despawns the non-friends already in the scene (the
  reference's `handleRenderFriendsOnlyChanged`).
- The reference's `FSRenderFriendsOnlyPersistsTP` companion (clear the toggle
  on teleport unless the user asked it to stick) is worth porting: it exists
  because people forget the option is on.

**Do better than the reference on the way back.** Firestorm's toggle is
one-way in practice: turning it *off* only stops the suppression, and the
hidden avatars stay gone until the region streams them again (a teleport away
and back), which reads as a bug. The derender work solved exactly this — the
suppression index keeps every hidden object's region-local id, so releasing a
suppression can queue those ids for a `RequestMultipleObjects` full cache miss
(`refetch_released_objects`) and the avatars come back within a round trip.
Reuse that release path here rather than reproducing the reference's one-way
behaviour.

Reference (Firestorm, read-only): `FSRenderFriendsOnly` /
`FSRenderFriendsOnlyPersistsTP` in `settings.xml`,
`LLViewerObjectList::isNonFriendDerendered`,
`handleRenderFriendsOnlyChanged` (`llviewercontrol.cpp`), the `menu_viewer.xml`
World entry, and the teleport reset in `llviewermessage.cpp`.

Builds on: the derender suppression / release machinery
([[viewer-derender-blacklist]]) and the friends model.
