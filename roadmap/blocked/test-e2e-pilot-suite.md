---
id: test-e2e-pilot-suite
title: The first end-to-end tests — one viewer, two viewers, two regions
topic: test
status: blocked
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [test-e2e-stage]
refs: [test-e2e-live-grids, server-world-chat-routing]
---

Context: [context/automation.md](../context/automation.md).

The mechanism is only proven by tests that would otherwise be manual. These
use only what the fake grid already supports, and each runs on both
backends.

## Wanted

- **Login and chrome**: a viewer logs in; the status bar shows the region
  name; the menu bar's World ▸ Build opens the Build floater.
- **Two viewers, one object**: viewer A rezzes a prim from the Build
  floater; viewer B sees it (the region's shared object store already
  broadcasts rezzes to other sessions), finds it by name, selects it, and
  its edit controls are **disabled** because B does not own it.
- **Pie menu**: right-click a fixture prim; the object pie opens with the
  expected slices; Touch sends the touch.
- **Two regions**: teleport from the catalogue region to its neighbour via
  the world map floater; the status bar and agent probe agree on the new
  region.
- **Two-viewer chat**: listed, but runs only on a live grid (skips on the
  fake grid) until [[server-world-chat-routing]] lands.

Acceptance: all but the chat test pass on both backends on the fake grid in
the pre-commit suite; the chat test passes against the local OpenSim.
