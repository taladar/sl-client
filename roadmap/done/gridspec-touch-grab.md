---
id: gridspec-touch-grab
title: Touch and grab on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-sim-script-messages,
  server-world-touch-and-grab, server-world-collision-and-physics,
  viewer-left-click-touches-a-prim-that-takes-none,
  viewer-left-click-click-actions, viewer-touch-cursor,
  viewer-build-tool-row-parity, gridspec-lsl-events,
  gridspec-lsl-detection-sensors, gridspec-lsl-physics-vehicles]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-12)

Measured and written up in `book/src/gridspec/building.md` § Touch and grab.

- **Discover.** `object-touch-grab` rewritten as a census with a second
  avatar standing by and a probe script that says every touch event it is
  handed, with what it detects, to its owner: a click with no surface data,
  with it and with data no prim has; a held button with no update, with
  updates and with updates that carry no surface; a release with no press,
  two presses and a release, updates with no press; another avatar's
  click; a click with the click action set to sit and to disabled; the
  flags a prim carries as a script with touch handlers lands, is replaced
  by one without and goes; a two-prim linkset with a script in the root,
  in both, and the child's with and without handlers under each value of
  `llPassTouches`; a drag, a drag with no grab and a spin of a prim with no
  script, by its owner and by the other avatar, and of one with a script;
  and a spin and a drag of a physical prim. Seven runs on aditi (Mauve)
  and eight on OpenSim (YEngine).
- **Findings.** Second Life raises `touch` **on its own clock** while a
  grab is open, 22.5 a second, whatever the viewer sends meanwhile, and
  once for a bare click; OpenSim raises one **for each `ObjectGrabUpdate`
  and never otherwise**. Both hand a script the `SurfaceInfo` block as the
  viewer sent it, nonsense included, and neither withholds a touch for a
  click action. A second press is ignored on Second Life and is a second
  `touch_start` on OpenSim; an update with no press opens a touch on
  Second Life. `llDetectedGrab` and `llDetectedType` differ.
  `PASS_NEVER` passes on OpenSim (any value but 0 does), and a child with
  no handler under it tells nobody on Second Life. A child of a linkset
  whose root takes touches carries the touch flag on Second Life and not
  on OpenSim. **An owner's `ObjectGrabUpdate` moves a prim that is not
  physical to the grab position on both grids**, with or without a grab
  before it, unless a script in it takes touches; an `ObjectSpinUpdate`
  gives it the rotation stated. OpenSim sends a terse update for every
  update of a held button, moved or not. A physical prim is barely pushed
  on OpenSim and pulled across the region on Second Life.
- **Fake grid.** A new `grabs` module moves and turns a dragged prim as
  measured, a `RegionChange::Moved` tells the other sessions in a terse
  update, and `EditPolicy::refused_drag` is the one row the
  grids differ in. The plain-prim legs of the census run offline on both
  flavours, held to seventeen measured answers. What a script is handed
  stays [[server-world-touch-and-grab]], which now has the measured model
  to build to.
- **Viewer.** A left click is a touch from press to release (`ObjectGrab`
  … `ObjectDeGrab`; it sent the two at once), with an `ObjectGrabUpdate`
  after the grab and on every move of the cursor over the surface — for a
  prim whose script takes touches only, since an update would move any
  other. A world touch states its position, normal and binormal in the
  region's frame (it stated the prim's own, which both grids hand a script
  unchanged). New world tests, and an `e2e_pilot` test on each fake
  flavour.
- **Not done here.** `sl-viewer-ctl world touch` against a live grid: the
  viewer's left click is covered by the fixture and fake-grid tests, and
  what a live grid makes of the messages by the census. A click on a prim
  that takes no touch still sends a grab and a degrab
  ([[viewer-left-click-touches-a-prim-that-takes-none]]); click actions
  ([[viewer-left-click-click-actions]]); the cursor
  ([[viewer-touch-cursor]]); dragging and spinning from the viewer
  ([[viewer-build-tool-row-parity]]). Second Life's pull on a physical
  prim was not understood ([[server-world-collision-and-physics]]). Not
  measured: touches on attachments and HUDs, from a distance, by two
  avatars at once, a press never released, a locked prim, a prim anybody
  may move, `STATUS_BLOCK_GRAB` ([[gridspec-lsl-physics-vehicles]]),
  XEngine ([[gridspec-lsl-events]]).

## Known already

Unacknowledged on both grids (the case checks circuit health); the fake grid
does not decode grabs.

## Discover

Run `object-touch-grab` on aditi; grab-drag of an owned non-physical prim;
physical throw; `sl-viewer-ctl world touch` live.

## Document

`book/src/gridspec/building.md` § Touch and grab.

## Fake grid

Large — [[server-world-touch-and-grab]].

## Viewer

Touch / grab via automation on both flavours.
