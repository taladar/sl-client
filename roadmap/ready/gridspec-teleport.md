---
id: gridspec-teleport
title: Teleport phases, flags, failures, cancel and access refusals on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, test-handover-distant-and-vehicle-aditi,
  viewer-own-avatar-broken-after-teleport, server-agent-transfer]
---

Context: [context/gridspec.md](../context/gridspec.md).

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
