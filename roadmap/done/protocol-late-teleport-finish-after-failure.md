---
id: protocol-late-teleport-finish-after-failure
title: A TeleportFinish handled after its TeleportFailed starts a phantom teleport
topic: protocol
status: done
origin: mixed-channel race audit (2026-11-23)
---

Context: [context/protocol.md](../context/protocol.md).

Reported by the audit from the client and OpenSim code. The trace has not been
independently re-checked or reproduced. OpenSim V2 transfers only, and it needs
`UpdateAgent` to fail quickly, so it should be rare.

## Fix (2026-11-24)

A UDP refusal of a teleport aimed at a known region is remembered for 10 s. A
`TeleportFinish` handled after it is ignored only if it names that region and
no teleport is running. A finish naming no region or another region, a finish
after the window, and a finish after an event-queue refusal are all still
honoured.

## Bad order

OpenSim queues `TeleportFinish` on the event queue, then can send
`TeleportFailed` by UDP. The failure can be handled first.

## Cause

The `TeleportFinish` arm of `handle_caps_event` enters a remote teleport when
none is running.

## Effect

The overlay returns and the client handshakes with a destination that already
dropped the agent. It fails after the 30 s timeout.

## Fix

Ignore a finish shortly after a refusal only if it names the refused target.
Never drop one with an unknown target, because the grid may really have moved
the agent.
