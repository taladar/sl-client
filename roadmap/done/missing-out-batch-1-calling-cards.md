---
id: missing-out-batch-1
title: calling cards
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

**Out batch 1 — calling cards.** `OfferCallingCard`,
`AcceptCallingCard`, `DeclineCallingCard`: the viewer→sim counterparts of the
inbound batch-5 events. Offer a calling card for an avatar; accept/decline an
incoming offer, echoing its `TransactionId`.

Implemented as `Session::offer_calling_card(to_agent_id: AgentKey,
transaction_id: TransactionId)`, `Session::accept_calling_card(transaction_id:
TransactionId, calling_card_folder: InventoryFolderKey)` and
`Session::decline_calling_card(transaction_id: TransactionId)` (mirroring the
existing `send_friendship_offer` / `accept_friendship` / `decline_friendship`
trio — `AcceptCallingCard` carries the same `FolderData` destination-folder
block as `AcceptFriendship`), backed by `send_offer_calling_card` /
`send_accept_calling_card` / `send_decline_calling_card` on the circuit. Wired
as `Command::{OfferCallingCard, AcceptCallingCard, DeclineCallingCard}`
through the tokio runtime, the `command_name` formatter, and the
`offer_calling_card` / `accept_calling_card` / `decline_calling_card` REPL
tokens. SL-only round-trip (OpenSim does not surface calling-card offers);
exercised by a pack-the-wire test asserting each message carries the right
dest/transaction/folder ids.
