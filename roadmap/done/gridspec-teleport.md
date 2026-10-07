---
id: gridspec-teleport
title: Teleport phases, flags, failures, cancel and access refusals on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, test-handover-distant-and-vehicle-aditi,
  viewer-own-avatar-broken-after-teleport, server-agent-transfer,
  protocol-agent-preferences-read-unanswered-on-sl,
  protocol-teleport-start-after-failure-rearms,
  server-fake-grid-teleport-edges, gridspec-landmarks-home,
  gridspec-teleport-lures]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-07)

Measured and written up in `book/src/gridspec/teleport.md`.

- **Discover.** Six scripted `sl-repl` runs with the trace log on (three per
  grid) read the raw messages; five cases then hold each grid to them.
  `teleport-local-phases`, `teleport-cross-region` and `teleport-failed`
  record the whole trace (order, every flags word, progress lines, landing,
  look-at, failure reason and alert) through a shared
  `sl_conformance::teleport_trace`; `teleport-cancel` and
  `teleport-access-refused` are new. All five ran on aditi, the first four on
  OpenSim, all five on both fake flavours. One OpenSim leg was by hand: a
  region set Moderate for a probe, and one set to an agent limit of 0.
- **Findings.** Second Life narrates a teleport (`resolving`, then the
  sentence `Sending to destination.`) and OpenSim sends no progress line.
  Second Life refuses *after* starting, over the **event queue**, with a key
  repeated in an `AlertInfo` (`no_host`, `nolandmark_tport`,
  `RegionTPAccessBlocked` with the region's rating in its parameters);
  OpenSim refuses *instead of* starting, over UDP, with a sentence. Second
  Life honours a cancel (`TPCancelled`); OpenSim's lands outside its window
  and the agent arrives. A local teleport is flagged `WITHIN_REGION` on
  Second Life and told to face the region's origin. OpenSim does not check
  the maturity preference at all.
- **Client.** The event-queue `TeleportFailed` was not handled: a refused
  teleport on Second Life ran into the session's thirty-second timeout. It
  now ends the teleport as the UDP message does. `Event::TeleportStarted`
  and `Event::TeleportLocal` carry their flags; both runtimes re-export
  `TeleportFlags` and `AlertInfo`. The REPL binds `$region` at login.
- **Fake grid.** `ImitatedGrid::teleport_policy`: progress lines, refusal
  transport and texts, the local teleport's flags and look-at, the
  `TeleportFinish`'s `LocationID` and region size, the answer to a cancel and
  the maturity check, per flavour. An event-queue failure or finish is held
  behind the acknowledgement of the last UDP line. The stored maturity
  preference is seeded from the account and carried across a teleport.
- **Viewer.** The teleport display shows a keyed failure as the notification
  catalogue's text (key and parameters kept beneath it) and a progress key as
  the reference's text; `DISABLE_CANCEL` hides its Cancel button, and Second
  Life's `TPCancelled` closes the display instead of reading as a failure.
  `e2e_pilot`
  drives a refused and an admitted teleport against the two flavours.
- **Not done here.** A successful landmark teleport's lines and Second
  Life's answer to a home teleport with no home ([[gridspec-landmarks-home]]);
  a lure's refusal ([[gridspec-teleport-lures]]); a teleport to a void region
  through `e2e_arrival` / `e2e_double_click` on a live grid (the conformance
  cases and the fake-flavour e2e cover the same events). The fake grid does
  not model a position past the region's edge, OpenSim's map block after a
  void teleport, a full region, or whether a source circuit is disabled per
  flavour ([[server-fake-grid-teleport-edges]]). Two client bugs were found
  and filed: [[protocol-agent-preferences-read-unanswered-on-sl]] and
  [[protocol-teleport-start-after-failure-rearms]].

## Known already

The fake grid mirrors OpenSim's `TransferAgent_V2`; failure keys
`invalid_tport` / `nolandmark_tport` / `no_host`; `CancelTeleport` unhandled.
OpenSim's local phase sequence and failure text are recorded; SL reports the
wire handle rather than the requested one; SL parcel landing points override
intra-region positions.

## Discover

- Run `teleport-local-phases`, `teleport-cross-region`, `teleport-failed` on
  aditi; record progress keys and order, `TeleportFlags`, timing.
- New probes: cancel mid-teleport; teleport into a maturity- or
  access-refused region; teleport to a void region.
- Viewer-observable via `e2e_arrival` / `e2e_double_click` with
  `SL_E2E_GRID`.

## Document

`book/src/gridspec/teleport.md`; link from `content/teleport.md`.

## Fake grid

Small to medium — in this task: per-flavour keys, strings, flags,
`CancelTeleport`, refusals.

## Viewer

Failure alerts and cancel as each grid answers them; world reset vs keep; `e2e`
on both flavours.
