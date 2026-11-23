---
id: protocol-logout-during-handover-is-undone
title: A logout during a teleport handover is silently undone
topic: protocol
status: bugs
origin: mixed-channel race audit (2026-11-23)
refs: [protocol-cancel-after-teleport-finish-strands-agent]
---

Context: [context/protocol.md](../context/protocol.md).

Confirmed by reading the code. Not reproduced live.

## Bad order

`TeleportFinish` over the event queue, then a local logout, then UDP
`AgentMovementComplete` from the destination.

## Cause

`initiate_logout` (`sl-proto/src/session/methods.rs`) leaves `pending_handover`
set. `commit_handover` has no state check: it replaces the root circuit that
holds the logout timer and sets `SessionState::Active`.

## Effect

`LoggedOut` never fires and the session carries on in the destination. A second
logout works.

## Fix

Abort the pending handover in `initiate_logout`, or refuse to commit a handover
while logging out.
