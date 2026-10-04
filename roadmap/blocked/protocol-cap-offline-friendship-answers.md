---
id: protocol-cap-offline-friendship-answers
title: Answer offline friendship offers over AcceptFriendship / DeclineFriendship
topic: protocol
status: blocked
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities, viewer-offline-im-drain]
blocked_by: [gridspec-friends-presence]
---

Context: [context/protocol.md](../context/protocol.md).

Second Life grants both. Firestorm uses them only for offers delivered offline
through `ReadOfflineMsgs` (which carry no transaction id), and reads offline
messages over that capability only when `ReadOfflineMsgs`, `AcceptFriendship`
and `AcceptGroupInvite` are all granted. Accept: POST
`{cap}?from=<uuid>&agent_name="<name>"` with an empty body → `{success}`.
Decline: DELETE `{cap}?from=<uuid>` → `{success}`. Online offers stay on UDP.

Our viewer drains offline messages over UDP `RetrieveInstantMessages` on
purpose, so every offer has a transaction id ([[viewer-offline-im-drain]]
says to revisit once these exist). Add the two commands, switch the offline
drain to `ReadOfflineMsgs` when all three capabilities are granted, and answer
cap-delivered offers over these.

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.

## Done together with [[gridspec-friends-presence]]

This capability is adopted inside that gridspec task, which measures the
feature on both grids over both paths, makes the fake grid serve the
capability per flavour, and checks the viewer. Claim that task.
