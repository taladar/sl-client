---
id: gridspec-avatar-presence
title: Other avatars in the region: full updates, coarse locations and kills
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-near-avatar-stuck-coarse-sphere,
  server-presence-service, server-fake-grid-agent-avatars-shared,
  gridspec-aditi-test-land, gridspec-friends-presence]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-08)

Measured and written up in `book/src/gridspec/avatars.md`.

- **Discover.** Two new conformance cases. `avatar-presence` has one
  resident act while a second only listens: both idle, a `TrackAgent`, a
  walk, a seventy-second climb away from a watcher whose draw distance is
  64 m, that watcher leaving and arriving again while the other hovers, a
  teleport to a neighbouring region and back, and a logout and login. Three
  runs on aditi, five on OpenSim. `parcel-privacy` (OpenSim only, as the
  estate owner) chops a corner off the region's parcel, turns `SeeAVs` off
  there and moves two residents in and out.
- **Findings.** The coarse feed comes every 1.333 s on Second Life and
  every 4.54 s on OpenSim, lists the agent itself with `You` on its entry,
  and never set `Prey` on either grid. Second Life rounds a position into
  an entry and OpenSim cuts it off; above what a byte holds Second Life
  states 1,020 m and OpenSim zero. **Neither grid withholds a same-region
  avatar for being far away**: one 1,123 m above a watcher with a 64 m draw
  distance was never killed on aditi, and was sent afresh, 1.2 s in, to that
  watcher arriving anew; OpenSim the same at 1,150 m. A departure's
  `KillObject` takes 0.3 to 0.4 s on
  Second Life after a teleport and 0.6 s after a logout, 0.2 s and 15 ms on
  OpenSim; the coarse entry goes with the feed's next update. A neighbour's
  avatars and its coarse feed come down the child circuit on both. On
  OpenSim a hidden parcel hides one way only (the one inside keeps seeing
  out), hides its own owner standing outside, and never touches the coarse
  feed.
- **Fake grid.** The coarse feed on each flavour's timer
  (`ArrivalPolicy::coarse_interval`, `sl_proto::CoarseRounding`), listing
  the NPCs and the agent; `avatar-presence` runs offline against both
  flavours for the feed's interval, self-listing and `You`. Everything that
  takes a second resident is [[server-fake-grid-agent-avatars-shared]],
  which now carries the figures.
- **Viewer.** It already keeps the two channels apart. A new
  `e2e_two_avatars` test takes a resident's avatar away while the coarse
  feed goes on listing it and sends it again: exact, approximate, exact on
  the radar. Two live viewers on each grid list each other on the radar and
  offer a teleport from the row (the existing
  `a_live_teleport_offer_is_declined_in_silence_and_then_taken`, run with
  `SL_E2E_GRID=opensim` and `=aditi`).
- **Not done here.** `SeeAVs` on Second Life (no land:
  [[gridspec-aditi-test-land]]). `Prey` for a resident who granted map
  rights ([[gridspec-friends-presence]]). Because nothing was ever taken
  out of the stream, what a larger draw distance or a camera beside the
  avatar brings back never ran: the case keeps those legs for a grid that
  does cull. The minimap's dots were checked against the fake grid only.
- **[[viewer-near-avatar-stuck-coarse-sphere]] is open again**: its
  draw-distance explanation does not survive the measurement.

## Known already

The fake grid shows only NPC fixtures and never sends
`CoarseLocationUpdate`; on aditi a coarse-only avatar never got a full update
([[viewer-near-avatar-stuck-coarse-sphere]]).

## Discover

Two avatars: coarse update cadence and range, `you` / `prey` indices, the
interest range for full avatar updates, parcel privacy (`SeeAVs`) hiding,
kill-on-departure timing; radar / minimap via the two-viewer automation.

## Document

`book/src/gridspec/avatars.md`.

## Fake grid

Large — [[server-fake-grid-agent-avatars-shared]].

## Viewer

Promote a coarse dot to a body; hidden avatars; `e2e_two_avatars`.
