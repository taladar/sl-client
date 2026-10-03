---
id: gridspec-agent-movement
title: How each grid moves an agent: AgentUpdate handling, speeds, terse updates
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-agentupdate-cadence-effects]
---

Context: [context/gridspec.md](../context/gridspec.md).

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
