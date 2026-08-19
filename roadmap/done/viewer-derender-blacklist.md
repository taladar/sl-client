---
id: viewer-derender-blacklist
title: Derender + asset blacklist
topic: viewer
status: done
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-object-context-menu]
refs: [viewer-block-list, viewer-avatar-complexity-limit]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's derender: remove an object (or avatar) from *your* view —
temporarily (until region re-entry) or permanently via a persisted **asset
blacklist** — the everyday tool against visual griefing and against the one
laggy object a parcel owner will not remove. Client-side only, distinct from
the server mute list ([[viewer-block-list]]).

Scope: "Derender" / "Derender + blacklist" on the object and avatar context
menus, suppression at the scene-mirror level (object add/update for a
blacklisted id is dropped before meshing — cheap), the blacklist floater
(entries with name / region / date, remove / re-render), per-account
persistence, and blacklist of specific asset ids (sounds, textures) the
sound explorer and friends can feed later.

Reference (Firestorm, read-only): `fsassetblacklist`,
`floater_fs_asset_blacklist.xml`, the FS derender menu handlers.

Builds on: the object context menu and the scene mirror (`objects.rs`).

## Built

- **`derender.rs`** — the model: `DerenderList` (entries + a derived id set,
  the per-avatar JSON store beside the account `settings.toml`, and a
  revision the floater rebuilds on), the guarded `RequestDerender` /
  `UnDerender` messages, and the systems that bracket the scene mirror.
  `check_derender` runs the reference's `derenderObject` guards (never the
  own agent, never a null id); the apply also stands the agent up when the
  target is its own seat and drops the target from the edit selection
  (the reference's `stopEditing`).
- **Re-render actually re-renders.** Removing an entry (or clearing the
  temporary ones) releases exactly that root's suppressed subtree and queues
  its region-local ids for a re-fetch (`refetch_released_objects` →
  `Command::RequestObjects`, batched at 64 per message), so the object is
  back a round trip later.
- **Suppression at ingest, transitively.** `index_derendered_objects` keeps a
  `ScopedObjectId → blacklisted root` map, seeded from the blacklisted full
  ids and extended to any object whose *parent* is already suppressed, so a
  derendered linkset root takes its child prims and a derendered avatar takes
  its attachments. `update_objects` / `update_avatar_objects` /
  `apply_coarse` drop a suppressed object before it is applied or even
  buffered — no tessellation, no texture request, no material — which is
  where the reference refuses too (`LLViewerObjectList::createObject`).
  Anything already in the scene is despawned by `enforce_derender` (by full
  id for a fresh entry, by scoped id for an object that arrived before its
  parent did).
- **Temporary vs permanent.** A permanent entry is persisted per avatar and
  re-applied at login; a temporary one is dropped on the next
  `TeleportStarted`, gated by the new `TempDerenderUntilTeleport` setting
  (the reference's `FSTempDerenderUntilTeleport`, default on).
- **Menus.** The object pie's `More ▸ Derender ▸ Blacklist / Temporary` and
  both attachment pies' derender tails went live at their already-pinned
  reference addresses (one `when` edit each); the other-avatar pie gained the
  reference's `More ▸ Derender >` sub-pie at south-east (its two slices at
  the reference's south / south-east slots), and the radar's row menu gained
  Derender / Derender + Blacklist — the reference `fsradarmenu` pair, since
  the radar is where a griefer is spotted. The attachment pies target the
  worn **object**, not the wearer, so their two action names are dispatched
  by `attachment_menu` and deliberately skipped by the shared avatar handler.
- **`asset_blacklist.rs`** — the Asset Blacklist floater (World ▸ Asset
  Blacklist, the reference's own menu home): filter box over a sortable,
  virtualized Name / Region / Type / Date / Permanent table, with Re-render
  (drop the selected entry) and Clear temporary.

## Divergences

- **Asset entries are honoured, but nothing in the UI produces one yet.**
  `DerenderKind` carries the reference's asset kinds (`Sound`, `Animation`,
  `Texture`) and each is refused at its own point of use — a blacklisted sound
  never plays (`world_sounds.rs`, all three of `SoundTrigger` /
  `AttachedSound` / `PreloadSound`), a blacklisted animation is dropped from
  the authoritative `AvatarAnimation` set so it never starts and its asset is
  never fetched (`animations.rs`), and a blacklisted texture is refused at the
  fetch gate (`TextureManager::request_from`, mirroring the list by revision).
  What is missing is the **producers**: the reference feeds `Sound` from its
  sound explorer, `Animation` from its animation explorer, and `Texture` only
  from its distributed blacklist data — so notes were added to
  [[viewer-sound-explorer]], [[viewer-animation-explorer]] and
  [[viewer-texture-preview-floater]] pointing at the action each needs to add.
  Until then an asset entry can be hand-written into the per-account file,
  which is exactly how the reference's own texture entries arrive.
- **No per-flag sound entries.** The reference's
  `eBlacklistFlag` (silence an avatar's *worn* / *rezzed* / *gesture* sounds
  rather than one asset) and the floater's Flags column belong with the sound
  explorer that produces them; noted there.
- **No Play / Stop Sound buttons** in the floater — they preview a blacklisted
  sound asset, which needs that same explorer.
- **Re-render is better than the reference's.** Firestorm only forgets the
  entry, leaving the object absent until the region streams it again (a
  teleport away and back). Ours restores it: the suppression index kept every
  hidden object's *region-local* id, so releasing an entry re-emits those
  objects from the session's own cache and they reappear at once.
  (Superseded detail, from [[viewer-render-friends-only]]: this first used a
  `RequestMultipleObjects` full cache miss, which works for prims but *never*
  for avatars — simulators resolve that message against prims only — so it was
  replaced by `Command::ResendCachedObjects`, which needs no round trip and
  covers both.)
- **No RLV guard on own attachments.** The reference refuses to derender
  your own attachment while RLV is enabled; RLV enforcement is a separate
  (blocked) family here, so the guard joins it there.
- **Single-select list**, like the block list; the reference's multi-select
  removal is a Firestorm addition.

## Verification

Unit-tested: the list algebra (indexing, the temporary → permanent upgrade
and its non-downgrade, removal, temporary clearing, the persistence dirty
flag), the request guards, the JSON round trip, and the floater's projection
(filter over name *or* region, every sort key, the local-time date stamp and
its out-of-range fallback). The avatar pie's pinned address table gained the
two new derender addresses.

Live-verified against the local OpenSim: derendering a prim from the object
pie removes it at once, the entry is listed in the floater and persisted to
`derender_blacklist.json` in the account directory, and Re-render brings the
object back.

The re-fetch was not in the first cut and the first live run exposed why:
the suppression index only learned a region-local id from a **wire update**,
and the simulator streams a static object once — so derendering a prim that
had stood there since login recorded nothing, and the release had nothing to
ask for. The purge is the other place that knows those ids, so
`enforce_derender` now records everything it despawns (`derender_remove`
returns the root and its descendants). Second trap found on the way: a
released set can span circuits, and `RequestMultipleObjects` goes out on one —
`split_scoped_object_ids` rejects a mixed batch and the command layer swallows
the error — so the re-fetch groups by circuit.

Live checks still to do: derendering an avatar with attachments, the blacklist
surviving a relog, and the temporary entries clearing on teleport.

Follow-up from [[viewer-render-friends-only]]: a derendered avatar now also
keeps its (hidden) coarse placeholder rather than being dropped from the
position path, so it stays on the radar and minimap — which is what the
reference does too (`FSRadarShowMutedAndDerendered`), and what stops the radar
reporting a derender as a region leave.
