---
id: gridspec-lsl-persistence
title: Script state across take, rez and region restart on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-script-persistence,
  viewer-disconnect-screen, gridspec-aditi-test-land]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Unmeasured on SL; OpenSim YEngine / XEngine keep state files, lost crossing
engines.

## Discover

Globals, state, timer, queue and pending sleep across take / rez; region restart
on OpenSim (and aditi if rights allow); `CHANGED_REGION_START`.

## Document

`book/src/gridspec/lsl.md` § Persistence.

## Fake grid

Large — [[server-lsl-script-persistence]].

## Viewer

Script recovery.

## From gridspec-lsl-events (2026-10-25)

`events/attach` measured the take and rez leg (`book/src/gridspec/lsl.md`
§ Events): the runner's `rerez` drive takes prim A into the owner's Trash
and rezzes it back. Second Life and YEngine keep the script's state — a new
key, `on_rez(0)` alone, the timer and a listen carry on — and **XEngine
resets the script**: `state_entry` runs, the globals are gone, then
`on_rez(0)`. The region restart (`CHANGED_REGION_START` and what runs first
after it) is left here: aditi needs estate rights for a restart and the
runner does not survive one.

## Done (2026-10-26)

- **Discover.** Two probes, `sl-lsl-runtime/probes/persistence/`: `rerez`
  (take into inventory and rez again) and `restart` (a region restart), each
  with a script that changes every global, goes into a state of its own,
  holds a timer, a listen and two permissions, stops a second script, then
  queues three link messages and sleeps through the take or the restart.
  `rerez` complete on all three grids; `restart` complete on both OpenSim
  engines, and on aditi it waits its patience out, since no restart is sent
  without estate rights.
- **Runner.** A `restart` drive: the runner keeps the credentials'
  `estate-owner` avatar on call (`GridTest::estate_on_call`,
  `context::StandbyLogin`), logs it in only to send the restart, hears the
  owner out until the region drops it, logs both avatars in again — retrying
  a login that OpenSim, while the region is down, lands in a neighbour — and
  renews the prims, whose keys a restart keeps. The restart's time is given
  back to the probe's deadline.
- **Found on the way.** Second Life and YEngine keep everything across a
  take and a rez, but YEngine ends a sleep in progress at the take while
  Second Life lets it run, and the two deliver the queued events on either
  side of `on_rez`. XEngine resets the script on a rez and starts the script
  it had stopped again — and it does the latter across a restart too, though
  a restart keeps its globals, state, permissions and queue; it loses the
  sleeping handler and the script time. YEngine keeps everything across a
  restart and ends the sleep at the shutdown. Both send `changed(32768)` then
  `CHANGED_REGION_START` and nothing else. An owner-say to an owner who is
  logged out is lost, so the restart probe holds its lines until the owner is
  back.
- **Document.** `book/src/gridspec/lsl.md` § Persistence, linked from
  `content/scripts.md` and from § Events; the `restart` drive, its notes and
  the absent-owner lesson in `book/src/tools/lsl-probes.md`.
- **Fake grid.** Large, as planned: the measured behaviour per flavour is
  noted on [[server-lsl-script-persistence]].
- **Viewer.** Nothing of a script's state reaches the viewer. The restart
  notice already opens the countdown card on both flavours (`e2e_windows`);
  the kick ends the session as any disconnect does, which
  [[viewer-disconnect-screen]] replaces, and it now notes that a login again
  while the region is still down lands in a neighbour on OpenSim. Second
  Life's restart column waits on [[gridspec-aditi-test-land]], which says so.
