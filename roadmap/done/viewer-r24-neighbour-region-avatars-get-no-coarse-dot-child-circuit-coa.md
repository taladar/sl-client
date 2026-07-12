---
id: viewer-r24
title: Neighbour-region avatars get no coarse dot — child-circuit CoarseLocationUpdate was dropped
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R24. Neighbour-region avatars get no coarse dot — child-circuit
`CoarseLocationUpdate` was dropped.** **Fixed.** `Session::dispatch_child`
folded a neighbour region's object stream in (via `try_dispatch_object`) but
had no arm for its `CoarseLocationUpdate`, so that message fell through to the
unhandled-message diagnostic — only the *root* region's coarse (minimap) list
reached the viewer, so an avatar present only in a neighbour region was never
even placed as a coarse "blue sphere". Now both the root and child dispatch
build the event via a shared `coarse_location_event` helper that tags it with
the source circuit's `region_handle` (a new field on
`Event::CoarseLocationUpdate`), and the viewer offsets a neighbour region's
dots by `region − origin` metres (the same convention terrain uses, via the
now-shared `metres_to_f32`) so they land on the right neighbour terrain rather
than overlapping the home region. The viewer reconciles coarse dots **per
region** (tracking each dot's source region), so a neighbour's update never
despawns another region's dots; and `DisableSimulator` emits an empty
`CoarseLocationUpdate` for the retiring region so its dots are dropped rather
than left stale. Surfaced while investigating R22b but *separate* from it
(that was a parcel-privacy case, root-region avatars).
