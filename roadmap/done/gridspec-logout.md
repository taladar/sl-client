---
id: gridspec-logout
title: Logout reply, timing and what logout does to seats and child circuits
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-logout-reply-sometimes-missing-on-opensim,
  gridspec-sit-stand, gridspec-neighbours-crossing, viewer-quit-progress,
  inventory-logout-reply-items, server-fake-grid-agent-avatars-shared]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-06)

Measured and written up in `book/src/gridspec/session.md` § Logout.

- **Discover.** `logout-clean` now logs out twice — settled, with its child
  circuits open, and the moment a second login's region is up — and records
  each answer and the login between them. The new two-avatar `logout-seated`
  sits one avatar on a cube it rezzed and logs it out under the other's eyes.
  Beside them, 21 scripted `sl-repl` logouts on OpenSim at seven hold times
  and one on aditi from a region with five neighbours, read off the
  `sl_proto::wire` trace.
- **Findings.** Second Life answers every logout in about 0.17 s. OpenSim's
  `LogoutReply` is a race with its own synchronous close: 5 of 20 settled
  logouts were answered and 0 of 15 early ones, and an unanswered request is
  never acknowledged either. The close always happens — the next login got
  in every time, so the "ghost presence" this was believed to leave does not
  exist. Both grids' replies carry an `InventoryData` block: the nil id on
  OpenSim, four item ids on a settled Second Life logout
  ([[inventory-logout-reply-items]]). OpenSim sends `DisableSimulator` down
  each child circuit; Second Life sends nothing down them before its reply.
  A seated logout on OpenSim re-sends the seat, kills the avatar for the
  observer 30 ms after the request, and the next login stands 1 m away.
- **Fake grid.** `ImitatedGrid::logout_reply`: the OpenSim flavour withholds
  the reply and the acknowledgement (`SimSession::set_withholds_logout_reply`).
  Both flavours' replies now carry the one nil `InventoryData` block the live
  grids send; it used to be an empty list, which neither does.
  `logout-clean` stays in the offline list for both flavours.
- **Viewer.** The quit deadline was three seconds, shorter than the session's
  five-second logout timeout, so every unanswered quit was ended by force
  before the session had finished (and saved the inventory cache). It is now
  derived from `sl_proto::LOGOUT_TIMEOUT`, newly public, plus two seconds.
  `e2e_login` quits on both flavours. No "Logging out..." feedback yet
  ([[viewer-quit-progress]]).
- **Also.** The conformance flight helper (`support::steer_towards`) steered
  on the last reported position, and Second Life reports none while the
  velocity holds: the avatar crossed the target at 14 m/s and swung over it
  until the budget ran out. It now reckons the position between updates and
  coasts into the target; the flight to the aditi build spot takes 6 s where
  it took 90 s or failed.
- **Not done here.** The seated logout on **Second Life**: aditi did not
  answer the `AgentRequestSit` on any of three runs — no `AvatarSitResponse`,
  no alert — so `logout-seated` fails there before it logs out. Handed to
  [[gridspec-sit-stand]]. `logout-seated` is live-only: the fake grid's
  residents are not shown to each other
  ([[server-fake-grid-agent-avatars-shared]]).

## Known already

SL answers `LogoutReply` in about 0.18 s; OpenSim queues and then drops it
(the client hits its 5 s timeout), and sometimes leaves the `LogoutRequest`
unacked ([[protocol-logout-reply-sometimes-missing-on-opensim]]). The fake
grid replies promptly on both flavours.

## Discover

`logout-clean` on both grids, repeated to see whether the OpenSim drop is
deterministic; logout while seated and with child circuits up (what each
grid sends, whether the ghost presence remains).

## Document

`book/src/gridspec/session.md` § Logout.

## Fake grid

Small — an `ImitatedGrid` row: OpenSim withholds the reply.

## Viewer

The timeout fallback must look identical to the user; quit flow on both fake
flavours.
