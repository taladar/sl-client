---
id: protocol-cancel-after-teleport-finish-strands-agent
title: Cancelling a teleport after its TeleportFinish strands the agent
topic: protocol
status: done
origin: mixed-channel race audit (2026-11-23)
refs: [protocol-teleport-deferred-teardown-handover, protocol-logout-during-handover-is-undone]
---

Context: [context/protocol.md](../context/protocol.md).

Confirmed by reading the client code and OpenSim's. Not reproduced live.

## Fix (2026-11-24)

The overlay no longer offers Cancel once the teleport is arriving, and a start
or progress line handled after the finish no longer moves it back to an earlier
phase. `cancel_teleport` does nothing once a handover is pending; the teleport
timeout still abandons a destination that never confirms.

A new teleport requested in that state still replaces the pending one, as
before. Firestorm lets it through on purpose as the way out of a stuck
teleport, and `handover_teleport_supersedes_pending` keeps that behaviour.

## Bad order

`TeleportFinish` arrives over the event queue, `CompleteAgentMovement` goes
out, the user presses Cancel, then `AgentMovementComplete` arrives over UDP.

## Cause

The overlay's Cancel button stays visible through `Phase::Arriving`
(`sl-viewer-places/src/teleport_progress.rs`). `cancel_teleport` then calls
`abort_pending_handover`, so the arrival comes from an unknown circuit and is
dropped. OpenSim's V2 transfer has no cancel check after the finish
(`EntityTransferModule.cs`). Firestorm hides Cancel at this point
(`llviewermessage.cpp`).

## Effect

The grid has the agent in the destination while the client believes it is in
the source. After a distant teleport the source closes the agent and the
session dies on the inactivity timeout. After a near teleport the source keeps
a child agent, so the link stays up with the avatar stuck.

## Fix

Hide Cancel in `Phase::Arriving`, and make `cancel_teleport` leave a pending
handover alone. The viewer watchdog relies on cancel to recover a hung
handover, so it needs a separate way to abandon one.
