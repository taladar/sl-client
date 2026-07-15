---
id: viewer-parcel-ban-line-display
title: Region-wide parcel ban-line / access display
topic: viewer
status: ideas
origin: user request (2026-07); research spun out of viewer-parcel-overlay-decode
refs: [viewer-parcel-overlay-decode, viewer-parcel-grid-flood-fill, viewer-minimap-parcel-overlay]
---

Context: [context/viewer.md](../context/viewer.md).

Show, reliably and region-wide, which parcels the agent cannot enter — the red
"ban line" fences in-world and/or a ban/collision tint on the minimap. Not
committed to yet; this note captures the research so it is not lost.

## Why the reference viewer's ban display is unreliable region-wide

Ban lines and the parcel overlay travel on **two different channels**, and only
the overlay is a whole-region push:

- **Parcel overlay** (`ParcelOverlay`, the 64×64 grid decoded by
  [[viewer-parcel-overlay-decode]]) is pushed for the whole region on entry, but
  it carries **no ban/access information** — only the ownership colour class,
  `sound_local`, `hidden_avatars`, and the two property-line bits. (Note the
  trap: the overlay's `PARCEL_GROUP` *colour* means "deeded to your active
  group"; it says nothing about whether *access* is group-restricted. Ownership
  and access are orthogonal.)
- **Ban / collision lines** come from a **proximity push**: as the agent nears a
  parcel it cannot enter, the sim sends a special `ParcelProperties` tagged
  `COLLISION_BANNED_PARCEL_SEQ_ID` / `COLLISION_NOT_ON_LIST_...` /
  `COLLISION_NOT_IN_GROUP_...`, carrying a bitmap of the offending squares. The
  reference viewer writes that into `mCollisionSegments` /
  `getCollisionBitmap()` — for the **single** `getCollisionRegionHandle()` only.
  On the OpenSim side `SendOutNearestBanLine` literally `//Only send one` and
  returns after the first.

So the minimap ban tint (`llnetmap.cpp:renderPropertyLinesForRegion`, the
`fCollision` branch reading `getCollisionBitmap()`) and the in-world ban fences
(`llviewerparcelmgr.cpp` collision-segment path) are both fed by that single,
proximity-scoped, current-region-only bitmap. That is why they flicker in and
out and never cover the whole region — **not** a missing
`ParcelPropertiesRequest`. Requesting `ParcelProperties` for every parcel would
**not** fix it: a normal `ParcelProperties` reply carries no collision segments;
those arrive only via the `COLLISION_*` proximity push.

## What a reliable region-wide display would actually require

1. **Enumerate the parcels** from the overlay grid via
   [[viewer-parcel-grid-flood-fill]] (footprints + a representative square
   each).
2. **One `ParcelPropertiesRequest` per parcel** to learn each parcel's
   `local_id` and its `ParcelFlags` (`USE_ACCESS_GROUP`, `USE_ACCESS_LIST`,
   `USE_BAN_LIST`, `DENY_ANONYMOUS`, …). The reference viewer requests these for
   the current parcel only (`llviewerparcelmgr.cpp` ~1932), never region-wide.
3. **One `ParcelAccessListRequest(AL_BAN | AL_ACCESS)` per parcel** to learn who
   specifically is banned/allowed, then evaluate each parcel against the agent
   (owner? group member? on the access list? banned?) to classify it as
   enterable or not.
4. Render the result: ban fences in-world along the non-enterable parcels'
   boundaries, and/or a ban tint on the minimap — this time from a
   region-complete data set rather than the proximity bitmap.

All of the protocol pieces already exist in the client (`ParcelProperties`,
`ParcelFlags`, `ParcelAccessListRequest`); the missing work is the enumeration +
per-parcel sweep + the enterability evaluation, layered on the overlay grid.

Open question before promoting this out of `ideas/`: whether a full per-parcel
sweep on every region entry is worth the request volume, or whether it should be
lazy / opt-in (a toggle, or only when a ban/parcel overlay is actually shown).

Reference (Firestorm, read-only): `llviewerparcelmgr.cpp` (collision segments,
`processParcelProperties` `COLLISION_*` handling,
`sendParcelAccessListRequest`), `llnetmap.cpp` (`renderPropertyLinesForRegion`),
OpenSim `LandManagementModule.SendOutNearestBanLine`.
