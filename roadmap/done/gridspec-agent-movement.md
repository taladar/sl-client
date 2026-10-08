---
id: gridspec-agent-movement
title: How each grid moves an agent: AgentUpdate handling, speeds, terse updates
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-agentupdate-cadence-effects,
  server-world-agent-movement, gridspec-animations, gridspec-neighbours-crossing]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-08)

Measured and written up in `book/src/gridspec/movement.md`.

- **Discover.** A new `agent-movement` conformance case holds one control
  after another — forward (stated once, ten and a hundred times a second),
  backward, sideways, the nudge and fast bits, always-run, a crouch, a turn
  on the spot, three jumps, a thirty-second climb, a hover, level flight, a
  descent and the fall from there — and samples every update of its own
  avatar and every statement of its animation set. It ran three times on
  aditi (two avatars, two regions) and on OpenSim, on a square of ground
  flattened for it in the local block's north-eastern region.
- **Findings.** The speeds are close and not the same (walk 3.20 against
  3.145 m/s, run 5.13 against 5.10, level flight 16.0 against 12.58, descent
  22.8 against 16.35, fall 52.8 against 50.1); `FAST_AT` does nothing on
  either grid, a crouch slows only Second Life's avatar, and neither has a
  ceiling within 520 m. **Second Life holds the avatar for `FINISH_ANIM`**:
  with the jump key held and nothing else said it never left the ground (18
  s), it stayed where it fell until the bit came, and an arrival at a
  region's landing point stood for 24 s until it did — which is what
  stopped the walk in [[gridspec-neighbours-crossing]]. OpenSim ignores the
  bit. Second Life reports a walking avatar eight to sixteen times a second
  and a steadily climbing one not at all; OpenSim reports a level walk once
  every two seconds. Both report a velocity that is the motion and a zero
  acceleration. A hundred `AgentUpdate`s a second change nothing on either,
  and both keep an avatar walking on one a second.
- **Fake grid.** Nothing, as planned: it moves no avatar, and the controller
  is [[server-world-agent-movement]], which now carries the measured figures
  and the instruction to take this case offline for both flavours.
- **Viewer.** Nothing had to change: its `FINISH_ANIM` logic, its
  controls-stated-once and its dead-reckoning through a silent circuit are
  what both grids need. A new live-only `e2e_movement` walks the viewer's
  avatar from its login spot, jumps it and walks it again by held keys, on
  both backends; it passed on OpenSim and on aditi.
- **Not done here.** How long Second Life's holds last unanswered, what
  makes an arrival a held one (one of three was), a hold during OpenSim's
  1.1 s `standup`, a turn on the spot on Second Life (three readings, no
  agreement), and swimming, ground-sitting, pushes and collisions. The
  non-built-in assets in Second Life's animation sets are noted in
  [[gridspec-animations]]. The viewer's *drawn* avatar through Second
  Life's silences was not looked at: the e2e reads the position the grid
  reports.

## Known already

The fake grid ignores `AgentUpdate`. SL waits for the viewer's `FINISH_ANIM`
after standup / landing / jump, OpenSim ends those states on a timer.

## Discover

An `agent-movement` conformance case (hold `AT_POS`, fly, run, jump; sample
terse self-updates: rate, speeds, ground clamp, hover, flight ceiling,
`SetAlwaysRun`, reaction to a high `AgentUpdate` rate); viewer via
`Probe::Agent` with a key-hold driver verb.

## Document

`book/src/gridspec/movement.md`.

## Fake grid

Large — [[server-world-agent-movement]].

## Viewer

Prediction reconciled to each grid's corrections; the `FINISH_ANIM` handshake on
SL.
