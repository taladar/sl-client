---
id: test-e2e-pilot-suite
title: The first end-to-end tests — one viewer, two viewers, two regions
topic: test
status: done
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

## Outcome (2026-09-30)

`sl-client-bevy-viewer/tests/e2e_pilot.rs`, in nextest's `e2e` group, each
test on both backends against the fake grid (about a minute for the five in
a release build):

- **Login and chrome**: the status bar's region read-out
  (`status-readout:region`, the text the bar shows) names the region, and
  Build ▸ Build Tools opens the Build window. This viewer keeps the build
  window under its own Build menu, not under World.
- **Two viewers, one object**: Alpha opens the Build window (on the Create
  tool), rezzes on the stock box (`WorldAction::Place`, new), drops into edit
  and names the prim through the General tab. Beta finds it by that name,
  switches to the Move tool, selects it (its selection probe is Alpha's prim)
  and its position and size fields are disabled; Alpha's are enabled.
- **Pie menu**: a test prim put on the grid through `FakeAgent::with_world`,
  flagged as a touch-handling scripted prim; its pie has Open, Create, Touch,
  Sit Here and Edit enabled and Pay disabled, and Touch reaches the grid as an
  `ObjectGrab` for that prim (read off the session's event stream).
- **Two regions**: a catalogue region and its eastern neighbour; World ▸
  World Map, search, click the result, Teleport. The agent probe, the status
  bar's read-out and the status probe all name the neighbour.
- **Two-viewer chat**: Alpha types a line into its nearby-chat bar and
  presses Enter; Beta's transcript and Alpha's own (the echo) show it from
  Alpha. It runs on the fake grid rather than only on a live grid, because
  [[server-world-chat-routing]] was done first for it.

What the pilot found and fixed on the way:

- **The fake grid told every viewer it could do nothing with any object**,
  the owner included (`update_flags` carried no permission bits). Every
  `ObjectUpdate` now leaves through `world::send_objects`, stamped with the
  receiving agent's flags (OpenSim's `GenerateClientFlags`, owner and
  everyone half), a new prim carries OpenSim's default masks, and a
  permissions edit re-sends the object.
- **A Create-tool click on an object selected that object**, and the rez's
  drop-into-edit then matched the pending rez against it too (it is within
  the match slop of a build point on its own face), so the new prim was never
  selected. A plain Create click no longer runs the selection gesture (the
  reference's `LLToolCompCreate`), and a pending rez resolves only against an
  object that entered the scene after it.
- **A world select waited only for build mode**, and the Build window opens
  on the Create tool, where the same click rezzes. Select and sweep now wait
  for a tool that selects (`EditTool::selects_objects`).
