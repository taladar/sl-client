---
id: viewer-parcel-ban-lines-on-refusal
title: Draw a parcel's ban line when the grid refuses entry
topic: viewer
status: ready
origin: gridspec-parcel-access-and-ban-lines (2026-10-05)
refs: [gridspec-parcel-access-and-ban-lines, viewer-parcel-ban-line-display,
  gridspec-sl-ban-line-trigger, viewer-minimap-parcel-overlay]
---

Context: [context/viewer.md](../context/viewer.md).

The viewer draws no ban lines. A refused avatar gets the notification (since
2026-10-05 the named one on Second Life) and nothing in the world shows where
the line is.

## What the grids send

Measured, `book/src/gridspec/land.md` § At the parcel's edge and § The ban
line:

- the **ban line** is a `ParcelProperties` for the closed parcel under a
  collision sequence id, read as `ParcelInfo::collision()`
  (`ParcelCollision::{NotInGroup, Banned, NotOnList}`); its `bitmap` is where
  the fence goes;
- the **refusal** is an `AlertMessage`: `NOTIFY: Cannot enter parcel: …` on
  Second Life, "You are banned from parcel" / "You do not have access to the
  parcel" on OpenSim;
- OpenSim pushes the line on every significant movement near the parcel;
  Second Life pushed it on some arrivals and on no approach
  ([[gridspec-sl-ban-line-trigger]]), so the line may be missing when the
  refusal comes.

## What to build

As the reference does (`llviewerparcelmgr.cpp` `processParcelProperties`,
`renderCollisionSegments`; `llviewermessage.cpp` `process_alert_message`):

1. keep the last collision record a region pushed — its kind and bitmap;
2. on a refusal, show the fence along that bitmap's edges for ten seconds
   (`PARCEL_BAN_LINES_DRAW_SECS_ON_COLLISION`), the red no-entry wall up to the
   line's height; in the reference's proximity mode, for one second after
   every push instead (`ShowBanLines`);
3. the same bitmap as the minimap's collision fill, which
   `sl-viewer-map/src/minimap.rs` notes it has no data for;
4. where a refusal comes with no record held — Second Life — ask for the
   parcel ahead of the avatar and draw its bitmap, which is what
   [[viewer-parcel-ban-line-display]] sketches region-wide.

Unit tests for the segment geometry from a bitmap; an `e2e` test once
[[server-fake-grid-parcel-access-enforcement]] makes the fake grid refuse
anybody.
