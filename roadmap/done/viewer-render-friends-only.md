---
id: viewer-render-friends-only
title: Show friends only (hide non-friend avatars)
topic: viewer
status: done
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

## Built

Note (2026-08-19, from the user): this is **not** a moderation feature. It is
what people reach for to attend a crowded event on hardware that cannot draw
the crowd — "my PC can't handle this", not "I don't want to see you". Two
design consequences, both taken:

- **The cost avoided must be the body, not the pixels.** The gate is the scene
  mirror's ingest, so a hidden avatar never builds a skeleton, never fetches
  bakes, and — through the suppression index's parent walk — never spawns its
  attachments either. Attachments are what actually sink a crowded region.
- **Presence must survive.** You still want to know who is there. So the cheap
  coarse placeholder is kept and merely made invisible
  (`hide_suppressed_avatars`), leaving the radar and the minimap fully
  populated while the world draws only friends.

Implementation, sharing `derender.rs` wholesale rather than duplicating it:

- **One suppression index, two sources.** `HiddenBy::{Blacklist, FriendsOnly}`
  tags every entry in the scoped index, so the filter reuses the ingest gate,
  the transitive parent walk, the purge and the release-with-re-fetch. A
  release is exact: befriending one avatar frees that avatar's subtree, turning
  the filter off frees everything it hid, and neither disturbs a blacklist
  entry.
- **Not one-way, unlike the reference.** Turning the filter off (or gaining a
  friend) queues the freed region-local ids for the `RequestMultipleObjects`
  re-fetch, so people come back within a round trip instead of at the next
  region entry — the flaw this task was written to avoid.
- `sync_friends_only_filter` mirrors the toggle, the own agent and the friends
  set (by `FriendsModel` revision) into the list, so the per-object gate stays
  one hash lookup; `resync_friends_only` applies the difference.
- **Controls:** World ▸ Show Friends Only (checked entry) and a Quick
  Preferences checkbox — the panel you can open mid-lag. Per avatar, like the
  reference. `RenderFriendsOnlyPersistsTP` (default off) keeps the reference's
  escape hatch: leaving is the natural moment to stop hiding people.

Also changed in `viewer-derender-blacklist`'s code as a consequence: a
derendered avatar likewise keeps its coarse placeholder (hidden) instead of
being dropped from the position path, so it stays on the radar — which is what
the reference does too (its radar lists derendered avatars, gated by
`FSRadarShowMutedAndDerendered`).

## Divergences

- **Animesh is exempt for free.** The reference needs an explicit
  `!avatar->isControlAvatar()`; a control avatar reaches our gate as an
  ordinary mesh object (never `pcode` 47), so the filter cannot see it.
- **No per-avatar "always render fully" override** — that is
  [[viewer-avatar-render-settings-manager]], which layers over the same
  suppression once the complexity limiter lands.

## Verification

Unit-tested: the predicate (spares friends and yourself, hides everyone else,
inert while off, and the scene gate being the union of both sources) and the
resync (purges whom it now hides, re-fetches whom it no longer hides, never
touches a blacklist suppression).

Live-verified against the local OpenSim with a second (non-friend) avatar:
toggling the filter on removes the body while the radar row stays put, toggling
it off brings the avatar back at once, and the radar raises **no** alerts
across either edge.

Two bugs the live runs found, both now fixed and both worth knowing:

- **`RequestMultipleObjects` cannot restore an avatar.** Simulators resolve it
  against prims (OpenSim's `Scene.RequestPrim` does `GetSceneObjectPart`; an
  avatar is a `ScenePresence`), so the request went out and the simulator
  answered with silence. The release path now re-emits from
  **our own session cache** instead — `Command::ResendCachedObjects` /
  `Session::resend_cached_objects`, new in `sl-proto` and wired through both
  runtimes — which costs no round trip, carries data the motion updates keep
  current, and works for avatars and prims alike. `RequestObjects` keeps its
  original job (a genuine cache miss) with its prim-only limit now documented.
- **Presence must be swapped atomically.** Despawning the body and waiting for
  the next `CoarseLocationUpdate` to spawn the placeholder left the avatar
  unrepresented for up to a second, and the radar — which tracks presence
  through whatever entity represents an avatar — reported a leave and then an
  enter, with the range-crossing alerts that ride along. `derender_agent` now
  spawns the hidden placeholder in the same frame, at the body's last pose.

Live checks still to do: a genuinely crowded region (the frame-time win this
feature exists for), and the befriend-while-filtered path.
