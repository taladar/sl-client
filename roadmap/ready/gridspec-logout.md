---
id: gridspec-logout
title: Logout reply, timing and what logout does to seats and child circuits
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-logout-reply-sometimes-missing-on-opensim]
---

Context: [context/gridspec.md](../context/gridspec.md).

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
