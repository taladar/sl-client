---
id: gridspec-lsl-task-inventory
title: LSL library behaviour on each grid — task inventory, giving, rezzing and notecards
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-task-inventory]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

No aditi or OpenSim measurements for this tranche beyond the library table's
documented values.

## Discover

Probes for: Inventory ordering, rez throttle, `object_rez` / `on_rez` order,
notecard line limits. Run on OpenSim YEngine / XEngine and aditi.

## Document

`book/src/gridspec/lsl.md` § Task inventory, giving, rezzing and notecards.

## Fake grid

Large — [[server-lsl-lib-task-inventory]].

## Viewer

Offers from objects.

## Done (2026-10-27)

Five probes, `inventory/listing`, `inventory/notecards`, `inventory/rez`,
`inventory/give` and `inventory/newer`, run on aditi, YEngine and XEngine;
the reading is `book/src/gridspec/lsl.md` § Task inventory, giving, rezzing
and notecards. The runner learned to put notecards (`<name>.notecard` beside
a probe's scripts) and objects (the `object` header key) into a probe's prims,
and to note an object's inventory offer (IM dialog 9). The viewer already
shows offers from objects (`sl-viewer-people`'s `offers_invites`); nothing
needed changing there. The rez throttle did not show at twenty rezzes in a
row on any grid; a longer burst belongs to `gridspec-lsl-throttles`.
