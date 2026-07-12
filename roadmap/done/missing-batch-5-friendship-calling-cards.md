---
id: missing-batch-5
title: friendship & calling cards
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

## Batch 5 — friendship & calling cards

`TerminateFriendship` (Low 300), `OfferCallingCard` (Low 301),
`AcceptCallingCard` (Low 302), `DeclineCallingCard` (Low 303).

Implemented as four inline `Event` variants (all payloads are key +
transaction-id newtype combos, so no dedicated domain struct was warranted):
`Event::FriendshipTerminated { other: FriendKey }` (the `AgentData` echo of this
agent's own id is dropped — only `ExBlock.OtherID`, the former friend, matters);
`Event::CallingCardOffered { offering_agent: AgentKey, transaction:
TransactionId }` (`AgentBlock.DestID`, this agent itself, is dropped — note a
calling card is a reference card to an avatar filed in the Calling Cards folder,
*not* a friendship request); `Event::CallingCardAccepted { agent: AgentKey,
transaction: TransactionId }` (the accepter's `FolderData` destination folder is
their inventory, not this agent's, so it is dropped); and
`Event::CallingCardDeclined { agent: AgentKey, transaction: TransactionId }`.
The `transaction` is the existing `TransactionId` newtype, correlating an
accept/decline back to the original offer.
